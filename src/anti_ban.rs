use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arc_swap::ArcSwap;
use dashmap::DashMap;
use governor::clock::{Clock, DefaultClock};
use governor::state::keyed::DefaultKeyedStateStore;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use rand::Rng;

use crate::rate_limit::RateLimitSettings;
use crate::upstash::UpstashStore;

type IpLimiter = RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;
type TidalLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;
type AccountLimiter = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

/// Adaptive per-IP reputation. Score drifts toward 0 over time; good
/// behavior raises it (up to +100), abuse lowers it (down to -100).
/// It never blocks anyone by itself — it only decides whether an
/// over-limit client gets a gentle delay or an instant 429.
#[derive(Clone, Copy, Debug)]
pub struct IpReputation {
    pub score: f32,
    pub seen: u64,
    pub rejects: u64,
    pub last_seen: i64,
}

impl IpReputation {
    /// Burst multiplier in [0.25, 2.0]; 1.0 when reputation is off/unknown.
    pub fn factor(&self) -> f32 {
        (1.0 + self.score / 100.0).clamp(0.25, 2.0)
    }
}

/// What changed when the pool was re-evaluated (drives Discord alerts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolTransition {
    EnteredConservation,
    ExitedConservation,
}

pub struct AntiBan {
    ip_limiter: ArcSwap<IpLimiter>,
    costly_limiter: ArcSwap<IpLimiter>,
    tidal_limiter: ArcSwap<TidalLimiter>,
    account_limiter: ArcSwap<AccountLimiter>,
    conserve_limiter: ArcSwap<TidalLimiter>,
    reputation: DashMap<IpAddr, IpReputation>,
    conserved: AtomicBool,
    /// Last built (effective_global_rps, account_rps) — rebuild only on change.
    last_built: Mutex<(u64, u64)>,
    settings: Arc<RateLimitSettings>,
    /// Shared cross-instance state (None = single-host mode, skip sync).
    upstash: std::sync::OnceLock<Arc<UpstashStore>>,
}

impl AntiBan {
    pub fn new(settings: Arc<RateLimitSettings>) -> Self {
        let this = Self {
            ip_limiter: ArcSwap::from_pointee(Self::build_ip_limiter(&settings)),
            costly_limiter: ArcSwap::from_pointee(Self::build_costly_limiter(&settings)),
            tidal_limiter: ArcSwap::from_pointee(Self::build_tidal_limiter(&settings)),
            account_limiter: ArcSwap::from_pointee(Self::build_account_limiter(&settings)),
            conserve_limiter: ArcSwap::from_pointee(Self::build_conserve_limiter(&settings)),
            reputation: DashMap::new(),
            conserved: AtomicBool::new(false),
            last_built: Mutex::new((0, 0)),
            settings,
            upstash: std::sync::OnceLock::new(),
        };
        this
    }

    /// Attach shared state once at startup (before serving).
    pub fn set_upstash(&self, store: Option<Arc<UpstashStore>>) {
        if let Some(s) = store {
            let _ = self.upstash.set(s);
        }
    }

    fn quota(rps: u64, burst: u64) -> Quota {
        Quota::per_second(NonZeroU32::new(rps.max(1) as u32).unwrap())
            .allow_burst(NonZeroU32::new(burst.max(1) as u32).unwrap())
    }

    fn build_ip_limiter(settings: &RateLimitSettings) -> IpLimiter {
        let rps = settings.ip_rps.load(Ordering::Relaxed);
        let burst = settings.ip_burst.load(Ordering::Relaxed);
        RateLimiter::keyed(Self::quota(rps, burst))
    }

    fn build_costly_limiter(settings: &RateLimitSettings) -> IpLimiter {
        let rps = settings.ip_costly_rps.load(Ordering::Relaxed);
        let burst = settings.ip_costly_burst.load(Ordering::Relaxed);
        RateLimiter::keyed(Self::quota(rps, burst))
    }

    fn build_tidal_limiter(settings: &RateLimitSettings) -> TidalLimiter {
        let rps = settings.tidal_rps.load(Ordering::Relaxed);
        let burst = settings.tidal_burst.load(Ordering::Relaxed);
        RateLimiter::direct(Self::quota(rps, burst))
    }

