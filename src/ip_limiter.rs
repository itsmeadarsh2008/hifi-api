use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::Ordering;

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::error::AppError;
use crate::AppState;

/// Routes that cause an upstream Tidal call. Everything else is cheap
/// (cached metadata, health, index) and gets the generous bucket.
fn is_costly(path: &str) -> bool {
    const COSTLY: &[&str] = &[
        "/track",
        "/dash",
        "/widevine",
        "/video",
        "/topvideos",
    ];
    COSTLY.iter().any(|p| path.starts_with(p))
}

/// Single-char searches carry no signal (and health checks use `s=a`).
pub(crate) fn is_junk_search(path: &str, query: Option<&str>) -> bool {
    if !path.starts_with("/search") {
        return false;
    }
    let Some(q) = query else {
        return false;
    };
    for (k, v) in form_urlencoded::parse(q.as_bytes()) {
        if k == "s" {
            return v.trim().chars().count() < 2;
        }
    }
    false
}

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

    if state
        .rate_limits
        .ip_denylist
        .read()
        .map(|l| l.contains(&ip))
        .unwrap_or(false)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"detail": "Forbidden"})),
        )
            .into_response();
    }

    if state
        .rate_limits
        .ip_allowlist
        .read()
        .map(|l| l.contains(&ip))
        .unwrap_or(false)
    {
        return next.run(req).await;
    }

    // Overall per-IP bucket (unchanged legacy behavior).
    if let Err(wait) = state.anti_ban.check_ip(ip) {
        let secs = wait.as_secs().max(1);
        state.anti_ban.note_reject(ip);
        return AppError::TooManyRequests(
            format!(
                "Rate limit exceeded. This IP is making too many requests; retry in {}s.",
                secs
            ),
            secs,
        )
        .into_response();
    }

    // Costly tier: graduated response instead of a harsh instant 429.
    if is_costly(req.uri().path()) {
        if let Err(wait) = state.anti_ban.check_costly(ip) {
            let rep = state.anti_ban.reputation_factor(ip);
            let cap_ms = state.rate_limits.ip_delay_cap_ms.load(Ordering::Relaxed) as f32;
            // Reputation scales patience: trusted IPs absorb longer waits
            // (effectively a bigger burst), abusive ones get cut off fast.
            let effective_cap_ms = cap_ms * rep;
            let wait_ms = wait.as_millis() as f32;
            if rep < 0.5 || wait_ms > effective_cap_ms {
                let secs = wait.as_secs().max(1);
                state.anti_ban.note_reject(ip);
                return AppError::TooManyRequests(
                    format!(
                        "Rate limit exceeded for expensive requests; retry in {}s.",
                        secs
                    ),
                    secs,
                )
                .into_response();
            }
            // Gentle slowdown: wait out the bucket, then re-check once.
            tokio::time::sleep(wait).await;
            if let Err(wait2) = state.anti_ban.check_costly(ip) {
                let secs = wait2.as_secs().max(1);
                state.anti_ban.note_reject(ip);
                return AppError::TooManyRequests(
                    format!(
                        "Rate limit exceeded for expensive requests; retry in {}s.",
                        secs
                    ),
                    secs,
                )
                .into_response();
            }
        }
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::{is_costly, is_junk_search};

    #[test]
    fn costly_routes_classified() {
        for p in [
            "/track/?id=1",
            "/trackManifests/123",
            "/trackManifests/?id=1",
            "/dash/123",
            "/widevine",
            "/video/?id=1",
            "/topvideos/",
        ] {
            assert!(is_costly(p), "expected costly: {}", p);
        }
        for p in [
            "/search/?s=abba",
            "/album/?id=1",
            "/health",
            "/",
            "/admin/accounts",
            "/cover/?id=1",
            "/lyrics/?id=1",
        ] {
            assert!(!is_costly(p), "expected cheap: {}", p);
        }
    }

    #[test]
    fn junk_search_detection() {
        assert!(is_junk_search("/search/", Some("s=a&limit=1")));
        assert!(is_junk_search("/search/", Some("s=%20")));
        assert!(!is_junk_search("/search/", Some("s=abba&limit=25")));
        assert!(!is_junk_search("/search/", None));
        assert!(!is_junk_search("/track/", Some("id=1")));
    }
}

pub(crate) fn client_ip(state: &AppState, req: &Request<Body>, fallback: SocketAddr) -> IpAddr {
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
