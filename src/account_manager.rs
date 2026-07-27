use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use chrono::Utc;
use serde::Deserialize;
use sqlx::FromRow;
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::AppError;

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
    pub notes: RwLock<String>,
    pub last_used: AtomicI64,
    pub request_count: AtomicU64,
    pub error_count: AtomicU64,
    pub rate_limit_hits: AtomicU64,
    pub rate_limited_until: AtomicI64,
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
            notes: RwLock::new(notes),
            last_used: AtomicI64::new(0),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            rate_limit_hits: AtomicU64::new(0),
            rate_limited_until: AtomicI64::new(0),
        }
    }
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
    pub notes: String,
    pub access_token: Option<String>,
    pub expires_at: Option<i64>,
}

pub struct AccountManager {
    accounts: RwLock<Vec<Arc<AccountState>>>,
    weights: SwitchingWeights,
    db: Option<SqlitePool>,
}

impl AccountManager {
    pub fn new(db: Option<SqlitePool>, weights: SwitchingWeights) -> Self {
        Self {
            accounts: RwLock::new(Vec::new()),
            weights,
            db,
        }
    }

    pub async fn load_from_db(&self) -> Result<(), AppError> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok(()),
        };

        let rows: Vec<DbAccountRow> = sqlx::query_as::<_, DbAccountRow>(
            "SELECT a.id, a.label, a.client_id, a.client_secret, a.refresh_token,
             a.user_id, a.is_active, a.notes,
             t.access_token, t.expires_at
             FROM accounts a
             LEFT JOIN tokens t ON t.account_id = a.id
             ORDER BY a.created_at ASC",
        )
        .fetch_all(db)
        .await?;

        let mut accounts = self.accounts.write().await;
        for row in rows {
            let state = Arc::new(AccountState::new(
                row.id,
                row.label,
                row.client_id,
                row.client_secret,
                row.refresh_token,
                row.user_id,
                row.is_active != 0,
                row.notes,
            ));
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

    pub async fn add_account(
        &self,
        label: String,
        client_id: String,
        client_secret: String,
        refresh_token: String,
    ) -> Result<Arc<AccountState>, AppError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();
        let state = Arc::new(AccountState::new(
            id.clone(),
            label.clone(),
            client_id.clone(),
            client_secret.clone(),
            refresh_token.clone(),
            None,
            true,
            String::new(),
        ));

        if let Some(db) = &self.db {
            sqlx::query(
                "INSERT INTO accounts (id, label, client_id, client_secret, refresh_token, is_active, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
            )
            .bind(&id)
            .bind(&label)
            .bind(&client_id)
            .bind(&client_secret)
            .bind(&refresh_token)
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

    pub async fn select_account(&self) -> Result<Arc<AccountState>, AppError> {
        let accounts = self.accounts.read().await;
        if accounts.is_empty() {
            return Err(AppError::Internal(
                "No Tidal credentials available; add an account via the admin panel".into(),
            ));
        }

        let now = Utc::now().timestamp();
        let mut scored: Vec<(f64, usize)> = Vec::new();

        for (i, account) in accounts.iter().enumerate() {
            if !account.is_active.load(Ordering::Relaxed) {
                continue;
            }

            let rate_limited_until = account.rate_limited_until.load(Ordering::Relaxed);
            if rate_limited_until > now {
                continue;
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

            scored.push((usage_score + recency_score + error_score, i));
        }

        if scored.is_empty() {
            return Err(AppError::ServiceUnavailable(
                "All accounts are inactive or rate-limited".into(),
            ));
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let best = &accounts[scored[0].1];
        best.last_used.store(now, Ordering::Relaxed);
        best.request_count.fetch_add(1, Ordering::Relaxed);
        Ok(best.clone())
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
        let until = Utc::now().timestamp() + duration_secs;
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

    pub async fn list_accounts(&self) -> Vec<Arc<AccountState>> {
        self.accounts.read().await.clone()
    }

    pub async fn account_count(&self) -> usize {
        self.accounts.read().await.len()
    }
}
