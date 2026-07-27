use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct SimilarAlbumsParams {
    pub id: i64,
    pub cursor: Option<String>,
}

fn extract_uuid_from_tidal_url(href: &str) -> Option<String> {
    let parts: Vec<&str> = href.split('/').collect();
    if parts.len() >= 9 {
        Some(parts[4..9].join("-"))
    } else {
        None
    }
}

pub async fn get_similar_albums(
    State(state): State<AppState>,
    Query(params): Query<SimilarAlbumsParams>,
) -> Result<Json<Value>, AppError> {
    let url = format!(
        "https://openapi.tidal.com/v2/albums/{}/relationships/similarAlbums",
        params.id
    );

    let account = state.account_manager.select_account().await?;
    let token = state
        .token_manager
        .get_token(&account, state.tidal_client.http_client())
        .await?;

    let query_params = vec![
        ("page[cursor]", params.cursor.as_deref().unwrap_or("")),
        ("countryCode", &state.config.country_code),
        ("include", "similarAlbums,similarAlbums.coverArt,similarAlbums.artists"),
    ];

    let payload = state
        .tidal_client
        .make_authed_request(&url, Some(query_params), &token)
        .await?;

    let included = payload.get("included").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let mut albums_map = std::collections::HashMap::new();
    let mut artworks_map = std::collections::HashMap::new();
    let mut artists_map = std::collections::HashMap::new();

    for item in &included {
        if let Some(itype) = item.get("type").and_then(|v| v.as_str()) {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                match itype {
                    "albums" => { albums_map.insert(id.to_string(), item.clone()); }
                    "artworks" => { artworks_map.insert(id.to_string(), item.clone()); }
                    "artists" => { artists_map.insert(id.to_string(), item.clone()); }
                    _ => {}
                }
            }
        }
    }

    let data_items = payload.get("data").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mut albums = Vec::new();

    for entry in &data_items {
        if let Some(aid) = entry.get("id").and_then(|v| v.as_str()) {
            let inc = albums_map.get(aid);
            let attr = inc.and_then(|i| i.get("attributes")).cloned().unwrap_or_default();

            let cover_id = inc
                .and_then(|i| i.get("relationships"))
                .and_then(|r| r.get("coverArt"))
                .and_then(|ca| ca.get("data"))
                .and_then(|d| d.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| item.get("id"))
                .and_then(|id| id.as_str())
                .and_then(|art_id| artworks_map.get(art_id))
                .and_then(|art| art.get("attributes"))
                .and_then(|a| a.get("files"))
                .and_then(|f| f.as_array())
                .and_then(|files| files.first())
                .and_then(|f| f.get("href"))
                .and_then(|h| h.as_str())
                .and_then(extract_uuid_from_tidal_url);

            let mut artist_list = Vec::new();
            if let Some(artist_data) = inc
                .and_then(|i| i.get("relationships"))
                .and_then(|r| r.get("artists"))
                .and_then(|a| a.get("data"))
                .and_then(|d| d.as_array())
            {
                for a_entry in artist_data {
                    if let Some(aid_str) = a_entry.get("id").and_then(|v| v.as_str()) {
                        if let Some(a_obj) = artists_map.get(aid_str) {
                            artist_list.push(json!({
                                "id": a_obj.get("id").and_then(|v| v.as_str()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
                                "name": a_obj.get("attributes").and_then(|a| a.get("name"))
                            }));
                        }
                    }
                }
            }

            albums.push(json!({
                "id": aid.parse::<i64>().unwrap_or(0),
                "name": attr.get("name"),
                "cover": cover_id.unwrap_or_default(),
                "artists": artist_list,
                "url": format!("http://www.tidal.com/album/{}", aid)
            }));
        }
    }

    Ok(Json(json!({
        "version": state.config.api_version,
        "albums": albums
    })))
}
