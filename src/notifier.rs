use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};
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

    fn embed(title: &str, description: &str, color: u32, fields: Vec<Value>) -> Value {
        json!({
            "embeds": [{
                "title": title,
                "description": description,
                "color": color,
                "fields": fields,
                "footer": { "text": "HiFi API" },
                "timestamp": Utc::now().to_rfc3339(),
            }]
        })
    }

    async fn send_throttled(&self, kind: &str, payload: Value) {
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
            .json(&payload)
            .send()
            .await
        {
            tracing::warn!("Failed to send Discord alert: {}", e);
        }
    }

    /// Fired when Tidal 403s an account (suspension risk).
    pub async fn alert_403(&self, label: &str, healthy: usize, total: usize) {
        let payload = Self::embed(
            "🚨 Account 403 — suspension risk",
            "Tidal forbade this account. It has been parked; check whether it needs fresh credentials.",
            0xF85149,
            vec![
                json!({"name": "Account", "value": label, "inline": true}),
                json!({"name": "Healthy", "value": format!("{}/{}", healthy, total), "inline": true}),
            ],
        );
        self.send_throttled("403", payload).await;
    }

    /// Fired when no usable account remains.
    pub async fn alert_all_down(&self, total: usize) {
        let payload = Self::embed(
            "🛑 All accounts down",
            "No active, non-rate-limited account available. Playback is returning 503.",
            0xF85149,
            vec![json!({"name": "Total accounts", "value": total.to_string(), "inline": true})],
        );
        self.send_throttled("down", payload).await;
    }

    /// Fired when auto-heal recovers an account.
    pub async fn alert_healed(&self, label: &str, healthy: usize, total: usize) {
        let payload = Self::embed(
            "✅ Account recovered",
            "Auto-heal refreshed credentials and returned the account to rotation.",
            0x3FB950,
            vec![
                json!({"name": "Account", "value": label, "inline": true}),
                json!({"name": "Healthy", "value": format!("{}/{}", healthy, total), "inline": true}),
            ],
        );
        self.send_throttled("healed", payload).await;
    }

    /// Manual test from the admin panel (bypasses throttle).
    pub async fn send_test(&self) -> Result<(), String> {
        if self.webhook_url.is_empty() {
            return Err("DISCORD_WEBHOOK_URL is not set".into());
        }
        let payload = Self::embed(
            "✅ Discord alerts working",
            "Test alert from the HiFi API admin panel. Ban and outage alerts will arrive as embeds like this one.",
            0x3FB950,
            vec![],
        );
        self.client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Failed to send Discord alert: {}", e))?;
        Ok(())
    }
}
