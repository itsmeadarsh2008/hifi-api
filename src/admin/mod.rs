pub mod accounts;
pub mod stats;
pub mod ui;

use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::AppState;

pub async fn admin_auth(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    if state.config.admin_key.is_empty() {
        return Ok(next.run(req).await);
    }

    let admin_key = req
        .headers()
        .get("X-Admin-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if admin_key == state.config.admin_key {
        return Ok(next.run(req).await);
    }

    Err((
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"detail": "Invalid or missing X-Admin-Key header"})),
    )
        .into_response())
}
