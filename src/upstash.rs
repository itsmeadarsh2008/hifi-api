//! Shared cross-instance state via Upstash Redis (REST API).
//!
//! When `UPSTASH_REDIS_REST_URL` + `UPSTASH_REDIS_REST_TOKEN` are set, every
//! instance coordinates through one Redis database instead of drifting apart:
//! rate-limit settings, per-account daily budgets, 429/403 cooldown parks,
//! Tidal access tokens, API-key usage, and the global upstream throttle.
//!
//! Design rules:
//! - **Fail-open.** Redis is an accelerator, not a dependency: every call has
//!   a short timeout and any error degrades to single-host behavior (today's
//!   semantics). Nothing here may fail a request or panic.
//! - **Local fast path stays.** Hot paths (account selection, token fast
//!   path, governor buckets) never block on Redis; sync is write-through on
//!   state changes plus periodic reconcile on the existing 30s/60s ticks.
//! - **No new crates.** Uses the existing `reqwest` client against the
//!   Upstash REST API (`GET /CMD/args…`, `POST /pipeline`).
//! - **Secrets stay in env.** The token lives only in memory; values are
//!   never logged (only key names and counts, at debug level).
//!
//! Key layout (prefix `hifi`):
//! - `hifi:settings:<name>` — rate-limit settings (plain strings)
//! - `hifi:usage:<utc_day>:<account_id>` — daily Tidal-call counters (int)
//! - `hifi:cooldown:<account_id>` — unix `rate_limited_until` (int)
//! - `hifi:token:<account_id>` — `{"t": access_token, "e": expires_at}`
//! - `hifi:apikey:<key_id>` — consumed quota units (int)
//! - `hifi:throttle:<epoch_sec>` — global fixed-window upstream counter
//!
//! Deliberately NOT synced: account credentials (stay in per-host SQLite —
//! syncing secrets widens exposure), the metadata response cache (latency;
//! per-host L1 is fine), IP reputation, request log, proxy state.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

const REST_TIMEOUT: Duration = Duration::from_secs(3);
const PREFIX: &str = "hifi";

