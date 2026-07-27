use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct PlaylistParams {
    pub id: String,
    #[serde(default = "default_playlist_limit")]
    #[allow(dead_code)]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_playlist_limit() -> i64 {
    100
}

pub async fn get_playlist(
    State(state): State<AppState>,
    Query(params): Query<PlaylistParams>,
) -> Result<Json<Value>, AppError> {
    let account = state.account_manager.select_account().await?;
    let token = state
        .token_manager
        .get_token(&account, state.tidal_client.http_client())
        .await?;

    let cc = &state.config.country_code;
    let offset_str = params.offset.to_string();

    let playlist_url = format!("https://api.tidal.com/v1/playlists/{}", params.id);
    let items_url = format!("https://api.tidal.com/v1/playlists/{}/items", params.id);

    let playlist_fut = state.tidal_client.make_authed_request(
        &playlist_url,
        Some(vec![("countryCode", cc)]),
        &token,
    );
    let items_fut = state.tidal_client.make_authed_request(
        &items_url,
        Some(vec![
            ("countryCode", cc),
            ("limit", "100"),
            ("offset", &offset_str),
        ]),
        &token,
    );

    let (playlist_result, items_result) = tokio::join!(playlist_fut, items_fut);

    let playlist_data = match playlist_result {
        Ok(d) => d,
        Err(e) => return Err(e),
    };

    let items_data = match items_result {
        Ok(d) => d,
        Err(e) => return Err(e),
    };

    let items = items_data
        .get("items")
        .cloned()
        .unwrap_or_else(|| items_data.clone());

    Ok(Json(json!({
        "version": state.config.api_version,
        "playlist": playlist_data,
        "items": items
    })))
}
