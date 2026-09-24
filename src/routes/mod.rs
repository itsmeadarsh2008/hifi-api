pub mod album;
pub mod artist;
pub mod cover;
pub mod health;
pub mod info;
pub mod lyrics;
pub mod mix;
pub mod playlist;
pub mod recommendations;
pub mod search;
pub mod similar_albums;
pub mod similar_artists;
pub mod topvideos;
pub mod track;
pub mod video;
pub mod widevine;

use std::future::Future;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;

use axum::Json;
use serde_json::{json, Value};

use crate::config::Config;
use crate::error::AppError;
use crate::AppState;

pub fn index(config: &Config) -> Json<Value> {
    Json(json!({
        "version": config.api_version,
        "Repo": "https://github.com/itsmeadarsh2008/hifi-api"
    }))
}

/// Upper bound for one direct playback op. Normal ops take seconds;
/// failover storms must not hang a request forever.
pub(crate) const PLAYBACK_OP_TIMEOUT_SECS: u64 = 120;

/// Live-op guard: counts in-flight playback requests for the panel while
/// held. Replaces the old queue's `active` gauge now that nothing queues.
pub(crate) struct InflightGuard {
    counter: Arc<AtomicU64>,
}

impl InflightGuard {
    pub(crate) fn track(state: &AppState) -> Self {
        let counter = state.playback_inflight.clone();
        counter.fetch_add(1, Ordering::Relaxed);
        Self { counter }
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Run a playback fetch directly: no queue, no waiting, no 202 polling.
/// Counts the op as in-flight while running and bounds it by the op
/// timeout. Distribution across accounts happens inside the fetch via the
/// normal weighted selection + failover.
pub(crate) async fn run_direct<T, Fut>(state: &AppState, fut: Fut) -> Result<T, AppError>
where
    Fut: Future<Output = Result<T, AppError>>,
{
    let _guard = InflightGuard::track(state);
    match tokio::time::timeout(Duration::from_secs(PLAYBACK_OP_TIMEOUT_SECS), fut).await {
        Ok(r) => r,
        Err(_) => Err(AppError::Timeout),
    }
}
