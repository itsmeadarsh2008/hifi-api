use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use chrono::Utc;
use rand::Rng;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::AppError;

fn build_client(proxy_url: Option<&str>) -> Result<Client, String> {
    let mut builder = Client::builder()
        .gzip(true)
        .http2_prior_knowledge()
        .http2_adaptive_window(true)
        .pool_max_idle_per_host(500)
        .pool_idle_timeout(Duration::from_secs(30))
        .user_agent("okhttp/5.3.2");
    if let Some(url) = proxy_url {
        let proxy =
            reqwest::Proxy::all(url).map_err(|e| format!("Invalid proxy URL: {}", e))?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|e| format!("Failed to build HTTP client: {}", e))
}

pub struct ProxyManager {
    config: Arc<Config>,
    proxies: RwLock<Vec<String>>,
    client: ArcSwap<Client>,
    /// Proxy URL currently in use (None = direct connection).
    current: RwLock<Option<String>>,
    /// True once a usable client is confirmed (always true when proxies disabled).
    ready: AtomicBool,
    /// Consecutive failures on the current client.
    fails: AtomicU64,
    /// Last resolve attempt (unix secs) — throttles resolve storms.
    last_try: AtomicI64,
    /// Set while a background rotation is in flight.
    rotating: AtomicBool,
}

impl ProxyManager {
    pub fn new(config: Arc<Config>) -> Self {
        let proxies = if config.use_proxies {
            Self::load_proxies_from_file(&config.proxies_file)
        } else {
            Vec::new()
        };

        let direct = build_client(None).expect("Failed to build HTTP client");
        Self {
            config,
            proxies: RwLock::new(proxies),
            client: ArcSwap::from_pointee(direct),
            current: RwLock::new(None),
            // Direct mode is always ready; proxy mode resolves in the background.
            ready: AtomicBool::new(false),
            fails: AtomicU64::new(0),
            last_try: AtomicI64::new(0),
            rotating: AtomicBool::new(false),
        }
    }

    pub fn proxies_enabled(&self) -> bool {
        self.config.use_proxies
    }

    fn load_proxies_from_file(path: &std::path::Path) -> Vec<String> {
        if !path.exists() {
            tracing::warn!("Proxies file {:?} not found.", path);
            return Vec::new();
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to read proxies file: {}", e);
                return Vec::new();
            }
        };

