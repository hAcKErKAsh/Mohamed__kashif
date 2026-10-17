//! Announcements (content management), the community feed, the directory,
//! notifications and the dashboard.
//!
//! Audience rule shared by content and posts: an item addressed to a unit is
//! visible to anyone whose grant (with the view permission) sits on the same
//! branch of the tree — at that unit, above it, or below it. A notice for
//! "Grade 4" reaches Grade 4's sections and the headmaster, but not Grade 9.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    audit,
    auth::Auth,
    authz::unit_path,
    engine,
    error::{AppError, AppResult},
    settings::{self, ContentPolicy},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/content", get(list_content).post(create_content))
        .route("/content/mine", get(my_content))
        .route("/content/{id}", get(get_content).put(update_content))
        .route("/content/{id}/submit", post(submit_content))
        .route("/content/{id}/archive", post(archive_content))
        .route("/feed", get(feed).post(create_post))
        .route("/feed/{id}/react", post(react))
        .route("/feed/{id}/comments", get(comments).post(add_comment))
        .route("/feed/{id}/remove", post(remove_post))
        .route("/feed/comments/{id}/remove", post(remove_comment))
        .route("/people", get(people))
        .route("/notifications", get(notifications))
        .route("/notifications/read", post(mark_read))
        .route("/dashboard", get(dashboard))
        .route("/settings/public", get(public_settings))
        .route("/me/profile", put(update_my_profile))
}

const AUDIENCE: &str = "exists (select 1 from unnest($1::ltree[]) p where p @> o.path or o.path @> p)";

async fn require_module(st: &AppState, m: &str) -> AppResult<()> {
    if settings::module_enabled(&st.db, m).await? { Ok(()) } else { Err(AppError::NotFound) }
}

// ---------------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ContentQuery {
    kind: Option<String>,
    limit: Option<i64>,
}

const CONTENT_JSON: &str = r#"jsonb_build_object(
    'id', c.id, 'kind', c.kind, 'title', c.title, 'summary', c.summary, 'status', c.status, 'pinned', c.pinned,
    'unit', o.name, 'unit_id', o.id, 'author', u.full_name, 'author_id', u.id, 'published_at', c.published_at,
    'updated_at', c.updated_at, 'created_at', c.created_at, 'approval_request_id', c.approval_request_id)"#;

