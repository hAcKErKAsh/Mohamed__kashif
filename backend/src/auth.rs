//! Authentication primitives: password hashing, opaque session tokens, the
//! request extractor that authenticates every API call, CSRF enforcement and a
//! small in-memory rate limiter.

use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{request::Parts, Method},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::Engine;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    authz::Principal,
    error::{AppError, AppResult},
    settings::{self, SecurityPolicy},
    state::AppState,
};

pub const SESSION_COOKIE: &str = "lyceum_session";

// ---------------------------------------------------------------------------
// Passwords
// ---------------------------------------------------------------------------

pub async fn hash_password(password: String) -> AppResult<String> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| anyhow::anyhow!("hash failed: {e}"))
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?
    .map_err(AppError::Other)
}

pub async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash)
            .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

/// A valid hash of a random password, used to keep login timing identical for
/// unknown accounts.
pub static DUMMY_HASH: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(b"not-a-real-password", &salt).unwrap().to_string()
});

const COMMON_PASSWORDS: &[&str] = &[
    "password", "password1", "password123", "123456789012", "qwertyuiop", "letmein",
    "welcome", "admin", "iloveyou", "monkey", "dragon", "football", "baseball", "school",
    "teacher", "student", "princess", "sunshine", "passw0rd", "changeme",
];

pub fn check_password_policy(password: &str, email: &str, policy: &SecurityPolicy) -> AppResult<()> {
    if password.chars().count() < policy.password_min_length {
        return Err(AppError::bad(format!(
            "Password must be at least {} characters. A short sentence works well.",
            policy.password_min_length
        )));
    }
    if password.len() > 256 {
        return Err(AppError::bad("Password is too long."));
    }
    let lower = password.to_lowercase();
    let stripped: String = lower.chars().filter(|c| c.is_alphanumeric()).collect();
    if COMMON_PASSWORDS.iter().any(|c| stripped == *c || stripped.trim_end_matches(char::is_numeric) == *c) {
        return Err(AppError::bad("That password is too common."));
    }
    let local = email.split('@').next().unwrap_or("").to_lowercase();
    if local.len() >= 4 && lower.contains(&local) {
        return Err(AppError::bad("Password must not contain your email name."));
    }
    let distinct: std::collections::HashSet<char> = password.chars().collect();
    if distinct.len() < 5 {
        return Err(AppError::bad("Password needs more variety."));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

pub fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub struct NewSession {
    pub id: Uuid,
    pub token: String,
    pub csrf: String,
    pub expires_at: DateTime<Utc>,
}

pub async fn create_session(
    db: &PgPool,
    user_id: Uuid,
    mfa_pending: bool,
    ip: &str,
    user_agent: &str,
    policy: &SecurityPolicy,
) -> AppResult<NewSession> {
    let token = random_token(32);
    let csrf = random_token(24);
    let expires_at = Utc::now() + chrono::Duration::hours(policy.session_absolute_hours.max(1));
    let id: Uuid = sqlx::query_scalar(
        r#"insert into sessions (user_id, token_hash, csrf_token, mfa_pending, ip, user_agent, expires_at)
           values ($1, $2, $3, $4, $5, $6, $7) returning id"#,
    )
    .bind(user_id)
    .bind(token_hash(&token))
    .bind(&csrf)
    .bind(mfa_pending)
    .bind(ip)
    .bind(user_agent.chars().take(300).collect::<String>())
    .bind(expires_at)
    .fetch_one(db)
    .await?;
    Ok(NewSession { id, token, csrf, expires_at })
}

pub fn session_cookie(token: String, secure: bool, max_age_secs: i64) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Strict)
        .path("/")
        .max_age(time_duration(max_age_secs))
        .build()
}

pub fn clear_cookie(secure: bool) -> Cookie<'static> {
    session_cookie(String::new(), secure, 0)
}

fn time_duration(secs: i64) -> time::Duration {
    time::Duration::seconds(secs)
}

// ---------------------------------------------------------------------------
// Request context
// ---------------------------------------------------------------------------

pub fn client_ip(parts: &Parts, trust_proxy: bool) -> String {
    if trust_proxy {
        if let Some(v) = parts.headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            if let Some(first) = v.split(',').next() {
                return first.trim().to_string();
            }
        }
    }
    parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

pub fn user_agent(parts: &Parts) -> String {
    parts
        .headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

/// Lightweight request metadata for unauthenticated endpoints.
pub struct ClientMeta {
    pub ip: String,
    pub user_agent: String,
}

impl FromRequestParts<AppState> for ClientMeta {
    type Rejection = AppError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        check_origin(parts, state)?;
        Ok(Self { ip: client_ip(parts, state.cfg.trust_proxy), user_agent: user_agent(parts) })
    }
}

