use std::future::Future;
use std::pin::Pin;

use axum::extract::{Query, State};
use axum::Json;
use futures::future::join_all;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct AlbumParams {
    pub id: i64,
    #[serde(default = "default_album_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_album_limit() -> i64 {
    100
}

pub async fn get_album(
    State(state): State<AppState>,
    Query(params): Query<AlbumParams>,
) -> Result<Json<Value>, AppError> {
    let account = state.account_manager.select_account().await?;
    let token = state
        .token_manager
        .get_token(&account, state.tidal_client.http_client())
        .await?;

    let album_url = format!("https://api.tidal.com/v1/albums/{}", params.id);
    let items_url = format!("https://api.tidal.com/v1/albums/{}/items", params.id);

    let client = state.tidal_client.http_client().clone();
    let cc = state.config.country_code.clone();
    let token_arc = token;

    let mut tasks: Vec<
        Pin<Box<dyn Future<Output = Result<Value, AppError>> + Send>>,
    > = Vec::new();

    {
        let client = client.clone();
        let token = token_arc.clone();
        let url = album_url;
        let cc = cc.clone();
        tasks.push(Box::pin(async move {
            let req = client
                .get(&url)
                .header("authorization", format!("Bearer {}", token))
                .query(&[("countryCode", &cc)]);
            let resp = req.send().await.map_err(AppError::from)?;
            let data: Value = resp.json().await.map_err(AppError::from)?;
            Ok(data)
        }));
    }

    let max_chunk = 100i64;
    let mut current_offset = params.offset;
    let mut remaining = params.limit;

    while remaining > 0 {
        let chunk_size = remaining.min(max_chunk);
        let offset_str = current_offset.to_string();
        let limit_str = chunk_size.to_string();
        let url = items_url.clone();
        let client = client.clone();
        let token = token_arc.clone();
        let cc = cc.clone();
        tasks.push(Box::pin(async move {
            let req = client
                .get(&url)
                .header("authorization", format!("Bearer {}", token))
                .query(&[
                    ("countryCode", &cc),
                    ("limit", &limit_str),
                    ("offset", &offset_str),
                ]);
            let resp = req.send().await.map_err(AppError::from)?;
            let data: Value = resp.json().await.map_err(AppError::from)?;
            Ok(data)
        }));
        current_offset += chunk_size;
        remaining -= chunk_size;
    }

    let results = join_all(tasks).await;
    let mut album_data = match &results[0] {
        Ok(d) => d.clone(),
        Err(e) => return Err(e.clone()),
    };

    let mut all_items: Vec<Value> = Vec::new();
    for result in &results[1..] {
        if let Ok(data) = result {
            if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
                all_items.extend(items.clone());
            } else if let Some(arr) = data.as_array() {
                all_items.extend(arr.clone());
            }
        }
    }

    if let Some(obj) = album_data.as_object_mut() {
        obj.insert("items".into(), json!(all_items));
    }

    Ok(Json(json!({
        "version": state.config.api_version,
        "data": album_data
    })))
}
