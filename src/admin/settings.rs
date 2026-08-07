use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

pub async fn get_settings(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "rate_limits": state.rate_limits.snapshot() }))
}

pub async fn update_settings(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let updates = body.get("rate_limits").unwrap_or(&body);

    state
        .rate_limits
        .apply(updates)
        .map_err(AppError::BadRequest)?;

    state.anti_ban.reload_limiter();

    if let Some(db) = &state.db {
        state.rate_limits.save_to_db(db).await;
    }

    Ok(Json(json!({
        "message": "Rate limits updated",
        "rate_limits": state.rate_limits.snapshot()
    })))
}