    fn build_account_limiter(settings: &RateLimitSettings) -> AccountLimiter {
        let rps = settings.account_rps.load(Ordering::Relaxed);
        let burst = settings.account_burst.load(Ordering::Relaxed);
        RateLimiter::keyed(Self::quota(rps, burst))
    }

    fn build_conserve_limiter(settings: &RateLimitSettings) -> TidalLimiter {
        let rps = settings.conserve_trickle_rps.load(Ordering::Relaxed);
        RateLimiter::direct(Self::quota(rps, rps))
    }

    pub fn reload_limiter(&self) {
        self.ip_limiter
            .store(Arc::new(Self::build_ip_limiter(&self.settings)));
        self.costly_limiter
            .store(Arc::new(Self::build_costly_limiter(&self.settings)));
        self.conserve_limiter
            .store(Arc::new(Self::build_conserve_limiter(&self.settings)));
        // Tidal + account limiters scale with the pool; rebuild via refresh_for_pool.
        if let Ok(mut sig) = self.last_built.lock() {
            *sig = (0, 0);
        }
    }

    /// Recompute pool-scaled quotas. Call periodically with live counts.
    /// Returns a conservation transition when the mode flips, so the
    /// caller can alert. Conservation engages only on a *depleted* pool
    /// (healthy at/under reserve while some active accounts are parked) —
    /// never merely because the pool is small.
    pub fn refresh_for_pool(&self, healthy: usize, active: usize) -> Option<PoolTransition> {
        let per_account = self.settings.account_rps.load(Ordering::Relaxed).max(1);
        let ceiling = self.settings.tidal_rps.load(Ordering::Relaxed).max(1);
        let effective = (per_account * healthy.max(1) as u64).clamp(1, ceiling);

        let changed = if let Ok(mut sig) = self.last_built.lock() {
            let changed = *sig != (effective, per_account);
            *sig = (effective, per_account);
            changed
        } else {
            false
        };
        if changed {
            let burst = (self.settings.account_burst.load(Ordering::Relaxed).max(1)
                * healthy.max(1) as u64)
                .clamp(1, self.settings.tidal_burst.load(Ordering::Relaxed).max(1));
            self.tidal_limiter.store(Arc::new(RateLimiter::direct(
                Self::quota(effective, burst),
            )));
            self.account_limiter
                .store(Arc::new(Self::build_account_limiter(&self.settings)));
        }

        let reserve = self.settings.reserve_accounts.load(Ordering::Relaxed).max(1) as usize;
        let reserve = reserve.min(active.max(1));
        // Engage only when the pool has actually lost accounts: healthy at or
        // under reserve while some active account is parked. Owner-disabled
        // accounts don't count (they lower `active` too).
        let should_conserve = healthy <= reserve && healthy < active;
        let was = self.conserved.swap(should_conserve, Ordering::Relaxed);
        match (was, should_conserve) {
            (false, true) => Some(PoolTransition::EnteredConservation),
            (true, false) => Some(PoolTransition::ExitedConservation),
            _ => None,
        }
    }

    pub fn in_conservation(&self) -> bool {
        self.conserved.load(Ordering::Relaxed)
    }

    /// Fail-fast gate used only in conservation mode: lets the trickle
    /// through, rejects the rest immediately (no queueing while fragile).
    pub fn check_conserve(&self) -> Result<(), Duration> {
        match self.conserve_limiter.load().check() {
            Ok(()) => Ok(()),
            Err(not_until) => Err(not_until.wait_time_from(DefaultClock::default().now())),
        }
    }

    pub fn check_ip(&self, ip: IpAddr) -> Result<(), Duration> {
        match self.ip_limiter.load().check_key(&ip) {
            Ok(()) => Ok(()),
            Err(not_until) => Err(not_until.wait_time_from(DefaultClock::default().now())),
        }
    }

    /// Costly-tier check (routes that hit Tidal). Returns the wait on
    /// failure so the caller can graduate: delay or reject.
    pub fn check_costly(&self, ip: IpAddr) -> Result<(), Duration> {
        match self.costly_limiter.load().check_key(&ip) {
            Ok(()) => Ok(()),
            Err(not_until) => Err(not_until.wait_time_from(DefaultClock::default().now())),
        }
    }

