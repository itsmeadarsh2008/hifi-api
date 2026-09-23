//! Shared cross-instance state via Redis.
//!
//! Two backends, one key layout (`hifi:*`):
//! - **Upstash REST** (`UPSTASH_REDIS_REST_URL` + `UPSTASH_REDIS_REST_TOKEN`):
//!   the private fleet's database, spoken over HTTPS (`GET /CMD/args…`,
//!   `POST /pipeline`).
//! - **Native** (`REDIS_POOL=rediss://…`): this host's pool Redis/Valkey,
//!   spoken as raw RESP2 over TLS. Selected when set — it wins
//!   over the Upstash pair, because a host belongs to exactly one pool.
//!   (The previous name `PUBLIC_POOL_REDIS_URL` is still honored as a
//!   deprecated fallback.)
//!
//! When every instance of a fleet points at the same database they coordinate
//! through it instead of drifting apart: app settings, Tidal access tokens,
//! API-key usage, and credential backups.
//!
//! Design rules:
//! - **Fail-open.** Redis is an accelerator, not a dependency: every call has
//!   a short timeout and any error degrades to single-host behavior (today's
//!   semantics). Nothing here may fail a request or panic.
//! - **Local fast path stays.** Hot paths (account selection, token fast
//!   path) never block on Redis; sync is write-through on
//!   state changes plus periodic reconcile on the existing 60s ticks.
//! - **Secrets stay in env.** Tokens live only in memory; values are
//!   never logged (only key names and counts, at debug level). The native URL
//!   embeds its password, so it is never logged either — see `backend_kind`.
//!
//! Key layout (prefix `hifi`), identical on both backends:
//! - `hifi:settings:<name>` — app settings (plain strings)
//! - `hifi:token:<account_id>` — `{"t": access_token, "e": expires_at}`
//! - `hifi:apikey:<key_id>` — consumed quota units (int)
//! - `hifi:account:<account_id>` — account record JSON (credential backup)
//! - `hifi:accounts` — SET of known account ids (restore index)
//! - `hifi:apikeydef:<key_id>` — API-key definition JSON (hash/flags/quota)
//! - `hifi:apikeys` — SET of known API-key ids (restore index)
//!
//! Deliberately NOT synced: the metadata response cache (latency; per-host
//! L1 is fine), request log, proxy pool state (per-host
//! egress by nature). Everything else — including account credentials and
//! API-key definitions — is backed up so wiped hosts restore themselves.

use std::sync::Arc;
use std::time::Duration;

use redis::AsyncCommands;
use serde_json::Value;

const REST_TIMEOUT: Duration = Duration::from_secs(3);
const NATIVE_TIMEOUT: Duration = Duration::from_secs(3);
const PREFIX: &str = "hifi";

/// Env var selecting the native backend (this host's pool Redis).
/// Wins over the Upstash pair when set; a host belongs to exactly one pool.
const NATIVE_URL_ENV: &str = "REDIS_POOL";

/// Previous name of [`NATIVE_URL_ENV`], still honored as a fallback with a
/// deprecation warning so un-updated hosts keep syncing. Remove once every
/// host has moved (then this becomes a hard rename).
const NATIVE_URL_ENV_LEGACY: &str = "PUBLIC_POOL_REDIS_URL";

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

/// Host part of an http(s) URL without scheme, path, or credentials.
fn host_of_url(url: &str) -> &str {
    let s = url.split("://").nth(1).unwrap_or(url);
    s.split('/').next().unwrap_or(s)
}

/// Host (never credentials) of a native client for display.
fn native_host(client: &redis::Client) -> String {
    match &client.get_connection_info().addr {
        redis::ConnectionAddr::Tcp(host, port) => format!("{}:{}", host, port),
        redis::ConnectionAddr::TcpTls { host, port, .. } => format!("{}:{}", host, port),
        _ => "unix-socket".to_string(),
    }
}

/// Pool selector (`USE=private|public`) for the shared Valkey instance:
/// private→db 0, public→db 1, overriding any db in the URL. Returns the
/// (pool label, db override); unset means "URL/default decides".
/// Anything else is an error — loading the wrong pool's accounts is worse
/// than no sync at all.
fn resolve_pool() -> Result<(Option<String>, Option<i64>), String> {
    let raw = std::env::var("USE").unwrap_or_default();
    let v = raw.trim().to_lowercase();
    if v.is_empty() {
        return Ok((None, None));
    }
    match v.as_str() {
        "private" => Ok((Some("private".to_string()), Some(0))),
        "public" => Ok((Some("public".to_string()), Some(1))),
        _ => Err(raw),
    }
}

