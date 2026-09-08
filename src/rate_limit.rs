use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::RwLock;

use serde_json::{json, Value};
use sqlx::SqlitePool;

pub struct RateLimitSettings {
    pub ip_rps: AtomicU64,
    pub ip_burst: AtomicU64,
    pub tidal_rps: AtomicU64,
    pub tidal_burst: AtomicU64,
    pub cooldown_429_secs: AtomicI64,
    pub cooldown_403_secs: AtomicI64,
    pub auto_heal: AtomicBool,
    // --- intelligent limits (scales with pool size) ---
    /// Sustained Tidal rps slice per healthy account. Effective global cap
    /// ≈ this × healthy count (still capped by tidal_rps ceiling).
    pub account_rps: AtomicU64,
    pub account_burst: AtomicU64,
    /// Keep at least this many accounts healthy at any cost. At/under this
    /// level the pool enters conservation mode (trickle + shed).
    pub reserve_accounts: AtomicU64,
    /// Global Tidal rps while in conservation mode.
    pub conserve_trickle_rps: AtomicU64,
    /// Max Tidal calls per account per UTC day. 0 = unlimited.
    pub daily_budget_per_account: AtomicU64,
    /// Discord alert when an account crosses this % of its daily budget.
    pub daily_budget_alert_pct: AtomicU64,
    // --- IP tiers + reputation (soft instead of harsh) ---
    /// Stricter bucket applied only to Tidal-hitting routes.
    pub ip_costly_rps: AtomicU64,
    pub ip_costly_burst: AtomicU64,
    /// Max artificial delay (ms) for soft-over-limit requests before 429.
    pub ip_delay_cap_ms: AtomicU64,
    pub reputation_enabled: AtomicBool,
    pub ip_allowlist: RwLock<Vec<IpAddr>>,
    pub ip_denylist: RwLock<Vec<IpAddr>>,
    // --- Atmos ---
    /// off | prefer — query param `atmos=` overrides per request.
    pub atmos_mode: RwLock<String>,
}

impl RateLimitSettings {
    pub fn from_env() -> Self {
        Self {
            ip_rps: AtomicU64::new(env_u64("RATE_LIMIT_RPS", 20)),
            ip_burst: AtomicU64::new(env_u64("RATE_LIMIT_BURST", 40)),
            tidal_rps: AtomicU64::new(env_u64("TIDAL_RPS", 12)),
            tidal_burst: AtomicU64::new(env_u64("TIDAL_BURST", 24)),
            cooldown_429_secs: AtomicI64::new(env_i64("COOLDOWN_429_SECS", 90)),
            cooldown_403_secs: AtomicI64::new(env_i64("COOLDOWN_403_SECS", 180)),
            auto_heal: AtomicBool::new(env_bool("AUTO_HEAL", true)),
            account_rps: AtomicU64::new(env_u64("TIDAL_RPS_PER_ACCOUNT", 2)),
            account_burst: AtomicU64::new(env_u64("TIDAL_BURST_PER_ACCOUNT", 4)),
            reserve_accounts: AtomicU64::new(env_u64("RESERVE_ACCOUNTS", 2)),
            conserve_trickle_rps: AtomicU64::new(env_u64("CONSERVE_TRICKLE_RPS", 1)),
            daily_budget_per_account: AtomicU64::new(env_u64("DAILY_BUDGET_PER_ACCOUNT", 6000)),
            daily_budget_alert_pct: AtomicU64::new(env_u64("DAILY_BUDGET_ALERT_PCT", 80)),
            ip_costly_rps: AtomicU64::new(env_u64("IP_COSTLY_RPS", 5)),
            ip_costly_burst: AtomicU64::new(env_u64("IP_COSTLY_BURST", 10)),
            ip_delay_cap_ms: AtomicU64::new(env_u64("IP_DELAY_CAP_MS", 2000)),
            reputation_enabled: AtomicBool::new(env_bool("REPUTATION_ENABLED", true)),
            ip_allowlist: RwLock::new(parse_ip_list(&std::env::var("IP_ALLOWLIST").unwrap_or_default())),
            ip_denylist: RwLock::new(parse_ip_list(&std::env::var("IP_DENYLIST").unwrap_or_default())),
            atmos_mode: RwLock::new(normalize_atmos_mode(&std::env::var("ATMOS_MODE").unwrap_or_default())),
        }
    }

