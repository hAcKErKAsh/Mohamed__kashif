//! Workflow designer API and the approvals inbox.

use std::collections::{HashSet, VecDeque};

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    audit,
    auth::Auth,
    authz::unit_path,
    engine::{self, Decision, Graph, NodeKind},
    error::{AppError, AppResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workflows", get(list_workflows).post(create_workflow))
        .route("/workflows/simulate", post(simulate))
        .route("/workflows/{id}", get(get_workflow).put(update_workflow))
        .route("/workflows/{id}/publish", post(publish))
        .route("/workflows/{id}/retire", post(retire))
        .route("/approvals/inbox", get(inbox))
        .route("/approvals/mine", get(mine))
        .route("/approvals/all", get(all_requests))
        .route("/approvals/{id}", get(request_detail))
        .route("/approvals/{id}/cancel", post(cancel))
        .route("/approvals/{id}/resubmit", post(resubmit))
        .route("/approvals/tasks/{id}/decide", post(decide))
}

async fn known_roles(st: &AppState) -> AppResult<HashSet<String>> {
    Ok(sqlx::query_scalar::<_, String>("select key from roles").fetch_all(&st.db).await?.into_iter().collect())
}

fn parse_graph(v: &Value) -> AppResult<Graph> {
    serde_json::from_value(v.clone()).map_err(|e| AppError::bad(format!("The workflow couldn't be read: {e}")))
}

async fn list_workflows(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.workflows.manage")?;
    let rows: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object(
               'key', w.key,
               'versions', jsonb_agg(jsonb_build_object('id', w.id, 'version', w.version, 'status', w.status,
                                                         'name', w.name, 'description', w.description,
                                                         'created_at', w.created_at, 'published_at', w.published_at,
                                                         'stages', (select count(*) from jsonb_array_elements(w.graph->'nodes') n where n->>'type' = 'approval'))
                                     order by w.version desc),
               'in_flight', (select count(*) from approval_requests r join workflow_definitions d on d.id = r.workflow_id
                             where d.key = w.key and r.status = 'in_progress'))
           from workflow_definitions w group by w.key order by w.key"#,
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

async fn get_workflow(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.workflows.manage")?;
    let row: Value = sqlx::query_scalar(
        "select jsonb_build_object('id', id, 'key', key, 'version', version, 'name', name, 'description', description,
                                   'graph', graph, 'status', status, 'created_at', created_at, 'published_at', published_at)
         from workflow_definitions where id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(row))
}

#[derive(Deserialize)]
struct WorkflowInput {
    key: Option<String>,
    name: String,
    #[serde(default)]
    description: String,
    graph: Value,
}

/// Creating always makes a new draft version, so running requests keep the
/// exact definition they started with.
async fn create_workflow(State(st): State<AppState>, a: Auth, Json(b): Json<WorkflowInput>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.workflows.manage")?;
    parse_graph(&b.graph)?;
    let key = b.key.unwrap_or_default().trim().to_lowercase();
    let mut tx = st.db.begin().await?;
    let version: i32 = sqlx::query_scalar("select coalesce(max(version), 0) + 1 from workflow_definitions where key = $1")
        .bind(&key)
        .fetch_one(&mut *tx)
        .await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into workflow_definitions (key, version, name, description, graph, created_by) values ($1, $2, $3, $4, $5, $6) returning id",
    )
    .bind(&key)
    .bind(version)
    .bind(b.name.trim())
    .bind(&b.description)
    .bind(&b.graph)
    .bind(a.id())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_check_violation() => AppError::bad("Key must be lowercase letters, numbers and underscores."),
        other => other.into(),
    })?;
    audit::record(&mut *tx, Some(a.id()), "workflow.draft_created", "workflow", Some(id.to_string()), Some(&a.ip), json!({ "key": key, "version": version })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": id, "version": version })))
}

