use axum::{
    extract::{Path, State},
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    audit,
    auth::Auth,
    authz::unit_path,
    error::{AppError, AppResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tree", get(tree))
        .route("/units", post(create_unit))
        .route("/units/{id}", patch(update_unit).delete(delete_unit))
        .route("/units/{id}/move", post(move_unit))
}

#[derive(sqlx::FromRow, serde::Serialize)]
struct UnitRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    kind: String,
    name: String,
    code: String,
    path: String,
    depth: i32,
    sort_order: i32,
    member_count: i64,
}

async fn tree(State(st): State<AppState>, _a: Auth) -> AppResult<Json<Value>> {
    let rows: Vec<UnitRow> = sqlx::query_as(
        r#"select o.id, o.parent_id, o.kind, o.name, o.code, o.path::text as path, nlevel(o.path) as depth,
                  o.sort_order, (select count(distinct m.user_id) from memberships m where m.org_unit_id = o.id) as member_count
           from org_units o
           order by o.path"#,
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

#[derive(Deserialize)]
struct CreateUnit {
    parent_id: Uuid,
    kind: String,
    name: String,
    code: String,
}

fn label_for(id: Uuid) -> String {
    format!("u{}", id.simple())
}

fn clean_code(code: &str) -> AppResult<String> {
    let c = code.trim().to_string();
    if c.is_empty() || c.len() > 32 || !c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
        return Err(AppError::bad("Code may use letters, numbers, dashes and underscores (max 32)."));
    }
    Ok(c)
}

async fn create_unit(State(st): State<AppState>, a: Auth, Json(b): Json<CreateUnit>) -> AppResult<Json<Value>> {
    let parent_path = unit_path(&st.db, b.parent_id).await?;
    a.principal.require_at("admin.org.manage", &parent_path)?;
    let name = b.name.trim();
    if name.is_empty() {
        return Err(AppError::bad("Give the unit a name."));
    }
    let code = clean_code(&b.code)?;
    let id = Uuid::new_v4();
    let path = format!("{parent_path}.{}", label_for(id));
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "insert into org_units (id, parent_id, kind, name, code, path, sort_order)
         values ($1, $2, $3, $4, $5, $6::ltree, (select coalesce(max(sort_order), 0) + 1 from org_units where parent_id = $2))",
    )
    .bind(id)
    .bind(b.parent_id)
    .bind(b.kind.trim())
    .bind(name)
    .bind(&code)
    .bind(&path)
    .execute(&mut *tx)
    .await?;
    audit::record(&mut *tx, Some(a.id()), "org.unit_created", "org_unit", Some(id.to_string()), Some(&a.ip), json!({ "name": name, "parent_id": b.parent_id })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
struct UpdateUnit {
    name: Option<String>,
    kind: Option<String>,
    code: Option<String>,
    sort_order: Option<i32>,
}

async fn update_unit(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<UpdateUnit>) -> AppResult<Json<Value>> {
    let path = unit_path(&st.db, id).await?;
    a.principal.require_at("admin.org.manage", &path)?;
    let code = b.code.as_deref().map(clean_code).transpose()?;
    sqlx::query(
        "update org_units set name = coalesce($2, name), kind = coalesce($3, kind), code = coalesce($4, code),
                              sort_order = coalesce($5, sort_order)
         where id = $1",
    )
    .bind(id)
    .bind(b.name.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(b.kind.as_deref().map(str::trim))
    .bind(code)
    .bind(b.sort_order)
    .execute(&st.db)
    .await?;
    audit::record(&st.db, Some(a.id()), "org.unit_updated", "org_unit", Some(id.to_string()), Some(&a.ip), json!({ "name": b.name, "kind": b.kind })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct MoveUnit {
    parent_id: Uuid,
}

async fn move_unit(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<MoveUnit>) -> AppResult<Json<Value>> {
    let old_path = unit_path(&st.db, id).await?;
    let new_parent = unit_path(&st.db, b.parent_id).await?;
    a.principal.require_at("admin.org.manage", &old_path)?;
    a.principal.require_at("admin.org.manage", &new_parent)?;
    if crate::authz::path_covers(&old_path, &new_parent) {
        return Err(AppError::bad("A unit can't be moved inside itself."));
    }
    if !old_path.contains('.') {
        return Err(AppError::bad("The root can't be moved."));
    }
    let new_path = format!("{new_parent}.{}", label_for(id));
    let mut tx = st.db.begin().await?;
    // Rewrite the prefix of the whole subtree in one statement.
    sqlx::query(
        "update org_units set path = $2::ltree || subpath(path, nlevel($1::ltree))
         where path <@ $1::ltree",
    )
    .bind(&old_path)
    .bind(&new_path)
    .execute(&mut *tx)
    .await?;
    sqlx::query("update org_units set parent_id = $2 where id = $1")
        .bind(id)
        .bind(b.parent_id)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut *tx, Some(a.id()), "org.unit_moved", "org_unit", Some(id.to_string()), Some(&a.ip), json!({ "parent_id": b.parent_id })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_unit(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let path = unit_path(&st.db, id).await?;
    a.principal.require_at("admin.org.manage", &path)?;
    let (children, members, used): (i64, i64, bool) = sqlx::query_as(
        "select (select count(*) from org_units where parent_id = $1),
                (select count(*) from memberships where org_unit_id = $1),
                exists(select 1 from approval_requests where org_unit_id = $1)
                  or exists(select 1 from content_items where org_unit_id = $1)
                  or exists(select 1 from form_submissions where org_unit_id = $1)",
    )
    .bind(id)
    .fetch_one(&st.db)
    .await?;
    if children > 0 || members > 0 || used {
        return Err(AppError::Conflict(
            "This unit still has sub-units, members or records. Move or remove them first.".into(),
        ));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("delete from org_units where id = $1").bind(id).execute(&mut *tx).await?;
    audit::record(&mut *tx, Some(a.id()), "org.unit_deleted", "org_unit", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}
