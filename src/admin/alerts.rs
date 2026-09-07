use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

pub async fn alert_status(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "alerts": { "discord_configured": state.notifier.configured() } })))
}

pub async fn alert_test(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    match state.notifier.send_test().await {
        Ok(()) => Ok(Json(json!({"message": "Test alert sent to Discord"}))),
        Err(e) => Err(AppError::BadRequest(e)),
    }
}
