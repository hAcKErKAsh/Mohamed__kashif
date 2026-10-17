//! School-configurable settings: the "no-code" control surface. Each key holds a
//! JSON document; defaults live here so a fresh install is fully usable.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityPolicy {
    pub password_min_length: usize,
    pub lockout_threshold: i32,
    pub lockout_minutes: i64,
    pub session_idle_minutes: i64,
    pub session_absolute_hours: i64,
    pub mfa_required_roles: Vec<String>,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            password_min_length: 12,
            lockout_threshold: 5,
            lockout_minutes: 15,
            session_idle_minutes: 60,
            session_absolute_hours: 12,
            mfa_required_roles: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContentPolicy {
    /// Workflow used when someone without `content.publish` submits content.
    pub content_workflow_key: String,
    /// Workflow used when someone without `social.post_direct` posts.
    pub post_workflow_key: String,
}

impl Default for ContentPolicy {
    fn default() -> Self {
        Self { content_workflow_key: "content_publish".into(), post_workflow_key: "post_moderation".into() }
    }
}

pub const KEYS: &[&str] = &["branding", "modules", "security", "content_policy", "profile_fields", "org_kinds"];

pub fn default_for(key: &str) -> Value {
    match key {
        "branding" => json!({ "name": "Lyceum", "tagline": "School operating system", "accent": "#2f5d50", "motto": "" }),
        "modules" => json!({ "social": true, "content": true, "forms": true, "approvals": true, "directory": true }),
        "security" => serde_json::to_value(SecurityPolicy::default()).unwrap(),
        "content_policy" => serde_json::to_value(ContentPolicy::default()).unwrap(),
        "profile_fields" => json!([]),
        "org_kinds" => json!(["school", "wing", "department", "grade", "section", "house", "club"]),
        _ => Value::Null,
    }
}

pub async fn get(db: &PgPool, key: &str) -> AppResult<Value> {
    let v: Option<Value> = sqlx::query_scalar("select value from settings where key = $1")
        .bind(key)
        .fetch_optional(db)
        .await?;
    Ok(v.unwrap_or_else(|| default_for(key)))
}

pub async fn get_typed<T: serde::de::DeserializeOwned + Default>(db: &PgPool, key: &str) -> AppResult<T> {
    Ok(serde_json::from_value(get(db, key).await?).unwrap_or_default())
}

pub async fn all(db: &PgPool) -> AppResult<serde_json::Map<String, Value>> {
    let mut out = serde_json::Map::new();
    for k in KEYS {
        out.insert(k.to_string(), get(db, k).await?);
    }
    Ok(out)
}

pub async fn module_enabled(db: &PgPool, module: &str) -> AppResult<bool> {
    Ok(get(db, "modules").await?.get(module).and_then(Value::as_bool).unwrap_or(true))
}