    pub fn snapshot(&self) -> Value {
        json!({
            "ip_rps": self.ip_rps.load(Ordering::Relaxed),
            "ip_burst": self.ip_burst.load(Ordering::Relaxed),
            "tidal_rps": self.tidal_rps.load(Ordering::Relaxed),
            "tidal_burst": self.tidal_burst.load(Ordering::Relaxed),
            "cooldown_429_secs": self.cooldown_429_secs.load(Ordering::Relaxed),
            "cooldown_403_secs": self.cooldown_403_secs.load(Ordering::Relaxed),
            "auto_heal": self.auto_heal.load(Ordering::Relaxed),
            "account_rps": self.account_rps.load(Ordering::Relaxed),
            "account_burst": self.account_burst.load(Ordering::Relaxed),
            "reserve_accounts": self.reserve_accounts.load(Ordering::Relaxed),
            "conserve_trickle_rps": self.conserve_trickle_rps.load(Ordering::Relaxed),
            "daily_budget_per_account": self.daily_budget_per_account.load(Ordering::Relaxed),
            "daily_budget_alert_pct": self.daily_budget_alert_pct.load(Ordering::Relaxed),
            "ip_costly_rps": self.ip_costly_rps.load(Ordering::Relaxed),
            "ip_costly_burst": self.ip_costly_burst.load(Ordering::Relaxed),
            "ip_delay_cap_ms": self.ip_delay_cap_ms.load(Ordering::Relaxed),
            "reputation_enabled": self.reputation_enabled.load(Ordering::Relaxed),
            "ip_allowlist": self.ip_allowlist.read().map(|v| v.iter().map(|ip| ip.to_string()).collect::<Vec<_>>().join(",")).unwrap_or_default(),
            "ip_denylist": self.ip_denylist.read().map(|v| v.iter().map(|ip| ip.to_string()).collect::<Vec<_>>().join(",")).unwrap_or_default(),
            "atmos_mode": self.atmos_mode.read().map(|v| v.clone()).unwrap_or_else(|_| "off".to_string()),
        })
    }

