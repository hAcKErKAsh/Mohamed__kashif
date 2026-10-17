//! Identity administration: the security console's people screens.

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    audit,
    auth::{self, Auth},
    authz::{unit_path, Principal},
    error::{AppError, AppResult},
    settings::{self, SecurityPolicy},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", get(get_user).patch(update_user))
        .route("/users/{id}/reset-password", post(reset_password))
        .route("/users/{id}/unlock", post(unlock))
        .route("/users/{id}/revoke-sessions", post(revoke_sessions))
        .route("/users/{id}/memberships", post(add_membership))
        .route("/memberships/{id}", delete(remove_membership))
        .route("/users/{id}/guardians", post(add_guardian))
        .route("/users/{id}/guardians/{guardian_id}", delete(remove_guardian))
        .route("/sessions", get(all_sessions))
        .route("/sessions/{id}", delete(revoke_session))
}

#[derive(Deserialize)]
struct ListQuery {
    q: Option<String>,
    role: Option<String>,
    unit: Option<Uuid>,
    status: Option<String>,
}

async fn list_users(State(st): State<AppState>, a: Auth, Query(q): Query<ListQuery>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.users.manage")?;
    let scope = a.principal.scope_paths("admin.users.manage");
    let unit_path = match q.unit {
        Some(u) => Some(unit_path(&st.db, u).await?),
        None => None,
    };
    let rows: Vec<(Uuid, String, String, String, Option<DateTime<Utc>>, Option<DateTime<Utc>>, bool, Value)> = sqlx::query_as(
        r#"select u.id, u.email::text, u.full_name, u.status, u.last_login_at, u.locked_until, u.totp_enabled,
                  coalesce(jsonb_agg(jsonb_build_object(
                      'membership_id', m.id, 'role_key', r.key, 'role_name', r.name, 'role_color', r.color,
                      'unit_id', o.id, 'unit_name', o.name)
                      order by nlevel(o.path)) filter (where m.id is not null), '[]'::jsonb)
           from users u
           left join memberships m on m.user_id = u.id
           left join roles r on r.id = m.role_id
           left join org_units o on o.id = m.org_unit_id
           where ($1::text is null or u.full_name ilike '%' || $1 || '%' or u.email::text ilike '%' || $1 || '%')
             and ($4::text is null or u.status = $4)
             and (
               not exists (select 1 from memberships mm where mm.user_id = u.id)
               or exists (select 1 from memberships mm join org_units oo on oo.id = mm.org_unit_id
                          where mm.user_id = u.id and oo.path <@ $2::ltree[])
             )
             and ($3::text is null or exists (select 1 from memberships mm join roles rr on rr.id = mm.role_id
                                              where mm.user_id = u.id and rr.key = $3))
             and ($5::ltree is null or exists (select 1 from memberships mm join org_units oo on oo.id = mm.org_unit_id
                                               where mm.user_id = u.id and oo.path <@ $5::ltree))
           group by u.id
           order by u.full_name
           limit 500"#,
    )
    .bind(q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(&scope)
    .bind(q.role.as_deref().filter(|s| !s.is_empty()))
    .bind(q.status.as_deref().filter(|s| !s.is_empty()))
    .bind(unit_path)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|(id, email, name, status, last, locked, mfa, memberships)| json!({
            "id": id, "email": email, "full_name": name, "status": status, "last_login_at": last,
            "locked": locked.is_some_and(|t| t > Utc::now()), "totp_enabled": mfa, "memberships": memberships
        }))
        .collect::<Vec<_>>())))
}

/// Granting a role is only allowed if the granter already holds every one of
/// its permissions at that unit — no one can mint more power than they have.
async fn ensure_can_grant(st: &AppState, p: &Principal, role_id: Uuid, unit_id: Uuid) -> AppResult<String> {
    let path = unit_path(&st.db, unit_id).await?;
    p.require_at("admin.users.manage", &path)?;
    let perms: Vec<String> = sqlx::query_scalar("select permission from role_permissions where role_id = $1")
        .bind(role_id)
        .fetch_all(&st.db)
        .await?;
    if let Some(missing) = perms.iter().find(|perm| !p.can_at(perm, &path)) {
        return Err(AppError::BadRequest(format!(
            "You can't grant this role here because it includes '{missing}', which you don't hold at this unit."
        )));
    }
    Ok(path)
}

