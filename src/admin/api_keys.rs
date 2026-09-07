use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct CreateKeyRequest {
    pub label: Option<String>,
    /// Lifetime request quota. 0 (default) = unlimited.
    pub quota: Option<u64>,
}

pub async fn create_key(
    State(state): State<AppState>,
    Json(body): Json<CreateKeyRequest>,
) -> Result<Json<Value>, AppError> {
    let (key, raw) = state
        .api_keys
        .create(body.label.unwrap_or_default(), body.quota.unwrap_or(0))
        .await?;
    Ok(Json(json!({
        "message": "API key created — copy it now, it is shown only once",
        "key": {
            "id": key.id,
            "label": key.label,
            "key_prefix": key.key_prefix,
            "quota": key.quota.load(std::sync::atomic::Ordering::Relaxed),
        },
        "api_key": raw,
    })))
}

pub async fn list_keys(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let keys = state.api_keys.list().await;
    let list: Vec<Value> = keys
        .iter()
        .map(|k| {
            json!({
                "id": k.id,
                "label": k.label,
                "key_prefix": k.key_prefix,
                "quota": k.quota.load(std::sync::atomic::Ordering::Relaxed),
                "used": k.used.load(std::sync::atomic::Ordering::Relaxed),
                "is_active": k.is_active.load(std::sync::atomic::Ordering::Relaxed),
            })
        })
        .collect();
    Ok(Json(json!({ "api_keys": list })))
}

pub async fn toggle_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let active = body
        .get("active")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| AppError::BadRequest("Missing {active: bool}".into()))?;
    state.api_keys.set_active(&id, active).await?;
    Ok(Json(json!({"message": format!("API key {} set to {}", id, if active { "active" } else { "inactive" })})))
}

pub async fn remove_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    state.api_keys.remove(&id).await?;
    Ok(Json(json!({ "message": "API key removed" })))
}
