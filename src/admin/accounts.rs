use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct AddAccountRequest {
    pub label: Option<String>,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
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

pub async fn test_account(
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
        .get_token(&account, state.tidal_client.http_client())
        .await
    {
        Ok(token) => {
            let resp = state
                .tidal_client
                .http_client()
                .get("https://api.tidal.com/v1/tracks/1/")
                .header("authorization", format!("Bearer {}", token))
                .send()
                .await;
            match resp {
                Ok(r) => {
                    if r.status().is_success() {
                        Ok(Json(json!({"status": "ok", "message": "Account is working"})))
                    } else {
                        Ok(Json(json!({"status": "error", "message": format!("Tidal returned {}", r.status())})))
                    }
                }
                Err(e) => Ok(Json(json!({"status": "error", "message": e.to_string()}))),
            }
        }
        Err(e) => Ok(Json(json!({"status": "error", "message": format!("Token refresh failed: {:?}", e)}))),
    }
}
