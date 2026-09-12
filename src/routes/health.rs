use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::AppState;

/// Liveness for load balancers plus Redis sync status. The Redis verdict is
/// cached (15s) so a dead Redis can never slow this endpoint down.
pub async fn health(State(state): State<AppState>) -> Json<Value> {
    let redis = match &state.upstash {
        None => json!({"configured": false, "status": "disabled"}),
        Some(store) => {
            if store.is_alive(15).await {
                json!({"configured": true, "status": "ok"})
            } else {
                json!({"configured": true, "status": "unreachable"})
            }
        }
    };
    Json(json!({"status": "healthy", "redis": redis}))
}
