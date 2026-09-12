use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rand::Rng;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::AppError;
use crate::upstash::UpstashStore;
use crate::AppState;

pub struct ApiKeyState {
    pub id: String,
    pub label: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub quota: AtomicU64, // 0 = unlimited
    pub used: AtomicU64,
    pub is_active: AtomicBool,
    /// Last definition mutation (unix ts). Drives newest-wins merges.
    /// `used` is synced separately and never overwritten by merges.
    pub updated_at: AtomicI64,
}

fn hash_key(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn generate_key() -> String {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 24];
    rng.fill(&mut bytes);
    format!("hifi_{}", hex_encode(&bytes))
}

pub struct ApiKeyManager {
    keys: RwLock<Vec<Arc<ApiKeyState>>>,
    db: Option<SqlitePool>,
    /// Shared cross-instance state (None = single-host mode, skip sync).
    upstash: std::sync::OnceLock<Arc<UpstashStore>>,
}

impl ApiKeyManager {
    pub fn new(db: Option<SqlitePool>) -> Self {
        Self {
            keys: RwLock::new(Vec::new()),
            db,
            upstash: std::sync::OnceLock::new(),
        }
    }

    /// Attach shared state once at startup (before serving).
    pub fn set_upstash(&self, store: Option<Arc<UpstashStore>>) {
        if let Some(s) = store {
            let _ = self.upstash.set(s);
        }
    }