/// For state-changing requests, a present Origin header must match the host or
/// an explicitly allowed origin.
fn check_origin(parts: &Parts, state: &AppState) -> AppResult<()> {
    if matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return Ok(());
    }
    let Some(origin) = parts.headers.get(axum::http::header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return Ok(());
    };
    let host = parts.headers.get(axum::http::header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let origin_host = origin.split("://").nth(1).unwrap_or("");
    if origin_host == host || state.cfg.allowed_origins.iter().any(|o| o == origin) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionUser {
    pub session_id: Uuid,
    pub csrf_token: String,
    pub mfa_pending: bool,
    pub last_seen_at: DateTime<Utc>,
    pub user_id: Uuid,
    pub email: String,
    pub full_name: String,
    pub must_change_password: bool,
    pub totp_enabled: bool,
}

async fn load_session(parts: &mut Parts, state: &AppState) -> AppResult<SessionUser> {
    check_origin(parts, state)?;
    let jar = CookieJar::from_headers(&parts.headers);
    let token = jar.get(SESSION_COOKIE).map(|c| c.value().to_string()).ok_or(AppError::Unauthenticated)?;
    if token.is_empty() {
        return Err(AppError::Unauthenticated);
    }
    let policy: SecurityPolicy = settings::get_typed(&state.db, "security").await?;
    let row: Option<SessionUser> = sqlx::query_as(
        r#"select s.id as session_id, s.csrf_token, s.mfa_pending, s.last_seen_at,
                  u.id as user_id, u.email::text as email, u.full_name, u.must_change_password, u.totp_enabled
           from sessions s join users u on u.id = s.user_id
           where s.token_hash = $1 and s.revoked_at is null and s.expires_at > now()
             and u.status = 'active'
             and s.last_seen_at > now() - make_interval(mins => $2)"#,
    )
    .bind(token_hash(&token))
    .bind(policy.session_idle_minutes.max(5) as i32)
    .fetch_optional(&state.db)
    .await?;
    let su = row.ok_or(AppError::Unauthenticated)?;

    if !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS) {
        let sent = parts.headers.get("x-csrf-token").and_then(|v| v.to_str().ok()).unwrap_or("");
        if !constant_time_eq(sent.as_bytes(), su.csrf_token.as_bytes()) {
            return Err(AppError::Forbidden);
        }
    }

    if Utc::now() - su.last_seen_at > chrono::Duration::seconds(60) {
        sqlx::query("update sessions set last_seen_at = now() where id = $1")
            .bind(su.session_id)
            .execute(&state.db)
            .await?;
    }
    Ok(su)
}

/// A fully authenticated caller with their resolved grants.
pub struct Auth {
    pub user: SessionUser,
    pub principal: Principal,
    pub ip: String,
}

impl Auth {
    pub fn id(&self) -> Uuid {
        self.user.user_id
    }
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = AppError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let user = load_session(parts, state).await?;
        if user.mfa_pending {
            return Err(AppError::Unauthenticated);
        }
        let principal = Principal::load(&state.db, user.user_id).await?;
        Ok(Self { user, principal, ip: client_ip(parts, state.cfg.trust_proxy) })
    }
}

/// A session that has passed the password step but still owes a second factor.
pub struct PendingAuth {
    pub user: SessionUser,
    pub ip: String,
    pub user_agent: String,
}

impl FromRequestParts<AppState> for PendingAuth {
    type Rejection = AppError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let user = load_session(parts, state).await?;
        Ok(Self { user, ip: client_ip(parts, state.cfg.trust_proxy), user_agent: user_agent(parts) })
    }
}

// ---------------------------------------------------------------------------
// Rate limiting (fixed window, in memory — sufficient for a single node).
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct RateLimiter {
    buckets: DashMap<String, (Instant, u32)>,
}

impl RateLimiter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Count a hit against `key`; Err once `limit` hits land inside `window`.
    pub fn hit(&self, key: &str, limit: u32, window: Duration) -> AppResult<()> {
        let now = Instant::now();
        let mut entry = self.buckets.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) > window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 > limit { Err(AppError::RateLimited) } else { Ok(()) }
    }

    pub fn sweep(&self, older_than: Duration) {
        let now = Instant::now();
        self.buckets.retain(|_, v| now.duration_since(v.0) < older_than);
    }
}

// ---------------------------------------------------------------------------
// TOTP second factor
// ---------------------------------------------------------------------------

pub fn totp_for(secret_b32: &str, account: &str, issuer: &str) -> AppResult<totp_rs::TOTP> {
    let secret = totp_rs::Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .map_err(|_| anyhow::anyhow!("bad totp secret"))?;
    totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret,
        Some(issuer.to_string()),
        account.to_string(),
    )
    .map_err(|e| AppError::Other(anyhow::anyhow!("totp: {e}")))
}

pub fn new_totp_secret() -> String {
    let mut buf = [0u8; 20];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    totp_rs::Secret::Raw(buf.to_vec()).to_encoded().to_string()
}
