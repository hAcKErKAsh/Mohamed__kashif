//! Roles, settings and the audit trail.

use axum::{
    extract::{Path, Query, State},
    routing::{get, patch, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    audit,
    auth::Auth,
    authz::{self, Principal},
    error::{AppError, AppResult},
    settings,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/roles", get(list_roles).post(create_role))
        .route("/roles/{id}", patch(update_role).delete(delete_role))
        .route("/settings", get(get_settings))
        .route("/settings/{key}", put(put_setting))
        .route("/audit", get(audit_list))
        .route("/audit/verify", get(audit_verify))
}

async fn require_root(st: &AppState, p: &Principal, perm: &str) -> AppResult<()> {
    let root: String = sqlx::query_scalar("select path::text from org_units where parent_id is null")
        .fetch_one(&st.db)
        .await?;
    p.require_at(perm, &root)
}

async fn list_roles(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    if !a.principal.can_anywhere("admin.roles.manage") && !a.principal.can_anywhere("admin.users.manage") && !a.principal.can_anywhere("admin.workflows.manage") && !a.principal.can_anywhere("admin.forms.manage") {
        return Err(AppError::Forbidden);
    }
    let roles: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object(
               'id', r.id, 'key', r.key, 'name', r.name, 'description', r.description, 'color', r.color,
               'is_system', r.is_system,
               'permissions', coalesce((select jsonb_agg(permission order by permission) from role_permissions where role_id = r.id), '[]'::jsonb),
               'member_count', (select count(distinct user_id) from memberships where role_id = r.id))
           from roles r order by r.is_system desc, r.name"#,
    )
    .fetch_all(&st.db)
    .await?;
    let catalog: Vec<Value> = authz::CATALOG
        .iter()
        .map(|p| json!({ "key": p.key, "group": p.group, "label": p.label }))
        .collect();
    Ok(Json(json!({ "roles": roles, "catalog": catalog })))
}

#[derive(Deserialize)]
struct RoleInput {
    key: Option<String>,
    name: Option<String>,
    description: Option<String>,
    color: Option<String>,
    permissions: Option<Vec<String>>,
}

fn check_perms(perms: &[String]) -> AppResult<()> {
    if let Some(bad) = perms.iter().find(|p| !authz::is_known(p)) {
        return Err(AppError::bad(format!("Unknown permission '{bad}'.")));
    }
    Ok(())
}

fn check_color(c: &str) -> AppResult<()> {
    let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
    if ok { Ok(()) } else { Err(AppError::bad("Colour must look like #2f5d50.")) }
}

