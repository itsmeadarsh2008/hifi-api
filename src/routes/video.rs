use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
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
) -> Result<Response, AppError> {
    let v = super::run_direct(
        &state,
        fetch_video_playback(&state, params.id, &params.quality, &params.mode, &params.presentation),
    )
    .await?;
    Ok(Json(v).into_response())
}

/// Core /video/ fetch (runs directly, no queue).
/// Fails over across playback accounts like the other playback routes so
/// one bad/banned account neither pins the load nor fails the request.
pub(crate) async fn fetch_video_playback(
    state: &AppState,
    id: i64,
    quality: &str,
    mode: &str,
    presentation: &str,
) -> Result<Value, AppError> {
    let url = format!("https://api.tidal.com/v1/videos/{}/playbackinfo", id);
    let pool = state.account_manager.playback_count().await.max(1);
    let mut failed_ids: Vec<String> = Vec::new();
    let mut last_err: Option<AppError> = None;

    for _ in 0..pool {
        let account = match state
            .account_manager
            .select_account_excluding(&failed_ids)
            .await
        {
            Ok(a) => a,
            Err(e) => return Err(last_err.unwrap_or(e)),
        };
        let hc = state.tidal_client.working_client().await?;
        let token = match state.token_manager.get_token(&account, &hc).await {
            Ok(t) => t,
            Err(e) => {
                state
                    .account_manager
                    .mark_account_error(&account.id, &format!("token failure: {:?}", e))
                    .await;
                failed_ids.push(account.id.clone());
                last_err = Some(e);
                continue;
            }
        };
        let params = || {
            vec![
                ("videoquality", quality),
                ("playbackmode", mode),
                ("assetpresentation", presentation),
            ]
        };
        match state
            .tidal_client
            .make_authed_request(&url, Some(params()), &token)
            .await
        {
            Ok(data) => {
                return Ok(json!({
                    "version": state.config.api_version,
                    "video": data
                }));
            }
            Err(AppError::UpstreamError(status, _)) if status.as_u16() == 401 => {
                // Refresh once and retry the same account before failing over.
                match state.token_manager.refresh_token(&account, &hc).await {
                    Ok(fresh) => {
                        match state
                            .tidal_client
                            .make_authed_request(&url, Some(params()), &fresh)
                            .await
                        {
                            Ok(data) => {
                                return Ok(json!({
                                    "version": state.config.api_version,
                                    "video": data
                                }));
                            }
                            Err(e2) => {
                                state
                                    .account_manager
                                    .mark_account_error(
                                        &account.id,
                                        &format!("video retry failed: {:?}", e2),
                                    )
                                    .await;
                                failed_ids.push(account.id.clone());
                                last_err = Some(e2);
                                continue;
                            }
                        }
                    }
                    Err(e2) => {
                        state
                            .account_manager
                            .mark_account_error(&account.id, &format!("token refresh failure: {:?}", e2))
                            .await;
                        failed_ids.push(account.id.clone());
                        last_err = Some(e2);
                        continue;
                    }
                }
            }
            Err(e @ AppError::UpstreamError(status, _))
                if status.as_u16() == 429
                    || status.as_u16() == 403
                    || status.as_u16() >= 500 =>
            {
                state
                    .account_manager
                    .mark_account_error(&account.id, &format!("Tidal HTTP {}", status.as_u16()))
                    .await;
                failed_ids.push(account.id.clone());
                last_err = Some(e);
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(last_err.unwrap_or(AppError::ServiceUnavailable(
        "All accounts failed".into(),
    )))
}
