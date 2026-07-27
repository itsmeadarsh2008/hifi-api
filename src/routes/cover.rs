use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct CoverParams {
    pub id: Option<i64>,
    pub q: Option<String>,
}

fn build_cover_entry(cover_slug: &str, name: Option<&str>, track_id: Option<Value>) -> Value {
    let slug = cover_slug.replace("-", "/");
    json!({
        "id": track_id,
        "name": name,
        "1280": format!("https://resources.tidal.com/images/{}/1280x1280.jpg", slug),
        "640": format!("https://resources.tidal.com/images/{}/640x640.jpg", slug),
        "80": format!("https://resources.tidal.com/images/{}/80x80.jpg", slug)
    })
}

pub async fn get_cover(
    State(state): State<AppState>,
    Query(params): Query<CoverParams>,
) -> Result<Json<Value>, AppError> {
    if params.id.is_none() && params.q.is_none() {
        return Err(AppError::BadRequest("Provide id or q query param".into()));
    }

    let account = state.account_manager.select_account().await?;
    let token = state
        .token_manager
        .get_token(&account, state.tidal_client.http_client())
        .await?;

    if let Some(id) = params.id {
        let url = format!("https://api.tidal.com/v1/tracks/{}/", id);
        let data = state
            .tidal_client
            .make_authed_request(
                &url,
                Some(vec![("countryCode", &state.config.country_code)]),
                &token,
            )
            .await?;

        let album = data.get("album").and_then(|v| v.as_object());
        let cover_slug = album
            .and_then(|a| a.get("cover"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::NotFound("Cover not found".into()))?;

        let album_title = album.and_then(|a| a.get("title")).and_then(|v| v.as_str());
        let track_title = data.get("title").and_then(|v| v.as_str());
        let album_id = album
            .and_then(|a| a.get("id"))
            .cloned()
            .unwrap_or(json!(id));

        let entry = build_cover_entry(
            cover_slug,
            album_title.or(track_title),
            Some(album_id),
        );

        return Ok(Json(json!({
            "version": state.config.api_version,
            "covers": [entry]
        })));
    }

    let q = params.q.as_deref().unwrap_or("");
    if q.is_empty() {
        return Err(AppError::BadRequest("Provide id or q query param".into()));
    }

    let data = state
        .tidal_client
        .make_authed_request(
            "https://api.tidal.com/v1/search/tracks",
            Some(vec![
                ("countryCode", &state.config.country_code),
                ("query", q),
                ("limit", "10"),
            ]),
            &token,
        )
        .await?;

    let items = data
        .get("items")
        .and_then(|v| v.as_array())
        .map(|v| v.iter().take(10).collect::<Vec<_>>())
        .ok_or_else(|| AppError::NotFound("Cover not found".into()))?;

    let covers: Vec<Value> = items
        .iter()
        .filter_map(|track| {
            let album = track.get("album").and_then(|v| v.as_object())?;
            let cover_slug = album.get("cover")?.as_str()?;
            let name = track.get("title").and_then(|v| v.as_str());
            let track_id = track.get("id").cloned();
            Some(build_cover_entry(cover_slug, name, track_id))
        })
        .collect();

    if covers.is_empty() {
        return Err(AppError::NotFound("Cover not found".into()));
    }

    Ok(Json(json!({
        "version": state.config.api_version,
        "covers": covers
    })))
}
