//! No-code form builder. A school defines a form (fields + who may submit +
//! which workflow approves it); people fill it in; the engine routes it.

use axum::{
    extract::{Path, State},
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
    settings,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/forms", get(available))
        .route("/forms/submissions/mine", get(my_submissions))
        .route("/forms/{key}", get(get_form))
        .route("/forms/{key}/submit", post(submit))
        .route("/admin/forms", get(admin_list).post(create_form))
        .route("/admin/forms/{id}", put(update_form))
        .route("/admin/forms/{id}/submissions", get(form_submissions))
}

const FIELD_TYPES: &[&str] = &["text", "textarea", "number", "date", "select", "checkbox", "email"];

pub fn validate_fields(v: &Value) -> AppResult<()> {
    let arr = v.as_array().ok_or_else(|| AppError::bad("Fields must be a list."))?;
    if arr.len() > 60 {
        return Err(AppError::bad("A form can have at most 60 fields."));
    }
    let mut keys = std::collections::HashSet::new();
    for f in arr {
        let key = f.get("key").and_then(Value::as_str).unwrap_or("");
        let label = f.get("label").and_then(Value::as_str).unwrap_or("");
        let ty = f.get("type").and_then(Value::as_str).unwrap_or("");
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return Err(AppError::bad(format!("Field '{label}' needs a key of lowercase letters, numbers or underscores.")));
        }
        if matches!(key, "requester" | "unit") {
            return Err(AppError::bad(format!("'{key}' is reserved; pick another field key.")));
        }
        if !keys.insert(key.to_string()) {
            return Err(AppError::bad(format!("Field key '{key}' is used twice.")));
        }
        if label.trim().is_empty() {
            return Err(AppError::bad(format!("Field '{key}' needs a label.")));
        }
        if !FIELD_TYPES.contains(&ty) {
            return Err(AppError::bad(format!("Field '{label}' has an unknown type '{ty}'.")));
        }
        if ty == "select" && !f.get("options").and_then(Value::as_array).is_some_and(|o| !o.is_empty()) {
            return Err(AppError::bad(format!("Choice field '{label}' needs options.")));
        }
    }
    Ok(())
}

/// Check submitted values against the form definition and coerce types.
pub fn validate_submission(fields: &Value, data: &Value) -> AppResult<Value> {
    let empty = vec![];
    let defs = fields.as_array().unwrap_or(&empty);
    let input = data.as_object().ok_or_else(|| AppError::bad("Form data must be an object."))?;
    let mut out = serde_json::Map::new();
    for f in defs {
        let key = f["key"].as_str().unwrap_or_default();
        let label = f["label"].as_str().unwrap_or(key);
        let ty = f["type"].as_str().unwrap_or("text");
        let required = f["required"].as_bool().unwrap_or(false);
        let raw = input.get(key).cloned().unwrap_or(Value::Null);
        let blank = raw.is_null() || raw.as_str().is_some_and(|s| s.trim().is_empty());
        if blank {
            if required && ty != "checkbox" {
                return Err(AppError::bad(format!("“{label}” is required.")));
            }
            if ty == "checkbox" && required && raw != Value::Bool(true) {
                return Err(AppError::bad(format!("Please confirm “{label}”.")));
            }
            continue;
        }
        let value = match ty {
            "number" => {
                let n = raw
                    .as_f64()
                    .or_else(|| raw.as_str().and_then(|s| s.trim().parse().ok()))
                    .ok_or_else(|| AppError::bad(format!("“{label}” must be a number.")))?;
                if let Some(min) = f["min"].as_f64() {
                    if n < min {
                        return Err(AppError::bad(format!("“{label}” must be at least {min}.")));
                    }
                }
                if let Some(max) = f["max"].as_f64() {
                    if n > max {
                        return Err(AppError::bad(format!("“{label}” must be at most {max}.")));
                    }
                }
                json!(n)
            }
            "checkbox" => {
                let b = raw.as_bool().unwrap_or(false);
                if required && !b {
                    return Err(AppError::bad(format!("Please confirm “{label}”.")));
                }
                json!(b)
            }
            "date" => {
                let s = raw.as_str().unwrap_or_default().trim();
                chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| AppError::bad(format!("“{label}” must be a date.")))?;
                json!(s)
            }
            "select" => {
                let s = raw.as_str().unwrap_or_default();
                let ok = f["options"].as_array().is_some_and(|o| o.iter().any(|x| x.as_str() == Some(s)));
                if !ok {
                    return Err(AppError::bad(format!("Pick one of the options for “{label}”.")));
                }
                json!(s)
            }
            "email" => {
                let s = raw.as_str().unwrap_or_default().trim();
                if !s.contains('@') || s.len() > 200 {
                    return Err(AppError::bad(format!("“{label}” must be an email address.")));
                }
                json!(s)
            }
            _ => {
                let s = raw.as_str().map(str::to_string).unwrap_or_else(|| raw.to_string());
                let limit = if ty == "textarea" { 5000 } else { 300 };
                if s.chars().count() > limit {
                    return Err(AppError::bad(format!("“{label}” is too long.")));
                }
                json!(s.trim())
            }
        };
        out.insert(key.to_string(), value);
    }
    Ok(Value::Object(out))
}

