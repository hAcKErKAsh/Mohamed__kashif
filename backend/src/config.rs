use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind: SocketAddr,
    pub static_dir: String,
    /// Set when served over HTTPS; marks cookies Secure and enables HSTS.
    pub secure_cookies: bool,
    /// Origins allowed to make state-changing requests (CSRF defence in depth).
    pub allowed_origins: Vec<String>,
    /// Trust X-Forwarded-For (only when behind a reverse proxy you control).
    pub trust_proxy: bool,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://campus:campus_dev@localhost/campus".into());
        let bind = std::env::var("BIND")
            .unwrap_or_else(|_| "0.0.0.0:8080".into())
            .parse()?;
        let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "../web/dist".into());
        let secure_cookies = std::env::var("SECURE_COOKIES").map(|v| v == "1" || v == "true").unwrap_or(false);
        let allowed_origins = std::env::var("ALLOWED_ORIGINS")
            .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default();
        let trust_proxy = std::env::var("TRUST_PROXY").map(|v| v == "1" || v == "true").unwrap_or(false);
        Ok(Self { database_url, bind, static_dir, secure_cookies, allowed_origins, trust_proxy })
    }
}