    /// Per-account pacing gate: waits for this account's slice.
    pub async fn throttle_account(&self, account_id: &str) {
        self.account_limiter
            .load()
            .until_key_ready(&account_id.to_string())
            .await;
    }

    pub async fn throttle_tidal(&self) {
        self.tidal_limiter.load().until_ready().await;
        self.apply_jitter().await;
        // Fleet-wide ceiling: without this, N hosts × local ceiling each
        // hit Tidal concurrently (e.g. 6 × 20rps). The local governor still
        // shapes per-host traffic; Redis caps the fleet total.
        self.throttle_tidal_global().await;
    }

    /// Shared fixed 1-second window counter. Sleeps to the next window when
    /// the fleet already spent this second's budget (bounded waits, then
    /// fail-open). Boundary bursts up to ~2× are possible with fixed
    /// windows — acceptable next to the per-host governor shaping.
    async fn throttle_tidal_global(&self) {
        let store = match self.upstash.get() {
            Some(s) => s.clone(),
            None => return,
        };
        let ceiling = self.settings.tidal_rps.load(Ordering::Relaxed).max(1) as i64;
        for _ in 0..4 {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let key = UpstashStore::k_throttle(now_ms / 1000);
            match store.incr_expire(&key, 3).await {
                Some(n) if n <= ceiling => return,
                Some(_) => {
                    // Over budget: wait out this window (+small desync jitter).
                    let wait_ms = (1000 - (now_ms % 1000)).max(1) as u64
                        + rand::thread_rng().gen_range(0..50);
                    tokio::time::sleep(Duration::from_millis(wait_ms.min(1500))).await;
                }
                // Redis unreachable: fail open, local limiter still applies.
                None => return,
            }
        }
    }

    // --- reputation ---

    pub fn reputation_factor(&self, ip: IpAddr) -> f32 {
        if !self.settings.reputation_enabled.load(Ordering::Relaxed) {
            return 1.0;
        }
        self.reputation.get(&ip).map(|r| r.factor()).unwrap_or(1.0)
    }