async fn list_content(State(st): State<AppState>, a: Auth, Query(q): Query<ContentQuery>) -> AppResult<Json<Value>> {
    require_module(&st, "content").await?;
    let scope = a.principal.scope_paths("content.view");
    let rows: Vec<Value> = sqlx::query_scalar(&format!(
        "select {CONTENT_JSON} from content_items c join org_units o on o.id = c.org_unit_id join users u on u.id = c.author_id
         where c.status = 'published' and {AUDIENCE} and ($2::text is null or c.kind = $2)
         order by c.pinned desc, c.published_at desc limit $3"
    ))
    .bind(&scope)
    .bind(q.kind.filter(|s| !s.is_empty()))
    .bind(q.limit.unwrap_or(50).clamp(1, 200))
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

async fn my_content(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    require_module(&st, "content").await?;
    let rows: Vec<Value> = sqlx::query_scalar(&format!(
        "select {CONTENT_JSON} from content_items c join org_units o on o.id = c.org_unit_id join users u on u.id = c.author_id
         where c.author_id = $1 order by c.updated_at desc limit 200"
    ))
    .bind(a.id())
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

async fn get_content(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    require_module(&st, "content").await?;
    let row: Option<(Value, String, String, Uuid)> = sqlx::query_as(&format!(
        "select {CONTENT_JSON} || jsonb_build_object('body', c.body), c.status, o.path::text, c.author_id
         from content_items c join org_units o on o.id = c.org_unit_id join users u on u.id = c.author_id where c.id = $1"
    ))
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let (item, status, path, author) = row.ok_or(AppError::NotFound)?;
    let visible = if status == "published" {
        a.principal
            .scope_paths("content.view")
            .iter()
            .any(|p| crate::authz::path_covers(p, &path) || crate::authz::path_covers(&path, p))
    } else {
        author == a.id() || a.principal.can_at("content.publish", &path)
    };
    if !visible {
        return Err(AppError::NotFound);
    }
    Ok(Json(item))
}

#[derive(Deserialize)]
struct ContentInput {
    kind: String,
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    body: String,
    org_unit_id: Uuid,
    #[serde(default)]
    pinned: bool,
}

fn check_content(b: &ContentInput) -> AppResult<()> {
    if !matches!(b.kind.as_str(), "announcement" | "circular" | "page") {
        return Err(AppError::bad("Unknown content type."));
    }
    if b.title.trim().is_empty() || b.title.len() > 200 {
        return Err(AppError::bad("Give it a title (up to 200 characters)."));
    }
    if b.body.len() > 100_000 || b.summary.len() > 500 {
        return Err(AppError::bad("That's too long."));
    }
    Ok(())
}

async fn create_content(State(st): State<AppState>, a: Auth, Json(b): Json<ContentInput>) -> AppResult<Json<Value>> {
    require_module(&st, "content").await?;
    check_content(&b)?;
    let path = unit_path(&st.db, b.org_unit_id).await?;
    a.principal.require_at("content.create", &path)?;
    let id: Uuid = sqlx::query_scalar(
        "insert into content_items (kind, title, summary, body, org_unit_id, author_id, pinned) values ($1, $2, $3, $4, $5, $6, $7) returning id",
    )
    .bind(&b.kind)
    .bind(b.title.trim())
    .bind(b.summary.trim())
    .bind(&b.body)
    .bind(b.org_unit_id)
    .bind(a.id())
    .bind(b.pinned && a.principal.can_at("content.publish", &path))
    .fetch_one(&st.db)
    .await?;
    audit::record(&st.db, Some(a.id()), "content.drafted", "content", Some(id.to_string()), Some(&a.ip), json!({ "title": b.title })).await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_content(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<ContentInput>) -> AppResult<Json<Value>> {
    require_module(&st, "content").await?;
    check_content(&b)?;
    let (author, status): (Uuid, String) = sqlx::query_as("select author_id, status from content_items where id = $1")
        .bind(id)
        .fetch_optional(&st.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let path = unit_path(&st.db, b.org_unit_id).await?;
    let publisher = a.principal.can_at("content.publish", &path);
    if author != a.id() && !publisher {
        return Err(AppError::Forbidden);
    }
    a.principal.require_at("content.create", &path)?;
    if status == "in_review" {
        return Err(AppError::Conflict("It's being reviewed. Cancel the review to edit it.".into()));
    }
    // Editing published content without publish rights sends it back to draft.
    let next_status = if status == "published" && !publisher { "draft" } else if status == "rejected" { "draft" } else { status.as_str() };
    sqlx::query(
        "update content_items set kind = $2, title = $3, summary = $4, body = $5, org_unit_id = $6, pinned = $7, status = $8, updated_at = now() where id = $1",
    )
    .bind(id)
    .bind(&b.kind)
    .bind(b.title.trim())
    .bind(b.summary.trim())
    .bind(&b.body)
    .bind(b.org_unit_id)
    .bind(b.pinned && publisher)
    .bind(next_status)
    .execute(&st.db)
    .await?;
    audit::record(&st.db, Some(a.id()), "content.edited", "content", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    Ok(Json(json!({ "ok": true, "status": next_status })))
}

async fn submit_content(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    require_module(&st, "content").await?;
    let (author, status, unit, title, kind, path, unit_kind, unit_name): (Uuid, String, Uuid, String, String, String, String, String) = sqlx::query_as(
        "select c.author_id, c.status, c.org_unit_id, c.title, c.kind, o.path::text, o.kind, o.name
         from content_items c join org_units o on o.id = c.org_unit_id where c.id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)?;
    if author != a.id() && !a.principal.can_at("content.publish", &path) {
        return Err(AppError::Forbidden);
    }
    if !matches!(status.as_str(), "draft" | "rejected") {
        return Err(AppError::Conflict("Only drafts can be submitted.".into()));
    }
    let mut tx = st.db.begin().await?;
    if a.principal.can_at("content.publish", &path) {
        sqlx::query("update content_items set status = 'published', published_at = now(), updated_at = now() where id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        audit::record(&mut *tx, Some(a.id()), "content.published", "content", Some(id.to_string()), Some(&a.ip), json!({})).await?;
        tx.commit().await?;
        return Ok(Json(json!({ "status": "published" })));
    }
    let policy: ContentPolicy = settings::get_typed(&st.db, "content_policy").await?;
    let wf = engine::published_workflow(&mut tx, &policy.content_workflow_key)
        .await?
        .ok_or_else(|| AppError::bad("Publishing needs an approval workflow, and none is published yet."))?;
    let data = json!({ "kind": kind, "audience_kind": unit_kind, "audience": unit_name, "title": title });
    let rid = engine::start(&mut tx, wf, "content", id, &format!("Publish: {title}"), data, a.id(), unit).await?;
    sqlx::query("update content_items set approval_request_id = $2 where id = $1").bind(id).bind(rid).execute(&mut *tx).await?;
    audit::record(&mut *tx, Some(a.id()), "content.submitted", "content", Some(id.to_string()), Some(&a.ip), json!({ "request_id": rid })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "status": "in_review", "request_id": rid })))
}

async fn archive_content(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let (author, path): (Uuid, String) = sqlx::query_as(
        "select c.author_id, o.path::text from content_items c join org_units o on o.id = c.org_unit_id where c.id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)?;
    if author != a.id() && !a.principal.can_at("content.publish", &path) {
        return Err(AppError::Forbidden);
    }
    sqlx::query("update content_items set status = 'archived', pinned = false, updated_at = now() where id = $1 and status <> 'in_review'")
        .bind(id)
        .execute(&st.db)
        .await?;
    audit::record(&st.db, Some(a.id()), "content.archived", "content", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Community feed
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct FeedQuery {
    before: Option<chrono::DateTime<chrono::Utc>>,
    unit: Option<Uuid>,
}

async fn feed(State(st): State<AppState>, a: Auth, Query(q): Query<FeedQuery>) -> AppResult<Json<Value>> {
    require_module(&st, "social").await?;
    let scope = a.principal.scope_paths("social.view");
    let unit_path = match q.unit {
        Some(u) => Some(unit_path(&st.db, u).await?),
        None => None,
    };
    let moderator_scope = a.principal.scope_paths("social.moderate");
    let rows: Vec<Value> = sqlx::query_scalar(&format!(
        r#"select jsonb_build_object(
               'id', p.id, 'body', p.body, 'status', p.status, 'created_at', p.created_at,
               'author', u.full_name, 'author_id', u.id,
               'author_role', (select r.name from memberships m join roles r on r.id = m.role_id
                               join org_units mo on mo.id = m.org_unit_id
                               where m.user_id = u.id order by nlevel(mo.path) limit 1),
               'unit', o.name, 'unit_id', o.id,
               'reactions', (select coalesce(jsonb_object_agg(kind, n), '{{}}'::jsonb) from
                               (select kind, count(*) n from post_reactions where post_id = p.id group by kind) x),
               'my_reaction', (select kind from post_reactions where post_id = p.id and user_id = $2),
               'comment_count', (select count(*) from post_comments c where c.post_id = p.id and not c.removed),
               'can_moderate', o.path <@ $5::ltree[])
           from posts p join org_units o on o.id = p.org_unit_id join users u on u.id = p.author_id
           where (p.status = 'published' or (p.author_id = $2 and p.status = 'pending'))
             and {AUDIENCE}
             and ($3::timestamptz is null or p.created_at < $3)
             and ($4::ltree is null or o.path <@ $4::ltree)
           order by p.created_at desc limit 30"#
    ))
    .bind(&scope)
    .bind(a.id())
    .bind(q.before)
    .bind(unit_path)
    .bind(&moderator_scope)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

#[derive(Deserialize)]
struct PostInput {
    body: String,
    org_unit_id: Uuid,
}

async fn create_post(State(st): State<AppState>, a: Auth, Json(b): Json<PostInput>) -> AppResult<Json<Value>> {
    require_module(&st, "social").await?;
    st.limiter.hit(&format!("post:{}", a.id()), 20, std::time::Duration::from_secs(600))?;
    let body = b.body.trim();
    if body.is_empty() || body.chars().count() > 5000 {
        return Err(AppError::bad("Posts need between 1 and 5000 characters."));
    }
    let (path, unit_kind, unit_name): (String, String, String) =
        sqlx::query_as("select path::text, kind, name from org_units where id = $1")
            .bind(b.org_unit_id)
            .fetch_optional(&st.db)
            .await?
            .ok_or(AppError::NotFound)?;
    a.principal.require_at("social.post", &path)?;
    let direct = a.principal.can_at("social.post_direct", &path);
    let mut tx = st.db.begin().await?;
    let id: Uuid = sqlx::query_scalar("insert into posts (author_id, org_unit_id, body, status) values ($1, $2, $3, $4) returning id")
        .bind(a.id())
        .bind(b.org_unit_id)
        .bind(body)
        .bind(if direct { "published" } else { "pending" })
        .fetch_one(&mut *tx)
        .await?;
    let mut status = "published";
    if !direct {
        let policy: ContentPolicy = settings::get_typed(&st.db, "content_policy").await?;
        match engine::published_workflow(&mut tx, &policy.post_workflow_key).await? {
            Some(wf) => {
                let preview: String = body.chars().take(60).collect();
                let data = json!({ "audience_kind": unit_kind, "audience": unit_name, "length": body.chars().count() });
                let rid = engine::start(&mut tx, wf, "post", id, &format!("Post: {preview}"), data, a.id(), b.org_unit_id).await?;
                sqlx::query("update posts set approval_request_id = $2 where id = $1").bind(id).bind(rid).execute(&mut *tx).await?;
                status = "pending";
            }
            None => return Err(AppError::bad("Your posts need moderation, but no moderation workflow is published.")),
        }
    }
    audit::record(&mut *tx, Some(a.id()), "post.created", "post", Some(id.to_string()), Some(&a.ip), json!({ "status": status })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": id, "status": status })))
}

async fn visible_post(st: &AppState, a: &Auth, id: Uuid) -> AppResult<(String, Uuid, String)> {
    let (path, author, status): (String, Uuid, String) = sqlx::query_as(
        "select o.path::text, p.author_id, p.status from posts p join org_units o on o.id = p.org_unit_id where p.id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let in_scope = a
        .principal
        .scope_paths("social.view")
        .iter()
        .any(|p| crate::authz::path_covers(p, &path) || crate::authz::path_covers(&path, p));
    if !in_scope || (status != "published" && author != a.id()) {
        return Err(AppError::NotFound);
    }
    Ok((path, author, status))
}

#[derive(Deserialize)]
struct ReactInput {
    kind: Option<String>,
}

async fn react(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<ReactInput>) -> AppResult<Json<Value>> {
    let (path, _, status) = visible_post(&st, &a, id).await?;
    a.principal.require_at("social.comment", &path).or_else(|_| {
        // Reacting is allowed wherever commenting is allowed on any ancestor branch.
        if a.principal.can_anywhere("social.comment") { Ok(()) } else { Err(AppError::Forbidden) }
    })?;
    if status != "published" {
        return Err(AppError::bad("You can react once the post is published."));
    }
    match b.kind.as_deref() {
        None | Some("") => {
            sqlx::query("delete from post_reactions where post_id = $1 and user_id = $2").bind(id).bind(a.id()).execute(&st.db).await?;
        }
        Some(k) => {
            if !matches!(k, "appreciate" | "celebrate" | "insightful") {
                return Err(AppError::bad("Unknown reaction."));
            }
            sqlx::query(
                "insert into post_reactions (post_id, user_id, kind) values ($1, $2, $3)
                 on conflict (post_id, user_id) do update set kind = excluded.kind, created_at = now()",
            )
            .bind(id)
            .bind(a.id())
            .bind(k)
            .execute(&st.db)
            .await?;
        }
    }
    Ok(Json(json!({ "ok": true })))
}

async fn comments(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let (path, _, _) = visible_post(&st, &a, id).await?;
    let moderator = a.principal.can_at("social.moderate", &path);
    let rows: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('id', c.id, 'body', c.body, 'author', u.full_name, 'author_id', u.id,
                                     'created_at', c.created_at, 'can_remove', c.author_id = $2 or $3)
           from post_comments c join users u on u.id = c.author_id
           where c.post_id = $1 and not c.removed order by c.created_at"#,
    )
    .bind(id)
    .bind(a.id())
    .bind(moderator)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

#[derive(Deserialize)]
struct CommentInput {
    body: String,
}

async fn add_comment(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<CommentInput>) -> AppResult<Json<Value>> {
    st.limiter.hit(&format!("comment:{}", a.id()), 60, std::time::Duration::from_secs(600))?;
    let (_, author, status) = visible_post(&st, &a, id).await?;
    a.principal.require_anywhere("social.comment")?;
    if status != "published" {
        return Err(AppError::bad("Comments open once the post is published."));
    }
    let body = b.body.trim();
    if body.is_empty() || body.chars().count() > 2000 {
        return Err(AppError::bad("Comments need between 1 and 2000 characters."));
    }
    let mut tx = st.db.begin().await?;
    let cid: Uuid = sqlx::query_scalar("insert into post_comments (post_id, author_id, body) values ($1, $2, $3) returning id")
        .bind(id)
        .bind(a.id())
        .bind(body)
        .fetch_one(&mut *tx)
        .await?;
    if author != a.id() {
        engine::notify(&mut tx, author, "comment", &format!("{} commented on your post", a.user.full_name), &format!("/feed#{id}")).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "id": cid })))
}

async fn remove_post(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let (path, author, _) = visible_post(&st, &a, id).await?;
    let moderator = a.principal.can_at("social.moderate", &path);
    if author != a.id() && !moderator {
        return Err(AppError::Forbidden);
    }
    sqlx::query("update posts set status = 'removed' where id = $1").bind(id).execute(&st.db).await?;
    audit::record(&st.db, Some(a.id()), if author == a.id() { "post.deleted" } else { "post.moderated" }, "post", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn remove_comment(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let (author, path): (Uuid, String) = sqlx::query_as(
        "select c.author_id, o.path::text from post_comments c join posts p on p.id = c.post_id
         join org_units o on o.id = p.org_unit_id where c.id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)?;
    if author != a.id() && !a.principal.can_at("social.moderate", &path) {
        return Err(AppError::Forbidden);
    }
    sqlx::query("update post_comments set removed = true where id = $1").bind(id).execute(&st.db).await?;
    if author != a.id() {
        audit::record(&st.db, Some(a.id()), "comment.moderated", "comment", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Directory, notifications, dashboard
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PeopleQuery {
    q: Option<String>,
    role: Option<String>,
}

async fn people(State(st): State<AppState>, a: Auth, Query(q): Query<PeopleQuery>) -> AppResult<Json<Value>> {
    require_module(&st, "directory").await?;
    a.principal.require_anywhere("people.view")?;
    let scope = a.principal.scope_paths("people.view");
    let rows: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('id', u.id, 'full_name', u.full_name, 'email', u.email,
               'roles', jsonb_agg(distinct jsonb_build_object('role', r.name, 'color', r.color, 'unit', o.name)))
           from users u join memberships m on m.user_id = u.id join roles r on r.id = m.role_id
           join org_units o on o.id = m.org_unit_id
           where u.status = 'active' and o.path <@ $1::ltree[]
             and ($2::text is null or u.full_name ilike '%' || $2 || '%')
             and ($3::text is null or r.key = $3)
           group by u.id order by u.full_name limit 300"#,
    )
    .bind(&scope)
    .bind(q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(q.role.filter(|s| !s.is_empty()))
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

async fn notifications(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    let rows: Vec<Value> = sqlx::query_scalar(
        "select jsonb_build_object('id', id, 'kind', kind, 'title', title, 'link', link, 'read', read_at is not null, 'created_at', created_at)
         from notifications where user_id = $1 order by created_at desc limit 50",
    )
    .bind(a.id())
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

async fn mark_read(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    sqlx::query("update notifications set read_at = now() where user_id = $1 and read_at is null")
        .bind(a.id())
        .execute(&st.db)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

async fn dashboard(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    let pending_tasks: i64 = sqlx::query_scalar(
        "select count(*) from approval_tasks t join approval_steps s on s.id = t.step_id
         where t.assignee_id = $1 and t.status = 'pending' and s.status = 'active'",
    )
    .bind(a.id())
    .fetch_one(&st.db)
    .await?;
    let overdue_tasks: i64 = sqlx::query_scalar(
        "select count(*) from approval_tasks t join approval_steps s on s.id = t.step_id
         where t.assignee_id = $1 and t.status = 'pending' and s.status = 'active' and s.due_at < now()",
    )
    .bind(a.id())
    .fetch_one(&st.db)
    .await?;
    let open_requests: i64 = sqlx::query_scalar(
        "select count(*) from approval_requests where requester_id = $1 and status in ('in_progress', 'returned')",
    )
    .bind(a.id())
    .fetch_one(&st.db)
    .await?;
    let returned: i64 = sqlx::query_scalar("select count(*) from approval_requests where requester_id = $1 and status = 'returned'")
        .bind(a.id())
        .fetch_one(&st.db)
        .await?;
    let content_scope = a.principal.scope_paths("content.view");
    let announcements: Vec<Value> = sqlx::query_scalar(&format!(
        "select {CONTENT_JSON} from content_items c join org_units o on o.id = c.org_unit_id join users u on u.id = c.author_id
         where c.status = 'published' and {AUDIENCE} order by c.pinned desc, c.published_at desc limit 4"
    ))
    .bind(&content_scope)
    .fetch_all(&st.db)
    .await?;
    let mut admin = Value::Null;
    if a.principal.can_anywhere("admin.audit.view") || a.principal.can_anywhere("admin.users.manage") {
        let (users, sessions, in_flight, failed_logins, locked): (i64, i64, i64, i64, i64) = sqlx::query_as(
            "select (select count(*) from users where status = 'active'),
                    (select count(distinct user_id) from sessions where revoked_at is null and expires_at > now() and last_seen_at > now() - interval '1 hour'),
                    (select count(*) from approval_requests where status = 'in_progress'),
                    (select count(*) from audit_log where action in ('auth.login_failed', 'auth.mfa_failed') and at > now() - interval '24 hours'),
                    (select count(*) from users where locked_until > now())",
        )
        .fetch_one(&st.db)
        .await?;
        let by_day: Vec<Value> = sqlx::query_scalar(
            "select jsonb_build_object('day', d::date, 'requests', (select count(*) from approval_requests where created_at::date = d::date))
             from generate_series(current_date - 13, current_date, interval '1 day') d order by d",
        )
        .fetch_all(&st.db)
        .await?;
        admin = json!({ "active_users": users, "online_users": sessions, "requests_in_flight": in_flight,
                        "failed_logins_24h": failed_logins, "locked_accounts": locked, "requests_by_day": by_day });
    }
    Ok(Json(json!({
        "pending_tasks": pending_tasks, "overdue_tasks": overdue_tasks,
        "open_requests": open_requests, "returned_requests": returned,
        "announcements": announcements, "admin": admin
    })))
}

async fn public_settings(State(st): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "branding": settings::get(&st.db, "branding").await? })))
}

#[derive(Deserialize)]
struct ProfileInput {
    profile: Value,
}

/// People can fill in the custom profile fields the school defines.
async fn update_my_profile(State(st): State<AppState>, a: Auth, Json(b): Json<ProfileInput>) -> AppResult<Json<Value>> {
    let fields = settings::get(&st.db, "profile_fields").await?;
    let cleaned = crate::routes::forms::validate_submission(&fields, &b.profile)?;
    sqlx::query("update users set profile = profile || $2, updated_at = now() where id = $1")
        .bind(a.id())
        .bind(&cleaned)
        .execute(&st.db)
        .await?;
    Ok(Json(json!({ "ok": true })))
}
