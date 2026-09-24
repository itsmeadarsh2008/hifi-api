use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::error::AppError;
use crate::AppState;

pub async fn widevine_proxy(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    // Upstream parity: forward the caller's Content-Type, defaulting to
    // application/octet-stream like binimum/hifi-api.
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let (status, ct, out) = super::run_direct(
        &state,
        fetch_widevine_license(&state, &method.to_string(), Some(content_type.as_str()), &body),
    )
    .await?;
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    Ok((code, [("Content-Type", ct.as_str())], Bytes::from(out)).into_response())
}

/// Core /widevine/ fetch (runs directly, no queue).
/// Returns (status, content_type, body). Fails over across playback
/// accounts on retryable statuses so widevine load spreads and one
/// banned account doesn't fail the request.
pub(crate) async fn fetch_widevine_license(
    state: &AppState,
    method: &str,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<(u16, String, Vec<u8>), AppError> {
    let pool = state.account_manager.playback_count().await.max(1);
    let mut failed_ids: Vec<String> = Vec::new();
    let mut last_err: Option<AppError> = None;
    // Last retryable HTTP response, returned when every account fails over.
    let mut last_http: Option<(u16, String, Vec<u8>)> = None;

    for _ in 0..pool {
        let account = match state
            .account_manager
            .select_account_excluding(&failed_ids)
            .await
        {
            Ok(a) => a,
            Err(e) => {
                return match last_http {
                    Some(r) => Ok(r),
                    None => Err(last_err.unwrap_or(e)),
                };
            }
        };
        let hc = state.tidal_client.working_client().await?;
        let token = match state.token_manager.get_token(&account, &hc).await {
            Ok(t) => t,
            Err(e) => {
                state
                    .account_manager
                    .mark_account_error(&account.id, &format!("token failure: {:?}", e))
                    .await;
                failed_ids.push(account.id.clone());
                last_err = Some(e);
                continue;
            }
        };

        let url = "https://api.tidal.com/v2/widevine";

        let send = |token: &str| {
            hc.request(
                method.parse::<Method>().unwrap_or(Method::POST),
                url,
            )
            .header("authorization", format!("Bearer {}", token))
            .header("User-Agent", state.config.user_agent.as_str())
            .body(body.to_vec())
            .header(
                "Content-Type",
                content_type.unwrap_or("application/octet-stream"),
            )
            .send()
        };

        // Send once; on 401 refresh the token and retry once on the same
        // account before failing over. Transport errors are proxy-level
        // (same as make_request): surface immediately, no failover.
        let mut token_owned = token;
        let mut resp = match send(&token_owned).await {
            Ok(r) => r,
            Err(_) => {
                return Err(AppError::ServiceUnavailable(
                    "Error communicating with widevine server".into(),
                ));
            }
        };
        if resp.status().as_u16() == 401 {
            match state.token_manager.refresh_token(&account, &hc).await {
                Ok(fresh) => {
                    token_owned = fresh;
                    match send(&token_owned).await {
                        Ok(r) => resp = r,
                        Err(_) => {
                            return Err(AppError::ServiceUnavailable(
                                "Error communicating with widevine server".into(),
                            ));
                        }
                    }
                }
                Err(e) => {
                    state
                        .account_manager
                        .mark_account_error(
                            &account.id,
                            &format!("token refresh failure: {:?}", e),
                        )
                        .await;
                    failed_ids.push(account.id.clone());
                    last_err = Some(e);
                    continue;
                }
            }
        }

        let status = resp.status();
        if state.config.dev_mode {
            tracing::info!("[DEV] {} {} → {}", method, url, status.as_u16());
        }
        let resp_content_type = resp
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/json")
            .to_string();
        let content = resp.bytes().await.unwrap_or_default().to_vec();

        let code = status.as_u16();
        if code == 429 || code == 403 || code >= 500 {
            state
                .account_manager
                .mark_account_error(&account.id, &format!("Tidal HTTP {}", code))
                .await;
            failed_ids.push(account.id.clone());
            last_http = Some((code, resp_content_type, content));
            last_err = Some(AppError::UpstreamError(status, "Upstream API error".into()));
            continue;
        }

        return Ok((code, resp_content_type, content));
    }

    match last_http {
        Some(r) => Ok(r),
        None => Err(last_err.unwrap_or(AppError::ServiceUnavailable(
            "All accounts failed".into(),
        ))),
    }
}
