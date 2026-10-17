use std::sync::Arc;

use sqlx::PgPool;

use crate::{auth::RateLimiter, config::Config};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub cfg: Arc<Config>,
    pub limiter: Arc<RateLimiter>,
}
