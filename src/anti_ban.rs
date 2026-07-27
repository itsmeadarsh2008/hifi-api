use std::num::NonZeroU32;
use std::time::Duration;

use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use rand::Rng;

pub struct AntiBan {
    global_limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
}

impl AntiBan {
    pub fn new() -> Self {
        Self {
            global_limiter: RateLimiter::direct(
                Quota::per_second(
                    NonZeroU32::new(50).unwrap(),
                )
                .allow_burst(NonZeroU32::new(100).unwrap()),
            ),
        }
    }

    pub async fn check_global_rate_limit(&self) -> bool {
        self.global_limiter.check().is_ok()
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
        Self::new()
    }
}
