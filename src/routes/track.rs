use axum::extract::{Path, Query, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::Redirect;
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
    #[serde(default = "default_adaptive")]
    pub adaptive: String,
    #[serde(default = "default_manifest_type")]
    pub manifestType: String,
    #[serde(default = "default_uri_scheme")]
    pub uriScheme: String,
    #[serde(default = "default_usage")]
    pub usage: String,
    #[serde(default)]
    pub countryCode: Option<String>,
    /// Atmos preference: true|prefer|only|off. Overrides the server default.
    /// Note: EAC3_JOC needs a Dolby-capable player + Widevine license.
    #[serde(default)]
    pub atmos: Option<String>,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct TrackManifestsQueryParams {
    pub id: String,
    #[serde(default = "default_adaptive")]
    pub adaptive: String,
    #[serde(default = "default_manifest_type")]
    pub manifestType: String,
    #[serde(default = "default_uri_scheme")]
    pub uriScheme: String,
    #[serde(default = "default_usage")]
    pub usage: String,
    #[serde(default)]
    pub countryCode: Option<String>,
    #[serde(default)]
    pub atmos: Option<String>,
}

#[derive(Deserialize)]
pub struct DashParams {
    #[serde(default)]
    pub atmos: Option<String>,
}

/// Resolve the effective format list: explicit `formats=` (comma or
/// repeated) wins, then Atmos preference moves EAC3_JOC first (`prefer`),
/// keeps only it (`only`), or strips it (`off`).
fn resolve_formats(raw_query: Option<&str>, atmos: Option<&str>, default_prefer: bool) -> Vec<String> {
    let mut explicit: Option<Vec<String>> = None;
    if let Some(q) = raw_query {
        let mut out = Vec::new();
        for (k, v) in form_urlencoded::parse(q.as_bytes()) {
            if k == "formats" {
                for part in v.split(',') {
                    let p = part.trim();
                    if !p.is_empty() {
                        out.push(p.to_string());
                    }
                }
            }
        }
        if !out.is_empty() {
            explicit = Some(out);
        }
    }

    let mode = atmos
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    match mode.as_deref() {
        // Explicit Atmos-only request.
        Some("only") => vec!["EAC3_JOC".to_string()],
        Some("true") | Some("1") | Some("prefer") => {
            // Ensure Atmos leads (add it if the explicit list lacks it).
            let mut formats = explicit.unwrap_or_else(default_formats);
            formats.retain(|f| f != "EAC3_JOC");
            formats.insert(0, "EAC3_JOC".to_string());
            formats
        }
        // Explicit lists are always respected verbatim.
        _ if explicit.is_some() => explicit.unwrap_or_else(default_formats),
        Some("false") | Some("0") | Some("off") => {
            let mut formats = default_formats();
            formats.retain(|f| f != "EAC3_JOC");
            formats
        }
        // No preference expressed: server default.
        _ if default_prefer => {
            let mut formats = default_formats();
            formats.retain(|f| f != "EAC3_JOC");
            formats.insert(0, "EAC3_JOC".to_string());
            formats
        }
        _ => default_formats(),
    }
}

fn atmos_default_on(state: &AppState) -> bool {
    state
        .rate_limits
        .atmos_mode
        .read()
        .map(|m| m.as_str() == "prefer")
        .unwrap_or(false)
}

/// True when the Tidal manifest actually carries a Dolby Atmos rendition.
fn manifest_has_atmos(result: &Value) -> bool {
    let attrs = result.pointer("/data/data/attributes");
    if let Some(formats) = attrs.and_then(|a| a.get("formats")).and_then(|f| f.as_array()) {
        if formats.iter().any(|f| f.as_str() == Some("EAC3_JOC")) {
            return true;
        }
    }
    if let Some(modes) = attrs.and_then(|a| a.get("audioModes")).and_then(|m| m.as_array()) {
        if modes.iter().any(|m| m.as_str() == Some("DOLBY_ATMOS")) {
            return true;
        }
    }
    false
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

async fn fetch_manifest_inner(
    state: &AppState,
    track_id: &str,
    params: &TrackManifestsParams,
    host: &str,
    raw_query: Option<&str>,
) -> Result<Value, AppError> {
    let formats = resolve_formats(raw_query, params.atmos.as_deref(), atmos_default_on(state));
    let url = format!("https://openapi.tidal.com/v2/trackManifests/{}", track_id);

    let country_code = params
        .countryCode
        .as_deref()
        .unwrap_or(&state.config.country_code);
    let mut all_params: Vec<(&str, &str)> = vec![
        ("adaptive", params.adaptive.as_str()),
        ("manifestType", params.manifestType.as_str()),
        ("uriScheme", params.uriScheme.as_str()),
        ("usage", params.usage.as_str()),
        ("countryCode", country_code),
    ];

    for fmt in &formats {
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
    let atmos_available = manifest_has_atmos(&result);
    if let Some(obj) = result.as_object_mut() {
        obj.insert("atmos_available".into(), json!(atmos_available));
    }
    if let Some(data) = result.get_mut("data") {
        if let Some(data_obj) = data.as_object_mut() {
            if let Some(data_inner) = data_obj.get_mut("data") {
                if let Some(attributes) = data_inner.get("attributes") {
                    if let Some(drm_data) = attributes.get("drmData") {
                        if let Some(_drm_obj) = drm_data.as_object() {
                            let proxy_url = format!("https://{}/widevine", host);
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

    Ok(result)
}

// Path-based: GET /trackManifests/{id}?formats=...  (our Rust style, keep for compat)
pub async fn get_track_manifests(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
    Query(params): Query<TrackManifestsParams>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Json<Value>, AppError> {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let result = fetch_manifest_inner(&state, &track_id, &params, host, query.as_deref()).await?;
    Ok(Json(result))
}

// Query-based: GET /trackManifests/?id=...&formats=...  (binimum hifi-api style)
pub async fn get_track_manifests_query(
    State(state): State<AppState>,
    Query(params): Query<TrackManifestsQueryParams>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Json<Value>, AppError> {
    let inner = TrackManifestsParams {
        adaptive: params.adaptive.clone(),
        manifestType: params.manifestType.clone(),
        uriScheme: params.uriScheme.clone(),
        usage: params.usage.clone(),
        countryCode: params.countryCode.clone(),
        atmos: params.atmos.clone(),
    };
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let result = fetch_manifest_inner(&state, &params.id, &inner, host, query.as_deref()).await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::resolve_formats;

    #[test]
    fn atmos_only_forces_single_format() {
        assert_eq!(
            resolve_formats(Some("formats=FLAC"), Some("only"), false),
            vec!["EAC3_JOC".to_string()]
        );
    }

    #[test]
    fn atmos_prefer_leads_with_eac3() {
        let out = resolve_formats(Some("formats=FLAC,AACLC"), Some("true"), false);
        assert_eq!(out[0], "EAC3_JOC");
        assert!(out.contains(&"FLAC".to_string()));
        assert!(out.contains(&"AACLC".to_string()));
        // No duplicates.
        let mut sorted = out.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), out.len());
    }

    #[test]
    fn explicit_formats_respected_verbatim_by_default() {
        let out = resolve_formats(Some("formats=FLAC&formats=AACLC"), None, false);
        assert_eq!(out, vec!["FLAC".to_string(), "AACLC".to_string()]);
    }

    #[test]
    fn atmos_off_strips_from_defaults_only() {
        let out = resolve_formats(None, Some("off"), false);
        assert!(!out.contains(&"EAC3_JOC".to_string()));
        assert!(out.contains(&"FLAC".to_string()));
        // ...but an explicit request is still honored.
        let out2 = resolve_formats(Some("formats=EAC3_JOC"), Some("off"), false);
        assert_eq!(out2, vec!["EAC3_JOC".to_string()]);
    }

    #[test]
    fn server_default_prefer_mode() {
        let out = resolve_formats(None, None, true);
        assert_eq!(out[0], "EAC3_JOC");
        let out2 = resolve_formats(None, None, false);
        assert!(out2.contains(&"EAC3_JOC".to_string()));
        assert_ne!(out2[0], "EAC3_JOC");
    }

    #[test]
    fn garbage_atmos_falls_back_to_defaults() {
        let out = resolve_formats(None, Some("banana"), false);
        assert!(out.contains(&"FLAC_HIRES".to_string()));
    }
}

pub async fn get_dash_stream(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
    Query(params): Query<DashParams>,
) -> Result<Redirect, AppError> {
    let url = format!("https://openapi.tidal.com/v2/trackManifests/{}", track_id);

    // Same Atmos semantics as /trackManifests, over the fixed /dash chain.
    let mode = params
        .atmos
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let formats = match mode.as_deref() {
        Some("only") => "EAC3_JOC",
        Some("true") | Some("1") | Some("prefer") => "EAC3_JOC,FLAC_HIRES,FLAC,AACLC",
        Some("false") | Some("0") | Some("off") => "FLAC_HIRES,FLAC,AACLC",
        _ if atmos_default_on(&state) => "EAC3_JOC,FLAC_HIRES,FLAC,AACLC",
        _ => "FLAC_HIRES,FLAC,EAC3_JOC,AACLC",
    };

    let all_params: Vec<(&str, &str)> = vec![
        ("adaptive", "true"),
        ("manifestType", "MPEG_DASH"),
        ("uriScheme", "HTTPS"),
        ("usage", "PLAYBACK"),
        ("countryCode", &state.config.country_code),
        ("formats", formats),
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