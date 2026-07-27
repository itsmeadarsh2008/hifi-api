use std::sync::Arc;

use axum::extract::{Query, State};
use axum::Json;
use futures::future::join_all;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Semaphore;

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct ArtistParams {
    pub id: Option<i64>,
    pub f: Option<i64>,
    #[serde(default)]
    pub skip_tracks: bool,
}

pub async fn get_artist(
    State(state): State<AppState>,
    Query(params): Query<ArtistParams>,
) -> Result<Json<Value>, AppError> {
    if params.id.is_none() && params.f.is_none() {
        return Err(AppError::BadRequest("Provide id or f query param".into()));
    }

    let account = state.account_manager.select_account().await?;
    let token = state
        .token_manager
        .get_token(&account, state.tidal_client.http_client())
        .await?;

    if let Some(id) = params.id {
        let artist_url = format!("https://api.tidal.com/v1/artists/{}", id);
        let data = state
            .tidal_client
            .make_authed_request(
                &artist_url,
                Some(vec![("countryCode", &state.config.country_code)]),
                &token,
            )
            .await?;

        let picture = data.get("picture").and_then(|v| v.as_str()).map(|s| s.to_string());
        let fallback = data
            .get("selectedAlbumCoverFallback")
            .and_then(|v| v.as_str()).map(|s| s.to_string());

        let mut artist_data = data;

        if picture.is_none() && fallback.is_some() {
            if let Some(obj) = artist_data.as_object_mut() {
                if let Some(ref fb) = fallback {
                    obj.insert("picture".into(), json!(fb));
                }
            }
        }

        let cover = picture
            .or(fallback)
            .map(|pic| {
                let slug = pic.replace("-", "/");
                json!({
                    "id": id,
                    "name": artist_data.get("name"),
                    "750": format!("https://resources.tidal.com/images/{}/750x750.jpg", slug)
                })
            });

        return Ok(Json(json!({
            "version": state.config.api_version,
            "artist": artist_data,
            "cover": cover
        })));
    }

    let f_id = params.f.unwrap();
    let albums_url = format!("https://api.tidal.com/v1/artists/{}/albums", f_id);
    let cc = &state.config.country_code;

    let fetch_albums = state.tidal_client.make_authed_request(
        &albums_url,
        Some(vec![("countryCode", cc), ("limit", "100")]),
        &token,
    );
    let fetch_singles = state.tidal_client.make_authed_request(
        &albums_url,
        Some(vec![("countryCode", cc), ("limit", "100"), ("filter", "EPSANDSINGLES")]),
        &token,
    );

    let (albums_res, singles_res) = tokio::join!(fetch_albums, fetch_singles);

    let mut unique_releases: Vec<Value> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for res in [&albums_res, &singles_res] {
        if let Ok(data) = res {
            if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    if let Some(id_val) = item.get("id") {
                        if !seen_ids.contains(&id_val.to_string()) {
                            seen_ids.insert(id_val.to_string());
                            unique_releases.push(item.clone());
                        }
                    }
                }
            }
        }
    }

    let album_ids: Vec<String> = unique_releases
        .iter()
        .filter_map(|item| item.get("id").and_then(|v| v.as_i64()).map(|v| v.to_string()))
        .collect();

    let page_data = json!({"items": unique_releases});

    if params.skip_tracks {
        let top_url = format!("https://api.tidal.com/v1/artists/{}/toptracks", f_id);
        let top_tracks = state
            .tidal_client
            .make_authed_request(
                &top_url,
                Some(vec![("countryCode", cc), ("limit", "15")]),
                &token,
            )
            .await
            .ok()
            .and_then(|v| v.get("items").cloned())
            .unwrap_or(json!([]));

        return Ok(Json(json!({
            "version": state.config.api_version,
            "albums": page_data,
            "tracks": top_tracks
        })));
    }

    if album_ids.is_empty() {
        return Ok(Json(json!({
            "version": state.config.api_version,
            "albums": page_data,
            "tracks": []
        })));
    }

    let sem = Arc::new(Semaphore::new(20));
    let mut track_tasks = Vec::new();

    for aid in &album_ids {
        let sem = sem.clone();
        let client = state.tidal_client.http_client().clone();
        let token = token.clone();
        let cc = state.config.country_code.clone();
        let aid = aid.clone();

        track_tasks.push(async move {
            let _permit = sem.acquire().await.unwrap();
            let url = format!("https://api.tidal.com/v1/pages/album");
            let req = client
                .get(&url)
                .header("authorization", format!("Bearer {}", token))
                .query(&[
                    ("albumId", aid.as_str()),
                    ("countryCode", cc.as_str()),
                    ("deviceType", "BROWSER"),
                ]);
            let resp = req.send().await.ok()?;
            let data: Value = resp.json().await.ok()?;

            let tracks: Vec<Value> = data
                .get("rows")
                .and_then(|v| v.as_array())
                .and_then(|rows| {
                    if rows.len() < 2 {
                        return None;
                    }
                    rows[1]
                        .get("modules")
                        .and_then(|v| v.as_array())
                        .and_then(|modules| {
                            modules.first()
                                .and_then(|m| m.get("pagedList"))
                                .and_then(|pl| pl.get("items"))
                                .and_then(|v| v.as_array())
                                .map(|items| {
                                    items
                                        .iter()
                                        .map(|item| {
                                            item.get("item").cloned().unwrap_or(item.clone())
                                        })
                                        .collect()
                                })
                        })
                })
                .unwrap_or_default();

            Some(tracks)
        });
    }

    let track_results = join_all(track_tasks).await;
    let all_tracks: Vec<Value> = track_results
        .into_iter()
        .flatten()
        .flatten()
        .collect();

    Ok(Json(json!({
        "version": state.config.api_version,
        "albums": page_data,
        "tracks": all_tracks
    })))
}
