use std::path::PathBuf;

#[derive(Clone, Debug)]
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
    pub api_version: String,
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
            api_version: "2.10".into(),
        }
    }
}
