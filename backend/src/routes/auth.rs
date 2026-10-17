use std::time::Duration;

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    audit,
    auth::{self, Auth, ClientMeta, PendingAuth},
    error::{AppError, AppResult},
    settings::{self, SecurityPolicy},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/mfa/verify", post(mfa_verify))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/password", post(change_password))
        .route("/mfa/setup", post(mfa_setup))
        .route("/mfa/enable", post(mfa_enable))
        .route("/mfa/disable", post(mfa_disable))
        .route("/sessions", get(my_sessions))
        .route("/sessions/{id}", delete(revoke_my_session))
}

#[derive(Deserialize)]
struct LoginBody {
    email: String,
    password: String,
}

const GENERIC_LOGIN_ERROR: &str = "That email and password don't match our records.";

async fn login(
    State(st): State<AppState>,
    meta: ClientMeta,
    jar: CookieJar,
    Json(body): Json<LoginBody>,
) -> AppResult<(CookieJar, Json<Value>)> {
    let email = body.email.trim().to_lowercase();
    st.limiter.hit(&format!("login-ip:{}", meta.ip), 30, Duration::from_secs(300))?;
    st.limiter.hit(&format!("login-email:{email}"), 10, Duration::from_secs(900))?;
    let policy: SecurityPolicy = settings::get_typed(&st.db, "security").await?;

    let user: Option<(Uuid, String, String, i32, Option<DateTime<Utc>>, bool)> = sqlx::query_as(
        "select id, password_hash, status, failed_logins, locked_until, totp_enabled from users where email = $1",
    )
    .bind(&email)
    .fetch_optional(&st.db)
    .await?;

    let Some((user_id, hash, status, failed, locked_until, totp_enabled)) = user else {
        // Same work as a real check so response time doesn't reveal accounts.
        auth::verify_password(body.password, auth::DUMMY_HASH.clone()).await;
        audit::record(&st.db, None, "auth.login_failed", "user", None, Some(&meta.ip), json!({ "email": email, "reason": "unknown" })).await?;
        return Err(AppError::BadRequest(GENERIC_LOGIN_ERROR.into()));
    };

    if locked_until.is_some_and(|t| t > Utc::now()) {
        auth::verify_password(body.password, auth::DUMMY_HASH.clone()).await;
        return Err(AppError::BadRequest(
            "This account is temporarily locked after repeated failed sign-ins. Try again later or ask your administrator.".into(),
        ));
    }

    if !auth::verify_password(body.password, hash).await || status != "active" {
        let failed = failed + 1;
        let lock = failed >= policy.lockout_threshold;
        sqlx::query(
            "update users set failed_logins = case when $2 then 0 else $3 end,
                              locked_until = case when $2 then now() + make_interval(mins => $4) else locked_until end
             where id = $1",
        )
        .bind(user_id)
        .bind(lock)
        .bind(failed)
        .bind(policy.lockout_minutes as i32)
        .execute(&st.db)
        .await?;
        audit::record(
            &st.db,
            Some(user_id),
            if lock { "auth.locked" } else { "auth.login_failed" },
            "user",
            Some(user_id.to_string()),
            Some(&meta.ip),
            json!({ "failed_attempts": failed }),
        )
        .await?;
        return Err(AppError::BadRequest(GENERIC_LOGIN_ERROR.into()));
    }

    sqlx::query("update users set failed_logins = 0, locked_until = null, last_login_at = now() where id = $1")
        .bind(user_id)
        .execute(&st.db)
        .await?;
    let session = auth::create_session(&st.db, user_id, totp_enabled, &meta.ip, &meta.user_agent, &policy).await?;
    audit::record(
        &st.db,
        Some(user_id),
        "auth.login",
        "session",
        Some(session.id.to_string()),
        Some(&meta.ip),
        json!({ "mfa_pending": totp_enabled }),
    )
    .await?;
    let max_age = (session.expires_at - Utc::now()).num_seconds();
    let csrf = session.csrf.clone();
    let jar = jar.add(auth::session_cookie(session.token, st.cfg.secure_cookies, max_age));
    Ok((jar, Json(json!({ "mfa_required": totp_enabled, "csrf_token": csrf }))))
}

#[derive(Deserialize)]
struct CodeBody {
    code: String,
}

