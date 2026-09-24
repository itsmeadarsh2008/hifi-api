use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};

use crate::AppState;

const MAX_ENTRIES: usize = 5000;

#[derive(Clone)]
pub struct LogEntry {
    pub ts: i64,
    pub method: String,
    pub path: String,
    /// Track/resource identifier when the request names one
    /// (path id for /trackManifests/{id} and /dash/{id},
    /// `id=` or `s=` query value otherwise). Empty when none.
    pub detail: String,
    pub status: u16,
    pub latency_ms: u64,
    pub client_ip: String,
}

pub struct RequestLog {
    entries: Mutex<VecDeque<LogEntry>>,
}

impl RequestLog {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(MAX_ENTRIES)),
        }
    }

    pub fn record(&self, entry: LogEntry) {
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= MAX_ENTRIES {
                entries.pop_front();
            }
            entries.push_back(entry);
        }
    }

    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.entries
            .lock()
            .map(|e| e.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn summary(&self, limit: usize) -> Value {
        let entries = self.snapshot();
        let total = entries.len();

        let mut by_endpoint: HashMap<String, usize> = HashMap::new();
        let mut errors_by_endpoint: HashMap<String, usize> = HashMap::new();
        let mut by_status: HashMap<String, usize> = HashMap::new();
        let mut by_ip: HashMap<String, usize> = HashMap::new();
        let mut by_track: HashMap<String, usize> = HashMap::new();
        let mut latencies: Vec<u64> = Vec::with_capacity(total);
        let mut errors: u64 = 0;
        // User-facing window for the headline error rate: infra noise
        // (root, health probes, the panel's own polling) is excluded so
        // the rate reflects real API traffic, not monitor 200s.
        let mut window_total: u64 = 0;
        let mut window_errors: u64 = 0;

        for e in &entries {
            *by_endpoint.entry(e.path.clone()).or_default() += 1;
            *by_status.entry(e.status.to_string()).or_default() += 1;
            *by_ip.entry(e.client_ip.clone()).or_default() += 1;
            if !e.detail.is_empty() {
                *by_track.entry(e.detail.clone()).or_default() += 1;
            }
            latencies.push(e.latency_ms);
            if e.status >= 400 {
                errors += 1;
                *errors_by_endpoint.entry(e.path.clone()).or_default() += 1;
            }
            if !is_infra_path(&e.path) {
                window_total += 1;
                if e.status >= 400 {
                    window_errors += 1;
                }
            }
        }

        latencies.sort_unstable();
        let pct = |p: f64| -> u64 {
            if latencies.is_empty() {
                return 0;
            }
            let idx = ((p * latencies.len() as f64) as usize).min(latencies.len() - 1);
            latencies[idx]
        };

        let mut top_endpoints: Vec<(String, usize)> = by_endpoint.into_iter().collect();
        top_endpoints.sort_by(|a, b| b.1.cmp(&a.1));
        let mut top_ips: Vec<(String, usize)> = by_ip.into_iter().collect();
        top_ips.sort_by(|a, b| b.1.cmp(&a.1));
        let mut top_tracks: Vec<(String, usize)> = by_track.into_iter().collect();
        top_tracks.sort_by(|a, b| b.1.cmp(&a.1));
        let mut top_err_endpoints: Vec<(String, usize)> = errors_by_endpoint.into_iter().collect();
        top_err_endpoints.sort_by_key(|(_, n)| std::cmp::Reverse(*n));

        let recent: Vec<Value> = entries
            .iter()
            .rev()
            .take(limit.min(100))
            .map(|e| {
                json!({
                    "ts": e.ts,
                    "method": e.method,
                    "path": e.path,
                    "detail": e.detail,
                    "status": e.status,
                    "latency_ms": e.latency_ms,
                    "client_ip": e.client_ip,
                })
            })
            .collect();

        json!({
            "total": total,
            "errors": errors,
            "error_rate": if window_total > 0 {
                format!("{:.2}%", (window_errors as f64 / window_total as f64) * 100.0)
            } else { "0.00%".into() },
            "window_total": window_total,
            "p50_ms": pct(0.5),
            "p95_ms": pct(0.95),
            "by_endpoint": top_endpoints.into_iter().take(20).map(|(k, v)| json!({"endpoint": k, "hits": v})).collect::<Vec<_>>(),
            "errors_by_endpoint": top_err_endpoints.into_iter().take(10).map(|(k, v)| json!({"endpoint": k, "errors": v})).collect::<Vec<_>>(),
            "by_status": by_status,
            "top_ips": top_ips.into_iter().take(10).map(|(k, v)| json!({"ip": k, "hits": v})).collect::<Vec<_>>(),
            "top_tracks": top_tracks.into_iter().take(10).map(|(k, v)| json!({"id": k, "hits": v})).collect::<Vec<_>>(),
            "recent": recent,
        })
    }
}

