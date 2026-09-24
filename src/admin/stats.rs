use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

pub async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let accounts = state.account_manager.list_accounts().await;
    let total_requests: u64 = accounts
        .iter()
        .map(|a| a.request_count.load(std::sync::atomic::Ordering::Relaxed))
        .sum();
    let total_errors: u64 = accounts
        .iter()
        .map(|a| a.error_count.load(std::sync::atomic::Ordering::Relaxed))
        .sum();
    // Headline error rate: user-facing responses over the recent window
    // (infra probes excluded), NOT account-attempt counters — one user
    // request failing over 3 accounts used to read as "66% error" here.
    // Cumulative account counters stay below for totals.
    let log = state.request_log.summary(0);
    let error_rate = log
        .get("error_rate")
        .and_then(|v| v.as_str())
        .unwrap_or("0.00%")
        .to_string();
    let active_count = accounts
        .iter()
        .filter(|a| a.is_active.load(std::sync::atomic::Ordering::Relaxed))
        .count();
    let premium_count = accounts
        .iter()
        .filter(|a| {
            a.is_active.load(std::sync::atomic::Ordering::Relaxed)
                && a.premium_status.try_read().map(|s| s.as_str() == "premium").unwrap_or(false)
        })
        .count();
    let playback_count = state.account_manager.playback_count().await;
    let pool = state.account_manager.playback_slots().await;
    // No queue exists: every playback request runs directly. Same card
    // shape as before (active live ops / pool size, nothing queued).
    let playback = json!({
        "pool_size": pool,
        "active": state.playback_inflight.load(std::sync::atomic::Ordering::Relaxed),
        "pending": 0,
        "jobs": 0,
    });
    let catalog = if !state.config.catalog_token.is_empty() {
        json!({"mode": "static_token"})
    } else if let Some(acc) = state.account_manager.find_catalog_account().await {
        json!({
            "mode": "account",
            "label": acc.label,
            "active": acc.is_active.load(std::sync::atomic::Ordering::Relaxed),
        })
    } else {
        json!({"mode": "pool"})
    };

    let redis = match &state.upstash {
        None => json!({"configured": false, "status": "disabled"}),
        Some(store) => {
            // Which instance we're synced to (backend + host only — the
            // native URL embeds its password, so it is never exposed).
            let endpoint = store.describe();
            if store.is_alive(15).await {
                json!({"configured": true, "status": "ok", "endpoint": endpoint})
            } else {
                json!({"configured": true, "status": "unreachable", "endpoint": endpoint})
            }
        }
    };

    Ok(Json(json!({
        "total_requests": total_requests,
        "total_errors": total_errors,
        "error_rate": error_rate,
        "total_accounts": accounts.len(),
        "active_accounts": active_count,
        "healthy_accounts": active_count,
        "premium_accounts": premium_count,
        "playback_accounts": playback_count,
        "playback": playback,
        "catalog": catalog,
        "redis": redis,
    })))
}
