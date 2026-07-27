use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::error::AppError;
use crate::AppState;

pub async fn widevine_proxy(
    State(state): State<AppState>,
    method: Method,
    body: Bytes,
) -> Result<Response, AppError> {
    let account = state.account_manager.select_account().await?;
    let token = state
        .token_manager
        .get_token(&account, &state.tidal_client.http_client())
        .await?;

    let url = "https://api.tidal.com/v2/widevine";

    let mut req = state
        .tidal_client
        .http_client()
        .request(method.clone(), url)
        .header("authorization", format!("Bearer {}", token))
        .body(body.to_vec());

    if method == Method::POST {
        req = req.header("Content-Type", "application/octet-stream");
    }

    let resp = req.send().await.map_err(|_| {
        AppError::ServiceUnavailable("Error communicating with widevine server".into())
    })?;

    let status = resp.status();
    let content_type = resp
        .headers()
        .get("Content-Type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let content = resp.bytes().await.unwrap_or_default();

    Ok((
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
        [("Content-Type", content_type.as_str())],
        content,
    )
        .into_response())
}
