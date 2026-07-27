use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use reqwest::Client;
use tokio::sync::RwLock;

use crate::config::Config;

pub struct ProxyManager {
    #[allow(dead_code)]
    config: Arc<Config>,
    proxies: RwLock<Vec<String>>,
    last_known_good: RwLock<Option<String>>,
}

impl ProxyManager {
    pub fn new(config: Arc<Config>) -> Self {
        let proxies = if config.use_proxies {
            Self::load_proxies_from_file(&config.proxies_file)
        } else {
            Vec::new()
        };

        Self {
            config,
            proxies: RwLock::new(proxies),
            last_known_good: RwLock::new(None),
        }
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

    pub async fn get_working_proxy(
        &self,
        avoid_proxy: Option<&str>,
    ) -> Option<String> {
        let proxies = self.proxies.read().await;
        if proxies.is_empty() {
            return None;
        }

        {
            let last_good = self.last_known_good.read().await;
            if let Some(ref proxy) = *last_good {
                if avoid_proxy.map_or(true, |a| proxy != a) {
                    if self.test_proxy(proxy).await {
                        return Some(proxy.clone());
                    }
                }
            }
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

        let candidates: Vec<&str> = shuffled.iter().take(10).map(|s| s.as_str()).collect();

        for proxy in candidates {
            if self.test_proxy(proxy).await {
                let proxy_str = proxy.to_string();
                let mut last_good = self.last_known_good.write().await;
                *last_good = Some(proxy_str.clone());
                return Some(proxy_str);
            }
        }

        None
    }
}
