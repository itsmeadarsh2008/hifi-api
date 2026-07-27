use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct LyricsParams {
    pub id: i64,
}

pub async fn get_lyrics(
    State(state): State<AppState>,
    Query(params): Query<LyricsParams>,
) -> Result<Json<Value>, AppError> {
    let account = state.account_manager.select_account().await?;
    let token = state
        .token_manager
        .get_token(&account, state.tidal_client.http_client())
        .await?;

    let url = format!("https://api.tidal.com/v1/tracks/{}/lyrics", params.id);
    let data = state
        .tidal_client
        .make_authed_request(
            &url,
            Some(vec![
                ("countryCode", &state.config.country_code),
                ("locale", "en_US"),
                ("deviceType", "BROWSER"),
            ]),
            &token,
        )
        .await?;

    if data.is_null() || data.as_object().map_or(true, |o| o.is_empty()) {
        return Err(AppError::NotFound("Lyrics not found".into()));
    }

    Ok(Json(json!({
        "version": state.config.api_version,
        "lyrics": data
    })))
}