async fn create_role(State(st): State<AppState>, a: Auth, Json(b): Json<RoleInput>) -> AppResult<Json<Value>> {
    require_root(&st, &a.principal, "admin.roles.manage").await?;
    let key = b.key.unwrap_or_default().trim().to_lowercase();
    let name = b.name.unwrap_or_default().trim().to_string();
    if name.is_empty() || key.is_empty() {
        return Err(AppError::bad("A role needs a name and a key."));
    }
    let perms = b.permissions.unwrap_or_default();
    check_perms(&perms)?;
    let color = b.color.unwrap_or_else(|| "#5b6b5d".into());
    check_color(&color)?;
    let mut tx = st.db.begin().await?;
    let id: Uuid = sqlx::query_scalar("insert into roles (key, name, description, color) values ($1, $2, $3, $4) returning id")
        .bind(&key)
        .bind(&name)
        .bind(b.description.unwrap_or_default())
        .bind(&color)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref d) if d.is_unique_violation() => AppError::Conflict("A role with that key exists.".into()),
            sqlx::Error::Database(ref d) if d.is_check_violation() => AppError::bad("Key must be lowercase letters, numbers and underscores."),
            other => other.into(),
        })?;
    for p in &perms {
        sqlx::query("insert into role_permissions (role_id, permission) values ($1, $2)")
            .bind(id)
            .bind(p)
            .execute(&mut *tx)
            .await?;
    }
    audit::record(&mut *tx, Some(a.id()), "role.created", "role", Some(id.to_string()), Some(&a.ip), json!({ "key": key, "permissions": perms })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_role(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<RoleInput>) -> AppResult<Json<Value>> {
    require_root(&st, &a.principal, "admin.roles.manage").await?;
    let (key, is_system): (String, bool) = sqlx::query_as("select key, is_system from roles where id = $1")
        .bind(id)
        .fetch_optional(&st.db)
        .await?
        .ok_or(AppError::NotFound)?;
    if let Some(c) = &b.color {
        check_color(c)?;
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("update roles set name = coalesce($2, name), description = coalesce($3, description), color = coalesce($4, color) where id = $1")
        .bind(id)
        .bind(b.name.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .bind(&b.description)
        .bind(&b.color)
        .execute(&mut *tx)
        .await?;
    let mut before: Vec<String> = Vec::new();
    if let Some(perms) = &b.permissions {
        check_perms(perms)?;
        if is_system && key == "administrator" {
            for must in ["admin.roles.manage", "admin.users.manage", "admin.settings.manage"] {
                if !perms.iter().any(|p| p == must) {
                    return Err(AppError::bad(format!("The administrator role must keep '{must}' so nobody gets locked out.")));
                }
            }
        }
        before = sqlx::query_scalar("select permission from role_permissions where role_id = $1 order by 1")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
        sqlx::query("delete from role_permissions where role_id = $1").bind(id).execute(&mut *tx).await?;
        for p in perms {
            sqlx::query("insert into role_permissions (role_id, permission) values ($1, $2) on conflict do nothing")
                .bind(id)
                .bind(p)
                .execute(&mut *tx)
                .await?;
        }
    }
    audit::record(&mut *tx, Some(a.id()), "role.updated", "role", Some(id.to_string()), Some(&a.ip), json!({ "key": key, "before": before, "after": b.permissions })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_role(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    require_root(&st, &a.principal, "admin.roles.manage").await?;
    let (key, is_system, members): (String, bool, i64) = sqlx::query_as(
        "select key, is_system, (select count(*) from memberships where role_id = r.id) from roles r where id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)?;
    if is_system {
        return Err(AppError::bad("Built-in roles can be edited but not deleted."));
    }
    if members > 0 {
        return Err(AppError::Conflict(format!("{members} people still hold this role.")));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("delete from roles where id = $1").bind(id).execute(&mut *tx).await?;
    audit::record(&mut *tx, Some(a.id()), "role.deleted", "role", Some(id.to_string()), Some(&a.ip), json!({ "key": key })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn get_settings(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.settings.manage")?;
    Ok(Json(Value::Object(settings::all(&st.db).await?)))
}

async fn put_setting(State(st): State<AppState>, a: Auth, Path(key): Path<String>, Json(value): Json<Value>) -> AppResult<Json<Value>> {
    require_root(&st, &a.principal, "admin.settings.manage").await?;
    if !settings::KEYS.contains(&key.as_str()) {
        return Err(AppError::NotFound);
    }
    validate_setting(&key, &value)?;
    let before = settings::get(&st.db, &key).await?;
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "insert into settings (key, value, updated_by, updated_at) values ($1, $2, $3, now())
         on conflict (key) do update set value = excluded.value, updated_by = excluded.updated_by, updated_at = now()",
    )
    .bind(&key)
    .bind(&value)
    .bind(a.id())
    .execute(&mut *tx)
    .await?;
    audit::record(&mut *tx, Some(a.id()), "settings.updated", "setting", Some(key.clone()), Some(&a.ip), json!({ "before": before, "after": value })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

fn validate_setting(key: &str, v: &Value) -> AppResult<()> {
    match key {
        "security" => {
            let p: settings::SecurityPolicy =
                serde_json::from_value(v.clone()).map_err(|_| AppError::bad("Security policy is malformed."))?;
            if p.password_min_length < 8 || p.password_min_length > 128 {
                return Err(AppError::bad("Minimum password length must be between 8 and 128."));
            }
            if !(3..=20).contains(&p.lockout_threshold) {
                return Err(AppError::bad("Lockout threshold must be between 3 and 20 attempts."));
            }
            if !(5..=24 * 60).contains(&p.session_idle_minutes) || !(1..=24 * 30).contains(&p.session_absolute_hours) {
                return Err(AppError::bad("Session limits are out of range."));
            }
        }
        "branding" => {
            if let Some(c) = v.get("accent").and_then(Value::as_str) {
                check_color(c)?;
            }
        }
        "modules" | "content_policy" => {
            if !v.is_object() {
                return Err(AppError::bad("Expected an object."));
            }
        }
        "profile_fields" => crate::routes::forms::validate_fields(v)?,
        "org_kinds" => {
            if !v.as_array().is_some_and(|a| a.iter().all(|x| x.as_str().is_some_and(|s| !s.is_empty()))) {
                return Err(AppError::bad("Unit kinds must be a list of names."));
            }
        }
        _ => {}
    }
    Ok(())
}

#[derive(Deserialize)]
struct AuditQuery {
    before: Option<i64>,
    action: Option<String>,
    actor: Option<Uuid>,
    entity: Option<String>,
}

async fn audit_list(State(st): State<AppState>, a: Auth, Query(q): Query<AuditQuery>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.audit.view")?;
    let rows: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('seq', l.seq, 'at', l.at, 'action', l.action, 'entity_type', l.entity_type,
                                     'entity_id', l.entity_id, 'ip', l.ip, 'detail', l.detail,
                                     'actor_id', l.actor_id, 'actor_name', u.full_name,
                                     'hash', encode(l.hash, 'hex'))
           from audit_log l left join users u on u.id = l.actor_id
           where ($1::bigint is null or l.seq < $1)
             and ($2::text is null or l.action like $2 || '%')
             and ($3::uuid is null or l.actor_id = $3)
             and ($4::text is null or l.entity_type = $4)
           order by l.seq desc limit 100"#,
    )
    .bind(q.before)
    .bind(q.action.filter(|s| !s.is_empty()))
    .bind(q.actor)
    .bind(q.entity.filter(|s| !s.is_empty()))
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

async fn audit_verify(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.audit.view")?;
    let (count, broken) = audit::verify_chain(&st.db).await?;
    audit::record(&st.db, Some(a.id()), "audit.verified", "audit_log", None, Some(&a.ip), json!({ "rows": count, "intact": broken.is_none() })).await?;
    Ok(Json(json!({ "rows": count, "intact": broken.is_none(), "first_broken_seq": broken })))
}