async fn ensure_module(st: &AppState) -> AppResult<()> {
    if settings::module_enabled(&st.db, "forms").await? { Ok(()) } else { Err(AppError::NotFound) }
}

async fn available(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    ensure_module(&st).await?;
    if !a.principal.can_anywhere("forms.submit") {
        return Ok(Json(json!([])));
    }
    let roles = a.principal.role_keys();
    let rows: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('id', f.id, 'key', f.key, 'name', f.name, 'description', f.description,
                                     'field_count', jsonb_array_length(f.fields), 'has_workflow', f.workflow_key is not null)
           from form_definitions f
           where f.active and (cardinality(f.allowed_roles) = 0 or f.allowed_roles && $1::text[])
           order by f.name"#,
    )
    .bind(&roles)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

async fn get_form(State(st): State<AppState>, a: Auth, Path(key): Path<String>) -> AppResult<Json<Value>> {
    ensure_module(&st).await?;
    let roles = a.principal.role_keys();
    let row: Value = sqlx::query_scalar(
        r#"select jsonb_build_object('id', f.id, 'key', f.key, 'name', f.name, 'description', f.description,
                                     'fields', f.fields, 'workflow_key', f.workflow_key)
           from form_definitions f
           where f.key = $1 and f.active and (cardinality(f.allowed_roles) = 0 or f.allowed_roles && $2::text[])"#,
    )
    .bind(&key)
    .bind(&roles)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(row))
}

#[derive(Deserialize)]
struct SubmitInput {
    data: Value,
    org_unit_id: Option<Uuid>,
}

