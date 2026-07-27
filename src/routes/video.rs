use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct VideoParams {
    pub id: i64,
    #[serde(default = "default_video_quality")]
    pub quality: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_presentation")]
    pub presentation: String,
}

fn default_video_quality() -> String { "HIGH".into() }
fn default_mode() -> String { "STREAM".into() }
fn default_presentation() -> String { "FULL".into() }

pub async fn get_video(
    State(state): State<AppState>,
    Query(params): Query<VideoParams>,
) -> Result<Json<Value>, AppError> {
    let account = state.account_manager.select_account().await?;
    let token = state
        .token_manager
        .get_token(&account, state.tidal_client.http_client())
        .await?;

    let url = format!("https://api.tidal.com/v1/videos/{}/playbackinfo", params.id);
    let data = state
        .tidal_client
        .make_authed_request(
            &url,
            Some(vec![
                ("videoquality", &params.quality),
                ("playbackmode", &params.mode),
                ("assetpresentation", &params.presentation),
            ]),
            &token,
        )
        .await?;

    Ok(Json(json!({
        "version": state.config.api_version,
        "video": data
    })))
}
