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
use crate::rate_limit::RateLimitSettings;
use crate::token_manager::TokenManager;

pub struct TidalClient {
    http_client: Client,
    token_manager: Arc<TokenManager>,
    account_manager: Arc<AccountManager>,
    anti_ban: Arc<AntiBan>,
    rate_limits: Arc<RateLimitSettings>,
    config: Arc<Config>,
}

impl TidalClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        http_client: Client,
        token_manager: Arc<TokenManager>,
        account_manager: Arc<AccountManager>,
        anti_ban: Arc<AntiBan>,
        rate_limits: Arc<RateLimitSettings>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            http_client,
            token_manager,
            account_manager,
            anti_ban,
            rate_limits,
            config,
        }
    }

    pub fn http_client(&self) -> &Client {
        &self.http_client
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

        let mut failed_ids: Vec<String> = Vec::new();
        let account_count = self.account_manager.account_count().await;
        let max_account_attempts = std::cmp::max(1, account_count);
        let mut last_account_error: Option<AppError> = None;

        for _account_try in 0..max_account_attempts {
            let account = if _account_try == 0 && failed_ids.is_empty() {
                match preferred_account.clone() {
                    Some(a) => a,
                    None => self.account_manager.select_account_excluding(&failed_ids).await?,
                }
            } else {
                self.account_manager.select_account_excluding(&failed_ids).await?
            };

            for attempt in 0..max_retries {
                self.anti_ban.until_ready().await;

                let token = match self
                    .token_manager
                    .get_token(&account, &self.http_client)
                    .await
                {
                    Ok(t) => t,
                    Err(e) => {
                        last_account_error = Some(e);
                        failed_ids.push(account.id.clone());
                        break;
                    }
                };

                if attempt > 0 {
                    let jitter = rand::thread_rng().gen_range(100..500);
                    tokio::time::sleep(Duration::from_millis(jitter)).await;
                }

                let mut req = self
                    .http_client
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

                match status.as_u16() {
                    401 => {
                        let _ = self.token_manager.refresh_token(&account, &self.http_client).await;
                        continue;
                    }
                    404 => {
                        let fresh_token = self
                            .token_manager
                            .refresh_token(&account, &self.http_client)
                            .await?;

                        let stored = account.access_token.read().await;
                        if let Some(ref stored_token) = *stored {
                            if stored_token != &fresh_token {
                                drop(stored);
                                let mut req2 = self
                                    .http_client
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
                                let resp2 = req2.send().await?;
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

    pub async fn make_authed_request(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
        token: &str,
    ) -> Result<Value, AppError> {
        let mut req = self
            .http_client
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