impl Default for RequestLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Paths excluded from the headline error-rate window: root, health
/// probes, and the panel's own polling read as user traffic otherwise.
/// Pure function — unit tested.
pub fn is_infra_path(path: &str) -> bool {
    path == "/" || path == "/health" || path.starts_with("/admin")
}

/// Collapse `/trackManifests/192157851` → `/trackManifests/:id` to bound cardinality.
pub fn normalize_path(path: &str) -> String {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return "/".to_string();
    }
    if segs.len() <= 2 {
        return format!("/{}", segs.join("/"));
    }
    format!("/{}/{}", segs[0], segs[1])
}

/// Pull the song/resource identifier out of a request so the log shows
/// *what* was requested, not just the endpoint shape:
/// path id for /trackManifests/{id} and /dash/{id}, else the `id=` query
/// value, else the search text (`s=`). Truncated to 64 chars.
fn extract_detail(path: &str, query: Option<&str>) -> String {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() >= 2 && (segs[0] == "trackManifests" || segs[0] == "dash") {
        return segs[1].chars().take(64).collect();
    }
    if let Some(q) = query {
        let mut search = None;
        for (k, v) in form_urlencoded::parse(q.as_bytes()) {
            if k == "id" && !v.is_empty() {
                return v.chars().take(64).collect();
            }
            if k == "s" && search.is_none() {
                search = Some(v.to_string());
            }
        }
        if let Some(s) = search {
            if !s.is_empty() {
                return s.chars().take(64).collect();
            }
        }
    }
    String::new()
}

pub async fn log_requests(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let method = req.method().to_string();
    let raw_path = req.uri().path().to_string();
    let path = normalize_path(&raw_path);
    let detail = extract_detail(&raw_path, req.uri().query());
    let ip = client_ip(&state, &req, addr);
    let start = Instant::now();

    let resp = next.run(req).await;
    let status = resp.status().as_u16();

    state.request_log.record(LogEntry {
        ts: chrono::Utc::now().timestamp(),
        method,
        path,
        detail,
        status,
        latency_ms: start.elapsed().as_millis() as u64,
        client_ip: ip.to_string(),
    });

    resp
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

#[cfg(test)]
mod tests {
    use super::{is_infra_path, LogEntry, RequestLog};

    fn entry(path: &str, status: u16) -> LogEntry {
        LogEntry {
            ts: 1_700_000_000,
            method: "GET".into(),
            path: path.into(),
            detail: String::new(),
            status,
            latency_ms: 5,
            client_ip: "127.0.0.1".into(),
        }
    }

    #[test]
    fn infra_paths_excluded() {
        assert!(is_infra_path("/"));
        assert!(is_infra_path("/health"));
        assert!(is_infra_path("/admin/stats"));
        assert!(is_infra_path("/admin/requests"));
        assert!(!is_infra_path("/track/"));
        assert!(!is_infra_path("/info/"));
        assert!(!is_infra_path("/pages/contribute.html"));
    }

    #[test]
    fn error_rate_counts_user_traffic_only() {
        let log = RequestLog::new();
        // Infra noise: must not move the rate.
        for _ in 0..10 {
            log.record(entry("/health", 200));
        }
        for _ in 0..10 {
            log.record(entry("/admin/stats", 200));
        }
        // User traffic: 3 ok + 1 failed => 25%.
        log.record(entry("/track/", 200));
        log.record(entry("/track/", 200));
        log.record(entry("/info/", 200));
        log.record(entry("/info/", 404));
        let s = log.summary(0);
        assert_eq!(s["total"], 24);
        assert_eq!(s["errors"], 1);
        assert_eq!(s["error_rate"], "25.00%");
        assert_eq!(s["window_total"], 4);
        let errs = s["errors_by_endpoint"].as_array().unwrap();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0]["endpoint"], "/info/");
        assert_eq!(errs[0]["errors"], 1);
    }

    #[test]
    fn error_rate_zero_without_user_traffic() {
        let log = RequestLog::new();
        log.record(entry("/health", 200));
        let s = log.summary(0);
        assert_eq!(s["error_rate"], "0.00%");
        assert_eq!(s["window_total"], 0);
    }
}
