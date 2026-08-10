use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use serde_json::{json, Value};
use sqlx::SqlitePool;

pub struct RateLimitSettings {
    pub ip_rps: AtomicU64,
    pub ip_burst: AtomicU64,
    pub cooldown_429_secs: AtomicI64,
    pub cooldown_403_secs: AtomicI64,
}

impl RateLimitSettings {
    pub fn from_env() -> Self {
        Self {
            ip_rps: AtomicU64::new(env_u64("RATE_LIMIT_RPS", 50)),
            ip_burst: AtomicU64::new(env_u64("RATE_LIMIT_BURST", 100)),
            cooldown_429_secs: AtomicI64::new(env_i64("COOLDOWN_429_SECS", 60)),
            cooldown_403_secs: AtomicI64::new(env_i64("COOLDOWN_403_SECS", 120)),
        }
    }

    pub fn snapshot(&self) -> Value {
        json!({
            "ip_rps": self.ip_rps.load(Ordering::Relaxed),
            "ip_burst": self.ip_burst.load(Ordering::Relaxed),
            "cooldown_429_secs": self.cooldown_429_secs.load(Ordering::Relaxed),
            "cooldown_403_secs": self.cooldown_403_secs.load(Ordering::Relaxed),
        })
    }

    pub fn apply(&self, updates: &Value) -> Result<(), String> {
        if let Some(v) = first_opt_u64(updates, &["ip_rps", "global_rps"])? {
            self.ip_rps.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["ip_burst", "global_burst"])? {
            self.ip_burst.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_i64(updates, &["cooldown_429_secs"])? {
            self.cooldown_429_secs.store(v.max(0), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_i64(updates, &["cooldown_403_secs"])? {
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
            let v_u64 = value.parse::<u64>().ok();
            let v_i64 = value.parse::<i64>().ok();
            match key.as_str() {
                "ip_rps" | "global_rps" => {
                    if let Some(v) = v_u64 {
                        self.ip_rps.store(v, Ordering::Relaxed);
                    }
                }
                "ip_burst" | "global_burst" => {
                    if let Some(v) = v_u64 {
                        self.ip_burst.store(v, Ordering::Relaxed);
                    }
                }
                "cooldown_429_secs" => {
                    if let Some(v) = v_i64 {
                        self.cooldown_429_secs.store(v, Ordering::Relaxed);
                    }
                }
                "cooldown_403_secs" => {
                    if let Some(v) = v_i64 {
                        self.cooldown_403_secs.store(v, Ordering::Relaxed);
                    }
                }
                _ => {}
            }
        }
    }

    pub async fn save_to_db(&self, db: &SqlitePool) {
        let entries = [
            ("ip_rps", self.ip_rps.load(Ordering::Relaxed).to_string()),
            ("ip_burst", self.ip_burst.load(Ordering::Relaxed).to_string()),
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

fn first_opt_u64(obj: &Value, keys: &[&str]) -> Result<Option<u64>, String> {
    for key in keys {
        match obj.get(key) {
            None => continue,
            Some(v) if v.is_null() => return Ok(None),
            Some(v) => {
                return v
                    .as_u64()
                    .map(Some)
                    .ok_or_else(|| format!("{} must be a positive integer", key));
            }
        }
    }
    Ok(None)
}

fn first_opt_i64(obj: &Value, keys: &[&str]) -> Result<Option<i64>, String> {
    for key in keys {
        match obj.get(key) {
            None => continue,
            Some(v) if v.is_null() => return Ok(None),
            Some(v) => {
                return v
                    .as_i64()
                    .map(Some)
                    .ok_or_else(|| format!("{} must be an integer", key));
            }
        }
    }
    Ok(None)
}