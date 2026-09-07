use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::State;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use moka::future::Cache;

use crate::AppState;

const CACHE_TTL_SECS: u64 = 600;
/// Safety cap for a single cached body. Our metadata endpoints return small
/// JSON; anything bigger passes through uncached (see below).
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Metadata prefixes safe to cache. Playback routes (/track, /trackManifests,
/// /dash, /widevine, /video, /topvideos) and /admin are NEVER cached — their
/// URLs/tokens expire and mutations must stay fresh.
fn cacheable(path: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "/info/",
        "/search/",
        "/album/",
        "/artist/",
        "/mix/",
        "/playlist/",
        "/cover/",
        "/lyrics/",
        "/recommendations/",
    ];
    PREFIXES.iter().any(|p| path.starts_with(p))
}

#[derive(Clone)]
pub(crate) struct CachedResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Bytes,
}

pub struct ResponseCache {
    cache: Cache<String, CachedResponse>,
    pub hits: AtomicU64,
    pub misses: AtomicU64,
}

impl ResponseCache {
    pub fn new() -> Self {
        Self {
            cache: Cache::builder()
                .time_to_live(Duration::from_secs(CACHE_TTL_SECS))
                .max_capacity(2000)
                .build(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    pub async fn invalidate_all(&self) {
        self.cache.invalidate_all();
    }

    pub(crate) async fn get(&self, key: &str) -> Option<CachedResponse> {
        self.cache.get(key).await
    }

    pub(crate) async fn insert(&self, key: String, value: CachedResponse) {
        self.cache.insert(key, value).await;
    }
}

impl Default for ResponseCache {
    fn default() -> Self {
        Self::new()
    }
}

fn build_response(cached: &CachedResponse, hit: bool) -> Response {
    let mut builder = Response::builder().status(cached.status);
    for (k, v) in &cached.headers {
        // Drop stale framing headers; the body is rebuilt.
        if k.eq_ignore_ascii_case("content-length") || k.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        builder = builder.header(k.as_str(), v.as_str());
    }
    builder
        .header("X-Cache", if hit { "HIT" } else { "MISS" })
        .body(Body::from(cached.body.clone()))
        .unwrap_or_else(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "cache rebuild failed",
            )
                .into_response()
        })
}

pub async fn cache_responses(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if req.method() != Method::GET || !cacheable(req.uri().path()) {
        return next.run(req).await;
    }
    let key = format!("{}?{}", req.uri().path(), req.uri().query().unwrap_or(""));

    if let Some(hit) = state.cache.get(&key).await {
        state.cache.hits.fetch_add(1, Ordering::Relaxed);
        return build_response(&hit, true);
    }
    state.cache.misses.fetch_add(1, Ordering::Relaxed);

    let mut resp = next.run(req).await;
    if resp.status() != StatusCode::OK {
        return resp;
    }
    // Oversized bodies pass through uncached (never buffer unboundedly).
    let too_big = resp
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .map_or(false, |n| n > MAX_BODY_BYTES);
    if too_big {
        resp.headers_mut()
            .insert("X-Cache", "SKIP".parse().unwrap());
        return resp;
    }

    let (parts, body) = resp.into_parts();
    let bytes = match to_bytes(body, MAX_BODY_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("Cache body collect failed (chunked overflow?): {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                "Upstream body too large to proxy",
            )
                .into_response();
        }
    };

    let mut headers = Vec::new();
    for (k, v) in parts.headers.iter() {
        if let Ok(vs) = v.to_str() {
            headers.push((k.to_string(), vs.to_string()));
        }
    }
    let cached = CachedResponse {
        status: parts.status.as_u16(),
        headers,
        body: bytes.clone(),
    };
    state.cache.insert(key, cached.clone()).await;

    // build_response sets X-Cache: MISS.
    build_response(&cached, false)
}
