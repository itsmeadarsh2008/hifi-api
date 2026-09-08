use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use reqwest::Client;
use serde_json::{json, Value};

use crate::account_manager::{AccountManager, AccountState};
use crate::anti_ban::AntiBan;
use crate::config::Config;
use crate::error::AppError;
use crate::notifier::Notifier;
use crate::proxy_manager::ProxyManager;
use crate::rate_limit::RateLimitSettings;
use crate::token_manager::TokenManager;

pub struct TidalClient {
    proxy_manager: Arc<ProxyManager>,
    token_manager: Arc<TokenManager>,
    account_manager: Arc<AccountManager>,
    anti_ban: Arc<AntiBan>,
    rate_limits: Arc<RateLimitSettings>,
    notifier: Arc<Notifier>,
    config: Arc<Config>,
}

impl TidalClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        proxy_manager: Arc<ProxyManager>,
        token_manager: Arc<TokenManager>,
        account_manager: Arc<AccountManager>,
        anti_ban: Arc<AntiBan>,
        rate_limits: Arc<RateLimitSettings>,
        notifier: Arc<Notifier>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            proxy_manager,
            token_manager,
            account_manager,
            anti_ban,
            rate_limits,
            notifier,
            config,
        }
    }

    /// Current HTTP client (whatever the proxy swap holds right now).
    pub fn http_client(&self) -> Client {
        self.proxy_manager.client()
    }

    /// Client gated for Tidal traffic: resolves a working proxy first,
    /// or errors (never silently leaks direct) unless fallback is enabled.
    pub async fn working_client(&self) -> Result<Client, AppError> {
        self.proxy_manager.working_client().await
    }

    pub fn proxy_manager(&self) -> &Arc<ProxyManager> {
        &self.proxy_manager
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn token_manager(&self) -> &TokenManager {
        &self.token_manager
    }

    pub fn account_manager(&self) -> &AccountManager {
        &self.account_manager
    }

    pub async fn make_request(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
    ) -> Result<Value, AppError> {
        self.make_request_with_account(url, params, None).await
    }

    pub async fn make_request_with_account(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
        preferred_account: Option<Arc<AccountState>>,
    ) -> Result<Value, AppError> {
        let max_retries = if self.config.use_proxies {
            self.config.max_retries
        } else {
            1
        };

        // Conservation shed: when the healthy pool is at/under reserve,
        // fail fast with 429 instead of spending the last accounts.
        if self.anti_ban.in_conservation() {
            if let Err(wait) = self.anti_ban.check_conserve() {
                let secs = wait.as_secs().max(1);
                return Err(AppError::TooManyRequests(
                    format!(
                        "Conservation mode: pool nearly exhausted, retry in {}s.",
                        secs
                    ),
                    secs,
                ));
            }
        }

        let http = self.working_client().await?;

        let mut failed_ids: Vec<String> = Vec::new();
        let account_count = self.account_manager.account_count().await;
        let max_account_attempts = std::cmp::max(1, account_count);
        let mut last_account_error: Option<AppError> = None;

        for _account_try in 0..max_account_attempts {
            let account = if _account_try == 0 && failed_ids.is_empty() {
                match preferred_account.clone() {
                    Some(a) => a,
                    None => match self.account_manager.select_account_excluding(&failed_ids).await {
                        Ok(a) => a,
                        Err(e) => {
                            self.alert_if_all_down(&e).await;
                            return Err(e);
                        }
                    },
                }
            } else {
                match self.account_manager.select_account_excluding(&failed_ids).await {
                    Ok(a) => a,
                    Err(e) => {
                        self.alert_if_all_down(&e).await;
                        return Err(e);
                    }
                }
            };

            self.maybe_alert_budget(&account).await;

            for attempt in 0..max_retries {
                self.anti_ban.throttle_tidal().await;
                self.anti_ban.throttle_account(&account.id).await;

                let token = match self
                    .token_manager
                    .get_token(&account, &http)
                    .await
                {
                    Ok(t) => t,
                    Err(e) => {
                        self.account_manager
                            .mark_account_error(&account.id, &format!("token failure: {:?}", e))
                            .await;
                        last_account_error = Some(e);
                        failed_ids.push(account.id.clone());
                        break;
                    }
                };

                if attempt > 0 {
                    let jitter = rand::thread_rng().gen_range(100..500);
                    tokio::time::sleep(Duration::from_millis(jitter)).await;
                }

                let mut req = http
                    .get(url)
                    .header("authorization", format!("Bearer {}", token))
                    .header("User-Agent", "okhttp/5.3.2")
                    .header("Accept", "*/*")
                    .header("Accept-Encoding", "gzip")
                    .header("X-Platform", "android")
                    .header("X-Tidal-Platform", "android");

                if let Some(ref p) = params {
                    req = req.query(&p);
                }

                let resp = match req.send().await {
                    Ok(r) => {
                        self.proxy_manager.note_success();
                        r
                    }
                    Err(e) => {
                        if e.is_connect() || e.is_timeout() {
                            self.proxy_manager.note_failure();
                        }
                        return Err(e.into());
                    }
                };
                let status = resp.status();

                match status.as_u16() {
                    401 => {
                        let _ = self.token_manager.refresh_token(&account, &http).await;
                        if attempt >= max_retries - 1 {
                            self.account_manager
                                .mark_account_error(&account.id, "Tidal 401 unauthorized")
                                .await;
                        }
                        continue;
                    }
                    404 => {
                        let fresh_token = self
                            .token_manager
                            .refresh_token(&account, &http)
                            .await?;

                        let stored = account.access_token.read().await;
                        if let Some(ref stored_token) = *stored {
                            if stored_token != &fresh_token {
                                drop(stored);
                                let mut req2 = http
                                    .get(url)
                                    .header("authorization", format!("Bearer {}", fresh_token))
                                    .header("User-Agent", "okhttp/5.3.2")
                                    .header("Accept", "*/*")
                                    .header("Accept-Encoding", "gzip")
                                    .header("X-Platform", "android")
                                    .header("X-Tidal-Platform", "android");
                                if let Some(ref p) = params {
                                    req2 = req2.query(&p);
                                }
                                let resp2 = match req2.send().await {
                                    Ok(r) => {
                                        self.proxy_manager.note_success();
                                        r
                                    }
                                    Err(e) => {
                                        if e.is_connect() || e.is_timeout() {
                                            self.proxy_manager.note_failure();
                                        }
                                        return Err(e.into());
                                    }
                                };
                                let status2 = resp2.status();
                                if status2.is_success() {
                                    let body2 = resp2.text().await?;
                                    let data: Value = serde_json::from_str(&body2)
                                        .map_err(|e| AppError::UpstreamError(
                                            status2,
                                            format!("Failed to parse Tidal response: {} | body: {}",
                                                e, body2.chars().take(200).collect::<String>()),
                                        ))?;
                                    return Ok(json!({"version": self.config.api_version, "data": data}));
                                }
                            }
                        }

                        return Err(AppError::NotFound("Resource not found".into()));
                    }
                    429 => {
                        self.account_manager
                            .mark_account_rate_limited(
                                &account.id,
                                self.rate_limits.cooldown_429_secs.load(Ordering::Relaxed),
                            )
                            .await;
                        failed_ids.push(account.id.clone());
                        last_account_error = Some(AppError::Timeout);
                        break;
                    }
                    403 => {
                        self.account_manager
                            .mark_account_rate_limited(
                                &account.id,
                                self.rate_limits.cooldown_403_secs.load(Ordering::Relaxed),
                            )
                            .await;
                        if attempt < max_retries - 1 {
                            continue;
                        }
                        self.account_manager
                            .mark_account_error(&account.id, "Tidal 403 forbidden")
                            .await;
                        let (healthy, total) = self.account_manager.healthy_count().await;
                        self.notifier.alert_403(&account.label, healthy, total).await;
                        failed_ids.push(account.id.clone());
                        last_account_error = Some(AppError::UpstreamError(
                            status,
                            "Upstream API error".into(),
                        ));
                        break;
                    }
                    _ => {
                        if !status.is_success() {
                            if attempt < max_retries - 1 && status.as_u16() >= 500 {
                                continue;
                            }
                            self.account_manager
                                .mark_account_error(
                                    &account.id,
                                    &format!("Tidal HTTP {}", status.as_u16()),
                                )
                                .await;
                            failed_ids.push(account.id.clone());
                            last_account_error = Some(AppError::UpstreamError(
                                status,
                                "Upstream API error".into(),
                            ));
                            break;
                        }
                    }
                }

                let body = resp.text().await?;
                let data: Value = serde_json::from_str(&body)
                    .map_err(|e| AppError::UpstreamError(
                        status,
                        format!("Failed to parse Tidal response: {} | body: {}",
                            e, body.chars().take(200).collect::<String>()),
                    ))?;

                // Preview-only (FULL requires subscription) → try next account instead of returning 30s snippet
                let is_preview = data
                    .get("assetPresentation")
                    .and_then(|v| v.as_str())
                    == Some("PREVIEW")
                    || data
                        .pointer("/data/attributes/trackPresentation")
                        .and_then(|v| v.as_str())
                        == Some("PREVIEW");
                if is_preview {
                    failed_ids.push(account.id.clone());
                    last_account_error = Some(AppError::ServiceUnavailable(format!(
                        "Preview only for track: account {} cannot provide FULL (subscription required)",
                        account.id
                    )));
                    break;
                }

                if url.contains("playbackinfo") || url.contains("trackManifests") {
                    return Ok(json!({"version": self.config.api_version, "data": data}));
                }

                return Ok(json!({"version": self.config.api_version, "data": data}));
            }
        }

        Err(last_account_error.unwrap_or(AppError::ServiceUnavailable(
            "All accounts failed after fallback".into(),
        )))
    }

    /// Warn once per account per day when its daily budget crosses the alert pct.
    async fn maybe_alert_budget(&self, account: &AccountState) {
        use std::sync::atomic::Ordering;
        let budget = self.rate_limits.daily_budget_per_account.load(Ordering::Relaxed);
        if budget == 0 {
            return;
        }
        let pct = self.rate_limits.daily_budget_alert_pct.load(Ordering::Relaxed).min(100);
        let used = account.day_requests.load(Ordering::Relaxed);
        if used * 100 < budget * pct {
            return;
        }
        let today = crate::account_manager::utc_day(chrono::Utc::now().timestamp());
        if account.day_alerted.load(Ordering::Relaxed) == today {
            return;
        }
        account.day_alerted.store(today, Ordering::Relaxed);
        // Stable codename (matches accounts roster reports).
        let mut ids: Vec<String> = self
            .account_manager
            .list_accounts()
            .await
            .iter()
            .map(|a| a.id.clone())
            .collect();
        ids.sort();
        let code = ids
            .iter()
            .position(|id| *id == account.id)
            .map(|i| format!("TIDAL-{}", i + 1))
            .unwrap_or_else(|| "TIDAL-?".to_string());
        self.notifier.alert_budget(&code, used, budget).await;
    }

    async fn alert_if_all_down(&self, e: &AppError) {
        if let AppError::ServiceUnavailable(msg) = e {
            if msg.contains("All accounts") {
                let (_, total) = self.account_manager.healthy_count().await;
                self.notifier.alert_all_down(total).await;
            }
        }
    }

    pub async fn make_authed_request(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
        token: &str,
    ) -> Result<Value, AppError> {
        let http = self.working_client().await?;
        let mut req = http
            .get(url)
            .header("authorization", format!("Bearer {}", token))
            .header("User-Agent", "okhttp/5.3.2")
            .header("Accept", "*/*")
            .header("Accept-Encoding", "gzip")
            .header("X-Platform", "android")
            .header("X-Tidal-Platform", "android");

        if let Some(ref p) = params {
            req = req.query(&p);
        }

        let resp = req.send().await?;
        let status = resp.status();

        if !status.is_success() {
            return Err(AppError::UpstreamError(status, "Upstream API error".into()));
        }

        let body = resp.text().await?;
        let data: Value = serde_json::from_str(&body)
            .map_err(|e| AppError::UpstreamError(
                status,
                format!("Failed to parse Tidal response: {} | body: {}",
                    e, body.chars().take(200).collect::<String>()),
            ))?;
        Ok(data)
    }
}
