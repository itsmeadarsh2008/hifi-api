use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct TopVideosParams {
    #[serde(default = "default_country")]
    pub countryCode: String,
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default = "default_device")]
    pub deviceType: String,
    #[serde(default = "default_video_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_country() -> String { "US".into() }
fn default_locale() -> String { "en_US".into() }
fn default_device() -> String { "BROWSER".into() }
fn default_video_limit() -> i64 { 25 }

pub async fn get_top_videos(
    State(state): State<AppState>,
    Query(params): Query<TopVideosParams>,
) -> Result<Json<Value>, AppError> {
    let account = state.account_manager.select_account().await?;
    let token = state
        .token_manager
        .get_token(&account, state.tidal_client.http_client())
        .await?;

    let url = "https://api.tidal.com/v1/pages/mymusic_recommended_videos";
    let data = state
        .tidal_client
        .make_authed_request(
            url,
            Some(vec![
                ("countryCode", &params.countryCode),
                ("locale", &params.locale),
                ("deviceType", &params.deviceType),
            ]),
            &token,
        )
        .await?;

    let mut all_videos: Vec<Value> = Vec::new();

    if let Some(rows) = data.get("rows").and_then(|v| v.as_array()) {
        for row in rows {
            if let Some(modules) = row.get("modules").and_then(|v| v.as_array()) {
                for module in modules {
                    let module_type = module.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    let is_video_type = module_type.contains("VIDEO")
                        || module_type.contains("PAGED_LIST");

                    if is_video_type {
                        if let Some(paged_list) = module.get("pagedList") {
                            if let Some(items) = paged_list.get("items").and_then(|v| v.as_array()) {
                                for item in items {
                                    let video = item.get("item").cloned().unwrap_or(item.clone());
                                    all_videos.push(video);
                                }
                            }
                        }
                        if let Some(item) = module.get("item") {
                            if item.is_object() {
                                all_videos.push(item.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    let limit = params.limit as usize;
    let offset = params.offset as usize;
    let paginated: Vec<Value> = all_videos
        .iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect();

    Ok(Json(json!({
        "version": state.config.api_version,
        "videos": paginated,
        "total": all_videos.len()
    })))
}