    /// Feed a completed request outcome back into the IP's score.
    /// `junk` = low-signal request (e.g. 1-char search).
    pub fn note_outcome(&self, ip: IpAddr, status: u16, junk: bool) {
        if !self.settings.reputation_enabled.load(Ordering::Relaxed) {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let mut entry = self
            .reputation
            .entry(ip)
            .or_insert(IpReputation { score: 0.0, seen: 0, rejects: 0, last_seen: now });
        entry.seen += 1;
        entry.last_seen = now;
        // Drift toward neutral so old sins fade.
        entry.score *= 0.999;
        if junk {
            entry.score -= 10.0;
        } else if status == 429 {
            // Already counted via note_reject at reject time; don't double-hit.
        } else if (400..500).contains(&status) {
            entry.score -= 5.0;
        } else if status >= 500 {
            entry.score -= 2.0;
        } else {
            entry.score += 1.0;
        }
        entry.score = entry.score.clamp(-100.0, 100.0);
    }

    pub fn note_reject(&self, ip: IpAddr) {
        if !self.settings.reputation_enabled.load(Ordering::Relaxed) {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let mut entry = self
            .reputation
            .entry(ip)
            .or_insert(IpReputation { score: 0.0, seen: 0, rejects: 0, last_seen: now });
        entry.rejects += 1;
        entry.last_seen = now;
        entry.score = (entry.score - 2.0).clamp(-100.0, 100.0);
    }

    pub fn evict_reputation(&self, max_age_secs: i64) {
        let now = chrono::Utc::now().timestamp();
        self.reputation
            .retain(|_, r| now - r.last_seen < max_age_secs);
    }

    pub fn reputation_snapshot(&self) -> Vec<(IpAddr, IpReputation)> {
        let mut out: Vec<(IpAddr, IpReputation)> = self
            .reputation
            .iter()
            .map(|e| (*e.key(), *e.value()))
            .collect();
        out.sort_by(|a, b| b.1.score.partial_cmp(&a.1.score).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    pub async fn apply_jitter(&self) {
        let jitter_ms = rand::thread_rng().gen_range(50..300);
        tokio::time::sleep(Duration::from_millis(jitter_ms)).await;
    }

    pub async fn apply_jitter_with_base(&self, base_ms: u64) {
        let jitter = rand::thread_rng().gen_range(0..=200);
        let total = (base_ms as f64 * (0.8 + rand::thread_rng().gen::<f64>() * 0.4)) as u64;
        tokio::time::sleep(Duration::from_millis(total + jitter)).await;
    }

    pub fn tidal_headers() -> Vec<(&'static str, &'static str)> {
        vec![
            ("User-Agent", "okhttp/5.3.2"),
            ("Accept", "*/*"),
            ("Accept-Encoding", "gzip"),
            ("Accept-Language", "en-US,en;q=0.9"),
            ("X-Platform", "android"),
            ("X-Tidal-Platform", "android"),
        ]
    }
}

impl Default for AntiBan {
    fn default() -> Self {
        Self::new(Arc::new(RateLimitSettings::from_env()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> Arc<RateLimitSettings> {
        let s = Arc::new(RateLimitSettings::from_env());
        s.ip_rps.store(1000, Ordering::Relaxed);
        s.ip_burst.store(1000, Ordering::Relaxed);
        s.tidal_rps.store(100, Ordering::Relaxed);
        s.tidal_burst.store(100, Ordering::Relaxed);
        s.account_rps.store(10, Ordering::Relaxed);
        s.account_burst.store(20, Ordering::Relaxed);
        s.reserve_accounts.store(2, Ordering::Relaxed);
        s.conserve_trickle_rps.store(1, Ordering::Relaxed);
        s
    }

    #[test]
    fn reputation_factor_bounds() {
        let mk = |score: f32| IpReputation { score, seen: 0, rejects: 0, last_seen: 0 }.factor();
        assert_eq!(mk(0.0), 1.0);
        assert_eq!(mk(100.0), 2.0);
        assert_eq!(mk(-100.0), 0.25);
        assert_eq!(mk(500.0), 2.0);
        assert_eq!(mk(-500.0), 0.25);
    }

    #[test]
    fn conservation_transitions() {
        let ab = AntiBan::new(test_settings());
        assert!(!ab.in_conservation());
        // 2 healthy of 7 active with reserve 2 → conserve.
        assert_eq!(
            ab.refresh_for_pool(2, 7),
            Some(PoolTransition::EnteredConservation)
        );
        assert!(ab.in_conservation());
        // No repeated transition while latched.
        assert_eq!(ab.refresh_for_pool(2, 7), None);
        assert_eq!(ab.refresh_for_pool(1, 7), None);
        // Recover above reserve → exit.
        assert_eq!(
            ab.refresh_for_pool(3, 7),
            Some(PoolTransition::ExitedConservation)
        );
        assert!(!ab.in_conservation());
    }

    #[test]
    fn no_conservation_without_depletion() {
        let ab = AntiBan::new(test_settings());
        // Full health, whatever the pool size.
        assert_eq!(ab.refresh_for_pool(7, 7), None);
        assert_eq!(ab.refresh_for_pool(1, 1), None);
        assert!(!ab.in_conservation());
        // Owner-disabled accounts don't count: 2 active of 2, both healthy.
        assert_eq!(ab.refresh_for_pool(2, 2), None);
        assert!(!ab.in_conservation());
    }

    #[test]
    fn small_pool_still_protected() {
        let ab = AntiBan::new(test_settings());
        // 1 healthy of 2 active with reserve 2 → conserve.
        assert_eq!(
            ab.refresh_for_pool(1, 2),
            Some(PoolTransition::EnteredConservation)
        );
        assert!(ab.in_conservation());
    }

    #[test]
    fn quota_scales_with_healthy_count() {
        let ab = AntiBan::new(test_settings());
        ab.refresh_for_pool(7, 7);
        let (eff, _) = ab.last_built.lock().unwrap().clone();
        // 10/account × 7, clamped to 100 ceiling.
        assert_eq!(eff, 70);
        ab.refresh_for_pool(2, 7);
        let (eff2, _) = ab.last_built.lock().unwrap().clone();
        assert_eq!(eff2, 20);
    }
}