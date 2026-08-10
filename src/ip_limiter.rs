use std::net::{IpAddr, SocketAddr};

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::AppState;

pub async fn enforce_ip_rate_limit(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if req.uri().path().starts_with("/admin") {
        return next.run(req).await;
    }

    let ip = client_ip(&state, &req, addr);
    match state.anti_ban.check_ip(ip) {
        Ok(()) => next.run(req).await,
        Err(retry_after) => {
            let secs = retry_after.as_secs().max(1);
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, secs.to_string())],
                Json(json!({
                    "detail": format!(
                        "Rate limit exceeded. This IP is making too many requests; retry in {}s.",
                        secs
                    )
                })),
            )
                .into_response()
        }
    }
}

fn client_ip(state: &AppState, req: &Request<Body>, fallback: SocketAddr) -> IpAddr {
    if state.config.trust_proxy {
        if let Some(xff) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
        {
            if let Some(first) = xff.split(',').next().map(|s| s.trim()) {
                if let Ok(ip) = first.parse::<IpAddr>() {
                    return ip.to_canonical();
                }
            }
        }
        if let Some(real) = req
            .headers()
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
        {
            if let Ok(ip) = real.trim().parse::<IpAddr>() {
                return ip.to_canonical();
            }
        }
    }
    fallback.ip().to_canonical()
}