    pub fn apply(&self, updates: &Value) -> Result<(), String> {
        if let Some(v) = first_opt_u64(updates, &["ip_rps", "global_rps"])? {
            self.ip_rps.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["ip_burst", "global_burst"])? {
            self.ip_burst.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["tidal_rps"])? {
            self.tidal_rps.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["tidal_burst"])? {
            self.tidal_burst.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_i64(updates, &["cooldown_429_secs"])? {
            self.cooldown_429_secs.store(v.max(0), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_i64(updates, &["cooldown_403_secs"])? {
            self.cooldown_403_secs.store(v.max(0), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_bool(updates, &["auto_heal"])? {
            self.auto_heal.store(v, Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["account_rps"])? {
            self.account_rps.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["account_burst"])? {
            self.account_burst.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["reserve_accounts"])? {
            self.reserve_accounts.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["conserve_trickle_rps"])? {
            self.conserve_trickle_rps.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["daily_budget_per_account"])? {
            self.daily_budget_per_account.store(v, Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["daily_budget_alert_pct"])? {
            self.daily_budget_alert_pct.store(v.min(100), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["ip_costly_rps"])? {
            self.ip_costly_rps.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["ip_costly_burst"])? {
            self.ip_costly_burst.store(v.max(1), Ordering::Relaxed);
        }
        if let Some(v) = first_opt_u64(updates, &["ip_delay_cap_ms"])? {
            self.ip_delay_cap_ms.store(v, Ordering::Relaxed);
        }
        if let Some(v) = first_opt_bool(updates, &["reputation_enabled"])? {
            self.reputation_enabled.store(v, Ordering::Relaxed);
        }
        if let Some(v) = first_opt_string(updates, &["ip_allowlist"])? {
            if let Ok(mut w) = self.ip_allowlist.write() {
                *w = parse_ip_list(&v);
            }
        }
        if let Some(v) = first_opt_string(updates, &["ip_denylist"])? {
            if let Ok(mut w) = self.ip_denylist.write() {
                *w = parse_ip_list(&v);
            }
        }
        if let Some(v) = first_opt_string(updates, &["atmos_mode"])? {
            if let Ok(mut w) = self.atmos_mode.write() {
                *w = normalize_atmos_mode(&v);
            }
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
                "auto_heal" => {
                    if value == "true" {
                        self.auto_heal.store(true, Ordering::Relaxed);
                    } else if value == "false" {
                        self.auto_heal.store(false, Ordering::Relaxed);
                    }
                }
                "account_rps" => {
                    if let Ok(v) = value.parse::<u64>() {
                        self.account_rps.store(v.max(1), Ordering::Relaxed);
                    }
                }
                "account_burst" => {
                    if let Ok(v) = value.parse::<u64>() {
                        self.account_burst.store(v.max(1), Ordering::Relaxed);
                    }
                }
                "reserve_accounts" => {
                    if let Ok(v) = value.parse::<u64>() {
                        self.reserve_accounts.store(v.max(1), Ordering::Relaxed);
                    }
                }
                "conserve_trickle_rps" => {
                    if let Ok(v) = value.parse::<u64>() {
                        self.conserve_trickle_rps.store(v.max(1), Ordering::Relaxed);
                    }
                }
                "daily_budget_per_account" => {
                    if let Ok(v) = value.parse::<u64>() {
                        self.daily_budget_per_account.store(v, Ordering::Relaxed);
                    }
                }
                "daily_budget_alert_pct" => {
                    if let Ok(v) = value.parse::<u64>() {
                        self.daily_budget_alert_pct.store(v.min(100), Ordering::Relaxed);
                    }
                }
                "ip_costly_rps" => {
                    if let Ok(v) = value.parse::<u64>() {
                        self.ip_costly_rps.store(v.max(1), Ordering::Relaxed);
                    }
                }
                "ip_costly_burst" => {
                    if let Ok(v) = value.parse::<u64>() {
                        self.ip_costly_burst.store(v.max(1), Ordering::Relaxed);
                    }
                }
                "ip_delay_cap_ms" => {
                    if let Ok(v) = value.parse::<u64>() {
                        self.ip_delay_cap_ms.store(v, Ordering::Relaxed);
                    }
                }
                "reputation_enabled" => {
                    if value == "true" {
                        self.reputation_enabled.store(true, Ordering::Relaxed);
                    } else if value == "false" {
                        self.reputation_enabled.store(false, Ordering::Relaxed);
                    }
                }
                "ip_allowlist" => {
                    if let Ok(mut w) = self.ip_allowlist.write() {
                        *w = parse_ip_list(&value);
                    }
                }
                "ip_denylist" => {
                    if let Ok(mut w) = self.ip_denylist.write() {
                        *w = parse_ip_list(&value);
                    }
                }
                "atmos_mode" => {
                    if let Ok(mut w) = self.atmos_mode.write() {
                        *w = normalize_atmos_mode(&value);
                    }
                }
                "tidal_rps" => {
                    if let Some(v) = v_u64 {
                        self.tidal_rps.store(v, Ordering::Relaxed);
                    }
                }
                "tidal_burst" => {
                    if let Some(v) = v_u64 {
                        self.tidal_burst.store(v, Ordering::Relaxed);
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
            ("tidal_rps", self.tidal_rps.load(Ordering::Relaxed).to_string()),
            ("tidal_burst", self.tidal_burst.load(Ordering::Relaxed).to_string()),
            ("cooldown_429_secs", self.cooldown_429_secs.load(Ordering::Relaxed).to_string()),
            ("cooldown_403_secs", self.cooldown_403_secs.load(Ordering::Relaxed).to_string()),
            ("auto_heal", self.auto_heal.load(Ordering::Relaxed).to_string()),
            ("ip_allowlist", self.ip_allowlist.read().map(|v| v.iter().map(|ip| ip.to_string()).collect::<Vec<_>>().join(",")).unwrap_or_default()),
            ("ip_denylist", self.ip_denylist.read().map(|v| v.iter().map(|ip| ip.to_string()).collect::<Vec<_>>().join(",")).unwrap_or_default()),
            ("atmos_mode", self.atmos_mode.read().map(|v| v.clone()).unwrap_or_else(|_| "off".to_string())),
            ("account_rps", self.account_rps.load(Ordering::Relaxed).to_string()),
            ("account_burst", self.account_burst.load(Ordering::Relaxed).to_string()),
            ("reserve_accounts", self.reserve_accounts.load(Ordering::Relaxed).to_string()),
            ("conserve_trickle_rps", self.conserve_trickle_rps.load(Ordering::Relaxed).to_string()),
            ("daily_budget_per_account", self.daily_budget_per_account.load(Ordering::Relaxed).to_string()),
            ("daily_budget_alert_pct", self.daily_budget_alert_pct.load(Ordering::Relaxed).to_string()),
            ("ip_costly_rps", self.ip_costly_rps.load(Ordering::Relaxed).to_string()),
            ("ip_costly_burst", self.ip_costly_burst.load(Ordering::Relaxed).to_string()),
            ("ip_delay_cap_ms", self.ip_delay_cap_ms.load(Ordering::Relaxed).to_string()),
            ("reputation_enabled", self.reputation_enabled.load(Ordering::Relaxed).to_string()),
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

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "true" || v == "1" || v == "yes"
        })
        .unwrap_or(default)
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

fn parse_ip_list(s: &str) -> Vec<IpAddr> {
    s.split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<IpAddr>().ok())
        .collect()
}

fn normalize_atmos_mode(s: &str) -> String {
    match s.trim().to_lowercase().as_str() {
        "prefer" => "prefer".to_string(),
        _ => "off".to_string(),
    }
}

fn first_opt_string(obj: &Value, keys: &[&str]) -> Result<Option<String>, String> {
    for key in keys {
        match obj.get(key) {
            None => continue,
            Some(v) if v.is_null() => return Ok(None),
            Some(v) => {
                return v
                    .as_str()
                    .map(|s| Some(s.to_string()))
                    .ok_or_else(|| format!("{} must be a string", key));
            }
        }
    }
    Ok(None)
}

fn first_opt_bool(obj: &Value, keys: &[&str]) -> Result<Option<bool>, String> {
    for key in keys {
        match obj.get(key) {
            None => continue,
            Some(v) if v.is_null() => return Ok(None),
            Some(v) => {
                return v
                    .as_bool()
                    .map(Some)
                    .ok_or_else(|| format!("{} must be a boolean", key));
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