use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use governor::clock::{Clock, DefaultClock};
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use rand::Rng;

use crate::rate_limit::RateLimitSettings;

type Limiter = RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;

pub struct AntiBan {
    limiter: ArcSwap<Limiter>,
    settings: Arc<RateLimitSettings>,
}

impl AntiBan {
    pub fn new(settings: Arc<RateLimitSettings>) -> Self {
        Self {
            limiter: ArcSwap::from_pointee(Self::build_limiter(&settings)),
            settings,
        }
    }

    fn build_limiter(settings: &RateLimitSettings) -> Limiter {
        let rps = settings.ip_rps.load(Ordering::Relaxed) as u32;
        let burst = settings.ip_burst.load(Ordering::Relaxed) as u32;
        RateLimiter::keyed(
            Quota::per_second(NonZeroU32::new(rps.max(1)).unwrap())
                .allow_burst(NonZeroU32::new(burst.max(1)).unwrap()),
        )
    }

    pub fn reload_limiter(&self) {
        self.limiter
            .store(Arc::new(Self::build_limiter(&self.settings)));
    }

    pub fn check_ip(&self, ip: IpAddr) -> Result<(), Duration> {
        match self.limiter.load().check_key(&ip) {
            Ok(()) => Ok(()),
            Err(not_until) => Err(not_until.wait_time_from(DefaultClock::default().now())),
        }
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