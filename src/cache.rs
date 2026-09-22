use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use std::sync::Arc;

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::State;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use moka::future::Cache;
use tokio::sync::Mutex;

use crate::AppState;

const CACHE_TTL_SECS: u64 = 3600;
/// Extra window after TTL expiry during which stale entries still serve
/// instantly (X-Cache: STALE) while a background refresh runs. Expiry
/// misses become hits; only cold keys miss.
const STALE_WINDOW_SECS: u64 = 3600;
/// Hard memory bound for cached bodies (moka weight = body bytes).
const MAX_CACHE_BYTES: u64 = 256 * 1024 * 1024;
/// Hard memory bound for negatively cached error bodies.
const MAX_NEG_BYTES: u64 = 32 * 1024 * 1024;
/// Hard TTL ceiling for negative entries (manual per-status expiry inside
/// is authoritative; this just sweeps).
const NEG_CACHE_TTL_SECS: u64 = 300;
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

/// Canonical cache key for a raw query string: percent-decode every pair
/// then sort, so `?s=a+b`, `?s=a%20b` and `?b=2&a=1`-style variants of the
/// same logical request share one entry instead of fragmenting the cache.
/// Decoding is safe here because the handlers decode identically before
/// responding. Pure function — unit tested.
fn normalize_query(query: Option<&str>) -> String {
    let q = query.unwrap_or("");
    if q.is_empty() {
        return String::new();
    }
    let mut pairs: Vec<(String, String)> = form_urlencoded::parse(q.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if pairs.len() < 2 {
        // Fast path still canonicalizes encoding (`a+b` == `a%20b`).
        let mut ser = form_urlencoded::Serializer::new(String::new());
        for (k, v) in &pairs {
            ser.append_pair(k, v);
        }
        return ser.finish();
    }
    pairs.sort();
    let mut ser = form_urlencoded::Serializer::new(String::new());
    for (k, v) in &pairs {
        ser.append_pair(k, v);
    }
    ser.finish()
}

/// Freshness of a cached entry of a given age (seconds). Entries younger
/// than the soft TTL serve instantly; entries inside the stale window
/// still serve instantly while a background refresh runs; anything older
/// is refetched inline. Pure function — unit tested.
fn freshness(age_secs: i64) -> Freshness {
    if age_secs < CACHE_TTL_SECS as i64 {
        Freshness::Fresh
    } else if age_secs < (CACHE_TTL_SECS + STALE_WINDOW_SECS) as i64 {
        Freshness::Stale
    } else {
        Freshness::Expired
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Freshness {
    Fresh,
    Stale,
    Expired,
}

/// How long to negatively cache an error status (seconds). `None` means
/// "never cache": only repeatable upstream verdicts (not-found, throttled,
/// server errors) get short TTLs so error stampedes collapse without
/// masking recovery. Pure function — unit tested.
fn negative_ttl(status: u16) -> Option<u64> {
    match status {
        404 => Some(60),
        429 | 500 | 502 | 503 | 504 => Some(15),
        _ => None,
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub(crate) struct CachedResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Bytes,
    fetched_at: i64,
}

/// A briefly cached error response with its own expiry.
#[derive(Clone)]
struct NegativeEntry {
    status: u16,
    headers: Vec<(String, String)>,
    body: Bytes,
    expires_at: i64,
}

pub struct ResponseCache {
    cache: Cache<String, CachedResponse>,
    /// Short-TTL error responses (404/429/5xx) so error stampedes
    /// collapse instead of hammering Tidal on every request.
    negative: Cache<String, NegativeEntry>,
    /// Per-key in-flight guards: concurrent identical requests collapse onto
    /// the leader instead of stampeding Tidal. Auto-evicts via TTL.
    inflight: Cache<String, Arc<Mutex<()>>>,
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub coalesced: AtomicU64,
    /// Stale-window serves (instant, refreshed in background).
    pub stale: AtomicU64,
    /// Serves from the negative cache (no upstream call).
    pub negative_hits: AtomicU64,
}

impl ResponseCache {
    pub fn new() -> Self {
        Self {
            cache: Cache::builder()
                .time_to_live(Duration::from_secs(CACHE_TTL_SECS + STALE_WINDOW_SECS))
                .weigher(|_k, v: &CachedResponse| v.body.len() as u32)
                .max_capacity(MAX_CACHE_BYTES)
                .build(),
            negative: Cache::builder()
                .time_to_live(Duration::from_secs(NEG_CACHE_TTL_SECS))
                .weigher(|_k, v: &NegativeEntry| v.body.len() as u32)
                .max_capacity(MAX_NEG_BYTES)
                .build(),
            inflight: Cache::builder()
                .time_to_live(Duration::from_secs(60))
                .max_capacity(10000)
                .build(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            coalesced: AtomicU64::new(0),
            stale: AtomicU64::new(0),
            negative_hits: AtomicU64::new(0),
        }
    }

    pub async fn invalidate_all(&self) {
        self.cache.invalidate_all();
        self.negative.invalidate_all();
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

fn build_response(status: u16, headers: &[(String, String)], body: Bytes, label: &str) -> Response {
    let mut builder = Response::builder().status(status);
    for (k, v) in headers {
        // Drop stale framing headers; the body is rebuilt.
        if k.eq_ignore_ascii_case("content-length") || k.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        builder = builder.header(k.as_str(), v.as_str());
    }
    builder
        .header("X-Cache", label)
        .body(Body::from(body))
        .unwrap_or_else(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "cache rebuild failed",
            )
                .into_response()
        })
}

/// True when the response declares more than the cacheable body cap.
/// Such bodies pass through uncached (never buffer unboundedly).
fn body_too_big(resp: &Response) -> bool {
    resp.headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .is_some_and(|n| n > MAX_BODY_BYTES)
}

/// Buffer a response body for caching. Returns `None` when the body is
/// uncollectable (caller surfaces 502, as before).
async fn collect_parts(resp: Response) -> Option<(u16, Vec<(String, String)>, Bytes)> {
    if resp.status() != StatusCode::OK {
        return None;
    }
    let (parts, body) = resp.into_parts();
    let bytes = to_bytes(body, MAX_BODY_BYTES).await.ok()?;
    let mut headers = Vec::new();
    for (k, v) in parts.headers.iter() {
        if let Ok(vs) = v.to_str() {
            headers.push((k.to_string(), vs.to_string()));
        }
    }
    Some((parts.status.as_u16(), headers, bytes))
}

pub async fn cache_responses(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if req.method() != Method::GET || !cacheable(req.uri().path()) {
        return next.run(req).await;
    }
    let key = format!("{}?{}", req.uri().path(), normalize_query(req.uri().query()));

    // Fresh entries serve instantly.
    if let Some(hit) = state.cache.get(&key).await {
        if freshness(now_secs() - hit.fetched_at) == Freshness::Fresh {
            state.cache.hits.fetch_add(1, Ordering::Relaxed);
            return build_response(hit.status, &hit.headers, hit.body.clone(), "HIT");
        }
        // Stale window: serve instantly and refresh in the background.
        // `next` + `req` move into the task untouched. A failed refresh
        // keeps the stale entry until hard expiry, so upstream blips never
        // surface as user-facing misses.
        if freshness(now_secs() - hit.fetched_at) == Freshness::Stale {
            state.cache.hits.fetch_add(1, Ordering::Relaxed);
            state.cache.stale.fetch_add(1, Ordering::Relaxed);
            let stale_resp =
                build_response(hit.status, &hit.headers, hit.body.clone(), "STALE");
            let bg_state = state.clone();
            let bg_key = key.clone();
            tokio::spawn(async move {
                let guard = bg_state
                    .cache
                    .inflight
                    .get_with(bg_key.clone(), async { Arc::new(Mutex::new(())) })
                    .await;
                // Another refresh already running: skip, don't pile up.
                let Ok(_lock) = guard.try_lock() else { return };
                let resp = next.run(req).await;
                if let Some((status, headers, bytes)) = collect_parts(resp).await {
                    bg_state.cache.insert(
                        bg_key,
                        CachedResponse {
                            status,
                            headers,
                            body: bytes,
                            fetched_at: now_secs(),
                        },
                    ).await;
                }
            });
            return stale_resp;
        }
    }

    // Negative cache: repeat errors serve instantly without upstream calls.
    if let Some(neg) = state.cache.negative.get(&key).await {
        if now_secs() < neg.expires_at {
            state.cache.negative_hits.fetch_add(1, Ordering::Relaxed);
            return build_response(neg.status, &neg.headers, neg.body.clone(), "NEGATIVE");
        }
    }

    // Singleflight: serialize identical concurrent misses on a per-key lock,
    // then re-check — followers get the leader's cached response.
    let guard = state
        .cache
        .inflight
        .get_with(key.clone(), async { Arc::new(Mutex::new(())) })
        .await;
    let _lock = guard.lock().await;
    if let Some(hit) = state.cache.get(&key).await {
        if freshness(now_secs() - hit.fetched_at) != Freshness::Expired {
            state.cache.hits.fetch_add(1, Ordering::Relaxed);
            state.cache.coalesced.fetch_add(1, Ordering::Relaxed);
            let label = if freshness(now_secs() - hit.fetched_at) == Freshness::Fresh {
                "HIT"
            } else {
                "STALE"
            };
            return build_response(hit.status, &hit.headers, hit.body.clone(), label);
        }
    }
    if let Some(neg) = state.cache.negative.get(&key).await {
        if now_secs() < neg.expires_at {
            state.cache.negative_hits.fetch_add(1, Ordering::Relaxed);
            return build_response(neg.status, &neg.headers, neg.body.clone(), "NEGATIVE");
        }
    }
    state.cache.misses.fetch_add(1, Ordering::Relaxed);

    let mut resp = next.run(req).await;
    let status = resp.status();
    if status == StatusCode::OK {
        // Oversized bodies pass through uncached (never buffer unboundedly).
        if body_too_big(&resp) {
            resp.headers_mut()
                .insert("X-Cache", "SKIP".parse().unwrap());
            return resp;
        }
        if let Some((status, headers, bytes)) = collect_parts(resp).await {
            let cached = CachedResponse {
                status,
                headers,
                body: bytes.clone(),
                fetched_at: now_secs(),
            };
            // build_response sets X-Cache: MISS.
            state.cache.insert(key, cached.clone()).await;
            return build_response(cached.status, &cached.headers, bytes, "MISS");
        }
        // Chunked overflow (no content-length, exceeded cap while reading).
        return (
            StatusCode::BAD_GATEWAY,
            "Upstream body too large to proxy",
        )
            .into_response();
    }

    // Cacheable error verdicts (404/429/5xx) collapse repeat-error
    // stampedes; everything else passes through uncached.
    if negative_ttl(status.as_u16()).is_some() && !body_too_big(&resp) {
        let ttl = negative_ttl(status.as_u16()).unwrap_or(15);
        let (parts, body) = resp.into_parts();
        match to_bytes(body, MAX_BODY_BYTES).await {
            Ok(bytes) if bytes.len() <= MAX_BODY_BYTES => {
                let mut headers = Vec::new();
                for (k, v) in parts.headers.iter() {
                    if let Ok(vs) = v.to_str() {
                        headers.push((k.to_string(), vs.to_string()));
                    }
                }
                let entry = NegativeEntry {
                    status: status.as_u16(),
                    headers: headers.clone(),
                    body: bytes.clone(),
                    expires_at: now_secs() + ttl as i64,
                };
                state.cache.negative.insert(key, entry).await;
                return build_response(status.as_u16(), &headers, bytes, "MISS");
            }
            // Uncollectable error body (chunked overflow past the cap):
            // 502 on an already-error response, same as the 200 path.
            _ => {
                return (
                    StatusCode::BAD_GATEWAY,
                    "Upstream error body too large to proxy",
                )
                    .into_response();
            }
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::{freshness, negative_ttl, normalize_query, Freshness};

    #[test]
    fn query_canonicalizes_encoding_and_order() {
        // Order variants share one key.
        assert_eq!(normalize_query(Some("b=2&a=1")), "a=1&b=2");
        assert_eq!(normalize_query(Some("a=1&b=2")), "a=1&b=2");
        // Encoding variants decode to the same key (`+` == `%20`).
        assert_eq!(
            normalize_query(Some("s=daft+punk")),
            normalize_query(Some("s=daft%20punk"))
        );
        // Repeated params keep their multiplicity, sorted.
        assert_eq!(
            normalize_query(Some("formats=FLAC&formats=AACLC&adaptive=true")),
            "adaptive=true&formats=AACLC&formats=FLAC"
        );
        // Single pair and empty queries still work.
        assert_eq!(normalize_query(Some("id=42")), "id=42");
        assert_eq!(normalize_query(None), "");
        assert_eq!(normalize_query(Some("")), "");
    }

    #[test]
    fn freshness_windows() {
        assert_eq!(freshness(0), Freshness::Fresh);
        assert_eq!(freshness(3599), Freshness::Fresh);
        assert_eq!(freshness(3600), Freshness::Stale);
        assert_eq!(freshness(7199), Freshness::Stale);
        assert_eq!(freshness(7200), Freshness::Expired);
        assert_eq!(freshness(999_999), Freshness::Expired);
    }

    #[test]
    fn negative_ttl_only_for_repeatable_verdicts() {
        assert_eq!(negative_ttl(404), Some(60));
        assert_eq!(negative_ttl(429), Some(15));
        assert_eq!(negative_ttl(500), Some(15));
        assert_eq!(negative_ttl(503), Some(15));
        assert_eq!(negative_ttl(200), None);
        assert_eq!(negative_ttl(400), None);
        assert_eq!(negative_ttl(401), None);
        assert_eq!(negative_ttl(403), None);
    }
}
