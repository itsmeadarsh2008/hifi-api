use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

static TEST_CACHE: Mutex<Option<(i64, Value)>> = Mutex::new(None);

#[derive(Deserialize)]
pub struct AddAccountRequest {
    pub label: Option<String>,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub user_id: Option<String>,
    /// When true, the account serves metadata only (upstream catalog role).
    #[serde(default)]
    pub catalog: Option<bool>,
    /// Upstream token.json style: role="catalog".
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Deserialize)]
pub struct ToggleAccountRequest {
    pub active: bool,
}

pub async fn list_accounts(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let accounts = state.account_manager.list_accounts().await;
    let mut list: Vec<Value> = Vec::with_capacity(accounts.len());
    for a in &accounts {
        list.push(json!({
            "id": a.id,
            "label": a.label,
            "client_id": a.client_id,
            "client_secret": a.client_secret,
            "refresh_token": a.refresh_token,
            "user_id": a.user_id.read().await.clone(),
            "is_active": a.is_active.load(std::sync::atomic::Ordering::Relaxed),
            "auto_disabled": a.auto_disabled.load(std::sync::atomic::Ordering::Relaxed),
            "heal_failures": a.heal_failures.load(std::sync::atomic::Ordering::Relaxed),
            "heal_next_retry": a.heal_next_retry.load(std::sync::atomic::Ordering::Relaxed),
            "request_count": a.request_count.load(std::sync::atomic::Ordering::Relaxed),
            "error_count": a.error_count.load(std::sync::atomic::Ordering::Relaxed),
            "is_catalog": a.is_catalog.load(std::sync::atomic::Ordering::Relaxed),
            "token_expires_at": a.token_expires_at.load(std::sync::atomic::Ordering::Relaxed),
            "last_used": a.last_used.load(std::sync::atomic::Ordering::Relaxed),
            "premium_status": a.premium_status.read().await.clone(),
            "premium_checked_at": a.premium_checked_at.load(std::sync::atomic::Ordering::Relaxed),
            "notes": a.notes.read().await.clone(),
        }));
    }

    Ok(Json(json!({ "accounts": list })))
}

pub async fn add_account(
    State(state): State<AppState>,
    Json(body): Json<AddAccountRequest>,
) -> Result<Json<Value>, AppError> {
    let account = state
        .account_manager
        .add_account(
            body.label.unwrap_or_default(),
            body.client_id,
            body.client_secret,
            body.refresh_token,
            body.user_id,
        )
        .await?;
    let is_catalog = body.catalog.unwrap_or(false)
        || body.role.as_deref().map(|r| r.eq_ignore_ascii_case("catalog")).unwrap_or(false);
    if is_catalog {
        state.account_manager.set_account_catalog(&account.id, true).await?;
    }

    Ok(Json(json!({
        "message": "Account added",
        "account": {
            "id": account.id,
            "label": account.label
        }
    })))
}

#[derive(Deserialize)]
pub struct CatalogAccountRequest {
    pub catalog: bool,
}

/// Flag or unflag an account as catalog-only (metadata, never playback).
pub async fn set_account_catalog(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CatalogAccountRequest>,
) -> Result<Json<Value>, AppError> {
    state.account_manager.set_account_catalog(&id, body.catalog).await?;
    let status = if body.catalog { "catalog-only" } else { "playback" };
    Ok(Json(json!({ "message": format!("Account {} set to {}", id, status) })))
}

pub async fn remove_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    state.account_manager.remove_account(&id).await?;
    Ok(Json(json!({ "message": "Account removed" })))
}

#[derive(Deserialize)]
pub struct UpdateAccountRequest {
    pub label: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub refresh_token: Option<String>,
    pub user_id: Option<String>,
}

pub async fn update_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateAccountRequest>,
) -> Result<Json<Value>, AppError> {
    state
        .account_manager
        .update_account(
            &id,
            body.label,
            body.client_id,
            body.client_secret,
            body.refresh_token,
            body.user_id,
        )
        .await?;
    Ok(Json(json!({ "message": "Account updated" })))
}

pub async fn toggle_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ToggleAccountRequest>,
) -> Result<Json<Value>, AppError> {
    state
        .account_manager
        .set_account_active(&id, body.active)
        .await?;
    // Owner intent wins: a manual toggle always clears the auto-disabled flag,
    // so auto-heal never overrides an explicit OFF.
    let _ = state.account_manager.set_auto_disabled(&id, false).await;
    let status = if body.active { "active" } else { "inactive" };
    Ok(Json(json!({ "message": format!("Account {} set to {}", id, status) })))
}

