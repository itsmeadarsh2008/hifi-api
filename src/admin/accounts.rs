use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{Path, State};
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
}

#[derive(Deserialize)]
pub struct ToggleAccountRequest {
    pub active: bool,
}

pub async fn list_accounts(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let accounts = state.account_manager.list_accounts().await;
    let list: Vec<Value> = accounts
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "label": a.label,
                "client_id": a.client_id,
                "client_secret": a.client_secret,
                "refresh_token": a.refresh_token,
                "user_id": futures::executor::block_on(async { a.user_id.read().await.clone() }),
                "is_active": a.is_active.load(std::sync::atomic::Ordering::Relaxed),
                "request_count": a.request_count.load(std::sync::atomic::Ordering::Relaxed),
                "error_count": a.error_count.load(std::sync::atomic::Ordering::Relaxed),
                "rate_limit_hits": a.rate_limit_hits.load(std::sync::atomic::Ordering::Relaxed),
                "rate_limited_until": a.rate_limited_until.load(std::sync::atomic::Ordering::Relaxed),
                "token_expires_at": a.token_expires_at.load(std::sync::atomic::Ordering::Relaxed),
                "last_used": a.last_used.load(std::sync::atomic::Ordering::Relaxed),
                "notes": futures::executor::block_on(async { a.notes.read().await.clone() }),
            })
        })
        .collect();

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

    Ok(Json(json!({
        "message": "Account added",
        "account": {
            "id": account.id,
            "label": account.label
        }
    })))
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
    let status = if body.active { "active" } else { "inactive" };
    Ok(Json(json!({ "message": format!("Account {} set to {}", id, status) })))
}

pub async fn refresh_account_token(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let account = state
        .account_manager
        .get_account_by_id(&id)
        .await
        .ok_or_else(|| AppError::NotFound(format!("Account {} not found", id)))?;

    match state
        .token_manager
        .refresh_token(&account, state.tidal_client.http_client())
        .await
    {
        Ok(_) => {
            state.account_manager.set_account_active(&id, true).await?;
            Ok(Json(json!({"status": "ok", "message": "Token refreshed, account reactivated"})))
        }
        Err(e) => Ok(Json(json!({"status": "error", "message": format!("{:?}", e)}))),
    }
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
    let client = state.tidal_client.http_client().clone();
    let token_manager = state.token_manager.clone();

    let mut handles = Vec::new();
    for account in &accounts {
        let acc = account.clone();
        let c = client.clone();
        let tm = token_manager.clone();
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
                                json!({"id": id, "label": label, "ok": true, "ms": elapsed, "status_code": status_code, "response_preview": response_preview, "response_body": body, "token_expires_at": token_expires_at, "is_active": is_active})
                            } else {
                                json!({"id": id, "label": label, "ok": false, "ms": elapsed, "status_code": status_code, "error": format!("HTTP {}", status_code), "response_preview": response_preview, "response_body": body, "token_expires_at": token_expires_at, "is_active": is_active})
                            }
                        }
                        Err(e) => {
                            let elapsed = start.elapsed().as_millis() as u64;
                            json!({"id": id, "label": label, "ok": false, "ms": elapsed, "error": e.to_string(), "token_expires_at": token_expires_at, "is_active": is_active})
                        }
                    }
                }
                Err(e) => {
                    let elapsed = start.elapsed().as_millis() as u64;
                    json!({"id": id, "label": label, "ok": false, "ms": elapsed, "error": format!("Token: {:?}", e), "token_expires_at": token_expires_at, "is_active": is_active})
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

    match state
        .token_manager
        .get_token(&account, state.tidal_client.http_client())
        .await
    {
        Ok(token) => {
            let token_ms = start.elapsed().as_millis() as u64;
            let resp = state
                .tidal_client
                .http_client()
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