enum Backend {
    Rest {
        client: reqwest::Client,
        base: String,
        token: String,
    },
    Native {
        client: redis::Client,
        mgr: tokio::sync::OnceCell<redis::aio::ConnectionManager>,
        /// Pool label from `USE` (None = the URL/default decided).
        pool: Option<String>,
        /// Logical db actually used (for display).
        db: i64,
    },
}

#[derive(Clone)]
pub struct UpstashStore {
    backend: Arc<Backend>,
    /// Cached liveness probe: (reachable, unix timestamp). Health endpoints
    /// must never block on a dead Redis, so at most one real PING happens
    /// per interval and every other caller gets the cached verdict.
    last_check: Arc<std::sync::Mutex<(bool, i64)>>,
}

impl UpstashStore {
    fn new(backend: Backend) -> Arc<Self> {
        Arc::new(Self {
            backend: Arc::new(backend),
            last_check: Arc::new(std::sync::Mutex::new((false, 0))),
        })
    }

    /// Build from env. `None` unless exactly one backend is configured —
    /// callers treat `None` as "single-host mode, skip all sync".
    ///
    /// When `REDIS_POOL` is present but unusable the store stays
    /// disabled rather than silently syncing to the other pool.
    pub fn from_env() -> Option<Arc<Self>> {
        let native_url = std::env::var(NATIVE_URL_ENV)
            .unwrap_or_default()
            .trim()
            .to_string();
        let native_url = if native_url.is_empty() {
            // Transitional fallback for hosts still carrying the old name.
            let legacy = std::env::var(NATIVE_URL_ENV_LEGACY)
                .unwrap_or_default()
                .trim()
                .to_string();
            if !legacy.is_empty() {
                tracing::warn!(
                    "{} is deprecated, rename it to {}",
                    NATIVE_URL_ENV_LEGACY,
                    NATIVE_URL_ENV
                );
            }
            legacy
        } else {
            native_url
        };
        if !native_url.is_empty() {
            if !(native_url.starts_with("redis://") || native_url.starts_with("rediss://")) {
                tracing::warn!(
                    "{} must be a redis:// or rediss:// URL; Redis sync disabled",
                    NATIVE_URL_ENV
                );
                return None;
            }
            let (pool, db_override) = match resolve_pool() {
                Ok(v) => v,
                Err(bad) => {
                    tracing::warn!(
                        "USE={:?} invalid (want public|private); Redis sync disabled",
                        bad
                    );
                    return None;
                }
            };
            let mut info: redis::ConnectionInfo = match native_url.parse() {
                Ok(i) => i,
                Err(e) => {
                    tracing::warn!("{} invalid ({}); Redis sync disabled", NATIVE_URL_ENV, e);
                    return None;
                }
            };
            if let Some(db) = db_override {
                info.redis.db = db;
            }
            let db = info.redis.db;
            match redis::Client::open(info) {
                Ok(client) => {
                    tracing::info!(
                        "Redis pool: {} (db {})",
                        pool.as_deref().unwrap_or("url-default"),
                        db
                    );
                    return Some(Self::new(Backend::Native {
                        client,
                        mgr: tokio::sync::OnceCell::new(),
                        pool,
                        db,
                    }));
                }
                Err(e) => {
                    tracing::warn!("{} invalid ({}); Redis sync disabled", NATIVE_URL_ENV, e);
                    return None;
                }
            }
        }

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
        Some(Self::new(Backend::Rest {
            client,
            base,
            token,
        }))
    }

