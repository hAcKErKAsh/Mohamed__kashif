use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("authentication required")]
    Unauthenticated,
    #[error("you do not have permission to do that")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Conflict(String),
    #[error("too many attempts, try again later")]
    RateLimited,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl AppError {
    pub fn bad(msg: impl Into<String>) -> Self {
        AppError::BadRequest(msg.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AppError::Unauthenticated => (StatusCode::UNAUTHORIZED, "unauthenticated"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            AppError::Db(sqlx::Error::RowNotFound) => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Db(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                (StatusCode::CONFLICT, "conflict")
            }
            AppError::Db(sqlx::Error::Database(e)) if e.is_check_violation() => {
                (StatusCode::BAD_REQUEST, "bad_request")
            }
            AppError::Db(_) | AppError::Other(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        // Never leak internals to clients; log them instead.
        let message = match &self {
            AppError::Db(sqlx::Error::RowNotFound) => "not found".to_string(),
            AppError::Db(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                "that value is already in use".to_string()
            }
            AppError::Db(sqlx::Error::Database(e)) if e.is_check_violation() => {
                "a value is not in the allowed format".to_string()
            }
            AppError::Db(_) | AppError::Other(_) => {
                tracing::error!(error = ?self, "internal error");
                "something went wrong".to_string()
            }
            other => other.to_string(),
        };
        (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response()
    }
}
