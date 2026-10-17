use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    http::{header, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use tower_http::{
    compression::CompressionLayer,
    limit::RequestBodyLimitLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;

use lyceum::{auth::RateLimiter, config::Config, engine, routes, seed, state::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "lyceum=info,tower_http=warn".into()))
        .init();

    let cfg = Config::from_env()?;
    let db = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&cfg.database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&db).await?;

    if std::env::args().any(|a| a == "--seed") || std::env::var("SEED_DEMO").is_ok_and(|v| v == "1") {
        seed::run(&db).await?;
    }

    let state = AppState { db: db.clone(), cfg: Arc::new(cfg.clone()), limiter: RateLimiter::new() };
    spawn_background(state.clone());

    let app = app(state);
    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    tracing::info!("Lyceum listening on http://{}", cfg.bind);
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

fn app(state: AppState) -> Router {
    let static_dir = state.cfg.static_dir.clone();
    let spa = ServeDir::new(&static_dir).fallback(ServeFile::new(format!("{static_dir}/index.html")));
    let secure = state.cfg.secure_cookies;
    Router::new()
        .nest("/api", routes::api().fallback(api_not_found))
        .route("/healthz", get(health))
        .fallback_service(spa)
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(CompressionLayer::new())
        .layer(middleware::from_fn(move |req, next| security_headers(req, next, secure)))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health(axum::extract::State(st): axum::extract::State<AppState>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i32>("select 1").fetch_one(&st.db).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "ok": false }))),
    }
}

async fn api_not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({ "error": { "code": "not_found", "message": "not found" } })))
}

async fn security_headers(req: Request<axum::body::Body>, next: Next, secure: bool) -> Response {
    let is_api = req.uri().path().starts_with("/api");
    let is_asset = req.uri().path().starts_with("/assets/");
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
             font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'; object-src 'none'",
        ),
    );
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("strict-origin-when-cross-origin"));
    h.insert("permissions-policy", HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"));
    h.insert("cross-origin-opener-policy", HeaderValue::from_static("same-origin"));
    if secure {
        h.insert(header::STRICT_TRANSPORT_SECURITY, HeaderValue::from_static("max-age=63072000; includeSubDomains"));
    }
    if is_api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    } else if is_asset {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable"));
    } else {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    res
}

fn spawn_background(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            if let Err(e) = engine::escalate_overdue(&state.db).await {
                tracing::warn!(error = ?e, "escalation sweep failed");
            }
            let _ = sqlx::query("delete from sessions where expires_at < now() - interval '7 days' or revoked_at < now() - interval '7 days'")
                .execute(&state.db)
                .await;
            state.limiter.sweep(Duration::from_secs(3600));
        }
    });
}