#[derive(Deserialize)]
struct MembershipInput {
    role_id: Uuid,
    org_unit_id: Uuid,
}

#[derive(Deserialize)]
struct CreateUser {
    email: String,
    full_name: String,
    #[serde(default)]
    memberships: Vec<MembershipInput>,
    #[serde(default)]
    profile: Option<Value>,
}

fn temp_password() -> String {
    // Four groups from an unambiguous alphabet: easy to read out, ~80 bits.
    const ALPHA: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    use rand::Rng;
    let mut rng = rand::rngs::OsRng;
    (0..4)
        .map(|_| (0..4).map(|_| ALPHA[rng.gen_range(0..ALPHA.len())] as char).collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

async fn create_user(State(st): State<AppState>, a: Auth, Json(b): Json<CreateUser>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.users.manage")?;
    if b.memberships.is_empty() && !a.principal.can_at("admin.users.manage", &root_path(&st).await?) {
        return Err(AppError::bad("Give the person at least one role."));
    }
    for m in &b.memberships {
        ensure_can_grant(&st, &a.principal, m.role_id, m.org_unit_id).await?;
    }
    let email = b.email.trim().to_lowercase();
    let name = b.full_name.trim();
    if name.is_empty() {
        return Err(AppError::bad("Enter the person's name."));
    }
    let password = temp_password();
    let hash = auth::hash_password(password.clone()).await?;
    let mut tx = st.db.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into users (email, full_name, password_hash, must_change_password, profile) values ($1, $2, $3, true, $4) returning id",
    )
    .bind(&email)
    .bind(name)
    .bind(hash)
    .bind(b.profile.unwrap_or_else(|| json!({})))
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => AppError::Conflict("Someone already uses that email.".into()),
        sqlx::Error::Database(ref d) if d.is_check_violation() => AppError::bad("That email address doesn't look right."),
        other => other.into(),
    })?;
    for m in &b.memberships {
        sqlx::query("insert into memberships (user_id, role_id, org_unit_id, created_by) values ($1, $2, $3, $4)")
            .bind(id)
            .bind(m.role_id)
            .bind(m.org_unit_id)
            .bind(a.id())
            .execute(&mut *tx)
            .await?;
    }
    audit::record(&mut *tx, Some(a.id()), "user.created", "user", Some(id.to_string()), Some(&a.ip), json!({ "email": email, "roles": b.memberships.len() })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": id, "temporary_password": password })))
}

async fn root_path(st: &AppState) -> AppResult<String> {
    Ok(sqlx::query_scalar("select path::text from org_units where parent_id is null")
        .fetch_one(&st.db)
        .await?)
}

/// Admins may manage a user if the user sits inside their scope (or has no
/// memberships yet).
async fn ensure_manages(st: &AppState, p: &Principal, user_id: Uuid) -> AppResult<()> {
    p.require_anywhere("admin.users.manage")?;
    let paths: Vec<String> = sqlx::query_scalar(
        "select o.path::text from memberships m join org_units o on o.id = m.org_unit_id where m.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&st.db)
    .await?;
    if paths.is_empty() || paths.iter().any(|path| p.can_at("admin.users.manage", path)) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn get_user(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ensure_manages(&st, &a.principal, id).await?;
    let user: Value = sqlx::query_scalar(
        r#"select jsonb_build_object(
               'id', u.id, 'email', u.email, 'full_name', u.full_name, 'status', u.status,
               'must_change_password', u.must_change_password, 'totp_enabled', u.totp_enabled,
               'locked', coalesce(u.locked_until > now(), false), 'failed_logins', u.failed_logins,
               'profile', u.profile, 'created_at', u.created_at, 'last_login_at', u.last_login_at,
               'password_changed_at', u.password_changed_at)
           from users u where u.id = $1"#,
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let memberships: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('id', m.id, 'role_id', r.id, 'role_key', r.key, 'role_name', r.name, 'role_color', r.color,
                                     'unit_id', o.id, 'unit_name', o.name, 'unit_kind', o.kind, 'created_at', m.created_at)
           from memberships m join roles r on r.id = m.role_id join org_units o on o.id = m.org_unit_id
           where m.user_id = $1 order by nlevel(o.path), r.name"#,
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    let guardians: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('id', u.id, 'full_name', u.full_name, 'email', u.email, 'relation', g.relation, 'direction', 'guardian')
           from guardianships g join users u on u.id = g.guardian_id where g.student_id = $1
           union all
           select jsonb_build_object('id', u.id, 'full_name', u.full_name, 'email', u.email, 'relation', g.relation, 'direction', 'child')
           from guardianships g join users u on u.id = g.student_id where g.guardian_id = $1"#,
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    let sessions: i64 = sqlx::query_scalar(
        "select count(*) from sessions where user_id = $1 and revoked_at is null and expires_at > now()",
    )
    .bind(id)
    .fetch_one(&st.db)
    .await?;
    let activity: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('seq', seq, 'at', at, 'action', action, 'ip', ip)
           from audit_log where actor_id = $1 order by seq desc limit 15"#,
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!({ "user": user, "memberships": memberships, "relations": guardians, "active_sessions": sessions, "activity": activity })))
}

