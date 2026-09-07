use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use serde_json::{json, Value};
use std::net::SocketAddr;

use crate::ip_limiter;
use crate::AppState;

const MAX_ENTRIES: usize = 5000;

#[derive(Clone)]
pub struct LogEntry {
    pub ts: i64,
    pub method: String,
    pub path: String,
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
        let mut by_status: HashMap<String, usize> = HashMap::new();
        let mut by_ip: HashMap<String, usize> = HashMap::new();
        let mut latencies: Vec<u64> = Vec::with_capacity(total);
        let mut errors: u64 = 0;

        for e in &entries {
            *by_endpoint.entry(e.path.clone()).or_default() += 1;
            *by_status.entry(e.status.to_string()).or_default() += 1;
            *by_ip.entry(e.client_ip.clone()).or_default() += 1;
            latencies.push(e.latency_ms);
            if e.status >= 400 {
                errors += 1;
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

        let recent: Vec<Value> = entries
            .iter()
            .rev()
            .take(limit.min(100))
            .map(|e| {
                json!({
                    "ts": e.ts,
                    "method": e.method,
                    "path": e.path,
                    "status": e.status,
                    "latency_ms": e.latency_ms,
                    "client_ip": e.client_ip,
                })
            })
            .collect();

        json!({
            "total": total,
            "errors": errors,
            "p50_ms": pct(0.5),
            "p95_ms": pct(0.95),
            "by_endpoint": top_endpoints.into_iter().take(20).map(|(k, v)| json!({"endpoint": k, "hits": v})).collect::<Vec<_>>(),
            "by_status": by_status,
            "top_ips": top_ips.into_iter().take(10).map(|(k, v)| json!({"ip": k, "hits": v})).collect::<Vec<_>>(),
            "recent": recent,
        })
    }
}

impl Default for RequestLog {
    fn default() -> Self {
        Self::new()
    }
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

pub async fn log_requests(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let method = req.method().to_string();
    let raw_path = req.uri().path().to_string();
    let path = normalize_path(&raw_path);
    let ip = ip_limiter::client_ip(&state, &req, addr).to_string();
    let start = Instant::now();

    let resp = next.run(req).await;

    state.request_log.record(LogEntry {
        ts: chrono::Utc::now().timestamp(),
        method,
        path,
        status: resp.status().as_u16(),
        latency_ms: start.elapsed().as_millis() as u64,
        client_ip: ip,
    });

    resp
}
