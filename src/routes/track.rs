use axum::extract::{Path, Query, Request, State};
use axum::response::Redirect;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

fn de_comma_list<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    Ok(s.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

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
    if result
        .pointer("/data/assetPresentation")
        .and_then(|v| v.as_str())
        == Some("PREVIEW")
    {
        let reason = result
            .pointer("/data/previewReason")
            .and_then(|v| v.as_str())
            .unwrap_or("FULL_REQUIRES_SUBSCRIPTION");
        return Err(AppError::ServiceUnavailable(format!(
            "Preview only ({}): track {} requires subscription or is not available as FULL in this region",
            reason, params.id
        )));
    }
    Ok(Json(result))
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct TrackManifestsParams {
    #[serde(default = "default_formats", deserialize_with = "de_comma_list")]
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
    Path(track_id): Path<String>,
    Query(params): Query<TrackManifestsParams>,
    req: Request,
) -> Result<Json<Value>, AppError> {
    let url = format!("https://openapi.tidal.com/v2/trackManifests/{}", track_id);

    let mut all_params: Vec<(&str, &str)> = vec![
        ("adaptive", params.adaptive.as_str()),
        ("manifestType", params.manifestType.as_str()),
        ("uriScheme", params.uriScheme.as_str()),
        ("usage", params.usage.as_str()),
    ];

    for fmt in &params.formats {
        all_params.push(("formats", fmt.as_str()));
    }

    let result = state
        .tidal_client
        .make_request(&url, Some(all_params))
        .await?;

    if result
        .pointer("/data/data/attributes/trackPresentation")
        .and_then(|v| v.as_str())
        == Some("PREVIEW")
    {
        let reason = result
            .pointer("/data/data/attributes/previewReason")
            .and_then(|v| v.as_str())
            .unwrap_or("PREVIEW");
        return Err(AppError::ServiceUnavailable(format!(
            "Preview only ({}): track {} not available as FULL",
            reason, track_id
        )));
    }

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

pub async fn get_dash_stream(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
) -> Result<Redirect, AppError> {
    let url = format!("https://openapi.tidal.com/v2/trackManifests/{}", track_id);

    let all_params: Vec<(&str, &str)> = vec![
        ("adaptive", "true"),
        ("manifestType", "MPEG_DASH"),
        ("uriScheme", "HTTPS"),
        ("usage", "PLAYBACK"),
        ("formats", "FLAC_HIRES,FLAC,EAC3_JOC,AACLC"),
    ];

    let result = state
        .tidal_client
        .make_request(&url, Some(all_params))
        .await?;

    if result
        .pointer("/data/data/attributes/trackPresentation")
        .and_then(|v| v.as_str())
        == Some("PREVIEW")
    {
        let reason = result
            .pointer("/data/data/attributes/previewReason")
            .and_then(|v| v.as_str())
            .unwrap_or("PREVIEW");
        return Err(AppError::ServiceUnavailable(format!(
            "Preview only ({}): track {} not available as FULL",
            reason, track_id
        )));
    }

    let uri = result
        .pointer("/data/data/attributes/uri")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Internal("No manifest URI in response".into()))?;

    Ok(Redirect::temporary(uri))
}