/// Percent-encode a single REST path segment (RFC3986 unreserved set passes
/// through; everything else — including `/`, spaces, `+`, `=` in tokens —
/// is encoded so keys/values survive the URL path).
fn enc(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for b in seg.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn result_str(v: &Value) -> Option<String> {
    match v.get("result")? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn result_i64(v: &Value) -> Option<i64> {
    match v.get("result")? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    }
}

#[derive(Clone)]
pub struct UpstashStore {
    client: reqwest::Client,
    base: String,
    token: String,
    /// Cached liveness probe: (reachable, unix timestamp). Health endpoints
    /// must never block on a dead Redis, so at most one real PING happens
    /// per interval and every other caller gets the cached verdict.
    last_check: Arc<std::sync::Mutex<(bool, i64)>>,
}

impl UpstashStore {
    /// Build from env. `None` unless both vars are set and well-formed —
    /// callers treat `None` as "single-host mode, skip all sync".
    pub fn from_env() -> Option<Arc<Self>> {
        let base = std::env::var("UPSTASH_REDIS_REST_URL")
            .unwrap_or_default()
            .trim()
            .trim_end_matches('/')
            .to_string();
        let token = std::env::var("UPSTASH_REDIS_REST_TOKEN")
            .unwrap_or_default()
            .trim()
            .to_string();
        if base.is_empty() || token.is_empty() {
            return None;
        }
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            tracing::warn!("UPSTASH_REDIS_REST_URL must be an http(s) URL; Redis sync disabled");
            return None;
        }
        let client = reqwest::Client::builder().timeout(REST_TIMEOUT).build().ok()?;
        Some(Arc::new(Self {
            client,
            base,
            token,
            last_check: Arc::new(std::sync::Mutex::new((false, 0))),
        }))
    }

    /// Liveness probe used once at startup (logs the outcome, never fatal).
    pub async fn ping(&self) -> bool {
        match self.cmd(vec!["PING".to_string()]).await {
            Some(body) => result_str(&body).as_deref() == Some("PONG"),
            None => false,
        }
    }

    /// Cached liveness for health endpoints: returns the last verdict when
    /// it is fresher than `min_interval_secs`, otherwise PINGs once and
    /// caches the outcome. Never slower than one PING, usually instant.
    pub async fn is_alive(&self, min_interval_secs: i64) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if let Ok(guard) = self.last_check.lock() {
            if now - guard.1 < min_interval_secs.max(1) {
                return guard.0;
            }
        }
        let ok = self.ping().await;
        if let Ok(mut guard) = self.last_check.lock() {
            *guard = (ok, now);
        }
        ok
    }

    fn single_url(&self, parts: &[String]) -> String {
        let mut u = self.base.clone();
        for p in parts {
            u.push('/');
            u.push_str(&enc(p));
        }
        u
    }

    /// One command via `GET /CMD/arg…`. Returns the `result` payload, or
    /// `None` on any transport/Redis error (fail-open).
    async fn cmd(&self, parts: Vec<String>) -> Option<Value> {
        let res = self
            .client
            .get(self.single_url(&parts))
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }
        let body: Value = res.json().await.ok()?;
        if body.get("error").is_some() {
            tracing::debug!("upstash error for {}: {}", parts[0], body);
            return None;
        }
        Some(body)
    }

    /// Batch via `POST /pipeline` with `[["CMD","arg"…], …]`. Returns the
    /// per-command payloads in order (shorter on transport error).
    async fn pipeline(&self, cmds: Vec<Vec<String>>) -> Option<Vec<Value>> {
        if cmds.is_empty() {
            return Some(Vec::new());
        }
        let res = self
            .client
            .post(format!("{}/pipeline", self.base))
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&cmds)
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }
        let body: Value = res.json().await.ok()?;
        body.as_array().cloned()
    }

    // --- primitives ---

    pub async fn get(&self, key: &str) -> Option<String> {
        let body = self.cmd(vec!["GET".into(), key.into()]).await?;
        result_str(&body)
    }

    pub async fn set(&self, key: &str, value: &str, ex_secs: Option<u64>) {
        let mut parts = vec!["SET".to_string(), key.to_string(), value.to_string()];
        if let Some(ex) = ex_secs {
            parts.push("EX".to_string());
            parts.push(ex.max(1).to_string());
        }
        let _ = self.cmd(parts).await;
    }

    /// Set only if absent. Returns true when this call created the key.
    pub async fn set_nx(&self, key: &str, value: &str, ex_secs: Option<u64>) -> bool {
        let mut parts = vec!["SET".to_string(), key.to_string(), value.to_string()];
        if let Some(ex) = ex_secs {
            parts.push("EX".to_string());
            parts.push(ex.max(1).to_string());
        }
        parts.push("NX".to_string());
        self.cmd(parts)
            .await
            .and_then(|b| result_str(&b))
            .map(|r| r == "OK")
            .unwrap_or(false)
    }

    pub async fn incr(&self, key: &str) -> Option<i64> {
        let body = self.cmd(vec!["INCR".into(), key.into()]).await?;
        result_i64(&body)
    }

    /// INCR + EXPIRE in one round trip (counters self-clean).
    pub async fn incr_expire(&self, key: &str, ex_secs: u64) -> Option<i64> {
        let out = self
            .pipeline(vec![
                vec!["INCR".into(), key.into()],
                vec!["EXPIRE".into(), key.into(), ex_secs.max(1).to_string()],
            ])
            .await?;
        out.first().and_then(result_i64)
    }

    pub async fn incrby(&self, key: &str, delta: i64) -> Option<i64> {
        let body = self
            .cmd(vec!["INCRBY".into(), key.into(), delta.to_string()])
            .await?;
        result_i64(&body)
    }

    pub async fn mget(&self, keys: &[String]) -> Vec<Option<String>> {
        let cmds: Vec<Vec<String>> =
            keys.iter().map(|k| vec!["GET".into(), k.clone()]).collect();
        match self.pipeline(cmds).await {
            Some(out) => out.iter().map(result_str).collect(),
            None => vec![None; keys.len()],
        }
    }

    pub async fn del_many(&self, keys: &[String]) {
        if keys.is_empty() {
            return;
        }
        let cmds: Vec<Vec<String>> =
            keys.iter().map(|k| vec!["DEL".into(), k.clone()]).collect();
        let _ = self.pipeline(cmds).await;
    }

    // --- key builders (single source of truth for the layout) ---

    pub fn k_settings(name: &str) -> String {
        format!("{PREFIX}:settings:{name}")
    }

    pub fn k_usage(day: i64, account_id: &str) -> String {
        format!("{PREFIX}:usage:{day}:{account_id}")
    }

    pub fn k_cooldown(account_id: &str) -> String {
        format!("{PREFIX}:cooldown:{account_id}")
    }

    pub fn k_token(account_id: &str) -> String {
        format!("{PREFIX}:token:{account_id}")
    }

    pub fn k_apikey(key_id: &str) -> String {
        format!("{PREFIX}:apikey:{key_id}")
    }

    pub fn k_throttle(epoch_sec: i64) -> String {
        format!("{PREFIX}:throttle:{epoch_sec}")
    }
}

