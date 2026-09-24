use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use reqwest::Client;
use serde_json::{json, Value};

use crate::account_manager::{AccountManager, AccountState};
use crate::config::Config;
use crate::error::AppError;
use crate::notifier::Notifier;
use crate::proxy_manager::ProxyManager;
use crate::token_manager::TokenManager;

pub struct TidalClient {
    proxy_manager: Arc<ProxyManager>,
    token_manager: Arc<TokenManager>,
    account_manager: Arc<AccountManager>,
    notifier: Arc<Notifier>,
    config: Arc<Config>,
}

impl TidalClient {
    pub fn new(
        proxy_manager: Arc<ProxyManager>,
        token_manager: Arc<TokenManager>,
        account_manager: Arc<AccountManager>,
        notifier: Arc<Notifier>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            proxy_manager,
            token_manager,
            account_manager,
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

        let http = self.working_client().await?;

        let mut failed_ids: Vec<String> = Vec::new();
        let account_count = self.account_manager.playback_count().await;
        let max_account_attempts = std::cmp::max(1, account_count);
        let mut last_account_error: Option<AppError> = None;

        for _account_try in 0..max_account_attempts {
            let account = if _account_try == 0 && failed_ids.is_empty() {
                match preferred_account.clone() {
                    Some(a) => {
                        // Account the preferred pick like any selection so
                        // the balancer sees its true load.
                        AccountManager::note_selection(&a);
                        a
                    }
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

            for attempt in 0..max_retries {

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
                    .header("User-Agent", self.config.user_agent.as_str())
                    .header("Accept", "*/*")
                    .header("Accept-Encoding", "gzip")
                    .header("Accept-Language", "en-US,en;q=0.9")
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
                                    .header("User-Agent", self.config.user_agent.as_str())
                                    .header("Accept", "*/*")
                                    .header("Accept-Encoding", "gzip")
                                    .header("Accept-Language", "en-US,en;q=0.9")
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
                        // No cooldown parking: fail over to the next account immediately.
                        failed_ids.push(account.id.clone());
                        last_account_error = Some(AppError::Timeout);
                        break;
                    }
                    403 => {
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
                self.dev_log("GET", url, status.as_u16(), &body);
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
            .header("User-Agent", self.config.user_agent.as_str())
            .header("Accept", "*/*")
            .header("Accept-Encoding", "gzip")
            .header("Accept-Language", "en-US,en;q=0.9")
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
        self.dev_log("GET", url, status.as_u16(), &body);
        let data: Value = serde_json::from_str(&body)
            .map_err(|e| AppError::UpstreamError(
                status,
                format!("Failed to parse Tidal response: {} | body: {}",
                    e, body.chars().take(200).collect::<String>()),
            ))?;
        Ok(data)
    }

    /// Verbose upstream logging (upstream DEV_MODE). No-op unless enabled.
    fn dev_log(&self, method: &str, url: &str, status: u16, body: &str) {
        if !self.config.dev_mode {
            return;
        }
        tracing::info!(
            "[DEV] {} {} → {}\n  body: {}",
            method,
            url,
            status,
            body.chars().take(1000).collect::<String>(),
        );
    }

    /// Metadata request (upstream catalog=True): static CATALOG_TOKEN first,
    /// then the dedicated catalog account, then the playback pool.
    /// Returns the wrapped {version, data} envelope like make_request.
    pub async fn make_catalog_request(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
    ) -> Result<Value, AppError> {
        let owned: Vec<(String, String)> = params
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let borrowed: Vec<(&str, &str)> =
            owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let data = self.catalog_get(url, borrowed).await?;
        Ok(json!({"version": self.config.api_version, "data": data}))
    }

    /// Raw metadata GET (unwrapped payload) with the same catalog preference.
    pub async fn make_catalog_authed_request(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
    ) -> Result<Value, AppError> {
        let owned: Vec<(String, String)> = params
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let borrowed: Vec<(&str, &str)> =
            owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        self.catalog_get(url, borrowed).await
    }

    /// Shared catalog resolution: static token → catalog account → pool.
    /// Catalog failures fall through to the pool so metadata stays up.
    async fn catalog_get(
        &self,
        url: &str,
        params: Vec<(&str, &str)>,
    ) -> Result<Value, AppError> {
        if !self.config.catalog_token.is_empty() {
            match self
                .catalog_static_get(url, params.clone())
                .await
            {
                Ok(data) => return Ok(data),
                Err(e) => {
                    tracing::debug!("Catalog static token failed, trying catalog account: {}", e);
                }
            }
        }
        if let Some(acc) = self.account_manager.next_active_catalog().await {
            match self.catalog_account_get(&acc, url, params.clone()).await {
                Ok(data) => return Ok(data),
                Err(e) => {
                    tracing::debug!("Catalog account failed, falling back to pool: {}", e);
                }
            }
        }
        // No catalog configured (or it failed): normal pool request.
        self.make_request(url, Some(params)).await
            .map(|wrapped| wrapped.get("data").cloned().unwrap_or(Value::Null))
    }

    async fn catalog_static_get(
        &self,
        url: &str,
        params: Vec<(&str, &str)>,
    ) -> Result<Value, AppError> {
        let http = self.working_client().await?;
        let mut req = http
            .get(url)
            .header("authorization", format!("Bearer {}", self.config.catalog_token))
            .header("User-Agent", self.config.user_agent.as_str())
            .header("Accept", "*/*")
            .header("Accept-Encoding", "gzip")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("X-Platform", "android")
            .header("X-Tidal-Platform", "android");
        if !params.is_empty() {
            req = req.query(&params);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(AppError::UpstreamError(status, "Catalog token request failed".into()));
        }
        let body = resp.text().await?;
        self.dev_log("GET", url, status.as_u16(), &body);
        serde_json::from_str(&body).map_err(|e| {
            AppError::UpstreamError(
                status,
                format!("Failed to parse Tidal response: {}", e),
            )
        })
    }

    async fn catalog_account_get(
        &self,
        account: &Arc<AccountState>,
        url: &str,
        params: Vec<(&str, &str)>,
    ) -> Result<Value, AppError> {
        let http = self.working_client().await?;
        let mut token = self.token_manager.get_token(account, &http).await?;
        for attempt in 0..2 {
            let mut req = http
                .get(url)
                .header("authorization", format!("Bearer {}", token))
                .header("User-Agent", self.config.user_agent.as_str())
                .header("Accept", "*/*")
                .header("Accept-Encoding", "gzip")
                .header("Accept-Language", "en-US,en;q=0.9")
                .header("X-Platform", "android")
                .header("X-Tidal-Platform", "android");
            if !params.is_empty() {
                req = req.query(&params);
            }
            let resp = req.send().await?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt == 0 {
                token = self.token_manager.refresh_token(account, &http).await?;
                continue;
            }
            if !status.is_success() {
                return Err(AppError::UpstreamError(status, "Catalog account request failed".into()));
            }
            let body = resp.text().await?;
            self.dev_log("GET", url, status.as_u16(), &body);
            return serde_json::from_str(&body).map_err(|e| {
                AppError::UpstreamError(
                    status,
                    format!("Failed to parse Tidal response: {}", e),
                )
            });
        }
        Err(AppError::Unauthorized("Catalog account unauthorized".into()))
    }
}

/// Probe fixtures shared with pool-contributor's premium check: mainstream
/// tracks expected FULL on any premium subscription.
const PROBE_TRACK_IDS: &[i64] = &[427520487, 39249713, 58990511, 144371283];
/// Per-request ceiling so one stuck probe can't hang the admin call.
const PROBE_REQ_SECS: u64 = 20;

impl TidalClient {
    /// Read the presentation out of a playbackinfo payload. Pure function —
    /// unit tested.
    pub(crate) fn presentation_of(body: &Value) -> Option<String> {
        body.get("assetPresentation")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Manual premium probe for one account: FULL anywhere → premium, all
    /// PREVIEW → preview-only, anything inconclusive → unknown/error.
    /// Read-only by design: records no errors, touches no flags — real
    /// traffic already handles sidelining on its own.
    pub async fn probe_account_premium(
        &self,
        account: &std::sync::Arc<AccountState>,
    ) -> (String, String) {
        let http = match self.working_client().await {
            Ok(h) => h,
            Err(e) => return ("unknown".into(), format!("no egress: {:?}", e)),
        };
        let mut token = match self.token_manager.get_token(account, &http).await {
            Ok(t) => t,
            Err(e) => return ("error".into(), format!("token failed: {:?}", e)),
        };
        let mut preview_reason = String::new();
        for track_id in PROBE_TRACK_IDS {
            let url = format!("https://api.tidal.com/v1/tracks/{}/playbackinfo", track_id);
            let mut tried_refresh = false;
            let presentation = loop {
                let send = http
                    .get(&url)
                    .query(&[
                        ("audioquality", "HI_RES_LOSSLESS"),
                        ("playbackmode", "STREAM"),
                        ("assetpresentation", "FULL"),
                    ])
                    .header("authorization", format!("Bearer {}", token))
                    .header("User-Agent", self.config.user_agent.as_str())
                    .send();
                let resp = match tokio::time::timeout(
                    std::time::Duration::from_secs(PROBE_REQ_SECS),
                    send,
                )
                .await
                {
                    Ok(Ok(r)) => r,
                    _ => {
                        return (
                            "unknown".into(),
                            "network error reaching Tidal".into(),
                        )
                    }
                };
                match resp.status().as_u16() {
                    // Stale token: refresh once per probe, retry same track.
                    401 if !tried_refresh => {
                        tried_refresh = true;
                        match self.token_manager.refresh_token(account, &http).await {
                            Ok(t) => {
                                token = t;
                                continue;
                            }
                            Err(e) => {
                                return (
                                    "error".into(),
                                    format!("token refresh failed: {:?}", e),
                                )
                            }
                        }
                    }
                    // Throttled / server-side / restricted: inconclusive,
                    // don't burn the remaining fixtures.
                    s if s == 429 || s >= 500 => {
                        return (
                            "unknown".into(),
                            format!("Tidal HTTP {} — try again later", s),
                        )
                    }
                    403 => {
                        return (
                            "unknown".into(),
                            "Tidal 403 — account may be restricted".into(),
                        )
                    }
                    _ => {
                        let body = resp.text().await.unwrap_or_default();
                        let data: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                        break Self::presentation_of(&data);
                    }
                }
            };
            match presentation.as_deref() {
                Some("FULL") => return ("premium".into(), String::new()),
                Some("PREVIEW") => {
                    if preview_reason.is_empty() {
                        preview_reason = "every probed track returned PREVIEW".to_string();
                    }
                }
                // Unparseable track: inconclusive, try the next fixture.
                _ => continue,
            }
        }
        if preview_reason.is_empty() {
            preview_reason = "no fixture gave a conclusive answer".to_string();
        }
        ("preview-only".into(), preview_reason)
    }
}

#[cfg(test)]
mod tests {
    use super::TidalClient;
    use serde_json::json;

    #[test]
    fn presentation_shapes() {
        assert_eq!(
            TidalClient::presentation_of(&json!({"assetPresentation": "FULL"})),
            Some("FULL".to_string())
        );
        assert_eq!(
            TidalClient::presentation_of(&json!({"assetPresentation": "PREVIEW"})),
            Some("PREVIEW".to_string())
        );
        assert_eq!(TidalClient::presentation_of(&json!({})), None);
        assert_eq!(TidalClient::presentation_of(&json!({"assetPresentation": 7})), None);
    }
}