async fn mfa_verify(
    State(st): State<AppState>,
    pending: PendingAuth,
    jar: CookieJar,
    Json(body): Json<CodeBody>,
) -> AppResult<(CookieJar, Json<Value>)> {
    st.limiter.hit(&format!("mfa:{}", pending.user.user_id), 6, Duration::from_secs(300))?;
    if !pending.user.mfa_pending {
        return Ok((jar, Json(json!({ "ok": true }))));
    }
    let secret: Option<String> = sqlx::query_scalar("select totp_secret from users where id = $1")
        .bind(pending.user.user_id)
        .fetch_one(&st.db)
        .await?;
    let secret = secret.ok_or(AppError::Unauthenticated)?;
    let totp = auth::totp_for(&secret, &pending.user.email, "Lyceum")?;
    if !totp.check_current(body.code.trim()).unwrap_or(false) {
        audit::record(&st.db, Some(pending.user.user_id), "auth.mfa_failed", "user", Some(pending.user.user_id.to_string()), Some(&pending.ip), json!({})).await?;
        return Err(AppError::bad("That code didn't work. Codes change every 30 seconds."));
    }
    // Rotate: retire the half-authenticated session and issue a fresh one.
    let policy: SecurityPolicy = settings::get_typed(&st.db, "security").await?;
    sqlx::query("update sessions set revoked_at = now() where id = $1")
        .bind(pending.user.session_id)
        .execute(&st.db)
        .await?;
    let session = auth::create_session(&st.db, pending.user.user_id, false, &pending.ip, &pending.user_agent, &policy).await?;
    audit::record(&st.db, Some(pending.user.user_id), "auth.mfa_passed", "session", Some(session.id.to_string()), Some(&pending.ip), json!({})).await?;
    let max_age = (session.expires_at - Utc::now()).num_seconds();
    let csrf = session.csrf.clone();
    Ok((jar.add(auth::session_cookie(session.token, st.cfg.secure_cookies, max_age)), Json(json!({ "ok": true, "csrf_token": csrf }))))
}

async fn logout(State(st): State<AppState>, pending: PendingAuth, jar: CookieJar) -> AppResult<(CookieJar, Json<Value>)> {
    sqlx::query("update sessions set revoked_at = now() where id = $1")
        .bind(pending.user.session_id)
        .execute(&st.db)
        .await?;
    audit::record(&st.db, Some(pending.user.user_id), "auth.logout", "session", Some(pending.user.session_id.to_string()), Some(&pending.ip), json!({})).await?;
    Ok((jar.add(auth::clear_cookie(st.cfg.secure_cookies)), Json(json!({ "ok": true }))))
}

async fn me(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    let policy: SecurityPolicy = settings::get_typed(&st.db, "security").await?;
    let roles = a.principal.role_keys();
    let mfa_required = !a.user.totp_enabled && roles.iter().any(|r| policy.mfa_required_roles.contains(r));
    let profile: Value = sqlx::query_scalar("select profile from users where id = $1")
        .bind(a.id())
        .fetch_one(&st.db)
        .await?;
    let unread: i64 = sqlx::query_scalar("select count(*) from notifications where user_id = $1 and read_at is null")
        .bind(a.id())
        .fetch_one(&st.db)
        .await?;
    let children: Vec<(Uuid, String)> = sqlx::query_as(
        "select u.id, u.full_name from guardianships g join users u on u.id = g.student_id where g.guardian_id = $1 order by u.full_name",
    )
    .bind(a.id())
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!({
        "user": {
            "id": a.id(),
            "email": a.user.email,
            "full_name": a.user.full_name,
            "must_change_password": a.user.must_change_password,
            "totp_enabled": a.user.totp_enabled,
            "mfa_setup_required": mfa_required,
            "profile": profile,
        },
        "csrf_token": a.user.csrf_token,
        "roles": roles,
        "permissions": a.principal.all_permissions(),
        "grants": a.principal.grants,
        "children": children.into_iter().map(|(id, name)| json!({ "id": id, "full_name": name })).collect::<Vec<_>>(),
        "unread_notifications": unread,
        "settings": {
            "branding": settings::get(&st.db, "branding").await?,
            "modules": settings::get(&st.db, "modules").await?,
            "profile_fields": settings::get(&st.db, "profile_fields").await?,
        }
    })))
}

#[derive(Deserialize)]
struct PasswordBody {
    current_password: String,
    new_password: String,
}

