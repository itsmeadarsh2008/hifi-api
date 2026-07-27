use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct MixParams {
    pub id: String,
}

pub async fn get_mix(
    State(state): State<AppState>,
    Query(params): Query<MixParams>,
) -> Result<Json<Value>, AppError> {
    let account = state.account_manager.select_account().await?;
    let token = state
        .token_manager
        .get_token(&account, state.tidal_client.http_client())
        .await?;

    let url = "https://api.tidal.com/v1/pages/mix";
    let data = state
        .tidal_client
        .make_authed_request(
            url,
            Some(vec![
                ("mixId", &params.id),
                ("countryCode", &state.config.country_code),
                ("deviceType", "BROWSER"),
            ]),
            &token,
        )
        .await?;

    let mut header = json!({});
    let mut items: Vec<Value> = Vec::new();

    if let Some(rows) = data.get("rows").and_then(|v| v.as_array()) {
        for row in rows {
            if let Some(modules) = row.get("modules").and_then(|v| v.as_array()) {
                for module in modules {
                    match module.get("type").and_then(|v| v.as_str()) {
                        Some("MIX_HEADER") => {
                            header = module.get("mix").cloned().unwrap_or_default();
                        }
                        Some("TRACK_LIST") => {
                            if let Some(paged_list) = module.get("pagedList") {
                                if let Some(raw_items) = paged_list.get("items").and_then(|v| v.as_array()) {
                                    items = raw_items
                                        .iter()
                                        .map(|i| i.get("item").cloned().unwrap_or(i.clone()))
                                        .collect();
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(Json(json!({
        "version": state.config.api_version,
        "mix": header,
        "items": items
    })))
}
