use axum::extract::{Query, Request, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct TrackParams {
    pub id: i64,
    #[serde(default = "default_quality")]
    pub quality: String,
    #[serde(default)]
    pub immersiveaudio: bool,
}

fn default_quality() -> String {
    "HI_RES_LOSSLESS".to_string()
}

pub async fn get_track(
    State(state): State<AppState>,
    Query(params): Query<TrackParams>,
) -> Result<Json<Value>, AppError> {
    let url = format!("https://api.tidal.com/v1/tracks/{}/playbackinfo", params.id);
    let result = state
        .tidal_client
        .make_request(
            &url,
            Some(vec![
                ("audioquality", &params.quality),
                ("playbackmode", "STREAM"),
                ("assetpresentation", "FULL"),
                ("immersiveaudio", if params.immersiveaudio { "true" } else { "false" }),
            ]),
        )
        .await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct TrackManifestsParams {
    pub id: String,
    #[serde(default = "default_formats")]
    pub formats: Vec<String>,
    #[serde(default = "default_adaptive")]
    pub adaptive: String,
    #[serde(default = "default_manifest_type")]
    pub manifestType: String,
    #[serde(default = "default_uri_scheme")]
    pub uriScheme: String,
    #[serde(default = "default_usage")]
    pub usage: String,
}

fn default_formats() -> Vec<String> {
    vec![
        "HEAACV1".into(),
        "AACLC".into(),
        "FLAC".into(),
        "FLAC_HIRES".into(),
        "EAC3_JOC".into(),
    ]
}
fn default_adaptive() -> String {
    "true".into()
}
fn default_manifest_type() -> String {
    "MPEG_DASH".into()
}
fn default_uri_scheme() -> String {
    "HTTPS".into()
}
fn default_usage() -> String {
    "PLAYBACK".into()
}

pub async fn get_track_manifests(
    State(state): State<AppState>,
    Query(params): Query<TrackManifestsParams>,
    req: Request,
) -> Result<Json<Value>, AppError> {
    let url = format!("https://openapi.tidal.com/v2/trackManifests/{}", params.id);

    let query_params = vec![
        ("adaptive", params.adaptive.as_str()),
        ("manifestType", params.manifestType.as_str()),
        ("uriScheme", params.uriScheme.as_str()),
        ("usage", params.usage.as_str()),
    ];

    let mut all_params: Vec<(String, String)> = query_params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    for fmt in &params.formats {
        all_params.push(("formats".into(), fmt.clone()));
    }

    let result = state
        .tidal_client
        .make_request(&url, None)
        .await?;

    let mut result = result;
    if let Some(data) = result.get_mut("data") {
        if let Some(data_obj) = data.as_object_mut() {
            if let Some(data_inner) = data_obj.get_mut("data") {
                if let Some(attributes) = data_inner.get("attributes") {
                    if let Some(drm_data) = attributes.get("drmData") {
                        if let Some(_drm_obj) = drm_data.as_object() {
                            let proxy_url = format!(
                                "{}/widevine",
                                req.uri().authority().map(|a| {
                                    format!("{}://{}", 
                                        if req.uri().scheme_str() == Some("https") { "https" } else { "http" },
                                        a
                                    )
                                }).unwrap_or_default().trim_end_matches('/')
                            );
                            if let Some(drm) = data_inner.as_object_mut() {
                                if let Some(attrs) = drm.get_mut("attributes") {
                                    if let Some(attrs_obj) = attrs.as_object_mut() {
                                        if let Some(drm) = attrs_obj.get_mut("drmData") {
                                            if let Some(drm_obj) = drm.as_object_mut() {
                                                drm_obj.insert("licenseUrl".into(), json!(proxy_url.clone()));
                                                drm_obj.insert("certificateUrl".into(), json!(proxy_url));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Json(result))
}
