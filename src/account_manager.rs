use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use chrono::Utc;
use rand::Rng;
use serde::Deserialize;
use sqlx::FromRow;
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::AppError;
use crate::rate_limit::RateLimitSettings;
use crate::upstash::UpstashStore;

#[derive(Clone, Debug)]
pub struct SwitchingWeights {
    pub balance: f64,
    pub recency: f64,
    pub error: f64,
}

impl Default for SwitchingWeights {
    fn default() -> Self {
        Self {
            balance: 0.4,
            recency: 0.3,
            error: 0.3,
        }
    }
}

pub struct AccountState {
    pub id: String,
    pub label: String,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub user_id: RwLock<Option<String>>,
    pub access_token: RwLock<Option<String>>,
    pub token_expires_at: AtomicI64,
    pub is_active: AtomicBool,
    /// True when the system (not the owner) deactivated the account.
    /// Only these are eligible for auto-heal; manual OFF is never touched.
    pub auto_disabled: AtomicBool,
    pub heal_failures: AtomicU64,
    pub heal_next_retry: AtomicI64,
    pub notes: RwLock<String>,
    pub last_used: AtomicI64,
    pub request_count: AtomicU64,
    pub error_count: AtomicU64,
    pub rate_limit_hits: AtomicU64,
    pub rate_limited_until: AtomicI64,
    /// Tidal calls served this UTC day (resets on rollover). Bounded by
    /// the per-account daily budget; persisted periodically.
    pub day_requests: AtomicU64,
    /// UTC day number (timestamp / 86400) the counter above belongs to.
    pub day_start: AtomicI64,
    /// UTC day number a budget alert was last sent for this account.
    pub day_alerted: AtomicI64,
}

impl AccountState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        label: String,
        client_id: String,
        client_secret: String,
        refresh_token: String,
        user_id: Option<String>,
        is_active: bool,
        notes: String,
    ) -> Self {
        Self {
            id,
            label,
            client_id,
            client_secret,
            refresh_token,
            user_id: RwLock::new(user_id),
            access_token: RwLock::new(None),
            token_expires_at: AtomicI64::new(0),
            is_active: AtomicBool::new(is_active),
            auto_disabled: AtomicBool::new(false),
            heal_failures: AtomicU64::new(0),
            heal_next_retry: AtomicI64::new(0),
            notes: RwLock::new(notes),
            last_used: AtomicI64::new(0),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            rate_limit_hits: AtomicU64::new(0),
            rate_limited_until: AtomicI64::new(0),
            day_requests: AtomicU64::new(0),
            day_start: AtomicI64::new(0),
            day_alerted: AtomicI64::new(0),
        }
    }
}

/// UTC day number for a unix timestamp. Pure function — unit tested.
pub fn utc_day(ts: i64) -> i64 {
    ts.div_euclid(86400)
}

#[derive(Debug, Deserialize, FromRow)]
pub struct DbAccountRow {
    pub id: String,
    pub label: String,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub user_id: Option<String>,
    pub is_active: i32,
    pub auto_disabled: Option<i32>,
    pub notes: String,
    pub access_token: Option<String>,
    pub expires_at: Option<i64>,
}

/// Extract a numeric user id from an auto-generated "Tidal Account (<id>)" label.
fn user_id_from_label(label: &str) -> Option<&str> {
    let inner = label
        .strip_prefix("Tidal Account (")?
        .strip_suffix(')')?;
    if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
        Some(inner)
    } else {
        None
    }
}

pub struct AccountManager {
    accounts: RwLock<Vec<Arc<AccountState>>>,
    weights: SwitchingWeights,
    settings: Arc<RateLimitSettings>,
    db: Option<SqlitePool>,
    /// Shared cross-instance state (None = single-host mode, skip sync).
    upstash: OnceLock<Arc<UpstashStore>>,
}

impl AccountManager {
    pub fn new(
        db: Option<SqlitePool>,
        weights: SwitchingWeights,
        settings: Arc<RateLimitSettings>,
    ) -> Self {
        Self {
            accounts: RwLock::new(Vec::new()),
            weights,
            settings,
            db,
            upstash: OnceLock::new(),
        }
    }