async fn change_password(State(st): State<AppState>, a: Auth, Json(body): Json<PasswordBody>) -> AppResult<Json<Value>> {
    st.limiter.hit(&format!("pw:{}", a.id()), 10, Duration::from_secs(900))?;
    let hash: String = sqlx::query_scalar("select password_hash from users where id = $1")
        .bind(a.id())
        .fetch_one(&st.db)
        .await?;
    if !auth::verify_password(body.current_password, hash).await {
        return Err(AppError::bad("Your current password is incorrect."));
    }
    let policy: SecurityPolicy = settings::get_typed(&st.db, "security").await?;
    auth::check_password_policy(&body.new_password, &a.user.email, &policy)?;
    let new_hash = auth::hash_password(body.new_password).await?;
    let mut tx = st.db.begin().await?;
    sqlx::query("update users set password_hash = $2, must_change_password = false, password_changed_at = now(), updated_at = now() where id = $1")
        .bind(a.id())
        .bind(new_hash)
        .execute(&mut *tx)
        .await?;
    // Sign out every other device.
    sqlx::query("update sessions set revoked_at = now() where user_id = $1 and id <> $2 and revoked_at is null")
        .bind(a.id())
        .bind(a.user.session_id)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut *tx, Some(a.id()), "auth.password_changed", "user", Some(a.id().to_string()), Some(&a.ip), json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn mfa_setup(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    if a.user.totp_enabled {
        return Err(AppError::Conflict("Two-step sign-in is already on.".into()));
    }
    let secret = auth::new_totp_secret();
    sqlx::query("update users set totp_secret = $2 where id = $1")
        .bind(a.id())
        .bind(&secret)
        .execute(&st.db)
        .await?;
    let totp = auth::totp_for(&secret, &a.user.email, "Lyceum")?;
    Ok(Json(json!({ "secret": secret, "otpauth_url": totp.get_url() })))
}

async fn mfa_enable(State(st): State<AppState>, a: Auth, Json(body): Json<CodeBody>) -> AppResult<Json<Value>> {
    st.limiter.hit(&format!("mfa-enable:{}", a.id()), 6, Duration::from_secs(300))?;
    let secret: Option<String> = sqlx::query_scalar("select totp_secret from users where id = $1")
        .bind(a.id())
        .fetch_one(&st.db)
        .await?;
    let secret = secret.ok_or_else(|| AppError::bad("Start setup first."))?;
    let totp = auth::totp_for(&secret, &a.user.email, "Lyceum")?;
    if !totp.check_current(body.code.trim()).unwrap_or(false) {
        return Err(AppError::bad("That code didn't match. Check your phone's clock and try the newest code."));
    }
    sqlx::query("update users set totp_enabled = true where id = $1").bind(a.id()).execute(&st.db).await?;
    audit::record(&st.db, Some(a.id()), "auth.mfa_enabled", "user", Some(a.id().to_string()), Some(&a.ip), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct DisableBody {
    password: String,
}

async fn mfa_disable(State(st): State<AppState>, a: Auth, Json(body): Json<DisableBody>) -> AppResult<Json<Value>> {
    let hash: String = sqlx::query_scalar("select password_hash from users where id = $1")
        .bind(a.id())
        .fetch_one(&st.db)
        .await?;
    if !auth::verify_password(body.password, hash).await {
        return Err(AppError::bad("Your password is incorrect."));
    }
    let policy: SecurityPolicy = settings::get_typed(&st.db, "security").await?;
    if a.principal.role_keys().iter().any(|r| policy.mfa_required_roles.contains(r)) {
        return Err(AppError::bad("Your role requires two-step sign-in."));
    }
    sqlx::query("update users set totp_enabled = false, totp_secret = null where id = $1")
        .bind(a.id())
        .execute(&st.db)
        .await?;
    audit::record(&st.db, Some(a.id()), "auth.mfa_disabled", "user", Some(a.id().to_string()), Some(&a.ip), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn my_sessions(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    let rows: Vec<(Uuid, Option<String>, Option<String>, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        "select id, ip, user_agent, created_at, last_seen_at from sessions
         where user_id = $1 and revoked_at is null and expires_at > now() and not mfa_pending
         order by last_seen_at desc",
    )
    .bind(a.id())
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|(id, ip, ua, created, seen)| json!({
            "id": id, "ip": ip, "user_agent": ua, "created_at": created, "last_seen_at": seen,
            "current": id == a.user.session_id
        }))
        .collect::<Vec<_>>())))
}

async fn revoke_my_session(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    let n = sqlx::query("update sessions set revoked_at = now() where id = $1 and user_id = $2 and revoked_at is null")
        .bind(id)
        .bind(a.id())
        .execute(&st.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    audit::record(&st.db, Some(a.id()), "auth.session_revoked", "session", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}