        let proxies: Vec<String> = content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        tracing::info!("Loaded {} proxies from file", proxies.len());
        proxies
    }

    /// Current client, no questions asked. Prefer `working_client` for Tidal traffic.
    pub fn client(&self) -> Client {
        (**self.client.load()).clone()
    }

    /// Resolve a usable client for Tidal traffic.
    /// - Proxies disabled → direct, always.
    /// - Proxies enabled + ready → proxied.
    /// - Proxies enabled + not ready → quick resolve (throttled); direct only if
    ///   FALLBACK_TO_DIRECT_CONNECTION=true, else 503 so the home IP never leaks.
    pub async fn working_client(&self) -> Result<Client, AppError> {
        if !self.config.use_proxies {
            return Ok(self.client());
        }
        if self.ready.load(Ordering::Relaxed) {
            return Ok(self.client());
        }
        if self.try_resolve().await {
            return Ok(self.client());
        }
        if self.config.fallback_to_direct {
            tracing::warn!("No working proxy — falling back to direct connection (HOST IP MAY BE EXPOSED)");
            return Ok(self.client());
        }
        Err(AppError::ServiceUnavailable(
            "No working proxy available and direct fallback is disabled".into(),
        ))
    }

    /// Attempt one resolve, throttled to at most once per 30s. Returns ready state.
    async fn try_resolve(&self) -> bool {
        if self.ready.load(Ordering::Relaxed) {
            return true;
        }
        let now = Utc::now().timestamp();
        if now - self.last_try.load(Ordering::Relaxed) < 30 {
            return false;
        }
        self.last_try.store(now, Ordering::Relaxed);
        match self.get_working_proxy(None).await {
            Some(proxy) => {
                self.swap_to(Some(proxy)).await;
                true
            }
            None => false,
        }
    }

    async fn swap_to(&self, proxy: Option<String>) {
        match build_client(proxy.as_deref()) {
            Ok(client) => {
                self.client.store(Arc::new(client));
                *self.current.write().await = proxy.clone();
                self.ready.store(true, Ordering::Relaxed);
                self.fails.store(0, Ordering::Relaxed);
                match proxy {
                    Some(p) => tracing::info!("Proxy active: {}", mask_proxy(&p)),
                    None => tracing::info!("Proxy active: direct connection"),
                }
            }
            Err(e) => {
                tracing::error!("{}", e);
                self.ready.store(false, Ordering::Relaxed);
            }
        }
    }

    /// Kick off the initial resolve in the background (never blocks startup).
    pub fn spawn_initial_resolve(self: &Arc<Self>) {
        if !self.config.use_proxies {
            self.ready.store(true, Ordering::Relaxed);
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            if this.try_resolve().await {
                tracing::info!("Initial proxy resolve succeeded");
            } else {
                tracing::warn!(
                    "No working proxy at startup{}. Retrying in the background.",
                    if this.config.fallback_to_direct {
                        " — direct fallback enabled (HOST IP MAY BE EXPOSED)"
                    } else {
                        " — Tidal traffic will 503 until one works"
                    }
                );
            }
        });
    }

    /// Record a successful Tidal round-trip.
    pub fn note_success(&self) {
        self.fails.store(0, Ordering::Relaxed);
    }

    /// Record a failed Tidal round-trip; rotate after 3 consecutive failures.
    pub fn note_failure(self: &Arc<Self>) {
        if !self.config.use_proxies {
            return;
        }
        let fails = self.fails.fetch_add(1, Ordering::Relaxed) + 1;
        if fails >= 3 {
            self.fails.store(0, Ordering::Relaxed);
            self.rotate();
        }
    }

    /// Swap to the next proxy immediately (round-robin); health is verified
    /// in the background so a failing request never blocks on proxy tests.
    fn rotate(self: &Arc<Self>) {
        if !self
            .rotating
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            let next = {
                let proxies = this.proxies.read().await;
                if proxies.is_empty() {
                    None
                } else {
                    let current = this.current.read().await;
                    let start = current
                        .as_ref()
                        .and_then(|c| proxies.iter().position(|p| p == c))
                        .map(|i| (i + 1) % proxies.len())
                        .unwrap_or(0);
                    Some(proxies[start].clone())
                }
            };
            match next {
                Some(proxy) => {
                    tracing::warn!("Rotating proxy after failures → {}", mask_proxy(&proxy));
                    this.swap_to(Some(proxy)).await;
                    // Verify in the background; if bad, mark not-ready so the
                    // next request resolves a tested one.
                    let check = this.current.read().await.clone();
                    if let Some(url) = check {
                        if !this.test_proxy(&url).await {
                            tracing::warn!("Rotated proxy failed health check: {}", mask_proxy(&url));
                            this.ready.store(false, Ordering::Relaxed);
                        }
                    }
                }
                None => {
                    tracing::warn!("Proxy rotation requested but pool is empty");
                    this.ready.store(false, Ordering::Relaxed);
                }
            }
            this.rotating.store(false, Ordering::Relaxed);
        });
    }

    pub async fn test_proxy(&self, proxy_url: &str) -> bool {
        let client = match Client::builder()
            .proxy(reqwest::Proxy::all(proxy_url).unwrap())
            .timeout(Duration::from_secs(5))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        match client.get("http://example.com").send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    pub async fn get_working_proxy(&self, avoid_proxy: Option<&str>) -> Option<String> {
        let proxies = self.proxies.read().await;
        if proxies.is_empty() {
            return None;
        }

        let mut shuffled = proxies.clone();
        {
            let mut rng = rand::thread_rng();
            for i in (1..shuffled.len()).rev() {
                let j = rng.gen_range(0..=i);
                shuffled.swap(i, j);
            }
        }

        if let Some(avoid) = avoid_proxy {
            shuffled.retain(|p| p != avoid);
        }

        if shuffled.is_empty() {
            return None;
        }

        let candidates: Vec<&str> = shuffled.iter().take(3).map(|s| s.as_str()).collect();

        for proxy in candidates {
            if self.test_proxy(proxy).await {
                return Some(proxy.to_string());
            }
        }

        None
    }

    pub async fn status(&self) -> Value {
        let proxies = self.proxies.read().await;
        let current = self.current.read().await;
        json!({
            "enabled": self.config.use_proxies,
            "ready": self.ready.load(Ordering::Relaxed),
            "current": current.as_ref().map(|p| mask_proxy(p)),
            "pool_size": proxies.len(),
            "consecutive_fails": self.fails.load(Ordering::Relaxed),
            "fallback_to_direct": self.config.fallback_to_direct,
        })
    }
}

fn mask_proxy(url: &str) -> String {
    // Show host:port but hide userinfo (http://user:pass@host:port → http://***@host:port).
    match url.split_once("://") {
        Some((scheme, rest)) => match rest.rsplit_once('@') {
            Some((_, host)) => format!("{}://***@{}", scheme, host),
            None => url.to_string(),
        },
        None => url.to_string(),
    }
}