async fn update_workflow(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<WorkflowInput>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.workflows.manage")?;
    parse_graph(&b.graph)?;
    let n = sqlx::query("update workflow_definitions set name = $2, description = $3, graph = $4 where id = $1 and status = 'draft'")
        .bind(id)
        .bind(b.name.trim())
        .bind(&b.description)
        .bind(&b.graph)
        .execute(&st.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::Conflict("Only drafts can be edited. Create a new version instead.".into()));
    }
    audit::record(&st.db, Some(a.id()), "workflow.draft_saved", "workflow", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn publish(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.workflows.manage")?;
    let (key, graph_v, status): (String, Value, String) =
        sqlx::query_as("select key, graph, status from workflow_definitions where id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?
            .ok_or(AppError::NotFound)?;
    if status != "draft" {
        return Err(AppError::Conflict("This version has already been published or retired.".into()));
    }
    let graph = parse_graph(&graph_v)?;
    if let Err(errs) = graph.validate(&known_roles(&st).await?) {
        return Ok(Json(json!({ "ok": false, "errors": errs })));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("update workflow_definitions set status = 'retired' where key = $1 and status = 'published'")
        .bind(&key)
        .execute(&mut *tx)
        .await?;
    sqlx::query("update workflow_definitions set status = 'published', published_at = now() where id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut *tx, Some(a.id()), "workflow.published", "workflow", Some(id.to_string()), Some(&a.ip), json!({ "key": key })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn retire(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.workflows.manage")?;
    sqlx::query("update workflow_definitions set status = 'retired' where id = $1 and status = 'published'")
        .bind(id)
        .execute(&st.db)
        .await?;
    audit::record(&st.db, Some(a.id()), "workflow.retired", "workflow", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SimulateInput {
    graph: Value,
    #[serde(default)]
    data: Value,
    org_unit_id: Uuid,
    requester_id: Option<Uuid>,
}

/// Dry-run: show which path a request with this data would take and who would
/// be asked at each stage, assuming every stage approves.
async fn simulate(State(st): State<AppState>, a: Auth, Json(b): Json<SimulateInput>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.workflows.manage")?;
    let graph = parse_graph(&b.graph)?;
    let validation = graph.validate(&known_roles(&st).await?).err().unwrap_or_default();
    let requester = b.requester_id.unwrap_or(a.id());
    let mut conn = st.db.acquire().await?;
    let roles: Vec<String> = sqlx::query_scalar(
        "select distinct r.key from memberships m join roles r on r.id = m.role_id where m.user_id = $1",
    )
    .bind(requester)
    .fetch_all(&mut *conn)
    .await?;
    let (kind, name, depth): (String, String, i32) =
        sqlx::query_as("select kind, name, nlevel(path) from org_units where id = $1")
            .bind(b.org_unit_id)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or(AppError::NotFound)?;
    let mut ctx = if b.data.is_object() { b.data.clone() } else { json!({}) };
    ctx["requester"] = json!({ "roles": roles });
    ctx["unit"] = json!({ "kind": kind, "name": name, "depth": depth });

    let mut trace = Vec::new();
    let mut visited = HashSet::new();
    let start = graph.nodes.iter().find(|n| n.kind == NodeKind::Start).map(|n| n.id.clone());
    let mut q: VecDeque<String> = start.into_iter().collect();
    while let Some(id) = q.pop_front() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(node) = graph.node(&id) else { continue };
        let mut entry = json!({ "node_id": node.id, "type": node.kind, "name": node.label() });
        let outs: Vec<&engine::Edge> = graph.edges.iter().filter(|e| e.from == id).collect();
        match node.kind {
            NodeKind::Approval => {
                let mut people = Vec::new();
                if let Some(rule) = &node.approvers {
                    let ids = engine::resolve_approvers(&mut conn, rule, b.org_unit_id, requester).await?;
                    people = sqlx::query_scalar::<_, String>("select full_name from users where id = any($1) order by full_name")
                        .bind(&ids)
                        .fetch_all(&mut *conn)
                        .await?;
                }
                entry["approvers"] = json!(people);
                entry["mode"] = json!(node.mode);
                entry["fallback"] = json!(people.is_empty());
                q.extend(outs.iter().filter(|e| e.on.as_deref().unwrap_or("approved") == "approved").map(|e| e.to.clone()));
            }
            NodeKind::Condition => {
                let chosen = outs
                    .iter()
                    .find(|e| !e.default && e.when.as_ref().is_some_and(|c| c.eval(&ctx)))
                    .or_else(|| outs.iter().find(|e| e.default));
                if let Some(e) = chosen {
                    entry["chose"] = json!(e.label.clone().unwrap_or_else(|| e.to.clone()));
                    q.push_back(e.to.clone());
                }
            }
            NodeKind::Parallel => {
                q.extend(outs.iter().filter(|e| e.when.as_ref().is_none_or(|c| c.eval(&ctx))).map(|e| e.to.clone()));
            }
            NodeKind::End => {
                entry["outcome"] = json!(node.outcome);
            }
            _ => q.extend(outs.iter().map(|e| e.to.clone())),
        }
        trace.push(entry);
    }
    Ok(Json(json!({ "trace": trace, "errors": validation })))
}

// ---------------------------------------------------------------------------
// Approvals
// ---------------------------------------------------------------------------

async fn inbox(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    let rows: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object(
               'task_id', t.id, 'request_id', r.id, 'title', r.title, 'subject_type', r.subject_type,
               'stage', s.node_name, 'mode', s.mode, 'due_at', s.due_at, 'assigned_at', t.created_at,
               'requester', u.full_name, 'unit', o.name, 'workflow', w.name, 'data', r.data)
           from approval_tasks t
           join approval_steps s on s.id = t.step_id
           join approval_requests r on r.id = t.request_id
           join workflow_definitions w on w.id = r.workflow_id
           join users u on u.id = r.requester_id
           join org_units o on o.id = r.org_unit_id
           where t.assignee_id = $1 and t.status = 'pending' and s.status = 'active'
           order by s.due_at nulls last, t.created_at"#,
    )
    .bind(a.id())
    .fetch_all(&st.db)
    .await?;
    let decided: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('task_id', t.id, 'request_id', r.id, 'title', r.title, 'decision', t.status,
                                     'decided_at', t.decided_at, 'request_status', r.status, 'requester', u.full_name)
           from approval_tasks t join approval_requests r on r.id = t.request_id join users u on u.id = r.requester_id
           where t.assignee_id = $1 and t.status in ('approved', 'rejected', 'returned')
           order by t.decided_at desc limit 30"#,
    )
    .bind(a.id())
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!({ "pending": rows, "recent": decided })))
}

