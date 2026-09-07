use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use tokio::sync::Mutex;

/// Discord-only ban/outage alerts. Empty webhook URL = disabled.
/// Same-type alerts are throttled to at most one per 15 minutes.
pub struct Notifier {
    webhook_url: String,
    client: reqwest::Client,
    last_sent: Mutex<HashMap<String, i64>>,
}

const MIN_INTERVAL_SECS: i64 = 900;

impl Notifier {
    pub fn new(webhook_url: String) -> Arc<Self> {
        if webhook_url.is_empty() {
            tracing::info!("Discord alerts disabled (DISCORD_WEBHOOK_URL not set)");
        } else {
            tracing::info!("Discord alerts enabled");
        }
        Arc::new(Self {
            webhook_url,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to build notifier HTTP client"),
            last_sent: Mutex::new(HashMap::new()),
        })
    }

    pub fn configured(&self) -> bool {
        !self.webhook_url.is_empty()
    }

    async fn send_throttled(&self, kind: &str, content: String) {
        if self.webhook_url.is_empty() {
            return;
        }
        {
            let mut last = self.last_sent.lock().await;
            let now = Utc::now().timestamp();
            if let Some(&prev) = last.get(kind) {
                if now - prev < MIN_INTERVAL_SECS {
                    tracing::debug!("Discord alert '{}' throttled", kind);
                    return;
                }
            }
            last.insert(kind.to_string(), now);
        }
        if let Err(e) = self
            .client
            .post(&self.webhook_url)
            .json(&json!({ "content": content }))
            .send()
            .await
        {
            tracing::warn!("Failed to send Discord alert: {}", e);
        }
    }

    /// Fired when Tidal 403s an account (suspension risk).
    pub async fn alert_403(&self, label: &str, healthy: usize, total: usize) {
        self.send_throttled(
            "403",
            format!(
                "🚨 **HiFi API: account 403**\n`{}` was forbidden by Tidal (possible suspension).\nHealthy accounts: {}/{}",
                label, healthy, total
            ),
        )
        .await;
    }

    /// Fired when no usable account remains.
    pub async fn alert_all_down(&self, total: usize) {
        self.send_throttled(
            "down",
            format!(
                "🛑 **HiFi API: all accounts down**\nNo active, non-rate-limited account available ({} total). Playback is returning 503.",
                total
            ),
        )
        .await;
    }

    /// Manual test from the admin panel (bypasses throttle).
    pub async fn send_test(&self) -> Result<(), String> {
        if self.webhook_url.is_empty() {
            return Err("DISCORD_WEBHOOK_URL is not set".into());
        }
        self.client
            .post(&self.webhook_url)
            .json(&json!({ "content": "✅ **HiFi API:** Discord alerts are working." }))
            .send()
            .await
            .map_err(|e| format!("Failed to send Discord alert: {}", e))?;
        Ok(())
    }
}