async fn submit(State(st): State<AppState>, a: Auth, Path(key): Path<String>, Json(b): Json<SubmitInput>) -> AppResult<Json<Value>> {
    ensure_module(&st).await?;
    let roles = a.principal.role_keys();
    let (form_id, name, fields, workflow_key): (Uuid, String, Value, Option<String>) = sqlx::query_as(
        "select id, name, fields, workflow_key from form_definitions
         where key = $1 and active and (cardinality(allowed_roles) = 0 or allowed_roles && $2::text[])",
    )
    .bind(&key)
    .bind(&roles)
    .fetch_optional(&st.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let unit = match b.org_unit_id {
        Some(u) => u,
        None => a.principal.home_unit().map(|g| g.org_unit_id).ok_or(AppError::Forbidden)?,
    };
    let path = unit_path(&st.db, unit).await?;
    a.principal.require_at("forms.submit", &path)?;
    let data = validate_submission(&fields, &b.data)?;

    let mut tx = st.db.begin().await?;
    let sub_id: Uuid = sqlx::query_scalar(
        "insert into form_submissions (form_id, submitter_id, org_unit_id, data) values ($1, $2, $3, $4) returning id",
    )
    .bind(form_id)
    .bind(a.id())
    .bind(unit)
    .bind(&data)
    .fetch_one(&mut *tx)
    .await?;
    let mut request_id = None;
    if let Some(wk) = workflow_key {
        let wf = engine::published_workflow(&mut tx, &wk)
            .await?
            .ok_or_else(|| AppError::bad("This form's approval workflow isn't published yet. Ask your administrator."))?;
        let title = format!("{name} · {}", a.user.full_name);
        let rid = engine::start(&mut tx, wf, "form", sub_id, &title, data.clone(), a.id(), unit).await?;
        sqlx::query("update form_submissions set approval_request_id = $2 where id = $1")
            .bind(sub_id)
            .bind(rid)
            .execute(&mut *tx)
            .await?;
        request_id = Some(rid);
    }
    audit::record(&mut *tx, Some(a.id()), "form.submitted", "form_submission", Some(sub_id.to_string()), Some(&a.ip), json!({ "form": key })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": sub_id, "request_id": request_id })))
}

async fn my_submissions(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    let rows: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('id', s.id, 'form', f.name, 'form_key', f.key, 'status', s.status,
                                     'created_at', s.created_at, 'request_id', s.approval_request_id, 'data', s.data)
           from form_submissions s join form_definitions f on f.id = s.form_id
           where s.submitter_id = $1 order by s.created_at desc limit 200"#,
    )
    .bind(a.id())
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

async fn admin_list(State(st): State<AppState>, a: Auth) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.forms.manage")?;
    let rows: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('id', f.id, 'key', f.key, 'name', f.name, 'description', f.description,
                                     'fields', f.fields, 'workflow_key', f.workflow_key, 'allowed_roles', f.allowed_roles,
                                     'active', f.active, 'updated_at', f.updated_at,
                                     'submissions', (select count(*) from form_submissions s where s.form_id = f.id))
           from form_definitions f order by f.name"#,
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}

#[derive(Deserialize)]
struct FormInput {
    key: Option<String>,
    name: String,
    #[serde(default)]
    description: String,
    fields: Value,
    workflow_key: Option<String>,
    #[serde(default)]
    allowed_roles: Vec<String>,
    #[serde(default = "yes")]
    active: bool,
}

fn yes() -> bool {
    true
}

async fn create_form(State(st): State<AppState>, a: Auth, Json(b): Json<FormInput>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.forms.manage")?;
    validate_fields(&b.fields)?;
    let key = b.key.unwrap_or_default().trim().to_lowercase();
    let mut tx = st.db.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into form_definitions (key, name, description, fields, workflow_key, allowed_roles, active)
         values ($1, $2, $3, $4, $5, $6, $7) returning id",
    )
    .bind(&key)
    .bind(b.name.trim())
    .bind(&b.description)
    .bind(&b.fields)
    .bind(b.workflow_key.filter(|s| !s.is_empty()))
    .bind(&b.allowed_roles)
    .bind(b.active)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => AppError::Conflict("A form with that key exists.".into()),
        sqlx::Error::Database(ref d) if d.is_check_violation() => AppError::bad("Key must be lowercase letters, numbers and underscores."),
        other => other.into(),
    })?;
    audit::record(&mut *tx, Some(a.id()), "form.created", "form", Some(id.to_string()), Some(&a.ip), json!({ "key": key })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_form(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<FormInput>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.forms.manage")?;
    validate_fields(&b.fields)?;
    let n = sqlx::query(
        "update form_definitions set name = $2, description = $3, fields = $4, workflow_key = $5, allowed_roles = $6,
                                     active = $7, updated_at = now() where id = $1",
    )
    .bind(id)
    .bind(b.name.trim())
    .bind(&b.description)
    .bind(&b.fields)
    .bind(b.workflow_key.filter(|s| !s.is_empty()))
    .bind(&b.allowed_roles)
    .bind(b.active)
    .execute(&st.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    audit::record(&st.db, Some(a.id()), "form.updated", "form", Some(id.to_string()), Some(&a.ip), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn form_submissions(State(st): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    a.principal.require_anywhere("admin.forms.manage")?;
    let rows: Vec<Value> = sqlx::query_scalar(
        r#"select jsonb_build_object('id', s.id, 'submitter', u.full_name, 'unit', o.name, 'status', s.status,
                                     'data', s.data, 'created_at', s.created_at, 'request_id', s.approval_request_id)
           from form_submissions s join users u on u.id = s.submitter_id join org_units o on o.id = s.org_unit_id
           where s.form_id = $1 order by s.created_at desc limit 500"#,
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!(rows)))
}
