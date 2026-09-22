use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;
use std::time::Duration;

use chrono::Utc;
use moka::future::Cache;
use reqwest::Client;
use serde_json::Value;
use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::account_manager::{AccountManager, AccountState};
use crate::error::AppError;
use crate::proxy_manager::ProxyManager;
use crate::upstash::UpstashStore;

pub struct TokenManager {
    db: Option<SqlitePool>,
    /// Per-account refresh guards: concurrent refreshes for *different*
    /// accounts proceed in parallel; only the same account serializes
    /// (double-checked after locking). Auto-evicts via TTL.
    refresh_locks: Cache<String, Arc<Mutex<()>>>,
    account_manager: OnceLock<Arc<AccountManager>>,
    proxy_manager: OnceLock<Arc<ProxyManager>>,
    /// Shared cross-instance state (None = single-host mode, skip sync).
    upstash: OnceLock<Arc<UpstashStore>>,
}

impl TokenManager {
    pub fn new(db: Option<SqlitePool>) -> Self {
        Self {
            db,
            refresh_locks: Cache::builder()
                .time_to_live(Duration::from_secs(3600))
                .max_capacity(500)
                .build(),
            account_manager: OnceLock::new(),
            proxy_manager: OnceLock::new(),
            upstash: OnceLock::new(),
        }
    }

    pub fn set_account_manager(&self, am: Arc<AccountManager>) {
        let _ = self.account_manager.set(am);
    }

    /// Attach the proxy manager so refreshes can rotate first when
    /// ROTATE_PROXIES_ON_REFRESH=true (upstream parity).
    pub fn set_proxy_manager(&self, pm: Arc<ProxyManager>) {
        let _ = self.proxy_manager.set(pm);
    }

    /// Attach shared state once at startup (before serving).
    pub fn set_upstash(&self, store: Option<Arc<UpstashStore>>) {
        if let Some(s) = store {
            let _ = self.upstash.set(s);
        }
    }

    /// A token freshly minted by a sibling instance, if still valid.
    async fn shared_token(&self, account: &AccountState) -> Option<String> {
        let store = self.upstash.get()?;
        let raw = store.get(&UpstashStore::k_token(&account.id)).await?;
        let v: Value = serde_json::from_str(&raw).ok()?;
        let token = v.get("t")?.as_str()?;
        let expires = v.get("e")?.as_i64()?;
        if token.is_empty() || Utc::now().timestamp() >= expires - 60 {
            return None;
        }
        *account.access_token.write().await = Some(token.to_string());
        account.token_expires_at.store(expires, Ordering::Relaxed);
        Some(token.to_string())
    }

    fn share_token(&self, account: &AccountState, token: &str, expires_at: i64, expires_in: i64) {
        let store = match self.upstash.get() {
            Some(s) => s.clone(),
            None => return,
        };
        let payload =
            serde_json::json!({"t": token, "e": expires_at}).to_string();
        let key = UpstashStore::k_token(&account.id);
        let ttl = expires_in.max(120) as u64;
        // Fire-and-forget: the local token is already usable; siblings will
        // pick this up on their next miss (or keep using their own).
        tokio::spawn(async move {
            store.set(&key, &payload, Some(ttl)).await;
        });
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
        // Per-account guard (same singleflight pattern as the response
        // cache): a cold start refreshing 40 accounts fans out instead of
        // queueing behind one global lock.
        let guard = self
            .refresh_locks
            .get_with(account.id.clone(), async { Arc::new(Mutex::new(())) })
            .await;
        let _guard = guard.lock().await;

        {
            let access_token = account.access_token.read().await;
            let expires_at = account.token_expires_at.load(std::sync::atomic::Ordering::Relaxed);
            if let Some(token) = access_token.as_ref() {
                if Utc::now().timestamp() < expires_at && !token.is_empty() {
                    return Ok(token.clone());
                }
            }
        }

        // A sibling instance may have minted a fresh token already — reuse
        // it instead of spending another Tidal refresh (also avoids
        // concurrent-refresh storms across the fleet).
        if let Some(token) = self.shared_token(account).await {
            return Ok(token);
        }

        // Upstream parity: rotate the proxy before refreshing when enabled.
        // Re-resolve afterwards so this refresh uses the new egress.
        let rotated_client;
        let http_client = if self
            .proxy_manager
            .get()
            .map(|pm| pm.should_rotate_on_refresh())
            .unwrap_or(false)
        {
            let pm = self.proxy_manager.get().unwrap().clone();
            pm.rotate_now();
            match pm.working_client().await {
                Ok(c) => {
                    rotated_client = c;
                    &rotated_client
                }
                Err(_) => http_client,
            }
        } else {
            http_client
        };

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

        let status_code = res.status().as_u16();
        if status_code == 400 || status_code == 401 || status_code == 403 {
            let error_data: Value = res.json().await.unwrap_or_default();
            let err_msg = error_data
                .get("error_description")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown auth error");
            tracing::warn!(
                "Refresh token rejected for account {} ({}): {} — deactivating",
                account.label, status_code, err_msg
            );
            account.is_active.store(false, std::sync::atomic::Ordering::Relaxed);
            if let Some(am) = self.account_manager.get() {
                let _ = am.set_account_active(&account.id, false).await;
                let _ = am.set_auto_disabled(&account.id, true).await;
            }
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
            .store(expires_at, Ordering::Relaxed);
        self.share_token(account, &new_token, expires_at, expires_in);

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
        proxy_manager: Arc<crate::proxy_manager::ProxyManager>,
    ) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(300)).await;
                match proxy_manager.working_client().await {
                    Ok(client) => self.prewarm_all(&manager, &client).await,
                    Err(e) => tracing::warn!("Token pre-warm skipped: {}", e),
                }
            }
        });
    }
}
