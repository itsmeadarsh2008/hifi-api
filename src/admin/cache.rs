use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

pub async fn cache_stats(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    use std::sync::atomic::Ordering;
    Ok(Json(json!({ "cache": {
        "hits": state.cache.hits.load(Ordering::Relaxed),
        "misses": state.cache.misses.load(Ordering::Relaxed),
        "coalesced": state.cache.coalesced.load(Ordering::Relaxed),
    } })))
}

pub async fn cache_clear(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    state.cache.invalidate_all().await;
    Ok(Json(json!({"message": "Response cache cleared"})))
}