    /// Attach shared state once at startup (before serving).
    pub fn set_upstash(&self, store: Option<Arc<UpstashStore>>) {
        if let Some(s) = store {
            let _ = self.upstash.set(s);
        }
    }

    fn upstash(&self) -> Option<Arc<UpstashStore>> {
        self.upstash.get().cloned()
    }

    pub async fn load_from_db(&self) -> Result<(), AppError> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok(()),
        };

        let rows: Vec<DbAccountRow> = sqlx::query_as::<_, DbAccountRow>(
            "SELECT a.id, a.label, a.client_id, a.client_secret, a.refresh_token,
             a.user_id, a.is_active, a.auto_disabled, a.notes,
             t.access_token, t.expires_at
             FROM accounts a
             LEFT JOIN tokens t ON t.account_id = a.id
             ORDER BY a.created_at ASC",
        )
        .fetch_all(db)
        .await?;

        let mut accounts = self.accounts.write().await;
        for row in rows {
            // Backfill user_id for accounts created before it was persisted
            // (label was "Tidal Account (<id>)").
            let user_id = match row.user_id {
                Some(uid) => Some(uid),
                None => user_id_from_label(&row.label).map(|s| s.to_string()),
            };
            if user_id.is_some() && self.db.is_some() {
                if let Some(db) = &self.db {
                    let _ = sqlx::query("UPDATE accounts SET user_id = ? WHERE id = ? AND user_id IS NULL")
                        .bind(&user_id)
                        .bind(&row.id)
                        .execute(db)
                        .await;
                }
            }
            let state = Arc::new(AccountState::new(
                row.id,
                row.label,
                row.client_id,
                row.client_secret,
                row.refresh_token,
                user_id,
                row.is_active != 0,
                row.notes,
            ));
            if row.auto_disabled.unwrap_or(0) != 0 {
                state.auto_disabled.store(true, Ordering::Relaxed);
            }
            if let (Some(token), Some(expires)) = (row.access_token, row.expires_at) {
                if !token.is_empty() && expires > 0 {
                    *state.access_token.write().await = Some(token);
                    state.token_expires_at.store(expires, Ordering::Relaxed);
                }
            }
            accounts.push(state);
        }

        tracing::info!("Loaded {} accounts from database", accounts.len());
        Ok(())
    }

    /// Drop all in-memory state and reload from the database (used after restore).
    pub async fn reload_from_db(&self) -> Result<(), AppError> {
        self.accounts.write().await.clear();
        self.load_from_db().await?;
        self.load_daily_usage().await;
        // Converge with the fleet (backup restores only touch SQLite).
        self.sync_usage_with_redis().await;
        self.merge_remote_cooldowns().await;
        Ok(())
    }

    /// Load today's per-account counters (survives restarts mid-day).
    pub async fn load_daily_usage(&self) {
        let db = match &self.db {
            Some(db) => db,
            None => return,
        };
        let today = utc_day(Utc::now().timestamp());
        let rows: Vec<(String, i64)> =
            match sqlx::query_as("SELECT account_id, count FROM daily_usage WHERE day = ?")
                .bind(today)
                .fetch_all(db)
                .await
            {
                Ok(r) => r,
                Err(_) => return,
            };
        let accounts = self.accounts.read().await;
        for (id, count) in rows {
            if let Some(a) = accounts.iter().find(|a| a.id == id) {
                a.day_start.store(today, Ordering::Relaxed);
                a.day_requests.store(count.max(0) as u64, Ordering::Relaxed);
            }
        }
    }

    /// Persist today's per-account counters (called periodically; cheap).
    pub async fn flush_daily_usage(&self) {
        let db = match &self.db {
            Some(db) => db,
            None => return,
        };
        let today = utc_day(Utc::now().timestamp());
        let snapshot: Vec<(String, u64, i64)> = {
            let accounts = self.accounts.read().await;
            accounts
                .iter()
                .map(|a| {
                    (
                        a.id.clone(),
                        a.day_requests.load(Ordering::Relaxed),
                        a.day_start.load(Ordering::Relaxed),
                    )
                })
                .collect()
        };
        for (id, count, day) in snapshot {
            // Only today's rows matter; stale days are dropped on read.
            if day != 0 && day != today {
                continue;
            }
            let _ = sqlx::query(
                "INSERT INTO daily_usage (account_id, day, count) VALUES (?, ?, ?)
                 ON CONFLICT(account_id) DO UPDATE SET day = excluded.day, count = excluded.count",
            )
            .bind(&id)
            .bind(today)
            .bind(count as i64)
            .execute(db)
            .await;
        }
        // Drop rows from previous days.
        let _ = sqlx::query("DELETE FROM daily_usage WHERE day != ?")
            .bind(today)
            .execute(db)
            .await;
    }

    pub async fn add_account(
        &self,
        label: String,
        client_id: String,
        client_secret: String,
        refresh_token: String,
        user_id: Option<String>,
    ) -> Result<Arc<AccountState>, AppError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();
        let state = Arc::new(AccountState::new(
            id.clone(),
            label.clone(),
            client_id.clone(),
            client_secret.clone(),
            refresh_token.clone(),
            user_id.clone(),
            true,
            String::new(),
        ));

        if let Some(db) = &self.db {
            sqlx::query(
                "INSERT INTO accounts (id, label, client_id, client_secret, refresh_token, user_id, is_active, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?)",
            )
            .bind(&id)
            .bind(&label)
            .bind(&client_id)
            .bind(&client_secret)
            .bind(&refresh_token)
            .bind(&user_id)
            .bind(now)
            .bind(now)
            .execute(db)
            .await?;

            sqlx::query("INSERT INTO account_metrics (account_id) VALUES (?)")
                .bind(&id)
                .execute(db)
                .await?;
        }

        self.accounts.write().await.push(state.clone());
        Ok(state)
    }

    pub async fn remove_account(&self, id: &str) -> Result<(), AppError> {
        {
            let mut accounts = self.accounts.write().await;
            accounts.retain(|a| a.id != id);
        }

        if let Some(db) = &self.db {
            sqlx::query("DELETE FROM accounts WHERE id = ?")
                .bind(id)
                .execute(db)
                .await?;
        }
        Ok(())
    }

    pub async fn get_account_by_id(&self, id: &str) -> Option<Arc<AccountState>> {
        let accounts = self.accounts.read().await;
        accounts.iter().find(|a| a.id == id).cloned()
    }

    pub async fn select_account_excluding(&self, exclude_ids: &[String]) -> Result<Arc<AccountState>, AppError> {
        let accounts = self.accounts.read().await;
        if accounts.is_empty() {
            return Err(AppError::Internal(
                "No Tidal credentials available; add an account via the admin panel".into(),
            ));
        }

        let now = Utc::now().timestamp();
        let mut scored: Vec<(f64, usize)> = Vec::new();

        for (i, account) in accounts.iter().enumerate() {
            if exclude_ids.contains(&account.id) {
                continue;
            }
            if !account.is_active.load(Ordering::Relaxed) {
                continue;
            }

            let rate_limited_until = account.rate_limited_until.load(Ordering::Relaxed);
            if rate_limited_until > now {
                continue;
            }

            // Daily budget: roll over at UTC midnight, exclude spent accounts.
            // 0 budget = unlimited.
            let budget = self.settings.daily_budget_per_account.load(Ordering::Relaxed);
            if budget > 0 {
                let today = utc_day(now);
                if account.day_start.load(Ordering::Relaxed) != today {
                    account.day_start.store(today, Ordering::Relaxed);
                    account.day_requests.store(0, Ordering::Relaxed);
                }
                if account.day_requests.load(Ordering::Relaxed) >= budget {
                    continue;
                }
            }

            let usage = account.request_count.load(Ordering::Relaxed).max(1) as f64;
            let last_used = account.last_used.load(Ordering::Relaxed);
            let recency = if last_used > 0 {
                (now - last_used) as f64
            } else {
                3600.0
            };
            let errors = account.error_count.load(Ordering::Relaxed).max(1) as f64;
            let total = account.request_count.load(Ordering::Relaxed).max(1) as f64;
            let error_rate = errors / total;

            let usage_score = self.weights.balance / usage;
            let recency_score = self.weights.recency * (recency / 3600.0).min(1.0).max(0.0);
            let error_score = self.weights.error * (1.0 - error_rate);

            let mut score = usage_score + recency_score + error_score;
            // Recovery ramp: a freshly unparked account scores highest on
            // usage+recency and would absorb everything until re-parked.
            // Ramp it back over 3 minutes so traffic spreads instead.
            if rate_limited_until > 0 {
                let recovered_ago = (now - rate_limited_until).max(0) as f64;
                if recovered_ago < 180.0 {
                    score *= (recovered_ago / 180.0).max(0.05);
                }
            }

            scored.push((score, i));
        }

        if scored.is_empty() {
            // Tell clients how long to back off: soonest parked-account recovery.
            let retry_after = accounts
                .iter()
                .filter(|a| a.is_active.load(Ordering::Relaxed))
                .map(|a| a.rate_limited_until.load(Ordering::Relaxed) - now)
                .filter(|&r| r > 0)
                .min()
                .unwrap_or(0)
                .max(0) as u64;
            return Err(AppError::ServiceUnavailableRetry(
                "All accounts are inactive, rate-limited, or have expired tokens".into(),
                retry_after,
            ));
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let best = &accounts[scored[0].1];
        best.last_used.store(now, Ordering::Relaxed);
        best.request_count.fetch_add(1, Ordering::Relaxed);
        // Count toward the daily budget (rollover already ensured above when
        // budgets are enabled; do it unconditionally here for fresh accounts
        // added mid-day or budget toggled on later).
        let today = utc_day(now);
        if best.day_start.load(Ordering::Relaxed) != today {
            best.day_start.store(today, Ordering::Relaxed);
            best.day_requests.store(0, Ordering::Relaxed);
        }
        best.day_requests.fetch_add(1, Ordering::Relaxed);
        // Global budget accounting: count this call in Redis without blocking
        // the request path (fire-and-forget). The 60s reconcile folds the
        // fleet-wide total back into the local enforcement counter.
        if let Some(store) = self.upstash() {
            let key = UpstashStore::k_usage(today, &best.id);
            tokio::spawn(async move {
                // Single round trip: INCR + self-cleaning expiry.
                let _ = store.incr_expire(&key, 172_800).await;
            });
        }
        Ok(best.clone())
    }

    /// Pull the fleet-wide daily totals from Redis and raise the local
    /// enforcement counters to at least the global value. Called at startup
    /// (after the SQLite load) and on the 60s flush tick, so per-account
    /// daily budgets are enforced fleet-wide within ~a minute. Overshoot is
    /// bounded by one reconcile interval — acceptable next to a 12k budget.
    pub async fn sync_usage_with_redis(&self) {
        let store = match self.upstash() {
            Some(s) => s,
            None => return,
        };
        let today = utc_day(Utc::now().timestamp());
        let ids: Vec<String> = {
            let accounts = self.accounts.read().await;
            accounts.iter().map(|a| a.id.clone()).collect()
        };
        if ids.is_empty() {
            return;
        }
        let keys: Vec<String> =
            ids.iter().map(|id| UpstashStore::k_usage(today, id)).collect();
        let values = store.mget(&keys).await;
        let accounts = self.accounts.read().await;
        for (id, remote) in ids.iter().zip(values.iter()) {
            let count = match remote {
                Some(v) => v.parse::<i64>().unwrap_or(0).max(0) as u64,
                None => continue,
            };
            if let Some(a) = accounts.iter().find(|a| &a.id == id) {
                if a.day_start.load(Ordering::Relaxed) != today {
                    a.day_start.store(today, Ordering::Relaxed);
                    a.day_requests.store(0, Ordering::Relaxed);
                }
                // Only ever raise: Redis holds the fleet sum, which includes
                // this host's own fire-and-forget increments.
                let _ = a.day_requests.fetch_max(count, Ordering::Relaxed);
            }
        }
    }

    /// Pull 429/403 parks broadcast by sibling instances and apply any that
    /// extend the local cooldown. Called at startup and on the 30s pool
    /// tick, so one host's park protects the account fleet-wide within ~30s
    /// instead of every host discovering the ban independently.
    pub async fn merge_remote_cooldowns(&self) {
        let store = match self.upstash() {
            Some(s) => s,
            None => return,
        };
        let ids: Vec<String> = {
            let accounts = self.accounts.read().await;
            accounts.iter().map(|a| a.id.clone()).collect()
        };
        if ids.is_empty() {
            return;
        }
        let keys: Vec<String> =
            ids.iter().map(|id| UpstashStore::k_cooldown(id)).collect();
        let values = store.mget(&keys).await;
        let accounts = self.accounts.read().await;
        let now = Utc::now().timestamp();
        for (id, remote) in ids.iter().zip(values.iter()) {
            let until = match remote {
                Some(v) => v.parse::<i64>().unwrap_or(0),
                None => continue,
            };
            if until <= now {
                continue;
            }
            if let Some(a) = accounts.iter().find(|a| &a.id == id) {
                let _ = a.rate_limited_until.fetch_max(until, Ordering::Relaxed);
                a.rate_limit_hits.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub async fn select_account(&self) -> Result<Arc<AccountState>, AppError> {
        self.select_account_excluding(&[]).await
    }

    pub async fn mark_account_error(&self, id: &str, message: &str) {
        let now = Utc::now().timestamp();
        if let Some(account) = self.get_account_by_id(id).await {
            account.error_count.fetch_add(1, Ordering::Relaxed);
            if let Some(db) = &self.db {
                let _ = sqlx::query(
                    "UPDATE account_metrics SET error_count = error_count + 1, last_error_at = ?, last_error_message = ? WHERE account_id = ?",
                )
                .bind(now)
                .bind(message)
                .bind(id)
                .execute(db)
                .await;
            }
        }
    }

    pub async fn mark_account_rate_limited(&self, id: &str, duration_secs: i64) {
        // Desync herd recoveries: without jitter every account parked by the
        // same burst unparks simultaneously and gets re-slammed together.
        let jitter = rand::thread_rng().gen_range(0..=(duration_secs.max(1) / 4));
        let until = Utc::now().timestamp() + duration_secs + jitter;
        if let Some(account) = self.get_account_by_id(id).await {
            account.rate_limit_hits.fetch_add(1, Ordering::Relaxed);
            account.rate_limited_until.store(until, Ordering::Relaxed);
            if let Some(db) = &self.db {
                let _ = sqlx::query(
                    "UPDATE account_metrics SET rate_limit_hits = rate_limit_hits + 1 WHERE account_id = ?",
                )
                .bind(id)
                .execute(db)
                .await;
            }
            // Broadcast the park so sibling instances stop using this
            // account too (merged by their 30s tick). Best-effort.
            if let Some(store) = self.upstash() {
                let ttl = (until - Utc::now().timestamp() + 120).max(60) as u64;
                store.set(&UpstashStore::k_cooldown(id), &until.to_string(), Some(ttl.min(7200))).await;
            }
        }
    }

    pub async fn update_account(
        &self,
        id: &str,
        label: Option<String>,
        client_id: Option<String>,
        client_secret: Option<String>,
        refresh_token: Option<String>,
        user_id: Option<String>,
    ) -> Result<(), AppError> {
        let mut accounts = self.accounts.write().await;
        let idx = accounts.iter().position(|a| a.id == id).ok_or_else(|| {
            AppError::NotFound(format!("Account {} not found", id))
        })?;

        let old = &accounts[idx];
        let new_label = label.unwrap_or_else(|| old.label.clone());
        let new_client_id = client_id.unwrap_or_else(|| old.client_id.clone());
        let new_client_secret = client_secret.unwrap_or_else(|| old.client_secret.clone());
        let new_refresh_token = refresh_token.unwrap_or_else(|| old.refresh_token.clone());
        let current_user_id = old.user_id.read().await.clone();
        let new_user_id = user_id.or(current_user_id);
        let new_notes = old.notes.read().await.clone();

        let updated = Arc::new(AccountState::new(
            old.id.clone(),
            new_label,
            new_client_id,
            new_client_secret,
            new_refresh_token,
            new_user_id.clone(),
            old.is_active.load(Ordering::Relaxed),
            new_notes,
        ));

        if let Some(db) = &self.db {
            let now = Utc::now().timestamp();
            sqlx::query(
                "UPDATE accounts SET label = ?, client_id = ?, client_secret = ?, refresh_token = ?, user_id = ?, updated_at = ? WHERE id = ?",
            )
            .bind(&updated.label)
            .bind(&updated.client_id)
            .bind(&updated.client_secret)
            .bind(&updated.refresh_token)
            .bind(&new_user_id)
            .bind(now)
            .bind(&old.id)
            .execute(db)
            .await?;
        }

        accounts[idx] = updated;
        Ok(())
    }

    /// Mark an account as system-disabled (eligible for auto-heal) or clear it
    /// (owner intent / successful recovery — never auto-healed while clear).
    pub async fn set_auto_disabled(&self, id: &str, disabled: bool) -> Result<(), AppError> {
        if let Some(account) = self.get_account_by_id(id).await {
            account.auto_disabled.store(disabled, Ordering::Relaxed);
            if disabled {
                account.heal_failures.store(0, Ordering::Relaxed);
                account.heal_next_retry.store(0, Ordering::Relaxed);
            }
            if let Some(db) = &self.db {
                sqlx::query(
                    "UPDATE accounts SET auto_disabled = ?, updated_at = ? WHERE id = ?",
                )
                .bind(disabled as i32)
                .bind(Utc::now().timestamp())
                .bind(id)
                .execute(db)
                .await?;
            }
            Ok(())
        } else {
            Err(AppError::NotFound(format!("Account {} not found", id)))
        }
    }

    pub async fn set_account_active(&self, id: &str, active: bool) -> Result<(), AppError> {
        if let Some(account) = self.get_account_by_id(id).await {
            account.is_active.store(active, Ordering::Relaxed);
            if let Some(db) = &self.db {
                sqlx::query(
                    "UPDATE accounts SET is_active = ?, updated_at = ? WHERE id = ?",
                )
                .bind(active as i32)
                .bind(Utc::now().timestamp())
                .bind(id)
                .execute(db)
                .await?;
            }
            Ok(())
        } else {
            Err(AppError::NotFound(format!("Account {} not found", id)))
        }
    }

    /// Emergency reset: clear all per-account rate-limit cooldowns.
    pub async fn clear_all_rate_limits(&self) -> usize {
        let accounts = self.accounts.read().await;
        let mut cleared = 0;
        let mut ids = Vec::new();
        for a in accounts.iter() {
            if a.rate_limited_until.swap(0, Ordering::Relaxed) > 0 {
                cleared += 1;
            }
            ids.push(a.id.clone());
        }
        drop(accounts);
        // Clear the broadcast parks too, or the next 30s merge re-parks them.
        if cleared > 0 {
            if let Some(store) = self.upstash() {
                let keys: Vec<String> =
                    ids.iter().map(|id| UpstashStore::k_cooldown(id)).collect();
                store.del_many(&keys).await;
            }
        }
        cleared
    }

    pub async fn active_count(&self) -> usize {
        self.accounts
            .read()
            .await
            .iter()
            .filter(|a| a.is_active.load(Ordering::Relaxed))
            .count()
    }

    pub async fn healthy_count(&self) -> (usize, usize) {
        let accounts = self.accounts.read().await;
        let now = Utc::now().timestamp();
        let total = accounts.len();
        let healthy = accounts
            .iter()
            .filter(|a| {
                a.is_active.load(Ordering::Relaxed)
                    && a.rate_limited_until.load(Ordering::Relaxed) <= now
            })
            .count();
        (healthy, total)
    }

    pub async fn list_accounts(&self) -> Vec<Arc<AccountState>> {
        self.accounts.read().await.clone()
    }

    pub async fn account_count(&self) -> usize {
        self.accounts.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::utc_day;

    #[test]
    fn utc_day_boundaries() {
        // 1970-01-01T00:00:00Z == day 0; 86399 still day 0; 86400 day 1.
        assert_eq!(utc_day(0), 0);
        assert_eq!(utc_day(86399), 0);
        assert_eq!(utc_day(86400), 1);
        // Negative timestamps (pre-1970) still bucket consistently.
        assert_eq!(utc_day(-1), -1);
        assert_eq!(utc_day(-86400), -1);
    }

    #[test]
    fn utc_day_rollover_detected() {
        let monday_2359 = 86400 * 100 + 86399;
        let tuesday_0001 = 86400 * 101 + 60;
        assert_ne!(utc_day(monday_2359), utc_day(tuesday_0001));
    }
}
