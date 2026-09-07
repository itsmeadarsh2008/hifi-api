use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize, Default)]
pub struct RequestsQuery {
    pub limit: Option<usize>,
}

pub async fn get_requests(
    State(state): State<AppState>,
    Query(q): Query<RequestsQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(
        json!({ "requests": state.request_log.summary(q.limit.unwrap_or(20)) }),
    ))
}
