use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use moka::future::Cache;
use reqwest::Client;
use serde_json::Value;
use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::account_manager::{AccountManager, AccountState};
use crate::error::AppError;

pub struct TokenManager {
    db: Option<SqlitePool>,
    #[allow(dead_code)]
    token_cache: Cache<String, (String, i64)>,
    refresh_lock: Mutex<String>,
}

impl TokenManager {
    pub fn new(db: Option<SqlitePool>) -> Self {
        Self {
            db,
            token_cache: Cache::builder()
                .time_to_live(Duration::from_secs(3600))
                .max_capacity(100)
                .build(),
            refresh_lock: Mutex::new(String::new()),
        }
    }

    pub async fn get_token(
        &self,
        account: &AccountState,
        http_client: &Client,
    ) -> Result<String, AppError> {
        {
            let access_token = account.access_token.read().await;
            let expires_at = account.token_expires_at.load(std::sync::atomic::Ordering::Relaxed);
            if let Some(token) = access_token.as_ref() {
                if Utc::now().timestamp() < expires_at && !token.is_empty() {
                    return Ok(token.clone());
                }
            }
        }

        self.refresh_token(account, http_client).await
    }

    pub async fn refresh_token(
        &self,
        account: &AccountState,
        http_client: &Client,
    ) -> Result<String, AppError> {
        let _guard = self.refresh_lock.lock().await;

        {
            let access_token = account.access_token.read().await;
            let expires_at = account.token_expires_at.load(std::sync::atomic::Ordering::Relaxed);
            if let Some(token) = access_token.as_ref() {
                if Utc::now().timestamp() < expires_at && !token.is_empty() {
                    return Ok(token.clone());
                }
            }
        }

        let res = http_client
            .post("https://auth.tidal.com/v1/oauth2/token")
            .form(&[
                ("client_id", account.client_id.as_str()),
                ("refresh_token", account.refresh_token.as_str()),
                ("grant_type", "refresh_token"),
                ("scope", "r_usr+w_usr+w_sub"),
            ])
            .basic_auth(&account.client_id, Some(&account.client_secret))
            .send()
            .await?;

        if res.status().as_u16() == 400 || res.status().as_u16() == 401 {
            let error_data: Value = res.json().await.unwrap_or_default();
            let err_msg = error_data
                .get("error_description")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown auth error");
            return Err(AppError::Unauthorized(format!("Tidal Auth Error: {}", err_msg)));
        }

        let status = res.status();
        if !status.is_success() {
            return Err(AppError::UpstreamError(
                status,
                format!("Token refresh failed with status {}", status),
            ));
        }

        let data: Value = res.json().await?;
        let new_token = data
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Internal("No access_token in response".into()))?
            .to_string();
        let expires_in = data
            .get("expires_in")
            .and_then(|v| v.as_i64())
            .unwrap_or(3600);
        let expires_at = Utc::now().timestamp() + expires_in - 60;

        *account.access_token.write().await = Some(new_token.clone());
        account
            .token_expires_at
            .store(expires_at, std::sync::atomic::Ordering::Relaxed);

        if let Some(db) = &self.db {
            let now = Utc::now().timestamp();
            let _ = sqlx::query(
                "INSERT INTO tokens (account_id, access_token, expires_at, refreshed_at)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(account_id) DO UPDATE SET access_token = excluded.access_token, expires_at = excluded.expires_at, refreshed_at = excluded.refreshed_at",
            )
            .bind(&account.id)
            .bind(&new_token)
            .bind(expires_at)
            .bind(now)
            .execute(db)
            .await;
        }

        Ok(new_token)
    }

    pub async fn prewarm_all(&self, manager: &AccountManager, http_client: &Client) {
        let accounts = manager.list_accounts().await;
        tracing::info!("Pre-warming tokens for {} accounts", accounts.len());

        for account in &accounts {
            let is_active = account.is_active.load(std::sync::atomic::Ordering::Relaxed);
            if !is_active {
                continue;
            }

            let expires_at = account.token_expires_at.load(std::sync::atomic::Ordering::Relaxed);
            let now = Utc::now().timestamp();

            if expires_at > now + 120 {
                continue;
            }

            tokio::time::sleep(Duration::from_millis(
                rand::random::<u64>() % 3000 + 500,
            ))
            .await;

            match self.refresh_token(account, http_client).await {
                Ok(_token) => {
                    tracing::info!(
                        "Pre-warmed token for account {} (expires at {})",
                        account.label,
                        account
                            .token_expires_at
                            .load(std::sync::atomic::Ordering::Relaxed)
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to pre-warm token for {}: {:?}", account.label, e);
                }
            }
        }
    }

    pub async fn start_prewarm_loop(
        self: Arc<Self>,
        manager: Arc<AccountManager>,
        http_client: Arc<Client>,
    ) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(300)).await;
                self.prewarm_all(&manager, &http_client).await;
            }
        });
    }
}
