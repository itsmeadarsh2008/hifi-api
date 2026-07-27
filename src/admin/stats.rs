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
    let active_count = accounts
        .iter()
        .filter(|a| a.is_active.load(std::sync::atomic::Ordering::Relaxed))
        .count();
    let rate_limited_count = accounts
        .iter()
        .filter(|a| {
            a.rate_limited_until.load(std::sync::atomic::Ordering::Relaxed)
                > chrono::Utc::now().timestamp()
        })
        .count();

    Ok(Json(json!({
        "total_requests": total_requests,
        "total_errors": total_errors,
        "error_rate": if total_requests > 0 {
            format!("{:.2}%", (total_errors as f64 / total_requests as f64) * 100.0)
        } else { "0.00%".into() },
        "total_accounts": accounts.len(),
        "active_accounts": active_count,
        "rate_limited_accounts": rate_limited_count,
        "healthy_accounts": active_count.saturating_sub(rate_limited_count)
    })))
}
