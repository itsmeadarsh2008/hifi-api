use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use crate::account_manager::AccountManager;
use crate::notifier::Notifier;
use crate::proxy_manager::ProxyManager;
use crate::rate_limit::RateLimitSettings;
use crate::token_manager::TokenManager;

const TICK_SECS: u64 = 300;
const BASE_BACKOFF_SECS: i64 = 300;
const MAX_BACKOFF_SECS: i64 = 3600;

/// Background loop: retry refresh on system-disabled accounts only.
/// Never touches owner-toggled OFF accounts (auto_disabled == false),
/// never touches accounts still in Tidal cooldown, and backs off
/// exponentially (5m → 1h cap) so dead accounts cost ~1 Tidal call/hour.
pub async fn start_autoheal_loop(
    account_manager: Arc<AccountManager>,
    token_manager: Arc<TokenManager>,
    proxy_manager: Arc<ProxyManager>,
    rate_limits: Arc<RateLimitSettings>,
    notifier: Arc<Notifier>,
) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
            if !rate_limits.auto_heal.load(Ordering::Relaxed) {
                continue;
            }
            let client = match proxy_manager.working_client().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!("Auto-heal skipped: {}", e);
                    continue;
                }
            };
            let now = Utc::now().timestamp();
            for account in account_manager.list_accounts().await {
                if !account.auto_disabled.load(Ordering::Relaxed) {
                    continue;
                }
                if account.is_active.load(Ordering::Relaxed) {
                    // Recovered through another path; clear the flag.
                    let _ = account_manager
                        .set_auto_disabled(&account.id, false)
                        .await;
                    continue;
                }
                if account.rate_limited_until.load(Ordering::Relaxed) > now {
                    continue;
                }
                if account.heal_next_retry.load(Ordering::Relaxed) > now {
                    continue;
                }
                tracing::info!("Auto-heal: retrying account {}", account.label);
                match token_manager.refresh_token(&account, &client).await {
                    Ok(_) => {
                        let _ = account_manager.set_account_active(&account.id, true).await;
                        let _ = account_manager.set_auto_disabled(&account.id, false).await;
                        tracing::info!("Auto-heal: account {} recovered", account.label);
                        let (healthy, total) = account_manager.healthy_count().await;
                        notifier.alert_healed(&account.label, healthy, total).await;
                    }
                    Err(e) => {
                        let failures =
                            account.heal_failures.fetch_add(1, Ordering::Relaxed) + 1;
                        let backoff =
                            (BASE_BACKOFF_SECS * 2_i64.pow(failures.min(5) as u32 - 1))
                                .min(MAX_BACKOFF_SECS);
                        account
                            .heal_next_retry
                            .store(now + backoff, Ordering::Relaxed);
                        tracing::warn!(
                            "Auto-heal: account {} still failing (attempt {}): {:?}; next retry in {}s",
                            account.label,
                            failures,
                            e,
                            backoff
                        );
                    }
                }
            }
        }
    });
}
