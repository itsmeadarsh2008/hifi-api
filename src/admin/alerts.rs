use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::notifier::Notifier;
use crate::AppState;

pub async fn alert_status(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "alerts": { "discord_configured": state.notifier.configured() } })))
}

pub async fn alert_test(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    match state.notifier.send_test().await {
        Ok(()) => Ok(Json(json!({"message": "Test alert sent to Discord"}))),
        Err(e) => Err(AppError::BadRequest(e)),
    }
}

#[derive(Deserialize)]
pub struct ReportRequest {
    pub kind: String,
}

/// On-demand Discord report triggered from the admin panel.
/// `kind` is "status" or "accounts". Bypasses the alert throttle.
pub async fn alert_report(
    State(state): State<AppState>,
    Json(body): Json<ReportRequest>,
) -> Result<Json<Value>, AppError> {
    let payload = match body.kind.as_str() {
        "status" => {
            let accounts = state.account_manager.list_accounts().await;
            let now = chrono::Utc::now().timestamp();
            let total = accounts.len();
            let mut healthy = 0usize;
            let mut limited = 0usize;
            let mut total_requests = 0u64;
            let mut total_errors = 0u64;
            for a in &accounts {
                let active = a.is_active.load(Ordering::Relaxed);
                let cooling = a.rate_limited_until.load(Ordering::Relaxed) > now;
                if active && !cooling {
                    healthy += 1;
                }
                if cooling {
                    limited += 1;
                }
                total_requests += a.request_count.load(Ordering::Relaxed);
                total_errors += a.error_count.load(Ordering::Relaxed);
            }
            let pstatus = state.proxy_manager.status().await;
            let proxy_summary = if pstatus
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                format!(
                    "{}",
                    pstatus
                        .get("current")
                        .and_then(|v| v.as_str())
                        .unwrap_or("resolving…")
                )
            } else {
                "direct (proxies off)".to_string()
            };
            let limits_summary = format!(
                "per-IP {}/{} · tidal {}/{} · cooldowns {}/{}s",
                state.rate_limits.ip_rps.load(Ordering::Relaxed),
                state.rate_limits.ip_burst.load(Ordering::Relaxed),
                state.rate_limits.tidal_rps.load(Ordering::Relaxed),
                state.rate_limits.tidal_burst.load(Ordering::Relaxed),
                state.rate_limits.cooldown_429_secs.load(Ordering::Relaxed),
                state.rate_limits.cooldown_403_secs.load(Ordering::Relaxed),
            );
            Notifier::status_report(
                healthy,
                total,
                limited,
                total_requests,
                total_errors,
                state.cache.hits.load(Ordering::Relaxed),
                state.cache.misses.load(Ordering::Relaxed),
                proxy_summary,
                limits_summary,
            )
        }
        "accounts" => {
            let mut accounts = state.account_manager.list_accounts().await;
            accounts.sort_by(|a, b| a.id.cmp(&b.id));
            let now = chrono::Utc::now().timestamp();
            let total = accounts.len();
            let lines: Vec<(String, String)> = accounts
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let code = format!("TIDAL-{}", i + 1);
                    let active = a.is_active.load(Ordering::Relaxed);
                    let until = a.rate_limited_until.load(Ordering::Relaxed);
                    let state_str = if !active {
                        "⛔ inactive".to_string()
                    } else if until > now {
                        format!("⏳ cooldown {}s", until - now)
                    } else {
                        "✅ active".to_string()
                    };
                    let status = format!(
                        "{} · {} req · {} err · {} RL-hits",
                        state_str,
                        a.request_count.load(Ordering::Relaxed),
                        a.error_count.load(Ordering::Relaxed),
                        a.rate_limit_hits.load(Ordering::Relaxed),
                    );
                    (code, status)
                })
                .collect();
            Notifier::accounts_report(total, lines)
        }
        other => {
            return Err(AppError::BadRequest(format!(
                "Unknown report kind '{}' (use status|accounts)",
                other
            )))
        }
    };

    match state.notifier.send_report(payload).await {
        Ok(()) => Ok(Json(json!({"message": "Report sent to Discord"}))),
        Err(e) => Err(AppError::BadRequest(e)),
    }
}
