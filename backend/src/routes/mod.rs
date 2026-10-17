use axum::Router;

use crate::state::AppState;

pub mod admin;
pub mod auth;
pub mod community;
pub mod forms;
pub mod org;
pub mod users;
pub mod workflows;

pub fn api() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::router())
        .nest("/org", org::router())
        .nest("/admin", users::router().merge(admin::router()))
        .merge(workflows::router())
        .merge(forms::router())
        .merge(community::router())
}
