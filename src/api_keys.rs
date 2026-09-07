use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
use crate::AppState;

pub struct ApiKeyState {
    pub id: String,
    pub label: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub quota: AtomicU64, // 0 = unlimited
    pub used: AtomicU64,
    pub is_active: AtomicBool,
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
}

impl ApiKeyManager {
    pub fn new(db: Option<SqlitePool>) -> Self {
        Self {
            keys: RwLock::new(Vec::new()),
            db,
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
        Ok((state, raw))
    }

    /// Drop all in-memory state and reload from the database (used after restore).
    pub async fn reload_from_db(&self) -> Result<(), AppError> {
        self.keys.write().await.clear();
        self.load_from_db().await
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
        if let Some(db) = &self.db {
            sqlx::query("UPDATE api_keys SET is_active = ? WHERE id = ?")
                .bind(active as i32)
                .bind(id)
                .execute(db)
                .await?;
        }
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
        Ok(())
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
        }
        Ok(key.label.clone())
    }
}
