use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use serde_json::{json, Value};
use sqlx::SqlitePool;

pub struct RateLimitSettings {
    pub global_rps: AtomicU64,
    pub global_burst: AtomicU64,
    pub cooldown_429_secs: AtomicI64,
    pub cooldown_403_secs: AtomicI64,
}

impl RateLimitSettings {
    pub fn from_env() -> Self {
        Self {
            global_rps: AtomicU64::new(env_u64("RATE_LIMIT_RPS", 50)),
            global_burst: AtomicU64::new(env_u64("RATE_LIMIT_BURST", 100)),
            cooldown_429_secs: AtomicI64::new(env_i64("COOLDOWN_429_SECS", 60)),
            cooldown_403_secs: AtomicI64::new(env_i64("COOLDOWN_403_SECS", 120)),
        }
    }

    pub fn snapshot(&self) -> Value {
        json!({
            "global_rps": self.global_rps.load(Ordering::Relaxed),
            "global_burst": self.global_burst.load(Ordering::Relaxed),
            "cooldown_429_secs": self.cooldown_429_secs.load(Ordering::Relaxed),
            "cooldown_403_secs": self.cooldown_403_secs.load(Ordering::Relaxed),
        })
    }

    pub fn apply(&self, updates: &Value) -> Result<(), String> {
        if let Some(v) = opt_u64(updates, "global_rps")? {
            self.global_rps.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = opt_u64(updates, "global_burst")? {
            self.global_burst.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = opt_i64(updates, "cooldown_429_secs")? {
            self.cooldown_429_secs.store(v.max(0), Ordering::Relaxed);
        }
        if let Some(v) = opt_i64(updates, "cooldown_403_secs")? {
            self.cooldown_403_secs.store(v.max(0), Ordering::Relaxed);
        }
        Ok(())
    }

    pub async fn load_from_db(&self, db: &SqlitePool) {
        let rows: Result<Vec<(String, String)>, sqlx::Error> =
            sqlx::query_as("SELECT key, value FROM settings").fetch_all(db).await;
        let rows = match rows {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("Failed to load settings from DB: {}", e);
                return;
            }
        };
        for (key, value) in rows {
            match key.as_str() {
                "global_rps" => {
                    if let Ok(v) = value.parse::<u64>() {
                        self.global_rps.store(v, Ordering::Relaxed);
                    }
                }
                "global_burst" => {
                    if let Ok(v) = value.parse::<u64>() {
                        self.global_burst.store(v, Ordering::Relaxed);
                    }
                }
                "cooldown_429_secs" => {
                    if let Ok(v) = value.parse::<i64>() {
                        self.cooldown_429_secs.store(v, Ordering::Relaxed);
                    }
                }
                "cooldown_403_secs" => {
                    if let Ok(v) = value.parse::<i64>() {
                        self.cooldown_403_secs.store(v, Ordering::Relaxed);
                    }
                }
                _ => {}
            }
        }
    }

    pub async fn save_to_db(&self, db: &SqlitePool) {
        let entries = [
            ("global_rps", self.global_rps.load(Ordering::Relaxed).to_string()),
            ("global_burst", self.global_burst.load(Ordering::Relaxed).to_string()),
            ("cooldown_429_secs", self.cooldown_429_secs.load(Ordering::Relaxed).to_string()),
            ("cooldown_403_secs", self.cooldown_403_secs.load(Ordering::Relaxed).to_string()),
        ];
        for (key, value) in entries {
            let _ = sqlx::query(
                "INSERT INTO settings (key, value) VALUES (?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            )
            .bind(key)
            .bind(value)
            .execute(db)
            .await;
        }
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
        .max(1)
}

fn env_i64(key: &str, default: i64) -> i64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
        .max(0)
}

fn opt_u64(obj: &Value, key: &str) -> Result<Option<u64>, String> {
    match obj.get(key) {
        None => Ok(None),
        Some(v) if v.is_null() => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{} must be a positive integer", key)),
    }
}

fn opt_i64(obj: &Value, key: &str) -> Result<Option<i64>, String> {
    match obj.get(key) {
        None => Ok(None),
        Some(v) if v.is_null() => Ok(None),
        Some(v) => v
            .as_i64()
            .map(Some)
            .ok_or_else(|| format!("{} must be an integer", key)),
    }
}