    pub async fn load_from_db(&self) -> Result<(), AppError> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok(()),
        };
        let rows: Vec<(String, String, String, String, i64, i64, i32)> = sqlx::query_as(
            "SELECT id, label, key_hash, key_prefix, quota, used, is_active FROM api_keys ORDER BY created_at ASC",
        )
        .fetch_all(db)
        .await?;
        let mut keys = self.keys.write().await;
        for (id, label, key_hash, key_prefix, quota, used, is_active) in rows {
            keys.push(Arc::new(ApiKeyState {
                id,
                label,
                key_hash,
                key_prefix,
                quota: AtomicU64::new(quota.max(0) as u64),
                used: AtomicU64::new(used.max(0) as u64),
                is_active: AtomicBool::new(is_active != 0),
                updated_at: AtomicI64::new(0),
            }));
        }
        tracing::info!("Loaded {} API keys from database", keys.len());
        Ok(())
    }

    /// Create a key. Returns (state, raw_key) — the raw key is shown ONCE.
    pub async fn create(&self, label: String, quota: u64) -> Result<(Arc<ApiKeyState>, String), AppError> {
        let raw = generate_key();
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        let state = Arc::new(ApiKeyState {
            id: id.clone(),
            label: label.clone(),
            key_hash: hash_key(&raw),
            key_prefix: raw.chars().take(12).collect(),
            quota: AtomicU64::new(quota),
            used: AtomicU64::new(0),
            is_active: AtomicBool::new(true),
            updated_at: AtomicI64::new(now),
        });
        if let Some(db) = &self.db {
            sqlx::query(
                "INSERT INTO api_keys (id, label, key_hash, key_prefix, quota, used, is_active, created_at)
                 VALUES (?, ?, ?, ?, ?, 0, 1, ?)",
            )
            .bind(&id)
            .bind(&label)
            .bind(&state.key_hash)
            .bind(&state.key_prefix)
            .bind(quota as i64)
            .bind(now)
            .execute(db)
            .await?;
        }
        self.keys.write().await.push(state.clone());
        self.push_key_to_redis(&state).await;
        Ok((state, raw))
    }

    /// Drop all in-memory state and reload from the database (used after restore).
    pub async fn reload_from_db(&self) -> Result<(), AppError> {
        self.keys.write().await.clear();
        self.load_from_db().await?;
        // Converge with the fleet (backup restores only touch SQLite).
        self.sync_usage_from_redis().await;
        self.merge_keys_from_redis().await;
        Ok(())
    }

    pub async fn list(&self) -> Vec<Arc<ApiKeyState>> {
        self.keys.read().await.clone()
    }

    pub async fn active_count(&self) -> usize {
        self.keys
            .read()
            .await
            .iter()
            .filter(|k| k.is_active.load(Ordering::Relaxed))
            .count()
    }

    pub async fn set_active(&self, id: &str, active: bool) -> Result<(), AppError> {
        let keys = self.keys.read().await;
        let key = keys
            .iter()
            .find(|k| k.id == id)
            .ok_or_else(|| AppError::NotFound(format!("API key {} not found", id)))?;
        key.is_active.store(active, Ordering::Relaxed);
        let now = chrono::Utc::now().timestamp();
        key.updated_at.store(now, Ordering::Relaxed);
        if let Some(db) = &self.db {
            sqlx::query("UPDATE api_keys SET is_active = ? WHERE id = ?")
                .bind(active as i32)
                .bind(id)
                .execute(db)
                .await?;
        }
        self.push_key_to_redis(key).await;
        Ok(())
    }

    pub async fn remove(&self, id: &str) -> Result<(), AppError> {
        {
            let mut keys = self.keys.write().await;
            let before = keys.len();
            keys.retain(|k| k.id != id);
            if keys.len() == before {
                return Err(AppError::NotFound(format!("API key {} not found", id)));
            }
        }
        if let Some(db) = &self.db {
            sqlx::query("DELETE FROM api_keys WHERE id = ?")
                .bind(id)
                .execute(db)
                .await?;
        }
        // Remove from the shared backup too (or the next merge resurrects it).
        if let Some(store) = self.upstash.get() {
            store.del_many(&[UpstashStore::k_apikeydef(id), UpstashStore::k_apikey(id)]).await;
            store.srem(&UpstashStore::k_apikeys_set(), id).await;
        }
        Ok(())
    }

    // --- Redis definition sync (hashes/flags only — raw keys are never stored) ---

    /// Serialize one key definition. Contains only the hash (irreversible),
    /// never the raw secret. `used` is synced separately via counters.
    fn key_to_json(key: &ApiKeyState) -> String {
        serde_json::json!({
            "id": key.id,
            "label": key.label,
            "key_hash": key.key_hash,
            "key_prefix": key.key_prefix,
            "quota": key.quota.load(Ordering::Relaxed),
            "is_active": key.is_active.load(Ordering::Relaxed),
            "updated_at": key.updated_at.load(Ordering::Relaxed),
        })
        .to_string()
    }

    /// Publish one definition + index entry (no expiry: backup semantics).
    async fn push_key_to_redis(&self, key: &ApiKeyState) {
        let store = match self.upstash.get() {
            Some(s) => s,
            None => return,
        };
        let payload = Self::key_to_json(key);
        store.set(&UpstashStore::k_apikeydef(&key.id), &payload, None).await;
        store.sadd(&UpstashStore::k_apikeys_set(), &key.id).await;
    }

    /// Union-merge key definitions from Redis: add unknown ids (restores
    /// wiped hosts), adopt newer records for known ids (`used` is preserved
    /// and only ever raised by the usage reconcile). Ids absent from the
    /// index are never resurrected. Called at startup and on the 60s tick.
    pub async fn merge_keys_from_redis(&self) {
        let store = match self.upstash.get() {
            Some(s) => s.clone(),
            None => return,
        };
        let remote_ids = match store.smembers(&UpstashStore::k_apikeys_set()).await {
            Some(ids) => ids,
            None => return,
        };
        if !remote_ids.is_empty() {
            let redis_keys: Vec<String> =
                remote_ids.iter().map(|id| UpstashStore::k_apikeydef(id)).collect();
            let values = store.mget(&redis_keys).await;
            let mut keys = self.keys.write().await;
            for (id, payload) in remote_ids.iter().zip(values.iter()) {
                let raw = match payload {
                    Some(r) => r,
                    None => continue,
                };
                let v: serde_json::Value = match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let str_field = |k: &str| {
                    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
                };
                let remote_updated = v.get("updated_at").and_then(|x| x.as_i64()).unwrap_or(0);
                if let Some(k) = keys.iter().find(|k| &k.id == id) {
                    if remote_updated <= k.updated_at.load(Ordering::Relaxed) {
                        continue;
                    }
                    // The hash identifies the secret itself: a differing hash
                    // under a known id means a different secret — never adopt
                    // blindly, the local record stays authoritative.
                    // (Labels are intentionally not merged: cosmetic only, and
                    // the field has no interior mutability. Restores still
                    // carry the original label.)
                    let remote_hash = str_field("key_hash");
                    if remote_hash.len() == 64 && remote_hash != k.key_hash {
                        tracing::debug!("Skipping API-key merge for {}: hash mismatch", id);
                        continue;
                    }
                    k.quota.store(
                        v.get("quota").and_then(|x| x.as_u64()).unwrap_or(0),
                        Ordering::Relaxed,
                    );
                    k.is_active.store(
                        v.get("is_active").and_then(|x| x.as_bool()).unwrap_or(true),
                        Ordering::Relaxed,
                    );
                    k.updated_at.store(remote_updated, Ordering::Relaxed);
                    if let Some(db) = &self.db {
                        let _ = sqlx::query(
                            "UPDATE api_keys SET label = ?, quota = ?, is_active = ? WHERE id = ?",
                        )
                        .bind(&k.label)
                        .bind(k.quota.load(Ordering::Relaxed) as i64)
                        .bind(k.is_active.load(Ordering::Relaxed) as i32)
                        .bind(&k.id)
                        .execute(db)
                        .await;
                    }
                } else {
                    let hash = str_field("key_hash");
                    if hash.len() != 64 {
                        continue; // refuse malformed records
                    }
                    let state = Arc::new(ApiKeyState {
                        id: id.clone(),
                        label: str_field("label"),
                        key_hash: hash,
                        key_prefix: str_field("key_prefix"),
                        quota: AtomicU64::new(
                            v.get("quota").and_then(|x| x.as_u64()).unwrap_or(0),
                        ),
                        used: AtomicU64::new(0),
                        is_active: AtomicBool::new(
                            v.get("is_active").and_then(|x| x.as_bool()).unwrap_or(true),
                        ),
                        updated_at: AtomicI64::new(remote_updated),
                    });
                    if let Some(db) = &self.db {
                        let now = chrono::Utc::now().timestamp();
                        let _ = sqlx::query(
                            "INSERT INTO api_keys (id, label, key_hash, key_prefix, quota, used, is_active, created_at)
                             VALUES (?, ?, ?, ?, ?, 0, ?, ?)
                             ON CONFLICT(id) DO NOTHING",
                        )
                        .bind(&state.id)
                        .bind(&state.label)
                        .bind(&state.key_hash)
                        .bind(&state.key_prefix)
                        .bind(state.quota.load(Ordering::Relaxed) as i64)
                        .bind(state.is_active.load(Ordering::Relaxed) as i32)
                        .bind(now)
                        .execute(db)
                        .await;
                    }
                    tracing::info!("Restored API key {} from Redis backup", state.label);
                    keys.push(state);
                }
            }
            let mut to_push = Vec::new();
            for k in keys.iter() {
                if !remote_ids.contains(&k.id) {
                    to_push.push(k.clone());
                }
            }
            drop(keys);
            for key in to_push {
                self.push_key_to_redis(&key).await;
            }
        }
    }

    /// Publish the full local roster and delete remote orphans, so a backup
    /// restore wins fleet-wide instead of being re-merged away.
    pub async fn publish_all_to_redis(&self) {
        let store = match self.upstash.get() {
            Some(s) => s.clone(),
            None => return,
        };
        let keys = self.keys.read().await;
        for k in keys.iter() {
            self.push_key_to_redis(k).await;
        }
        let local_ids: Vec<String> = keys.iter().map(|k| k.id.clone()).collect();
        drop(keys);
        if let Some(remote_ids) = store.smembers(&UpstashStore::k_apikeys_set()).await {
            let mut orphan_keys = Vec::new();
            for id in &remote_ids {
                if !local_ids.contains(id) {
                    orphan_keys.push(UpstashStore::k_apikeydef(id));
                    orphan_keys.push(UpstashStore::k_apikey(id));
                    store.srem(&UpstashStore::k_apikeys_set(), id).await;
                }
            }
            store.del_many(&orphan_keys).await;
        }
    }

    /// Opt-in lockdown: while no active key exists the API stays open
    /// (backward compatible). Once the first key is created, every
    /// non-exempt route requires X-API-Key or the owner X-Admin-Key.
    pub async fn enforce_api_key(
        State(state): State<AppState>,
        req: Request<Body>,
        next: Next,
    ) -> Response {
        let path = req.uri().path();
        if path == "/" || path == "/health" || path.starts_with("/admin") {
            return next.run(req).await;
        }
        if state.api_keys.active_count().await == 0 {
            return next.run(req).await;
        }

        let admin_key = req
            .headers()
            .get("X-Admin-Key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !state.config.admin_key.is_empty() && admin_key == state.config.admin_key {
            return next.run(req).await;
        }

        match req.headers().get("X-API-Key").and_then(|v| v.to_str().ok()) {
            Some(raw) => match state.api_keys.check_and_consume(raw).await {
                Ok(_) => next.run(req).await,
                Err(_) => (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"detail": "Invalid API key, inactive, or quota exhausted"})),
                )
                    .into_response(),
            },
            None => (
                StatusCode::UNAUTHORIZED,
                Json(json!({"detail": "Missing X-API-Key header"})),
            )
                .into_response(),
        }
    }

    /// Validate a raw key and consume one quota unit. Ok(label) on success.
    pub async fn check_and_consume(&self, raw: &str) -> Result<String, AppError> {
        let hash = hash_key(raw);
        let keys = self.keys.read().await;
        let key = keys
            .iter()
            .find(|k| k.key_hash == hash && k.is_active.load(Ordering::Relaxed))
            .ok_or_else(|| AppError::Unauthorized("Invalid or inactive API key".into()))?;
        let quota = key.quota.load(Ordering::Relaxed);
        if quota > 0 {
            let used = key.used.fetch_add(1, Ordering::Relaxed) + 1;
            if used > quota {
                return Err(AppError::Unauthorized("API key quota exhausted".into()));
            }
            if let Some(db) = &self.db {
                let _ = sqlx::query("UPDATE api_keys SET used = ? WHERE id = ?")
                    .bind(used as i64)
                    .bind(&key.id)
                    .execute(db)
                    .await;
            }
            // Fleet-wide quota accounting without blocking the request.
            if let Some(store) = self.upstash.get().cloned() {
                let redis_key = UpstashStore::k_apikey(&key.id);
                tokio::spawn(async move {
                    let _ = store.incrby(&redis_key, 1).await;
                });
            }
        }
        Ok(key.label.clone())
    }

    /// Pull fleet-wide consumed-quota totals and raise local counters to at
    /// least the global value, so quotas are enforced across instances
    /// within ~a reconcile interval. Called on the 60s tick and reloads.
    pub async fn sync_usage_from_redis(&self) {
        let store = match self.upstash.get() {
            Some(s) => s.clone(),
            None => return,
        };
        let ids: Vec<(String, u64)> = {
            let keys = self.keys.read().await;
            keys.iter()
                .filter(|k| k.quota.load(Ordering::Relaxed) > 0)
                .map(|k| (k.id.clone(), k.used.load(Ordering::Relaxed)))
                .collect()
        };
        if ids.is_empty() {
            return;
        }
        let redis_keys: Vec<String> =
            ids.iter().map(|(id, _)| UpstashStore::k_apikey(id)).collect();
        let values = store.mget(&redis_keys).await;
        let keys = self.keys.read().await;
        for ((id, _), remote) in ids.iter().zip(values.iter()) {
            let count = match remote {
                Some(v) => v.parse::<i64>().unwrap_or(0).max(0) as u64,
                None => continue,
            };
            if let Some(k) = keys.iter().find(|k| &k.id == id) {
                let _ = k.used.fetch_max(count, Ordering::Relaxed);
            }
        }
    }
}
