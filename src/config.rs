use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub admin_key: String,
    pub country_code: String,
    pub host: String,
    pub port: u16,
    pub use_proxies: bool,
    pub trust_proxy: bool,
    pub proxies_file: PathBuf,
    pub fallback_to_direct: bool,
    pub max_retries: u32,
    pub discord_webhook_url: String,
    pub api_version: String,
    /// Upstash Redis REST base URL (empty = multi-host sync disabled).
    pub upstash_url: String,
    /// Upstash Redis REST token. Kept in memory only; redacted from Debug.
    pub upstash_token: String,
}

// Manual Debug: the REST token must never appear in logs.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &self.database_url)
            .field("admin_key", &"<redacted>")
            .field("country_code", &self.country_code)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("use_proxies", &self.use_proxies)
            .field("trust_proxy", &self.trust_proxy)
            .field("proxies_file", &self.proxies_file)
            .field("fallback_to_direct", &self.fallback_to_direct)
            .field("max_retries", &self.max_retries)
            .field("discord_webhook_url", &self.discord_webhook_url)
            .field("api_version", &self.api_version)
            .field("upstash_url", &self.upstash_url)
            .field("upstash_token", &"<redacted>")
            .finish()
    }
}

impl Config {
    pub fn from_env() -> Self {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "hifi.db".into());
        let admin_key = std::env::var("ADMIN_KEY").unwrap_or_else(|_| String::new());
        let country_code = std::env::var("COUNTRY_CODE").unwrap_or_else(|_| "US".into());
        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let port = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8000u16);
        let use_proxies = std::env::var("USE_PROXIES")
            .unwrap_or_default()
            .to_lowercase()
            == "true";
        let trust_proxy = std::env::var("TRUST_PROXY_HEADERS")
            .unwrap_or_else(|_| "true".into())
            .to_lowercase()
            == "true";
        let proxies_file = std::env::var("PROXIES_FILE")
            .unwrap_or_else(|_| "proxies.txt".into())
            .into();
        let fallback_to_direct = std::env::var("FALLBACK_TO_DIRECT_CONNECTION")
            .unwrap_or_default()
            .to_lowercase()
            == "true";
        let max_retries = std::env::var("MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2)
            .max(1);
        let discord_webhook_url = std::env::var("DISCORD_WEBHOOK_URL").unwrap_or_default();
        let upstash_url = std::env::var("UPSTASH_REDIS_REST_URL")
            .unwrap_or_default()
            .trim()
            .trim_end_matches('/')
            .to_string();
        let upstash_token = std::env::var("UPSTASH_REDIS_REST_TOKEN")
            .unwrap_or_default()
            .trim()
            .to_string();

        Self {
            database_url,
            admin_key,
            country_code,
            host,
            port,
            use_proxies,
            trust_proxy,
            proxies_file,
            fallback_to_direct,
            max_retries,
            discord_webhook_url,
            api_version: "2.10".into(),
            upstash_url,
            upstash_token,
        }
    }
}