pub async fn refresh_account_token(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {    let account = state
        .account_manager
        .get_account_by_id(&id)
        .await
        .ok_or_else(|| AppError::NotFound(format!("Account {} not found", id)))?;

    let hc = state.tidal_client.working_client().await?;
    match state
        .token_manager
        .refresh_token(&account, &hc)
        .await
    {
        Ok(_) => {
            state.account_manager.set_account_active(&id, true).await?;
            let _ = state.account_manager.set_auto_disabled(&id, false).await;
            Ok(Json(json!({"status": "ok", "message": "Token refreshed, account reactivated"})))
        }
        Err(e) => {
            state
                .account_manager
                .mark_account_error(&id, &format!("manual refresh failed: {:?}", e))
                .await;
            Err(AppError::UpstreamError(
                StatusCode::BAD_GATEWAY,
                format!("Token refresh failed: {:?}", e),
            ))
        }
    }
}

/// Manual premium probe for one account: FULL on any fixture track means
/// the subscription serves full quality, all-PREVIEW means snippet-only.
/// Display only — records the verdict, never touches flags or counters.
pub async fn check_account_premium(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let account = state
        .account_manager
        .get_account_by_id(&id)
        .await
        .ok_or_else(|| AppError::NotFound(format!("Account {} not found", id)))?;

    let (status, reason) = state.tidal_client.probe_account_premium(&account).await;
    state.account_manager.set_premium(&id, &status).await;
    Ok(Json(json!({
        "account_id": id,
        "premium": status,
        "reason": reason,
        "checked_at": chrono::Utc::now().timestamp(),
    })))
}

pub async fn test_all_accounts(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let now = chrono::Utc::now().timestamp();
    if let Ok(cache) = TEST_CACHE.lock() {
        if let Some((ts, ref results)) = *cache {
            if now - ts < 30 {
                return Ok(Json(results.clone()));
            }
        }
    }

    let accounts = state.account_manager.list_accounts().await;
    let country = &state.config.country_code;
    let client = state.tidal_client.working_client().await?;
    let token_manager = state.token_manager.clone();
    let tidal_client = state.tidal_client.clone();
    let account_manager = state.account_manager.clone();
    // Premium fixtures are throttled: N accounts × 4 probe tracks must not
    // hit Tidal at once. Token+search checks stay fully concurrent.
    let probe_sem = std::sync::Arc::new(tokio::sync::Semaphore::new(4));

    let mut handles = Vec::new();
    for account in &accounts {
        let acc = account.clone();
        let c = client.clone();
        let tm = token_manager.clone();
        let tc = tidal_client.clone();
        let am = account_manager.clone();
        let sem = probe_sem.clone();
        let cc = country.clone();
        handles.push(tokio::spawn(async move {
            let label = acc.label.clone();
            let id = acc.id.clone();
            let token_expires_at = acc.token_expires_at.load(std::sync::atomic::Ordering::Relaxed);
            let is_active = acc.is_active.load(std::sync::atomic::Ordering::Relaxed);
            let start = Instant::now();
            match tm.get_token(&acc, &c).await {
                Ok(token) => {
                    let url = format!(
                        "https://api.tidal.com/v1/search/tracks?query=test&limit=1&countryCode={}",
                        cc
                    );
                    match c.get(&url)
                        .header("authorization", format!("Bearer {}", token))
                        .header("User-Agent", "okhttp/5.3.2")
                        .header("Accept", "*/*")
                        .header("Accept-Encoding", "gzip")
                        .send()
                        .await
                    {
                        Ok(resp) => {
                            let elapsed = start.elapsed().as_millis() as u64;
                            let status_code = resp.status().as_u16();
                            let body = resp.text().await.unwrap_or_default();
                            let response_preview = if body.len() > 500 {
                                format!("{}...", &body[..500])
                            } else {
                                body.clone()
                            };
                            if status_code == 200 {
                                // Basic check passed: probe premium (throttled).
                                // Display only — never sidelines the account.
                                let _permit = sem.acquire_owned().await.ok();
                                let (premium, premium_reason) =
                                    tc.probe_account_premium(&acc).await;
                                am.set_premium(&id, &premium).await;
                                json!({"id": id, "label": label, "ok": true, "ms": elapsed, "status_code": status_code, "response_preview": response_preview, "response_body": body, "token_expires_at": token_expires_at, "is_active": is_active, "premium": premium, "premium_reason": premium_reason})
                            } else {
                                am.set_premium(&id, "unknown").await;
                                json!({"id": id, "label": label, "ok": false, "ms": elapsed, "status_code": status_code, "error": format!("HTTP {}", status_code), "response_preview": response_preview, "response_body": body, "token_expires_at": token_expires_at, "is_active": is_active, "premium": "unknown", "premium_reason": "basic check failed — fix login first"})
                            }
                        }
                        Err(e) => {
                            let elapsed = start.elapsed().as_millis() as u64;
                            am.set_premium(&id, "unknown").await;
                            json!({"id": id, "label": label, "ok": false, "ms": elapsed, "error": e.to_string(), "token_expires_at": token_expires_at, "is_active": is_active, "premium": "unknown", "premium_reason": "basic check failed — fix login first"})
                        }
                    }
                }
                Err(e) => {
                    let elapsed = start.elapsed().as_millis() as u64;
                    am.set_premium(&id, "unknown").await;
                    json!({"id": id, "label": label, "ok": false, "ms": elapsed, "error": format!("Token: {:?}", e), "token_expires_at": token_expires_at, "is_active": is_active, "premium": "unknown", "premium_reason": "basic check failed — fix login first"})
                }
            }
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(r) = handle.await {
            results.push(r);
        }
    }

    let payload = json!({ "results": results });

    if let Ok(mut cache) = TEST_CACHE.lock() {
        *cache = Some((chrono::Utc::now().timestamp(), payload.clone()));
    }

    Ok(Json(payload))
}

pub async fn export_accounts(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let accounts = state.account_manager.list_accounts().await;
    let exported: Vec<Value> = accounts
        .iter()
        .map(|a| {
            let is_catalog = a.is_catalog.load(std::sync::atomic::Ordering::Relaxed);
            let mut obj = json!({
                "label": a.label,
                "client_id": a.client_id,
                "client_secret": a.client_secret,
                "refresh_token": a.refresh_token,
                "user_id": futures::executor::block_on(async { a.user_id.read().await.clone() }),
            });
            if is_catalog {
                obj["role"] = json!("catalog");
            }
            obj
        })
        .collect();
    Ok(Json(json!({ "accounts": exported })))
}

pub async fn import_accounts(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let arr = if let Some(a) = body.get("accounts").and_then(|v| v.as_array()) {
        a.clone()
    } else if let Some(a) = body.as_array() {
        a.clone()
    } else if let Some(a) = body.get("credentials").and_then(|v| v.as_array()) {
        a.clone()
    } else {
        return Err(AppError::BadRequest(
            "Expected {accounts: [...]} or [...] with client_id/client_secret/refresh_token".into(),
        ));
    };

    let existing = state.account_manager.list_accounts().await;
    let existing_tokens: std::collections::HashSet<String> =
        existing.iter().map(|a| a.refresh_token.clone()).collect();

    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut errors: Vec<Value> = Vec::new();

    for (i, val) in arr.iter().enumerate() {
        let client_id = val
            .get("client_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let client_secret = val
            .get("client_secret")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let refresh_token = val
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if client_id.is_empty() || client_secret.is_empty() || refresh_token.is_empty() {
            errors.push(json!({"index": i, "error": "missing client_id/client_secret/refresh_token"}));
            skipped += 1;
            continue;
        }
        if existing_tokens.contains(&refresh_token) {
            skipped += 1;
            continue;
        }
        let label = val
            .get("label")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let user_id = val
            .get("user_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let is_catalog = val
            .get("role")
            .and_then(|v| v.as_str())
            .map(|r| r.eq_ignore_ascii_case("catalog"))
            .unwrap_or(false)
            || val.get("catalog").and_then(|v| v.as_bool()).unwrap_or(false);

        match state
            .account_manager
            .add_account(label, client_id, client_secret, refresh_token, user_id)
            .await
        {
            Ok(acc) => {
                if is_catalog {
                    let _ = state.account_manager.set_account_catalog(&acc.id, true).await;
                }
                imported += 1;
            }
            Err(e) => {
                errors.push(json!({"index": i, "error": format!("{:?}", e)}));
                skipped += 1;
            }
        }
    }

    Ok(Json(json!({
        "imported": imported,
        "skipped": skipped,
        "errors": errors
    })))
}

pub async fn test_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let account = state
        .account_manager
        .get_account_by_id(&id)
        .await
        .ok_or_else(|| AppError::NotFound(format!("Account {} not found", id)))?;

    let token_expires_at = account.token_expires_at.load(std::sync::atomic::Ordering::Relaxed);
    let is_active = account.is_active.load(std::sync::atomic::Ordering::Relaxed);
    let start = Instant::now();

    let hc = state.tidal_client.working_client().await?;
    match state
        .token_manager
        .get_token(&account, &hc)
        .await
    {
        Ok(token) => {
            let token_ms = start.elapsed().as_millis() as u64;
            let resp = hc
                .get("https://api.tidal.com/v1/tracks/1/")
                .header("authorization", format!("Bearer {}", token))
                .send()
                .await;
            match resp {
                Ok(r) => {
                    let status_code = r.status().as_u16();
                    let body_text = r.text().await.unwrap_or_default();
                    let total_ms = start.elapsed().as_millis() as u64;
                    let response_json: Value = serde_json::from_str(&body_text)
                        .unwrap_or(json!({"raw": body_text}));
                    Ok(Json(json!({
                        "status": if status_code == 200 { "ok" } else { "error" },
                        "ms": total_ms,
                        "token_ms": token_ms,
                        "status_code": status_code,
                        "token_expires_at": token_expires_at,
                        "is_active": is_active,
                        "response": response_json
                    })))
                }
                Err(e) => {
                    let total_ms = start.elapsed().as_millis() as u64;
                    Ok(Json(json!({
                        "status": "error",
                        "ms": total_ms,
                        "token_ms": token_ms,
                        "error": e.to_string(),
                        "token_expires_at": token_expires_at,
                        "is_active": is_active,
                    })))
                }
            }
        }
        Err(e) => {
            let total_ms = start.elapsed().as_millis() as u64;
            Ok(Json(json!({
                "status": "error",
                "ms": total_ms,
                "error": format!("Token: {:?}", e),
                "token_expires_at": token_expires_at,
                "is_active": is_active,
            })))
        }
    }
}
