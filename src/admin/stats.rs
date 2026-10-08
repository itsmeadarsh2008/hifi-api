use std::sync::Arc;
use std::sync::OnceLock;

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::account_manager::AccountManager;
use crate::error::AppError;
use crate::upstash::UpstashStore;
use crate::AppState;

/// (total, active, premium) over the live roster. Premium counts only
/// active accounts whose last probe came back premium.
pub async fn snapshot_counts(manager: &AccountManager) -> (usize, usize, usize) {
    let accounts = manager.list_accounts().await;
    let total = accounts.len();
    let active = accounts
        .iter()
        .filter(|a| a.is_active.load(std::sync::atomic::Ordering::Relaxed))
        .count();
    let premium = accounts
        .iter()
        .filter(|a| {
            a.is_active.load(std::sync::atomic::Ordering::Relaxed)
                && a.premium_status
                    .try_read()
                    .map(|s| s.as_str() == "premium")
                    .unwrap_or(false)
        })
        .count();
    (total, active, premium)
}

static INSTANCE_ID: OnceLock<String> = OnceLock::new();

/// Publish this instance's counts to the pool Redis (60s tick + once at
/// startup). Pool dashboards read the freshest record — see key layout in
/// upstash.rs (`k_stats_set` / `k_stats`). Best-effort: never fails boot.
pub async fn publish_heartbeat(
    manager: &AccountManager,
    store: Option<Arc<UpstashStore>>,
) {
    let Some(store) = store else {
        return;
    };
    let id = INSTANCE_ID
        .get_or_init(|| uuid::Uuid::new_v4().to_string())
        .clone();
    let (total, active, premium) = snapshot_counts(manager).await;
    let payload = json!({
        "id": id,
        "total": total,
        "active": active,
        "premium": premium,
        "at": chrono::Utc::now().timestamp(),
    })
    .to_string();
    store
        .set(&UpstashStore::k_stats(&id), &payload, Some(180))
        .await;
    store.sadd(&UpstashStore::k_stats_set(), &id).await;
}

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
    // Same numbers the Redis heartbeat publishes.
    let (total_count, active_count, premium_count) =
        snapshot_counts(&state.account_manager).await;
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
        "total_accounts": total_count,
        "active_accounts": active_count,
        "healthy_accounts": active_count,
        "premium_accounts": premium_count,
        "playback_accounts": playback_count,
        "playback": playback,
        "catalog": catalog,
        "redis": redis,
    })))
}

#[cfg(test)]
mod tests {
    use super::snapshot_counts;
    use crate::account_manager::{AccountManager, SwitchingWeights};
    use std::sync::Arc;

    #[tokio::test]
    async fn snapshot_counts_premium_only_when_active() {
        let am = Arc::new(AccountManager::new(None, SwitchingWeights::default()));
        let a = am
            .add_account("a".into(), "c".into(), "s".into(), "rt-a".into(), None)
            .await
            .unwrap();
        let b = am
            .add_account("b".into(), "c".into(), "s".into(), "rt-b".into(), None)
            .await
            .unwrap();
        am.set_premium(&a.id, "premium").await;
        am.set_premium(&b.id, "premium").await;
        am.set_account_active(&b.id, false).await.unwrap();
        // Premium verdict on an inactive account must not count.
        assert_eq!(snapshot_counts(&am).await, (2, 1, 1));
    }
}