#[cfg(test)]
mod tests {
    use super::{enc, result_i64, result_str, UpstashStore};
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn path_segments_encoded() {
        assert_eq!(enc("hifi:usage:123:abc"), "hifi%3Ausage%3A123%3Aabc");
        assert_eq!(enc("a/b c+d=e"), "a%2Fb%20c%2Bd%3De");
        assert_eq!(enc("AZaz09-_.~"), "AZaz09-_.~");
        // Multi-byte UTF-8 encodes per byte.
        assert_eq!(enc("é"), "%C3%A9");
    }

    #[test]
    fn result_parsing_shapes() {
        assert_eq!(result_str(&json!({"result": "OK"})), Some("OK".into()));
        assert_eq!(result_str(&json!({"result": 42})), Some("42".into()));
        assert_eq!(result_str(&json!({"result": null})), None);
        assert_eq!(result_str(&json!({"error": "NOAUTH"})), None);
        assert_eq!(result_i64(&json!({"result": 7})), Some(7));
        assert_eq!(result_i64(&json!({"result": "12"})), Some(12));
        assert_eq!(result_i64(&json!({"result": "PONG"})), None);
    }

    #[test]
    fn key_layout_stable() {
        assert_eq!(UpstashStore::k_settings("tidal_rps"), "hifi:settings:tidal_rps");
        assert_eq!(UpstashStore::k_usage(42, "id"), "hifi:usage:42:id");
        assert_eq!(UpstashStore::k_cooldown("id"), "hifi:cooldown:id");
        assert_eq!(UpstashStore::k_token("id"), "hifi:token:id");
        assert_eq!(UpstashStore::k_apikey("id"), "hifi:apikey:id");
        assert_eq!(UpstashStore::k_throttle(99), "hifi:throttle:99");
    }

        #[test]
    fn disabled_without_env() {        let saved_url = std::env::var("UPSTASH_REDIS_REST_URL").ok();
        let saved_tok = std::env::var("UPSTASH_REDIS_REST_TOKEN").ok();
        unsafe {
            std::env::remove_var("UPSTASH_REDIS_REST_URL");
            std::env::remove_var("UPSTASH_REDIS_REST_TOKEN");
        }
        assert!(UpstashStore::from_env().is_none());
        unsafe {
            std::env::set_var("UPSTASH_REDIS_REST_URL", "https://example.upstash.io");
        }
        // Token still missing → still disabled.
        assert!(UpstashStore::from_env().is_none());
        unsafe {
            match saved_url {
                Some(v) => std::env::set_var("UPSTASH_REDIS_REST_URL", v),
                None => std::env::remove_var("UPSTASH_REDIS_REST_URL"),
            }
            match saved_tok {
                Some(v) => std::env::set_var("UPSTASH_REDIS_REST_TOKEN", v),
                None => std::env::remove_var("UPSTASH_REDIS_REST_TOKEN"),
            }
        }
    }

    #[tokio::test]
    async fn liveness_uses_cache_without_network() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mk = |verdict: bool| UpstashStore {
            client: reqwest::Client::new(),
            base: "https://invalid.invalid".into(),
            token: "x".into(),
            last_check: Arc::new(std::sync::Mutex::new((verdict, now))),
        };
        // Fresh cache entries are served without any HTTP request (an
        // unresolvable host would fail if actually contacted).
        assert!(mk(true).is_alive(15).await);
        assert!(!mk(false).is_alive(15).await);
    }
}