#[derive(Deserialize)]
struct UpdateUser {
    full_name: Option<String>,
    status: Option<String>,
    profile: Option<Value>,
}

async fn update_user(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<UpdateUser>) -> AppResult<Json<Value>> {
    ensure_manages(&st, &a.principal, id).await?;
    if b.status.as_deref() == Some("disabled") && id == a.id() {
        return Err(AppError::bad("You can't disable your own account."));
    }
    if let Some(s) = &b.status {
        if s != "active" && s != "disabled" {
            return Err(AppError::bad("Status must be active or disabled."));
        }
    }
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "update users set full_name = coalesce($2, full_name), status = coalesce($3, status),
                          profile = coalesce($4, profile), updated_at = now() where id = $1",
    )
    .bind(id)
    .bind(b.full_name.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(&b.status)
    .bind(&b.profile)
    .execute(&mut *tx)
    .await?;
    if b.status.as_deref() == Some("disabled") {
        sqlx::query("update sessions set revoked_at = now() where user_id = $1 and revoked_at is null")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    audit::record(&mut *tx, Some(a.id()), "user.updated", "user", Some(id.to_string()), Some(&a.ip), json!({ "status": b.status, "name_changed": b.full_name.is_some(), "profile_changed": b.profile.is_some() })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn reset_password(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ensure_manages(&st, &a.principal, id).await?;
    let password = temp_password();
    let hash = auth::hash_password(password.clone()).await?;
    let mut tx = st.db.begin().await?;
    sqlx::query("update users set password_hash = $2, must_change_password = true, failed_logins = 0, locked_until = null, password_changed_at = now() where id = $1")
        .bind(id)
        .bind(hash)
        .execute(&mut *tx)
        .await?;
    sqlx::query("update sessions set revoked_at = now() where user_id = $1 and revoked_at is null")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut *tx, Some(a.id()), "user.password_reset", "user", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({ "temporary_password": password })))
}

async fn unlock(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ensure_manages(&st, &a.principal, id).await?;
    sqlx::query("update users set failed_logins = 0, locked_until = null where id = $1").bind(id).execute(&st.db).await?;
    audit::record(&st.db, Some(a.id()), "user.unlocked", "user", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn revoke_sessions(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    ensure_manages(&st, &a.principal, id).await?;
    let n = sqlx::query("update sessions set revoked_at = now() where user_id = $1 and revoked_at is null")
        .bind(id)
        .execute(&st.db)
        .await?
        .rows_affected();
    audit::record(&st.db, Some(a.id()), "user.sessions_revoked", "user", Some(id.to_string()), Some(&a.ip), json!({ "count": n })).await?;
    Ok(Json(json!({ "revoked": n })))
}

async fn add_membership(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<MembershipInput>) -> AppResult<Json<Value>> {
    ensure_can_grant(&st, &a.principal, b.role_id, b.org_unit_id).await?;
    let mut tx = st.db.begin().await?;
    let mid: Uuid = sqlx::query_scalar(
        "insert into memberships (user_id, role_id, org_unit_id, created_by) values ($1, $2, $3, $4) returning id",
    )
    .bind(id)
    .bind(b.role_id)
    .bind(b.org_unit_id)
    .bind(a.id())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => AppError::Conflict("They already have that role there.".into()),
        other => other.into(),
    })?;
    audit::record(&mut *tx, Some(a.id()), "membership.granted", "user", Some(id.to_string()), Some(&a.ip), json!({ "role_id": b.role_id, "org_unit_id": b.org_unit_id })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": mid })))
}

async fn remove_membership(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let (user_id, role_id, unit_id, path): (Uuid, Uuid, Uuid, String) = sqlx::query_as(
        "select m.user_id, m.role_id, m.org_unit_id, o.path::text from memberships m join org_units o on o.id = m.org_unit_id where m.id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)?;
    a.principal.require_at("admin.users.manage", &path)?;
    // Never let the last root administrator be removed.
    let is_last_admin: bool = sqlx::query_scalar(
        r#"select exists(select 1 from role_permissions where role_id = $1 and permission = 'admin.roles.manage')
             and (select parent_id is null from org_units where id = $2)
             and (select count(*) from memberships m join org_units o on o.id = m.org_unit_id
                  join role_permissions rp on rp.role_id = m.role_id and rp.permission = 'admin.roles.manage'
                  where o.parent_id is null) <= 1"#,
    )
    .bind(role_id)
    .bind(unit_id)
    .fetch_one(&st.db)
    .await?;
    if is_last_admin {
        return Err(AppError::bad("This is the last top-level administrator. Grant the role to someone else first."));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("delete from memberships where id = $1").bind(id).execute(&mut *tx).await?;
    audit::record(&mut *tx, Some(a.id()), "membership.revoked", "user", Some(user_id.to_string()), Some(&a.ip), json!({ "role_id": role_id, "org_unit_id": unit_id })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct GuardianInput {
    guardian_id: Uuid,
    #[serde(default)]
    relation: Option<String>,
}

async fn add_guardian(State(st): State<AppState>, a: Auth, Path(student): Path<Uuid>, Json(b): Json<GuardianInput>) -> AppResult<Json<Value>> {
    ensure_manages(&st, &a.principal, student).await?;
    ensure_manages(&st, &a.principal, b.guardian_id).await?;
    let relation = b.relation.unwrap_or_else(|| "guardian".into());
    sqlx::query("insert into guardianships (guardian_id, student_id, relation) values ($1, $2, $3) on conflict (guardian_id, student_id) do update set relation = excluded.relation")
        .bind(b.guardian_id)
        .bind(student)
        .bind(&relation)
        .execute(&st.db)
        .await?;
    audit::record(&st.db, Some(a.id()), "guardianship.linked", "user", Some(student.to_string()), Some(&a.ip), json!({ "guardian_id": b.guardian_id, "relation": relation })).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn remove_guardian(State(st): State<AppState>, a: Auth, Path((student, guardian)): Path<(Uuid, Uuid)>) -> AppResult<Json<Value>> {
    ensure_manages(&st, &a.principal, student).await?;
    sqlx::query("delete from guardianships where guardian_id = $1 and student_id = $2")
        .bind(guardian)
        .bind(student)
        .execute(&st.db)
        .await?;
    audit::record(&st.db, Some(a.id()), "guardianship.unlinked", "user", Some(student.to_string()), Some(&a.ip), json!({ "guardian_id": guardian })).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn all_sessions(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.sessions.manage")?;
    let policy: SecurityPolicy = settings::get_typed(&st.db, "security").await?;
    let rows: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('id', s.id, 'user_id', u.id, 'full_name', u.full_name, 'email', u.email,
                                     'ip', s.ip, 'user_agent', s.user_agent, 'created_at', s.created_at,
                                     'last_seen_at', s.last_seen_at, 'expires_at', s.expires_at)
           from sessions s join users u on u.id = s.user_id
           where s.revoked_at is null and s.expires_at > now() and not s.mfa_pending
             and s.last_seen_at > now() - make_interval(mins => $1)
           order by s.last_seen_at desc limit 300"#,
    )
    .bind(policy.session_idle_minutes as i32)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

async fn revoke_session(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.sessions.manage")?;
    sqlx::query("update sessions set revoked_at = now() where id = $1").bind(id).execute(&st.db).await?;
    audit::record(&st.db, Some(a.id()), "session.revoked", "session", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}