    /// Backend label for startup logs. Never includes secrets (the native URL
    /// embeds its password, so only the kind is exposed).
    pub fn backend_kind(&self) -> &'static str {
        match self.backend.as_ref() {
            Backend::Rest { .. } => "upstash-rest",
            Backend::Native { .. } => "native-redis",
        }
    }

    /// Safe endpoint label for the admin panel: backend kind + host + db,
    /// plus the `USE` pool when one selected it. Never includes credentials
    /// — the native URL embeds its password and the REST bearer token is
    /// secret too.
    pub fn describe(&self) -> String {
        match self.backend.as_ref() {
            Backend::Rest { base, .. } => {
                format!("upstash-rest @ {}", host_of_url(base))
            }
            Backend::Native { client, pool, db, .. } => {
                let mut s = format!("native-redis @ {} (db {})", native_host(client), db);
                if let Some(p) = pool {
                    s.push_str(&format!(", USE={}", p));
                }
                s
            }
        }
    }

    /// Liveness probe used once at startup (logs the outcome, never fatal).
    pub async fn ping(&self) -> bool {
        match self.backend.as_ref() {
            Backend::Rest { .. } => match self.cmd(vec!["PING".to_string()]).await {
                Some(body) => result_str(&body).as_deref() == Some("PONG"),
                None => false,
            },
            Backend::Native { .. } => {
                matches!(
                    self.native_run(|mut m| async move {
                        let v: String = redis::cmd("PING").query_async(&mut m).await?;
                        Ok(v)
                    })
                    .await
                    .as_deref(),
                    Some("PONG")
                )
            }
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

    fn single_url(base: &str, parts: &[String]) -> String {
        let mut u = base.to_string();
        for p in parts {
            u.push('/');
            u.push_str(&enc(p));
        }
        u
    }

    /// One command via `GET /CMD/arg…`. Returns the `result` payload, or
    /// `None` on any transport/Redis error (fail-open). REST only.
    async fn cmd(&self, parts: Vec<String>) -> Option<Value> {
        let Backend::Rest { client, base, token } = self.backend.as_ref() else {
            return None;
        };
        let res = client
            .get(Self::single_url(base, &parts))
            .header("Authorization", format!("Bearer {}", token))
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
    /// per-command payloads in order (shorter on transport error). REST only.
    async fn pipeline(&self, cmds: Vec<Vec<String>>) -> Option<Vec<Value>> {
        let Backend::Rest { client, base, token } = self.backend.as_ref() else {
            return None;
        };
        if cmds.is_empty() {
            return Some(Vec::new());
        }
        let res = client
            .post(format!("{}/pipeline", base))
            .header("Authorization", format!("Bearer {}", token))
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

    /// Run one native command against a managed connection with connect +
    /// command timeouts. `None` on any error (fail-open). Native only.
    async fn native_run<T, F, Fut>(&self, f: F) -> Option<T>
    where
        F: FnOnce(redis::aio::ConnectionManager) -> Fut,
        Fut: std::future::Future<Output = redis::RedisResult<T>>,
    {
        let Backend::Native { client, mgr, .. } = self.backend.as_ref() else {
            return None;
        };
        let m = if let Some(m) = mgr.get() {
            m.clone()
        } else {
            match tokio::time::timeout(
                NATIVE_TIMEOUT,
                redis::aio::ConnectionManager::new(client.clone()),
            )
            .await
            {
                Ok(Ok(m)) => {
                    // A lost race just connects twice; harmless.
                    let _ = mgr.set(m.clone());
                    m
                }
                Ok(Err(e)) => {
                    tracing::debug!("native redis connect failed: {}", e);
                    return None;
                }
                Err(_) => {
                    tracing::debug!("native redis connect timed out");
                    return None;
                }
            }
        };
        match tokio::time::timeout(NATIVE_TIMEOUT, f(m)).await {
            Ok(Ok(v)) => Some(v),
            Ok(Err(e)) => {
                tracing::debug!("native redis command failed: {}", e);
                None
            }
            Err(_) => {
                tracing::debug!("native redis command timed out");
                None
            }
        }
    }

    // --- primitives ---

    pub async fn get(&self, key: &str) -> Option<String> {
        match self.backend.as_ref() {
            Backend::Rest { .. } => {
                let body = self.cmd(vec!["GET".into(), key.into()]).await?;
                result_str(&body)
            }
            Backend::Native { .. } => {
                self.native_run(|mut m| async move {
                    let v: Option<String> = m.get(key).await?;
                    Ok(v)
                })
                .await?
            }
        }
    }

    pub async fn set(&self, key: &str, value: &str, ex_secs: Option<u64>) {
        match self.backend.as_ref() {
            Backend::Rest { .. } => {
                let mut parts = vec!["SET".to_string(), key.to_string(), value.to_string()];
                if let Some(ex) = ex_secs {
                    parts.push("EX".to_string());
                    parts.push(ex.max(1).to_string());
                }
                let _ = self.cmd(parts).await;
            }
            Backend::Native { .. } => {
                let ex = ex_secs.map(|e| e.max(1));
                self.native_run(|mut m| async move {
                    if let Some(n) = ex {
                        let (): () = m.set_ex(key, value, n).await?;
                    } else {
                        let (): () = m.set(key, value).await?;
                    }
                    Ok(())
                })
                .await;
            }
        }
    }

    /// Set only if absent. Returns true when this call created the key.
    pub async fn set_nx(&self, key: &str, value: &str, ex_secs: Option<u64>) -> bool {
        match self.backend.as_ref() {
            Backend::Rest { .. } => {
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
            Backend::Native { .. } => {
                let ex = ex_secs.map(|e| e.max(1));
                self.native_run(|mut m| async move {
                    let mut opts =
                        redis::SetOptions::default().conditional_set(redis::ExistenceCheck::NX);
                    if let Some(n) = ex {
                        opts = opts.with_expiration(redis::SetExpiry::EX(n));
                    }
                    let v: Option<String> = m.set_options(key, value, opts).await?;
                    Ok(v)
                })
                .await
                .map(|v| v.as_deref() == Some("OK"))
                .unwrap_or(false)
            }
        }
    }

    pub async fn incr(&self, key: &str) -> Option<i64> {
        match self.backend.as_ref() {
            Backend::Rest { .. } => {
                let body = self.cmd(vec!["INCR".into(), key.into()]).await?;
                result_i64(&body)
            }
            Backend::Native { .. } => {
                self.native_run(|mut m| async move {
                    let n: i64 = m.incr(key, 1).await?;
                    Ok(n)
                })
                .await
            }
        }
    }

    /// INCR + EXPIRE in one round trip (counters self-clean).
    pub async fn incr_expire(&self, key: &str, ex_secs: u64) -> Option<i64> {
        match self.backend.as_ref() {
            Backend::Rest { .. } => {
                let out = self
                    .pipeline(vec![
                        vec!["INCR".into(), key.into()],
                        vec!["EXPIRE".into(), key.into(), ex_secs.max(1).to_string()],
                    ])
                    .await?;
                out.first().and_then(result_i64)
            }
            Backend::Native { .. } => {
                self.native_run(|mut m| async move {
                    let (n, _): (i64, bool) = redis::pipe()
                        .cmd("INCR")
                        .arg(key)
                        .cmd("EXPIRE")
                        .arg(key)
                        .arg(ex_secs.max(1))
                        .query_async(&mut m)
                        .await?;
                    Ok(n)
                })
                .await
            }
        }
    }

    pub async fn incrby(&self, key: &str, delta: i64) -> Option<i64> {
        match self.backend.as_ref() {
            Backend::Rest { .. } => {
                let body = self
                    .cmd(vec!["INCRBY".into(), key.into(), delta.to_string()])
                    .await?;
                result_i64(&body)
            }
            Backend::Native { .. } => {
                self.native_run(|mut m| async move {
                    let n: i64 = m.incr(key, delta).await?;
                    Ok(n)
                })
                .await
            }
        }
    }

    pub async fn mget(&self, keys: &[String]) -> Vec<Option<String>> {
        match self.backend.as_ref() {
            Backend::Rest { .. } => {
                let cmds: Vec<Vec<String>> =
                    keys.iter().map(|k| vec!["GET".into(), k.clone()]).collect();
                match self.pipeline(cmds).await {
                    Some(out) => out.iter().map(result_str).collect(),
                    None => vec![None; keys.len()],
                }
            }
            Backend::Native { .. } => {
                if keys.is_empty() {
                    return Vec::new();
                }
                self.native_run(|mut m| async move {
                    let mut c = redis::cmd("MGET");
                    for k in keys {
                        c.arg(k);
                    }
                    let v: Vec<Option<String>> = c.query_async(&mut m).await?;
                    Ok(v)
                })
                .await
                .unwrap_or_else(|| vec![None; keys.len()])
            }
        }
    }

    pub async fn del_many(&self, keys: &[String]) {
        match self.backend.as_ref() {
            Backend::Rest { .. } => {
                if keys.is_empty() {
                    return;
                }
                let cmds: Vec<Vec<String>> =
                    keys.iter().map(|k| vec!["DEL".into(), k.clone()]).collect();
                let _ = self.pipeline(cmds).await;
            }
            Backend::Native { .. } => {
                if keys.is_empty() {
                    return;
                }
                self.native_run(|mut m| async move {
                    let mut c = redis::cmd("DEL");
                    for k in keys {
                        c.arg(k);
                    }
                    let (): () = c.query_async(&mut m).await?;
                    Ok(())
                })
                .await;
            }
        }
    }

    // --- set helpers (membership indexes for restorable collections) ---

    pub async fn sadd(&self, set: &str, member: &str) {
        match self.backend.as_ref() {
            Backend::Rest { .. } => {
                let _ = self.cmd(vec!["SADD".into(), set.into(), member.into()]).await;
            }
            Backend::Native { .. } => {
                self.native_run(|mut m| async move {
                    let (): () = m.sadd(set, member).await?;
                    Ok(())
                })
                .await;
            }
        }
    }

    pub async fn srem(&self, set: &str, member: &str) {
        match self.backend.as_ref() {
            Backend::Rest { .. } => {
                let _ = self.cmd(vec!["SREM".into(), set.into(), member.into()]).await;
            }
            Backend::Native { .. } => {
                self.native_run(|mut m| async move {
                    let (): () = m.srem(set, member).await?;
                    Ok(())
                })
                .await;
            }
        }
    }

    pub async fn smembers(&self, set: &str) -> Option<Vec<String>> {
        match self.backend.as_ref() {
            Backend::Rest { .. } => {
                let body = self.cmd(vec!["SMEMBERS".into(), set.into()]).await?;
                match body.get("result")? {
                    Value::Array(items) => Some(
                        items
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect(),
                    ),
                    _ => None,
                }
            }
            Backend::Native { .. } => {
                self.native_run(|mut m| async move {
                    let v: Vec<String> = m.smembers(set).await?;
                    Ok(v)
                })
                .await
            }
        }
    }

    // --- key builders (single source of truth for the layout) ---

    pub fn k_settings(name: &str) -> String {
        format!("{PREFIX}:settings:{name}")
    }

    pub fn k_token(account_id: &str) -> String {
        format!("{PREFIX}:token:{account_id}")
    }

    pub fn k_apikey(key_id: &str) -> String {
        format!("{PREFIX}:apikey:{key_id}")
    }

    pub fn k_account(account_id: &str) -> String {
        format!("{PREFIX}:account:{account_id}")
    }

    pub fn k_accounts_set() -> String {
        format!("{PREFIX}:accounts")
    }

    pub fn k_apikeydef(key_id: &str) -> String {
        format!("{PREFIX}:apikeydef:{key_id}")
    }

    pub fn k_apikeys_set() -> String {
        format!("{PREFIX}:apikeys")
    }
}

#[cfg(test)]
mod tests {
    use super::{enc, result_i64, result_str, UpstashStore};
    use serde_json::json;
    use std::sync::Arc;

    /// Serialize env-mutating tests in this module: they share the process
    /// environment and must not interleave.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvGuard {
        saved: Vec<(String, Option<String>)>,
    }

    impl EnvGuard {
        fn take(keys: &[&str]) -> Self {
            let saved = keys
                .iter()
                .map(|k| (k.to_string(), std::env::var(k).ok()))
                .collect();
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, v) in self.saved.drain(..) {
                unsafe {
                    match v {
                        Some(val) => std::env::set_var(&k, val),
                        None => std::env::remove_var(&k),
                    }
                }
            }
        }
    }

    const STORE_KEYS: &[&str] = &[
        "UPSTASH_REDIS_REST_URL",
        "UPSTASH_REDIS_REST_TOKEN",
        "REDIS_POOL",
        "PUBLIC_POOL_REDIS_URL",
        "USE",
    ];

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
        assert_eq!(UpstashStore::k_settings("atmos_mode"), "hifi:settings:atmos_mode");
        assert_eq!(UpstashStore::k_token("id"), "hifi:token:id");
        assert_eq!(UpstashStore::k_apikey("id"), "hifi:apikey:id");
        assert_eq!(UpstashStore::k_account("id"), "hifi:account:id");
        assert_eq!(UpstashStore::k_accounts_set(), "hifi:accounts");
        assert_eq!(UpstashStore::k_apikeydef("id"), "hifi:apikeydef:id");
        assert_eq!(UpstashStore::k_apikeys_set(), "hifi:apikeys");
    }

    #[test]
    fn describe_exposes_host_but_never_secrets() {
        // REST backend: host shown, bearer token never.
        let rest = UpstashStore::new(super::Backend::Rest {
            client: reqwest::Client::new(),
            base: "https://glad-kitten-177729.upstash.io".into(),
            token: "super-secret-token".into(),
        });
        let label = rest.describe();
        assert!(label.contains("upstash-rest"), "{}", label);
        assert!(label.contains("glad-kitten-177729.upstash.io"), "{}", label);
        assert!(!label.contains("super-secret-token"), "{}", label);
        // Native backend: host shown, URL-embedded password never.
        // Client::open only parses (no I/O), so this is offline-safe.
        let client =
            redis::Client::open("rediss://default:hunter2@db.example.dev:6379").unwrap();
        let native = UpstashStore::new(super::Backend::Native {
            client,
            mgr: tokio::sync::OnceCell::new(),
            pool: None,
            db: 0,
        });
        let label = native.describe();
        assert!(label.contains("native-redis"), "{}", label);
        assert!(label.contains("db.example.dev"), "{}", label);
        assert!(!label.contains("hunter2"), "{}", label);
        assert!(!label.contains("default"), "{}", label);
    }


    #[test]
    fn disabled_without_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::take(STORE_KEYS);
        unsafe {
            std::env::remove_var("UPSTASH_REDIS_REST_URL");
            std::env::remove_var("UPSTASH_REDIS_REST_TOKEN");
            std::env::remove_var("REDIS_POOL");
            std::env::remove_var("PUBLIC_POOL_REDIS_URL");
            std::env::remove_var("USE");
        }
        assert!(UpstashStore::from_env().is_none());
        unsafe {
            std::env::set_var("UPSTASH_REDIS_REST_URL", "https://example.upstash.io");
        }
        // Token still missing → still disabled.
        assert!(UpstashStore::from_env().is_none());
    }

    #[test]
    fn use_private_selects_db0_overriding_url_db() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::take(STORE_KEYS);
        unsafe {
            std::env::remove_var("UPSTASH_REDIS_REST_URL");
            std::env::remove_var("UPSTASH_REDIS_REST_TOKEN");
            std::env::remove_var("PUBLIC_POOL_REDIS_URL");
            // Even when the URL points at db 7, USE=private wins with db 0.
            std::env::set_var("REDIS_POOL", "rediss://default:pw@db.example.dev:6379/7");
            std::env::set_var("USE", "private");
        }
        let store = UpstashStore::from_env().expect("USE=private should build");
        assert_eq!(store.backend_kind(), "native-redis");
        let label = store.describe();
        assert!(label.contains("db 0"), "{}", label);
        assert!(label.contains("USE=private"), "{}", label);
    }

    #[test]
    fn use_public_selects_db1() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::take(STORE_KEYS);
        unsafe {
            std::env::remove_var("UPSTASH_REDIS_REST_URL");
            std::env::remove_var("UPSTASH_REDIS_REST_TOKEN");
            std::env::remove_var("PUBLIC_POOL_REDIS_URL");
            std::env::set_var("REDIS_POOL", "rediss://default:pw@db.example.dev:6379");
            std::env::set_var("USE", "PUBLIC");
        }
        let store = UpstashStore::from_env().expect("USE=public should build");
        let label = store.describe();
        assert!(label.contains("db 1"), "{}", label);
        assert!(label.contains("USE=public"), "{}", label);
    }

    #[test]
    fn use_unset_respects_url_db() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::take(STORE_KEYS);
        unsafe {
            std::env::remove_var("UPSTASH_REDIS_REST_URL");
            std::env::remove_var("UPSTASH_REDIS_REST_TOKEN");
            std::env::remove_var("PUBLIC_POOL_REDIS_URL");
            std::env::remove_var("USE");
            std::env::set_var("REDIS_POOL", "rediss://default:pw@db.example.dev:6379/3");
        }
        let store = UpstashStore::from_env().expect("explicit db should build");
        let label = store.describe();
        assert!(label.contains("db 3"), "{}", label);
        assert!(!label.contains("USE="), "{}", label);
    }

    #[test]
    fn invalid_use_disables_rather_than_wrong_pool() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::take(STORE_KEYS);
        unsafe {
            std::env::remove_var("UPSTASH_REDIS_REST_URL");
            std::env::remove_var("UPSTASH_REDIS_REST_TOKEN");
            std::env::remove_var("PUBLIC_POOL_REDIS_URL");
            std::env::set_var("REDIS_POOL", "rediss://default:pw@db.example.dev:6379");
            // A typo must never silently load the other pool's accounts.
            std::env::set_var("USE", "privat");
        }
        assert!(UpstashStore::from_env().is_none());
    }

    #[test]
    fn native_backend_wins_over_upstash() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::take(STORE_KEYS);
        unsafe {
            // Both backends configured: the native pool must win — a host
            // belongs to exactly one pool.
            std::env::set_var("UPSTASH_REDIS_REST_URL", "https://example.upstash.io");
            std::env::set_var("UPSTASH_REDIS_REST_TOKEN", "dummy");
            std::env::remove_var("USE");
            std::env::set_var(
                "REDIS_POOL",
                "rediss://default:pw@public.example.dev:6379",
            );
        }
        // Client::open only parses (no I/O), so this is offline-safe.
        let store = UpstashStore::from_env().expect("native backend should build");
        assert_eq!(store.backend_kind(), "native-redis");
    }

    #[test]
    fn legacy_var_still_selects_native() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::take(STORE_KEYS);
        unsafe {
            std::env::remove_var("UPSTASH_REDIS_REST_URL");
            std::env::remove_var("UPSTASH_REDIS_REST_TOKEN");
            std::env::remove_var("REDIS_POOL");
            std::env::remove_var("USE");
            // Hosts not yet renamed keep syncing via the deprecated name.
            std::env::set_var(
                "PUBLIC_POOL_REDIS_URL",
                "rediss://default:pw@legacy.example.dev:6379",
            );
        }
        let store = UpstashStore::from_env().expect("legacy var should still work");
        assert_eq!(store.backend_kind(), "native-redis");
        assert!(store.describe().contains("legacy.example.dev"));
    }

    #[test]
    fn new_var_wins_over_legacy() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::take(STORE_KEYS);
        unsafe {
            std::env::remove_var("UPSTASH_REDIS_REST_URL");
            std::env::remove_var("UPSTASH_REDIS_REST_TOKEN");
            std::env::remove_var("USE");
            std::env::set_var("REDIS_POOL", "rediss://default:pw@new.example.dev:6379");
            std::env::set_var(
                "PUBLIC_POOL_REDIS_URL",
                "rediss://default:pw@legacy.example.dev:6379",
            );
        }
        let store = UpstashStore::from_env().expect("native backend should build");
        assert_eq!(store.backend_kind(), "native-redis");
        assert!(store.describe().contains("new.example.dev"));
    }

    #[test]
    fn invalid_native_url_disables() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::take(STORE_KEYS);
        unsafe {
            std::env::remove_var("UPSTASH_REDIS_REST_URL");
            std::env::remove_var("UPSTASH_REDIS_REST_TOKEN");
            std::env::remove_var("REDIS_POOL");
            std::env::remove_var("PUBLIC_POOL_REDIS_URL");
            std::env::remove_var("USE");
            // Present but unusable: stay disabled rather than syncing nowhere.
            std::env::set_var("REDIS_POOL", "not-a-redis-url");
        }
        assert!(UpstashStore::from_env().is_none());
    }

    #[test]
    fn wrong_scheme_native_url_disables() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::take(STORE_KEYS);
        unsafe {
            std::env::remove_var("UPSTASH_REDIS_REST_URL");
            std::env::remove_var("UPSTASH_REDIS_REST_TOKEN");
            std::env::remove_var("REDIS_POOL");
            std::env::remove_var("PUBLIC_POOL_REDIS_URL");
            std::env::remove_var("USE");
            // An Upstash REST URL pasted into the wrong var must not sync.
            std::env::set_var("REDIS_POOL", "https://example.upstash.io");
        }
        assert!(UpstashStore::from_env().is_none());
    }

    #[tokio::test]
    async fn liveness_uses_cache_without_network() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mk = |verdict: bool| UpstashStore {
            backend: Arc::new(super::Backend::Rest {
                client: reqwest::Client::new(),
                base: "https://invalid.invalid".into(),
                token: "x".into(),
            }),
            last_check: Arc::new(std::sync::Mutex::new((verdict, now))),
        };
        // Fresh cache entries are served without any HTTP request (an
        // unresolvable host would fail if actually contacted).
        assert!(mk(true).is_alive(15).await);
        assert!(!mk(false).is_alive(15).await);
    }
}
