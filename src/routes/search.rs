use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct SearchParams {
    pub s: Option<String>,
    pub a: Option<String>,
    pub al: Option<String>,
    pub v: Option<String>,
    pub p: Option<String>,
    pub i: Option<String>,
    #[serde(default)]
    pub offset: i64,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    25
}

pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Value>, AppError> {
    if let Some(ref isrc) = params.i {
        let isrc = isrc.trim().to_string();
        if !isrc.is_empty() {
            let url = "https://api.tidal.com/v1/tracks".to_string();
            let limit_str = params.limit.to_string();
            let offset_str = params.offset.to_string();
            let result = state
                .tidal_client
                .make_request(
                    &url,
                    Some(vec![
                        ("isrc", &isrc),
                        ("limit", &limit_str),
                        ("offset", &offset_str),
                        ("countryCode", &state.config.country_code),
                    ]),
                )
                .await?;
            return Ok(Json(result));
        }
    }

    let limit_str = params.limit.to_string();
    let offset_str = params.offset.to_string();

    let query_defs: Vec<(&str, &str, Vec<(&str, &str)>)> = vec![
        (
            params.s.as_deref().unwrap_or(""),
            "https://api.tidal.com/v1/search/tracks",
            vec![
                ("query", params.s.as_deref().unwrap_or("")),
                ("limit", &limit_str),
                ("offset", &offset_str),
                ("countryCode", &state.config.country_code),
            ],
        ),
        (
            params.a.as_deref().unwrap_or(""),
            "https://api.tidal.com/v1/search/top-hits",
            vec![
                ("query", params.a.as_deref().unwrap_or("")),
                ("limit", &limit_str),
                ("offset", &offset_str),
                ("types", "ARTISTS,TRACKS"),
                ("countryCode", &state.config.country_code),
            ],
        ),
        (
            params.al.as_deref().unwrap_or(""),
            "https://api.tidal.com/v1/search/top-hits",
            vec![
                ("query", params.al.as_deref().unwrap_or("")),
                ("limit", &limit_str),
                ("offset", &offset_str),
                ("types", "ALBUMS"),
                ("countryCode", &state.config.country_code),
            ],
        ),
        (
            params.v.as_deref().unwrap_or(""),
            "https://api.tidal.com/v1/search/top-hits",
            vec![
                ("query", params.v.as_deref().unwrap_or("")),
                ("limit", &limit_str),
                ("offset", &offset_str),
                ("types", "VIDEOS"),
                ("countryCode", &state.config.country_code),
            ],
        ),
        (
            params.p.as_deref().unwrap_or(""),
            "https://api.tidal.com/v1/search/top-hits",
            vec![
                ("query", params.p.as_deref().unwrap_or("")),
                ("limit", &limit_str),
                ("offset", &offset_str),
                ("types", "PLAYLISTS"),
                ("countryCode", &state.config.country_code),
            ],
        ),
    ];

    for (value, url, query_params) in &query_defs {
        if !value.is_empty() {
            let pairs: Vec<(&str, &str)> = query_params
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect();
            let url_str = url.to_string();
            let result = state.tidal_client.make_request(&url_str, Some(pairs)).await?;
            return Ok(Json(result));
        }
    }

    Err(AppError::BadRequest(
        "Provide one of s, a, al, v, p, or i".into(),
    ))
}
