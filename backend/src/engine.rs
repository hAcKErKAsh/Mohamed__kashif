//! Multi-stage, multi-path approval engine.
//!
//! A workflow is a directed acyclic graph of nodes:
//!
//! * `start` — entry point, exactly one.
//! * `approval` — a stage. Approvers are resolved from the org tree when the
//!   stage activates. `mode` decides how their votes combine: `any` (first
//!   decision wins), `all` (unanimous), or `quorum` (N approvals).
//! * `condition` — exclusive gateway; the first outgoing edge whose `when`
//!   matches the request data is taken, otherwise the `default` edge.
//! * `parallel` — fork; every outgoing edge whose condition holds is taken.
//! * `join` — waits until no stage of the current round is still active.
//! * `end` — terminal with `outcome` `approved` or `rejected`.
//!
//! Approval nodes route on outcome (`on: approved | rejected`). A missing
//! rejection edge means "reject the request". Any approver may also *return*
//! the request to the requester, who can edit and resubmit; the graph then runs
//! again from `start` in a new round.
//!
//! All engine operations run inside the caller's transaction so a decision and
//! everything it triggers commit or roll back together.

use std::collections::{HashMap, HashSet, VecDeque};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

// ---------------------------------------------------------------------------
// Graph model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// What to do when a stage resolves to nobody: `admins` hands it to
    /// workflow administrators, `skip` passes the stage automatically.
    #[serde(default = "default_fallback")]
    pub fallback: String,
}

fn default_fallback() -> String {
    "admins".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Start,
    Approval,
    Condition,
    Parallel,
    Join,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Any,
    All,
    Quorum,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Any => "any",
            Mode::All => "all",
            Mode::Quorum => "quorum",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: NodeKind,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub approvers: Option<ApproverRule>,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub quorum: Option<i32>,
    #[serde(default)]
    pub sla_hours: Option<i64>,
    /// Role (searched from the request's unit upward) that receives the stage
    /// if the SLA lapses.
    #[serde(default)]
    pub escalate_to: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    /// Free-form layout hints for the visual builder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<Value>,
}

impl Node {
    pub fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.id.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApproverRule {
    /// Holders of `role`. `search`:
    /// * `nearest` — the closest ancestor-or-self unit that has any holder.
    /// * `all_ancestors` — every holder from the unit up to the root.
    /// * `kind:<unit kind>` — holders at the nearest ancestor of that kind.
    Role {
        role: String,
        #[serde(default = "default_search")]
        search: String,
    },
    /// Union of several roles, e.g. a leadership panel voting by quorum.
    Roles {
        roles: Vec<String>,
        #[serde(default = "default_search")]
        search: String,
    },
    Users { ids: Vec<Uuid> },
    /// The requester's linked guardians (parents).
    Guardians,
}

fn default_search() -> String {
    "nearest".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<Cond>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Cond {
    All { all: Vec<Cond> },
    Any { any: Vec<Cond> },
    Not { not: Box<Cond> },
    Cmp { field: String, op: String, #[serde(default)] value: Value },
}

impl Cond {
    pub fn eval(&self, ctx: &Value) -> bool {
        match self {
            Cond::All { all } => all.iter().all(|c| c.eval(ctx)),
            Cond::Any { any } => any.iter().any(|c| c.eval(ctx)),
            Cond::Not { not } => !not.eval(ctx),
            Cond::Cmp { field, op, value } => {
                let lhs = lookup(ctx, field);
                compare(lhs, op, value)
            }
        }
    }
}

fn lookup<'a>(ctx: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(ctx, |v, key| v.get(key))
}

fn as_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    if let (Some(x), Some(y)) = (as_num(a), as_num(b)) {
        if !(a.is_boolean() ^ b.is_boolean()) {
            return (x - y).abs() < f64::EPSILON;
        }
    }
    match (a, b) {
        (Value::String(x), Value::String(y)) => x.eq_ignore_ascii_case(y),
        _ => a == b,
    }
}

fn compare(lhs: Option<&Value>, op: &str, rhs: &Value) -> bool {
    match op {
        "exists" => lhs.is_some_and(|v| !v.is_null() && v != &Value::String(String::new())),
        "empty" => lhs.is_none_or(|v| v.is_null() || v == &Value::String(String::new())),
        _ => {
            let Some(lhs) = lhs else { return op == "ne" };
            match op {
                "eq" => loose_eq(lhs, rhs),
                "ne" => !loose_eq(lhs, rhs),
                "gt" | "gte" | "lt" | "lte" => match (as_num(lhs), as_num(rhs)) {
                    (Some(a), Some(b)) => match op {
                        "gt" => a > b,
                        "gte" => a >= b,
                        "lt" => a < b,
                        _ => a <= b,
                    },
                    _ => false,
                },
                "in" => rhs.as_array().is_some_and(|arr| arr.iter().any(|v| loose_eq(lhs, v))),
                "contains" => match lhs {
                    Value::Array(arr) => arr.iter().any(|v| loose_eq(v, rhs)),
                    Value::String(s) => rhs.as_str().is_some_and(|r| s.to_lowercase().contains(&r.to_lowercase())),
                    _ => false,
                },
                _ => false,
            }
        }
    }
}

impl Graph {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    fn outgoing<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Edge> + 'a {
        self.edges.iter().filter(move |e| e.from == id)
    }

