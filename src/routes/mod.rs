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

use axum::Json;
use serde_json::{json, Value};

use crate::config::Config;

pub fn index(config: &Config) -> Json<Value> {
    Json(json!({
        "version": config.api_version,
        "Repo": "https://github.com/binimum/hifi-api"
    }))
}
