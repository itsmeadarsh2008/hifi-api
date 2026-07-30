use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;

const AUTH_CLIENT_ID: &str = "fX2JxdmntZWK0ixT";
const AUTH_CLIENT_SECRET: &str = "1Nm5AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg=";
const REQUEST_CLIENT_ID: &str = "lw3vR6GE1vtNBsjv";
const REQUEST_CLIENT_SECRET: &str = "Y8tIpqKJxs9BEIwYr0I9bSbMWDsogXJx9LaN3mCHwD4%3D";

#[derive(Clone)]
pub enum SetupStatus {
    Pending,
    Complete {
        account_id: String,
        label: String,
        user_id: String,
    },
    Error(String),
}

pub struct SetupSession {
    pub device_code: String,
    pub status: SetupStatus,
}

pub type Sessions = Arc<RwLock<HashMap<String, SetupSession>>>;

pub fn new_session_store() -> Sessions {
    Arc::new(RwLock::new(HashMap::new()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct DeviceAuthorization {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    interval: i64,
    expires_in: i64,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    token_type: String,
    user: UserInfo,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct UserInfo {
    #[serde(rename = "userId")]
    user_id: String,
    #[serde(default)]
    country_code: String,
}

pub async fn start_setup(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let http_client = state.tidal_client.http_client();

    let auth_resp: DeviceAuthorization = http_client
        .post("https://auth.tidal.com/v1/oauth2/device_authorization")
        .form(&[("client_id", AUTH_CLIENT_ID), ("scope", "r_usr+w_usr+w_sub")])
        .send()
        .await
        .map_err(|e| AppError::ServiceUnavailable(format!("Device auth request failed: {}", e)))?
        .json()
        .await
        .map_err(|e| AppError::ServiceUnavailable(format!("Device auth parse failed: {}", e)))?;

    let session_id = Uuid::new_v4().to_string();
    let device_code = auth_resp.device_code.clone();
    let raw_url = auth_resp.verification_uri_complete;
    let url = if raw_url.starts_with("http") {
        raw_url
    } else {
        format!("https://{}", raw_url)
    };

    {
        let mut sessions = state.setup_sessions.write().await;
        sessions.insert(
            session_id.clone(),
            SetupSession {
                device_code: device_code.clone(),
                status: SetupStatus::Pending,
            },
        );
    }

    let sessions = state.setup_sessions.clone();
    let sid = session_id.clone();
    let am = state.account_manager.clone();
    let hc = http_client.clone();

    tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
        loop {
            if tokio::time::Instant::now() > deadline {
                let mut sessions = sessions.write().await;
                if let Some(session) = sessions.get_mut(&sid) {
                    session.status = SetupStatus::Error("Timed out waiting for authorization".into());
                }
                break;
            }

            tokio::time::sleep(Duration::from_secs(5)).await;

            let res = match hc
                .post("https://auth.tidal.com/v1/oauth2/token")
                .form(&[
                    ("client_id", AUTH_CLIENT_ID),
                    ("scope", "r_usr+w_usr+w_sub"),
                    ("device_code", &device_code),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .basic_auth(AUTH_CLIENT_ID, Some(AUTH_CLIENT_SECRET))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let mut sessions = sessions.write().await;
                    if let Some(session) = sessions.get_mut(&sid) {
                        session.status = SetupStatus::Error(format!("Poll failed: {}", e));
                    }
                    break;
                }
            };

            if !res.status().is_success() {
                continue;
            }

            let token_resp: TokenResponse = match res.json().await {
                Ok(t) => t,
                Err(e) => {
                    let mut sessions = sessions.write().await;
                    if let Some(session) = sessions.get_mut(&sid) {
                        session.status = SetupStatus::Error(format!("Parse failed: {}", e));
                    }
                    break;
                }
            };

            let user_id = token_resp.user.user_id.clone();
            let label = format!("Tidal Account ({})", user_id);

            match am
                .add_account(
                    label.clone(),
                    REQUEST_CLIENT_ID.to_string(),
                    REQUEST_CLIENT_SECRET.to_string(),
                    token_resp.refresh_token,
                    Some(user_id.clone()),
                )
                .await
            {
                Ok(acc) => {
                    let mut sessions = sessions.write().await;
                    if let Some(session) = sessions.get_mut(&sid) {
                        session.status = SetupStatus::Complete {
                            account_id: acc.id.clone(),
                            label,
                            user_id,
                        };
                    }
                }
                Err(e) => {
                    let mut sessions = sessions.write().await;
                    if let Some(session) = sessions.get_mut(&sid) {
                        session.status = SetupStatus::Error(format!("Add failed: {}", e));
                    }
                }
            }
            break;
        }
    });

    Ok(Json(json!({
        "session_id": session_id,
        "verification_uri": url
    })))
}

pub async fn check_setup(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let sessions = state.setup_sessions.read().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| AppError::NotFound("Session not found".into()))?;

    match &session.status {
        SetupStatus::Pending => Ok(Json(json!({"status": "pending"}))),
        SetupStatus::Complete {
            account_id,
            label,
            user_id,
        } => Ok(Json(json!({
            "status": "complete",
            "account_id": account_id,
            "label": label,
            "user_id": user_id,
        }))),
        SetupStatus::Error(msg) => Ok(Json(json!({
            "status": "error",
            "error": msg,
        }))),
    }
}