    /// Structural validation run before a definition can be published.
    pub fn validate(&self, known_roles: &HashSet<String>) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        let mut ids = HashSet::new();
        for n in &self.nodes {
            if n.id.is_empty() || n.id.len() > 48 {
                errs.push("Every step needs a short id.".to_string());
            }
            if !ids.insert(n.id.as_str()) {
                errs.push(format!("Step id '{}' is used twice.", n.id));
            }
        }
        let starts = self.nodes.iter().filter(|n| n.kind == NodeKind::Start).count();
        if starts != 1 {
            errs.push(format!("A workflow needs exactly one start (found {starts})."));
        }
        if !self.nodes.iter().any(|n| n.kind == NodeKind::End) {
            errs.push("Add at least one end step.".into());
        }
        for e in &self.edges {
            if !ids.contains(e.from.as_str()) || !ids.contains(e.to.as_str()) {
                errs.push(format!("Connection {} → {} points at a missing step.", e.from, e.to));
            }
        }
        for n in &self.nodes {
            let out: Vec<&Edge> = self.outgoing(&n.id).collect();
            let label = n.label();
            match n.kind {
                NodeKind::Start | NodeKind::Join => {
                    if out.len() != 1 {
                        errs.push(format!("'{label}' must lead to exactly one next step."));
                    }
                }
                NodeKind::Approval => {
                    match &n.approvers {
                        None => errs.push(format!("'{label}' has no approvers.")),
                        Some(ApproverRule::Role { role, search }) => {
                            if !known_roles.contains(role) {
                                errs.push(format!("'{label}' uses unknown role '{role}'."));
                            }
                            if !(search == "nearest" || search == "all_ancestors" || search.starts_with("kind:")) {
                                errs.push(format!("'{label}' has an unknown approver search '{search}'."));
                            }
                        }
                        Some(ApproverRule::Roles { roles, .. }) => {
                            if roles.is_empty() {
                                errs.push(format!("'{label}' lists no roles."));
                            }
                            for role in roles.iter().filter(|r| !known_roles.contains(*r)) {
                                errs.push(format!("'{label}' uses unknown role '{role}'."));
                            }
                        }
                        Some(ApproverRule::Users { ids }) if ids.is_empty() => {
                            errs.push(format!("'{label}' lists no users."))
                        }
                        _ => {}
                    }
                    if n.mode == Mode::Quorum && n.quorum.unwrap_or(0) < 1 {
                        errs.push(format!("'{label}' needs a quorum of at least 1."));
                    }
                    let approved = out.iter().filter(|e| e.on.as_deref().unwrap_or("approved") == "approved").count();
                    if approved != 1 {
                        errs.push(format!("'{label}' needs exactly one 'approved' path."));
                    }
                    if out.iter().filter(|e| e.on.as_deref() == Some("rejected")).count() > 1 {
                        errs.push(format!("'{label}' has more than one 'rejected' path."));
                    }
                }
                NodeKind::Condition => {
                    if out.iter().filter(|e| e.default).count() != 1 {
                        errs.push(format!("Decision '{label}' needs exactly one default path."));
                    }
                    if out.iter().any(|e| !e.default && e.when.is_none()) {
                        errs.push(format!("Every non-default path from '{label}' needs a rule."));
                    }
                }
                NodeKind::Parallel => {
                    if out.len() < 2 {
                        errs.push(format!("Parallel split '{label}' needs two or more branches."));
                    }
                }
                NodeKind::End => {
                    if !out.is_empty() {
                        errs.push(format!("End step '{label}' cannot lead anywhere."));
                    }
                    if !matches!(n.outcome.as_deref(), Some("approved") | Some("rejected")) {
                        errs.push(format!("End step '{label}' must be 'approved' or 'rejected'."));
                    }
                }
            }
        }
        if errs.is_empty() {
            if let Some(cycle_at) = self.find_cycle() {
                errs.push(format!("The flow loops back on itself at '{cycle_at}'. Use 'send back' instead of loops."));
            }
            if let Some(start) = self.nodes.iter().find(|n| n.kind == NodeKind::Start) {
                let reach = self.reachable(&start.id);
                for n in &self.nodes {
                    if !reach.contains(n.id.as_str()) {
                        errs.push(format!("'{}' can never be reached.", n.label()));
                    }
                }
            }
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }

