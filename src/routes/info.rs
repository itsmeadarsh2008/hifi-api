use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct InfoParams {
    pub id: i64,
}

pub async fn get_info(
    State(state): State<AppState>,
    Query(params): Query<InfoParams>,
) -> Result<Json<Value>, AppError> {
    let url = format!("https://api.tidal.com/v1/tracks/{}/", params.id);
    let result = state
        .tidal_client
        .make_request(
            &url,
            Some(vec![("countryCode", &state.config.country_code)]),
        )
        .await?;
    Ok(Json(result))
}