const REQUEST_SUMMARY: &str = r#"jsonb_build_object(
    'id', r.id, 'title', r.title, 'status', r.status, 'subject_type', r.subject_type, 'created_at', r.created_at,
    'updated_at', r.updated_at, 'completed_at', r.completed_at, 'workflow', w.name, 'unit', o.name,
    'requester', u.full_name, 'round', r.round,
    'current', (select string_agg(s.node_name, ', ') from approval_steps s where s.request_id = r.id and s.status = 'active'))"#;

async fn mine(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    let rows: Vec<Value> = sqlx::query_scalar(&format!(
        "select {REQUEST_SUMMARY} from approval_requests r join workflow_definitions w on w.id = r.workflow_id
         join org_units o on o.id = r.org_unit_id join users u on u.id = r.requester_id
         where r.requester_id = $1 order by r.created_at desc limit 200"
    ))
    .bind(a.id())
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

#[derive(Deserialize)]
struct AllQuery {
    status: Option<String>,
}

async fn all_requests(State(st): State<AppState>, a: Auth, Query(q): Query<AllQuery>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("approvals.view_all")?;
    let scope = a.principal.scope_paths("approvals.view_all");
    let rows: Vec<Value> = sqlx::query_scalar(&format!(
        "select {REQUEST_SUMMARY} from approval_requests r join workflow_definitions w on w.id = r.workflow_id
         join org_units o on o.id = r.org_unit_id join users u on u.id = r.requester_id
         where o.path <@ $1::ltree[] and ($2::text is null or r.status = $2)
         order by r.created_at desc limit 300"
    ))
    .bind(&scope)
    .bind(q.status.filter(|s| !s.is_empty()))
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

async fn request_detail(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let (requester, unit_id, workflow_id): (Uuid, Uuid, Uuid) =
        sqlx::query_as("select requester_id, org_unit_id, workflow_id from approval_requests where id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?
            .ok_or(AppError::NotFound)?;
    let path = unit_path(&st.db, unit_id).await?;
    let is_assignee: bool = sqlx::query_scalar("select exists(select 1 from approval_tasks where request_id = $1 and assignee_id = $2)")
        .bind(id)
        .bind(a.id())
        .fetch_one(&st.db)
        .await?;
    let is_guardian: bool = sqlx::query_scalar("select exists(select 1 from guardianships where guardian_id = $1 and student_id = $2)")
        .bind(a.id())
        .bind(requester)
        .fetch_one(&st.db)
        .await?;
    if requester != a.id() && !is_assignee && !is_guardian && !a.principal.can_at("approvals.view_all", &path) {
        return Err(AppError::Forbidden);
    }
    let request: Value = sqlx::query_scalar(&format!(
        "select {REQUEST_SUMMARY} || jsonb_build_object('data', r.data, 'requester_id', r.requester_id, 'subject_id', r.subject_id)
         from approval_requests r join workflow_definitions w on w.id = r.workflow_id
         join org_units o on o.id = r.org_unit_id join users u on u.id = r.requester_id where r.id = $1"
    ))
    .bind(id)
    .fetch_one(&st.db)
    .await?;
    let steps: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('id', s.id, 'node_id', s.node_id, 'name', s.node_name, 'type', s.node_type,
                                     'status', s.status, 'mode', s.mode, 'quorum', s.quorum, 'round', s.round,
                                     'activated_at', s.activated_at, 'completed_at', s.completed_at, 'due_at', s.due_at,
                                     'escalated', s.escalated,
                                     'tasks', coalesce((select jsonb_agg(jsonb_build_object(
                                          'id', t.id, 'assignee', u.full_name, 'assignee_id', u.id, 'status', t.status,
                                          'comment', t.comment, 'decided_at', t.decided_at) order by t.created_at)
                                       from approval_tasks t join users u on u.id = t.assignee_id where t.step_id = s.id), '[]'::jsonb))
           from approval_steps s where s.request_id = $1 order by s.round, s.activated_at, s.node_id"#,
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    let events: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('id', e.id, 'at', e.at, 'kind', e.kind, 'node_id', e.node_id, 'message', e.message,
                                     'actor', u.full_name, 'detail', e.detail)
           from approval_events e left join users u on u.id = e.actor_id where e.request_id = $1 order by e.id"#,
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    let graph: Value = sqlx::query_scalar("select graph from workflow_definitions where id = $1")
        .bind(workflow_id)
        .fetch_one(&st.db)
        .await?;
    let my_task: Option<Uuid> = sqlx::query_scalar(
        "select t.id from approval_tasks t join approval_steps s on s.id = t.step_id
         where t.request_id = $1 and t.assignee_id = $2 and t.status = 'pending' and s.status = 'active' limit 1",
    )
    .bind(id)
    .bind(a.id())
    .fetch_optional(&st.db)
    .await?;
    Ok(Json(json!({
        "request": request, "steps": steps, "events": events, "graph": graph,
        "my_task_id": my_task, "is_requester": requester == a.id()
    })))
}

#[derive(Deserialize)]
struct DecideInput {
    decision: Decision,
    comment: Option<String>,
}

async fn decide(State(st): State<AppState>, a: Auth, Path(task_id): Path<Uuid>, Json(b): Json<DecideInput>) -> AppResult<Json<Value>> {
    let comment = b.comment.map(|c| c.trim().chars().take(2000).collect::<String>()).filter(|c| !c.is_empty());
    let mut tx = st.db.begin().await?;
    let request_id = engine::decide(&mut tx, task_id, a.id(), b.decision, comment).await?;
    audit::record(&mut *tx, Some(a.id()), "approval.decided", "approval_request", Some(request_id.to_string()), Some(&a.ip), json!({ "task_id": task_id, "decision": format!("{:?}", b.decision).to_lowercase() })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true, "request_id": request_id })))
}

async fn cancel(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let mut tx = st.db.begin().await?;
    engine::cancel(&mut tx, id, a.id()).await?;
    audit::record(&mut *tx, Some(a.id()), "approval.cancelled", "approval_request", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ResubmitInput {
    data: Option<Value>,
}

async fn resubmit(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<ResubmitInput>) -> AppResult<Json<Value>> {
    let mut tx = st.db.begin().await?;
    if let Some(d) = &b.data {
        // Re-validate edited form data against the form definition.
        let form_fields: Option<Value> = sqlx::query_scalar(
            "select f.fields from approval_requests r join form_submissions s on s.id = r.subject_id and r.subject_type = 'form'
             join form_definitions f on f.id = s.form_id where r.id = $1",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(fields) = form_fields {
            crate::routes::forms::validate_submission(&fields, d)?;
        }
    }
    engine::resubmit(&mut tx, id, a.id(), b.data).await?;
    audit::record(&mut *tx, Some(a.id()), "approval.resubmitted", "approval_request", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}