    fn reachable<'a>(&'a self, from: &'a str) -> HashSet<&'a str> {
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([from]);
        while let Some(id) = q.pop_front() {
            if seen.insert(id) {
                for e in self.outgoing(id) {
                    q.push_back(e.to.as_str());
                }
            }
        }
        seen
    }

    fn find_cycle(&self) -> Option<String> {
        // Kahn's algorithm; anything left over sits on a cycle.
        let mut indeg: HashMap<&str, usize> = self.nodes.iter().map(|n| (n.id.as_str(), 0)).collect();
        for e in &self.edges {
            *indeg.entry(e.to.as_str()).or_default() += 1;
        }
        let mut q: VecDeque<&str> = indeg.iter().filter(|(_, d)| **d == 0).map(|(k, _)| *k).collect();
        let mut seen = 0;
        while let Some(id) = q.pop_front() {
            seen += 1;
            for e in self.outgoing(id) {
                let d = indeg.get_mut(e.to.as_str()).unwrap();
                *d -= 1;
                if *d == 0 {
                    q.push_back(e.to.as_str());
                }
            }
        }
        if seen == self.nodes.len() {
            None
        } else {
            indeg.into_iter().find(|(_, d)| *d > 0).map(|(k, _)| k.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RequestRow {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub title: String,
    pub data: Value,
    pub requester_id: Uuid,
    pub org_unit_id: Uuid,
    pub status: String,
    pub round: i32,
}

async fn load_request(tx: &mut PgConnection, id: Uuid) -> AppResult<RequestRow> {
    sqlx::query_as(
        "select id, workflow_id, subject_type, subject_id, title, data, requester_id, org_unit_id, status, round
         from approval_requests where id = $1 for update",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)
}

async fn load_graph(tx: &mut PgConnection, workflow_id: Uuid) -> AppResult<Graph> {
    let g: Value = sqlx::query_scalar("select graph from workflow_definitions where id = $1")
        .bind(workflow_id)
        .fetch_one(&mut *tx)
        .await?;
    serde_json::from_value(g).map_err(|e| AppError::Other(anyhow::anyhow!("stored graph invalid: {e}")))
}

pub async fn published_workflow(tx: &mut PgConnection, key: &str) -> AppResult<Option<Uuid>> {
    Ok(sqlx::query_scalar("select id from workflow_definitions where key = $1 and status = 'published'")
        .bind(key)
        .fetch_optional(&mut *tx)
        .await?)
}

async fn event(
    tx: &mut PgConnection,
    request_id: Uuid,
    actor: Option<Uuid>,
    kind: &str,
    node_id: Option<&str>,
    message: Option<&str>,
    detail: Value,
) -> AppResult<()> {
    sqlx::query(
        "insert into approval_events (request_id, actor_id, kind, node_id, message, detail) values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(request_id)
    .bind(actor)
    .bind(kind)
    .bind(node_id)
    .bind(message)
    .bind(detail)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

pub async fn notify(tx: &mut PgConnection, user_id: Uuid, kind: &str, title: &str, link: &str) -> AppResult<()> {
    sqlx::query("insert into notifications (user_id, kind, title, link) values ($1, $2, $3, $4)")
        .bind(user_id)
        .bind(kind)
        .bind(title)
        .bind(link)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

/// Build the context conditions are evaluated against: the request's own data
/// plus facts about the requester and the unit it was raised in.
async fn eval_context(tx: &mut PgConnection, req: &RequestRow) -> AppResult<Value> {
    let roles: Vec<String> = sqlx::query_scalar(
        "select distinct r.key from memberships m join roles r on r.id = m.role_id where m.user_id = $1",
    )
    .bind(req.requester_id)
    .fetch_all(&mut *tx)
    .await?;
    let (unit_kind, unit_name, depth): (String, String, i32) =
        sqlx::query_as("select kind, name, nlevel(path) from org_units where id = $1")
            .bind(req.org_unit_id)
            .fetch_one(&mut *tx)
            .await?;
    let mut ctx = req.data.clone();
    if !ctx.is_object() {
        ctx = json!({});
    }
    let obj = ctx.as_object_mut().unwrap();
    obj.insert("requester".into(), json!({ "roles": roles }));
    obj.insert("unit".into(), json!({ "kind": unit_kind, "name": unit_name, "depth": depth }));
    Ok(ctx)
}

/// Resolve who should decide a stage. The requester is always excluded: nobody
/// approves their own request.
pub async fn resolve_approvers(
    tx: &mut PgConnection,
    rule: &ApproverRule,
    org_unit_id: Uuid,
    requester: Uuid,
) -> AppResult<Vec<Uuid>> {
    let ids: Vec<Uuid> = match rule {
        ApproverRule::Users { ids } => {
            sqlx::query_scalar("select id from users where id = any($1) and status = 'active' and id <> $2")
                .bind(ids)
                .bind(requester)
                .fetch_all(&mut *tx)
                .await?
        }
        ApproverRule::Guardians => {
            sqlx::query_scalar(
                "select g.guardian_id from guardianships g join users u on u.id = g.guardian_id
                 where g.student_id = $1 and u.status = 'active'",
            )
            .bind(requester)
            .fetch_all(&mut *tx)
            .await?
        }
        ApproverRule::Role { role, search } => role_holders(tx, role, search, org_unit_id, requester).await?,
        ApproverRule::Roles { roles, search } => {
            let mut all = Vec::new();
            for role in roles {
                all.extend(role_holders(tx, role, search, org_unit_id, requester).await?);
            }
            all
        }
    };
    let mut seen = HashSet::new();
    Ok(ids.into_iter().filter(|id| seen.insert(*id)).collect())
}

async fn role_holders(
    tx: &mut PgConnection,
    role: &str,
    search: &str,
    org_unit_id: Uuid,
    requester: Uuid,
) -> AppResult<Vec<Uuid>> {
    let rows: Vec<(Uuid, i32, String)> = sqlx::query_as(
        r#"select m.user_id, nlevel(o.path), o.kind
           from memberships m
           join roles r on r.id = m.role_id
           join org_units o on o.id = m.org_unit_id
           join users u on u.id = m.user_id
           where r.key = $1 and u.status = 'active' and m.user_id <> $3
             and o.path @> (select path from org_units where id = $2)
           order by nlevel(o.path) desc"#,
    )
    .bind(role)
    .bind(org_unit_id)
    .bind(requester)
    .fetch_all(&mut *tx)
    .await?;
    let filtered: Vec<(Uuid, i32, String)> = match search.strip_prefix("kind:") {
        Some(kind) => rows.into_iter().filter(|r| r.2 == kind).collect(),
        None => rows,
    };
    if search == "all_ancestors" {
        return Ok(filtered.into_iter().map(|r| r.0).collect());
    }
    let deepest = filtered.first().map(|r| r.1);
    Ok(filtered.into_iter().filter(|r| Some(r.1) == deepest).map(|r| r.0).collect())
}

async fn admin_fallback(tx: &mut PgConnection, requester: Uuid) -> AppResult<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        r#"select distinct m.user_id from memberships m
           join role_permissions rp on rp.role_id = m.role_id and rp.permission = 'admin.workflows.manage'
           join org_units o on o.id = m.org_unit_id and o.parent_id is null
           join users u on u.id = m.user_id and u.status = 'active'
           where m.user_id <> $1"#,
    )
    .bind(requester)
    .fetch_all(&mut *tx)
    .await?)
}

/// Start a new approval request and run it until it waits on people.
#[allow(clippy::too_many_arguments)]
pub async fn start(
    tx: &mut PgConnection,
    workflow_id: Uuid,
    subject_type: &str,
    subject_id: Uuid,
    title: &str,
    data: Value,
    requester: Uuid,
    org_unit_id: Uuid,
) -> AppResult<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        r#"insert into approval_requests (workflow_id, subject_type, subject_id, title, data, requester_id, org_unit_id)
           values ($1, $2, $3, $4, $5, $6, $7) returning id"#,
    )
    .bind(workflow_id)
    .bind(subject_type)
    .bind(subject_id)
    .bind(title)
    .bind(&data)
    .bind(requester)
    .bind(org_unit_id)
    .fetch_one(&mut *tx)
    .await?;
    event(tx, id, Some(requester), "submitted", None, None, json!({})).await?;
    sync_subject(tx, subject_type, subject_id, "in_progress").await?;
    run_from_start(tx, id).await?;
    Ok(id)
}

async fn run_from_start(tx: &mut PgConnection, request_id: Uuid) -> AppResult<()> {
    let req = load_request(tx, request_id).await?;
    let graph = load_graph(tx, req.workflow_id).await?;
    let start = graph
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Start)
        .ok_or_else(|| AppError::bad("workflow has no start"))?;
    let next: Vec<String> = graph.outgoing(&start.id).map(|e| e.to.clone()).collect();
    walk(tx, &req, &graph, next).await
}

/// Enter each queued node, following automatic nodes until every path is
/// either waiting on people or finished.
async fn walk(tx: &mut PgConnection, req: &RequestRow, graph: &Graph, initial: Vec<String>) -> AppResult<()> {
    let ctx = eval_context(tx, req).await?;
    let mut queue: VecDeque<String> = initial.into();
    while let Some(node_id) = queue.pop_front() {
        let status: String = sqlx::query_scalar("select status from approval_requests where id = $1")
            .bind(req.id)
            .fetch_one(&mut *tx)
            .await?;
        if status != "in_progress" {
            return Ok(());
        }
        let node = graph.node(&node_id).ok_or_else(|| AppError::bad(format!("missing node {node_id}")))?;
        match node.kind {
            NodeKind::Start => {}
            NodeKind::End => {
                let outcome = node.outcome.as_deref().unwrap_or("approved");
                if outcome == "rejected" {
                    finalize(tx, req, "rejected", None).await?;
                    return Ok(());
                }
                if active_count(tx, req).await? == 0 && queue.is_empty() && !join_waiting(tx, req).await? {
                    finalize(tx, req, "approved", None).await?;
                    return Ok(());
                }
            }
            NodeKind::Condition => {
                let chosen = graph
                    .outgoing(&node.id)
                    .find(|e| !e.default && e.when.as_ref().is_some_and(|c| c.eval(&ctx)))
                    .or_else(|| graph.outgoing(&node.id).find(|e| e.default))
                    .ok_or_else(|| AppError::bad("decision has no matching path"))?;
                pass_step(tx, req, node, Some(json!({ "path": chosen.label.clone().unwrap_or(chosen.to.clone()) }))).await?;
                queue.push_back(chosen.to.clone());
            }
            NodeKind::Parallel => {
                let branches: Vec<String> = graph
                    .outgoing(&node.id)
                    .filter(|e| e.when.as_ref().is_none_or(|c| c.eval(&ctx)))
                    .map(|e| e.to.clone())
                    .collect();
                pass_step(tx, req, node, Some(json!({ "branches": branches.len() }))).await?;
                queue.extend(branches);
            }
            NodeKind::Join => {
                let existing: Option<Uuid> = sqlx::query_scalar(
                    "select id from approval_steps where request_id = $1 and round = $2 and node_id = $3",
                )
                .bind(req.id)
                .bind(req.round)
                .bind(&node.id)
                .fetch_optional(&mut *tx)
                .await?;
                let step_id = match existing {
                    Some(id) => id,
                    None => {
                        sqlx::query_scalar(
                            "insert into approval_steps (request_id, round, node_id, node_name, node_type, status)
                             values ($1, $2, $3, $4, 'join', 'waiting') returning id",
                        )
                        .bind(req.id)
                        .bind(req.round)
                        .bind(&node.id)
                        .bind(node.label())
                        .fetch_one(&mut *tx)
                        .await?
                    }
                };
                // Branches still queued in this same walk must land first.
                let pending_here = queue.iter().any(|q| q != &node.id);
                if active_count(tx, req).await? == 0 && !pending_here {
                    let fired: Option<Uuid> = sqlx::query_scalar(
                        "update approval_steps set status = 'passed', completed_at = now()
                         where id = $1 and status = 'waiting' returning id",
                    )
                    .bind(step_id)
                    .fetch_optional(&mut *tx)
                    .await?;
                    if fired.is_some() {
                        event(tx, req.id, None, "joined", Some(&node.id), None, json!({})).await?;
                        queue.retain(|q| q != &node.id);
                        queue.extend(graph.outgoing(&node.id).map(|e| e.to.clone()));
                    }
                }
            }
            NodeKind::Approval => {
                let rule = node.approvers.as_ref().ok_or_else(|| AppError::bad("stage without approvers"))?;
                let mut approvers = resolve_approvers(tx, rule, req.org_unit_id, req.requester_id).await?;
                let mut via_fallback = false;
                if approvers.is_empty() {
                    if graph.fallback == "skip" {
                        pass_step(tx, req, node, Some(json!({ "skipped": "no approvers" }))).await?;
                        event(tx, req.id, None, "skipped", Some(&node.id), Some("No one holds this role; stage skipped."), json!({})).await?;
                        queue.extend(next_for(graph, &node.id, "approved"));
                        continue;
                    }
                    approvers = admin_fallback(tx, req.requester_id).await?;
                    via_fallback = true;
                    if approvers.is_empty() {
                        return Err(AppError::bad(format!("No one can approve '{}'.", node.label())));
                    }
                }
                let due = node.sla_hours.map(|h| Utc::now() + chrono::Duration::hours(h));
                let quorum = match node.mode {
                    Mode::Quorum => Some(node.quorum.unwrap_or(1).min(approvers.len() as i32).max(1)),
                    _ => None,
                };
                let step_id: Uuid = sqlx::query_scalar(
                    "insert into approval_steps (request_id, round, node_id, node_name, node_type, status, mode, quorum, due_at)
                     values ($1, $2, $3, $4, 'approval', 'active', $5, $6, $7) returning id",
                )
                .bind(req.id)
                .bind(req.round)
                .bind(&node.id)
                .bind(node.label())
                .bind(node.mode.as_str())
                .bind(quorum)
                .bind(due)
                .fetch_one(&mut *tx)
                .await?;
                for a in &approvers {
                    sqlx::query("insert into approval_tasks (step_id, request_id, assignee_id) values ($1, $2, $3)")
                        .bind(step_id)
                        .bind(req.id)
                        .bind(a)
                        .execute(&mut *tx)
                        .await?;
                    notify(tx, *a, "approval_task", &format!("Needs your decision: {}", req.title), &format!("/approvals/{}", req.id)).await?;
                }
                event(
                    tx,
                    req.id,
                    None,
                    "stage_started",
                    Some(&node.id),
                    None,
                    json!({ "approvers": approvers.len(), "mode": node.mode.as_str(), "fallback": via_fallback }),
                )
                .await?;
            }
        }
    }
    Ok(())
}

fn next_for(graph: &Graph, node_id: &str, outcome: &str) -> Vec<String> {
    graph
        .outgoing(node_id)
        .filter(|e| e.on.as_deref().unwrap_or("approved") == outcome)
        .map(|e| e.to.clone())
        .collect()
}

async fn pass_step(tx: &mut PgConnection, req: &RequestRow, node: &Node, detail: Option<Value>) -> AppResult<()> {
    let ty = match node.kind {
        NodeKind::Condition => "condition",
        NodeKind::Parallel => "parallel",
        _ => "approval",
    };
    sqlx::query(
        "insert into approval_steps (request_id, round, node_id, node_name, node_type, status, completed_at)
         values ($1, $2, $3, $4, $5, 'passed', now())",
    )
    .bind(req.id)
    .bind(req.round)
    .bind(&node.id)
    .bind(node.label())
    .bind(ty)
    .execute(&mut *tx)
    .await?;
    if let Some(d) = detail {
        if node.kind == NodeKind::Condition {
            event(tx, req.id, None, "routed", Some(&node.id), None, d).await?;
        }
    }
    Ok(())
}

async fn active_count(tx: &mut PgConnection, req: &RequestRow) -> AppResult<i64> {
    Ok(sqlx::query_scalar(
        "select count(*) from approval_steps where request_id = $1 and round = $2 and status = 'active'",
    )
    .bind(req.id)
    .bind(req.round)
    .fetch_one(&mut *tx)
    .await?)
}

async fn join_waiting(tx: &mut PgConnection, req: &RequestRow) -> AppResult<bool> {
    Ok(sqlx::query_scalar(
        "select exists(select 1 from approval_steps where request_id = $1 and round = $2 and status = 'waiting')",
    )
    .bind(req.id)
    .bind(req.round)
    .fetch_one(&mut *tx)
    .await?)
}

/// Close the request with a final status and propagate it to its subject.
async fn finalize(tx: &mut PgConnection, req: &RequestRow, status: &str, actor: Option<Uuid>) -> AppResult<()> {
    sqlx::query(
        "update approval_tasks set status = 'withdrawn', decided_at = now() where request_id = $1 and status = 'pending'",
    )
    .bind(req.id)
    .execute(&mut *tx)
    .await?;
    let step_status = if status == "approved" { "passed" } else { "cancelled" };
    sqlx::query(
        "update approval_steps set status = $2, completed_at = now() where request_id = $1 and status in ('active', 'waiting')",
    )
    .bind(req.id)
    .bind(step_status)
    .execute(&mut *tx)
    .await?;
    let completed = if status == "returned" { None } else { Some(Utc::now()) };
    sqlx::query("update approval_requests set status = $2, updated_at = now(), completed_at = $3 where id = $1")
        .bind(req.id)
        .bind(status)
        .bind(completed)
        .execute(&mut *tx)
        .await?;
    event(tx, req.id, actor, status, None, None, json!({})).await?;
    sync_subject(tx, &req.subject_type, req.subject_id, status).await?;
    let verb = match status {
        "approved" => "was approved",
        "rejected" => "was declined",
        "returned" => "was sent back to you for changes",
        _ => "was cancelled",
    };
    if actor != Some(req.requester_id) {
        notify(tx, req.requester_id, "approval_result", &format!("“{}” {verb}", req.title), &format!("/approvals/{}", req.id)).await?;
    }
    Ok(())
}

/// Mirror a request's lifecycle onto the thing being approved.
async fn sync_subject(tx: &mut PgConnection, subject_type: &str, subject_id: Uuid, status: &str) -> AppResult<()> {
    match subject_type {
        "form" => {
            let s = match status {
                "in_progress" => "in_review",
                other => other,
            };
            sqlx::query("update form_submissions set status = $2, updated_at = now() where id = $1")
                .bind(subject_id)
                .bind(s)
                .execute(&mut *tx)
                .await?;
        }
        "content" => {
            let s = match status {
                "in_progress" => "in_review",
                "approved" => "published",
                "rejected" => "rejected",
                _ => "draft",
            };
            sqlx::query(
                "update content_items set status = $2, updated_at = now(),
                        published_at = case when $2 = 'published' then now() else published_at end
                 where id = $1",
            )
            .bind(subject_id)
            .bind(s)
            .execute(&mut *tx)
            .await?;
        }
        "post" => {
            let s = match status {
                "approved" => "published",
                "rejected" | "cancelled" => "rejected",
                _ => "pending",
            };
            sqlx::query("update posts set status = $2, created_at = case when $2 = 'published' then now() else created_at end where id = $1")
                .bind(subject_id)
                .bind(s)
                .execute(&mut *tx)
                .await?;
        }
        _ => {}
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Reject,
    Return,
}

/// Record a decision on a task and advance the workflow if the stage resolves.
pub async fn decide(
    tx: &mut PgConnection,
    task_id: Uuid,
    actor: Uuid,
    decision: Decision,
    comment: Option<String>,
) -> AppResult<Uuid> {
    let (step_id, request_id, assignee, task_status): (Uuid, Uuid, Uuid, String) = sqlx::query_as(
        "select step_id, request_id, assignee_id, status from approval_tasks where id = $1",
    )
    .bind(task_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    // Lock the request first so concurrent decisions on one request serialise.
    let req = load_request(tx, request_id).await?;
    if assignee != actor {
        return Err(AppError::Forbidden);
    }
    let task_status: String = sqlx::query_scalar("select status from approval_tasks where id = $1")
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap_or(task_status);
    if task_status != "pending" || req.status != "in_progress" {
        return Err(AppError::Conflict("This decision has already been made.".into()));
    }
    if decision != Decision::Approve && comment.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Err(AppError::bad("Please add a short reason so the requester knows what to do next."));
    }
    let new_status = match decision {
        Decision::Approve => "approved",
        Decision::Reject => "rejected",
        Decision::Return => "returned",
    };
    sqlx::query("update approval_tasks set status = $2, comment = $3, decided_at = now() where id = $1")
        .bind(task_id)
        .bind(new_status)
        .bind(&comment)
        .execute(&mut *tx)
        .await?;

    let (node_id, mode, quorum, step_status): (String, String, Option<i32>, String) =
        sqlx::query_as("select node_id, mode, quorum, status from approval_steps where id = $1")
            .bind(step_id)
            .fetch_one(&mut *tx)
            .await?;
    event(tx, req.id, Some(actor), new_status, Some(&node_id), comment.as_deref(), json!({})).await?;
    if step_status != "active" {
        return Ok(req.id);
    }

    if decision == Decision::Return {
        close_step(tx, step_id, "returned").await?;
        finalize(tx, &req, "returned", Some(actor)).await?;
        return Ok(req.id);
    }

    let (approved, rejected, total): (i64, i64, i64) = sqlx::query_as(
        "select count(*) filter (where status = 'approved'), count(*) filter (where status = 'rejected'),
                count(*) filter (where status <> 'withdrawn')
         from approval_tasks where step_id = $1",
    )
    .bind(step_id)
    .fetch_one(&mut *tx)
    .await?;
    let outcome = match mode.as_str() {
        "all" => {
            if rejected > 0 {
                Some("rejected")
            } else if approved == total {
                Some("approved")
            } else {
                None
            }
        }
        "quorum" => {
            let need = quorum.unwrap_or(1) as i64;
            if approved >= need {
                Some("approved")
            } else if total - rejected < need {
                Some("rejected")
            } else {
                None
            }
        }
        _ => Some(if decision == Decision::Approve { "approved" } else { "rejected" }),
    };
    let Some(outcome) = outcome else { return Ok(req.id) };
    close_step(tx, step_id, outcome).await?;

    let graph = load_graph(tx, req.workflow_id).await?;
    let next = next_for(&graph, &node_id, outcome);
    if outcome == "rejected" && next.is_empty() {
        finalize(tx, &req, "rejected", Some(actor)).await?;
        return Ok(req.id);
    }
    if next.is_empty() {
        // Nothing downstream (shouldn't pass validation) — treat as done.
        if active_count(tx, &req).await? == 0 {
            finalize(tx, &req, "approved", Some(actor)).await?;
        }
        return Ok(req.id);
    }
    walk(tx, &req, &graph, next).await?;
    Ok(req.id)
}

async fn close_step(tx: &mut PgConnection, step_id: Uuid, status: &str) -> AppResult<()> {
    sqlx::query("update approval_steps set status = $2, completed_at = now() where id = $1")
        .bind(step_id)
        .bind(status)
        .execute(&mut *tx)
        .await?;
    sqlx::query("update approval_tasks set status = 'withdrawn', decided_at = now() where step_id = $1 and status = 'pending'")
        .bind(step_id)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

pub async fn cancel(tx: &mut PgConnection, request_id: Uuid, actor: Uuid) -> AppResult<()> {
    let req = load_request(tx, request_id).await?;
    if req.requester_id != actor {
        return Err(AppError::Forbidden);
    }
    if !matches!(req.status.as_str(), "in_progress" | "returned") {
        return Err(AppError::Conflict("This request is already closed.".into()));
    }
    finalize(tx, &req, "cancelled", Some(actor)).await
}

/// Resubmit a returned request with (optionally) updated data. The workflow
/// version stays pinned to the one the request started on.
pub async fn resubmit(tx: &mut PgConnection, request_id: Uuid, actor: Uuid, data: Option<Value>) -> AppResult<()> {
    let req = load_request(tx, request_id).await?;
    if req.requester_id != actor {
        return Err(AppError::Forbidden);
    }
    if req.status != "returned" {
        return Err(AppError::Conflict("Only requests sent back to you can be resubmitted.".into()));
    }
    sqlx::query(
        "update approval_requests set status = 'in_progress', round = round + 1, data = coalesce($2, data), updated_at = now()
         where id = $1",
    )
    .bind(req.id)
    .bind(&data)
    .execute(&mut *tx)
    .await?;
    if let (Some(d), "form") = (&data, req.subject_type.as_str()) {
        sqlx::query("update form_submissions set data = $2 where id = $1")
            .bind(req.subject_id)
            .bind(d)
            .execute(&mut *tx)
            .await?;
    }
    event(tx, req.id, Some(actor), "resubmitted", None, None, json!({})).await?;
    sync_subject(tx, &req.subject_type, req.subject_id, "in_progress").await?;
    run_from_start(tx, req.id).await
}

/// Hand overdue stages to their escalation role. Called periodically.
pub async fn escalate_overdue(db: &sqlx::PgPool) -> AppResult<u64> {
    let overdue: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "select s.id, s.request_id, s.node_id from approval_steps s
         join approval_requests r on r.id = s.request_id and r.status = 'in_progress'
         where s.status = 'active' and not s.escalated and s.due_at < now()
         limit 100",
    )
    .fetch_all(db)
    .await?;
    let mut n = 0;
    for (step_id, request_id, node_id) in overdue {
        let mut tx = db.begin().await?;
        let req = load_request(&mut tx, request_id).await?;
        let graph = load_graph(&mut tx, req.workflow_id).await?;
        let mut added = 0;
        if let Some(role) = graph.node(&node_id).and_then(|n| n.escalate_to.clone()) {
            let rule = ApproverRule::Role { role, search: "nearest".into() };
            for a in resolve_approvers(&mut tx, &rule, req.org_unit_id, req.requester_id).await? {
                let inserted = sqlx::query(
                    "insert into approval_tasks (step_id, request_id, assignee_id) values ($1, $2, $3) on conflict do nothing",
                )
                .bind(step_id)
                .bind(req.id)
                .bind(a)
                .execute(&mut *tx)
                .await?;
                if inserted.rows_affected() > 0 {
                    added += 1;
                    notify(&mut tx, a, "approval_task", &format!("Escalated to you: {}", req.title), &format!("/approvals/{}", req.id)).await?;
                }
            }
        }
        sqlx::query("update approval_steps set escalated = true where id = $1").bind(step_id).execute(&mut *tx).await?;
        event(&mut tx, req.id, None, "escalated", Some(&node_id), None, json!({ "added": added })).await?;
        tx.commit().await?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions() {
        let ctx = json!({ "days": "4", "type": "Medical", "requester": { "roles": ["student"] } });
        let c: Cond = serde_json::from_value(json!({ "all": [
            { "field": "days", "op": "gt", "value": 3 },
            { "any": [ { "field": "type", "op": "eq", "value": "medical" }, { "field": "type", "op": "eq", "value": "family" } ] },
            { "field": "requester.roles", "op": "contains", "value": "student" }
        ]}))
        .unwrap();
        assert!(c.eval(&ctx));
        let c2: Cond = serde_json::from_value(json!({ "not": { "field": "missing", "op": "exists" } })).unwrap();
        assert!(c2.eval(&ctx));
    }

    #[test]
    fn validation_catches_cycles_and_dangling() {
        let roles: HashSet<String> = ["teacher".to_string()].into();
        let g: Graph = serde_json::from_value(json!({
            "nodes": [
                { "id": "s", "type": "start" },
                { "id": "a", "type": "approval", "approvers": { "kind": "role", "role": "teacher" } },
                { "id": "e", "type": "end", "outcome": "approved" }
            ],
            "edges": [ { "from": "s", "to": "a" }, { "from": "a", "to": "e", "on": "approved" } ]
        }))
        .unwrap();
        assert!(g.validate(&roles).is_ok());

        let bad: Graph = serde_json::from_value(json!({
            "nodes": [
                { "id": "s", "type": "start" },
                { "id": "a", "type": "approval", "approvers": { "kind": "role", "role": "ghost" } },
                { "id": "e", "type": "end", "outcome": "approved" }
            ],
            "edges": [ { "from": "s", "to": "a" }, { "from": "a", "to": "a", "on": "approved" } ]
        }))
        .unwrap();
        let errs = bad.validate(&roles).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("unknown role")));
    }
}
