use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use tracing::info;

use crate::account_manager::AccountManager;
use crate::error::AppError;

const AUTH_CLIENT_ID: &str = "fX2JxdmntZWK0ixT";
const AUTH_CLIENT_SECRET: &str = "1Nm5AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg=";
const REQUEST_CLIENT_ID: &str = "lw3vR6GE1vtNBsjv";
const REQUEST_CLIENT_SECRET: &str = "Y8tIpqKJxs9BEIwYr0I9bSbMWDsogXJx9LaN3mCHwD4%3D";

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

fn de_string_or_int<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    struct S;
    impl Visitor<'_> for S {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string or integer")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_owned())
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> {
            Ok(v)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }
    }
    d.deserialize_any(S)
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_in: i64,
    #[serde(default)]
    token_type: String,
    #[serde(default)]
    user: Option<UserInfo>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct UserInfo {
    #[serde(rename = "userId", deserialize_with = "de_string_or_int")]
    user_id: String,
    #[serde(default)]
    country_code: String,
}

pub async fn run_setup(
    account_manager: &AccountManager,
    http_client: &Client,
) -> Result<(), AppError> {
    let auth_resp: DeviceAuthorization = http_client
        .post("https://auth.tidal.com/v1/oauth2/device_authorization")
        .form(&[("client_id", AUTH_CLIENT_ID), ("scope", "r_usr+w_usr+w_sub")])
        .send()
        .await
        .map_err(|e| AppError::ServiceUnavailable(format!("Device auth request failed: {}", e)))?
        .json()
        .await
        .map_err(|e| AppError::ServiceUnavailable(format!("Device auth parse failed: {}", e)))?;

    let url = &auth_resp.verification_uri_complete;
    let device_code = &auth_resp.device_code;

    let full_url = if url.starts_with("http") {
        url.clone()
    } else {
        format!("https://{}", url)
    };

    info!("========================================================");
    info!("  TIDAL AUTHORIZATION REQUIRED");
    info!("  Open this URL in your browser to authorize:");
    info!("  {}", full_url);
    info!("========================================================");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);

    let token_resp: TokenResponse = loop {
        if tokio::time::Instant::now() > deadline {
            return Err(AppError::Timeout);
        }

        tokio::time::sleep(Duration::from_secs(5)).await;

        let res = http_client
            .post("https://auth.tidal.com/v1/oauth2/token")
            .form(&[
                ("client_id", AUTH_CLIENT_ID),
                ("scope", "r_usr+w_usr+w_sub"),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .basic_auth(AUTH_CLIENT_ID, Some(AUTH_CLIENT_SECRET))
            .send()
            .await
            .map_err(|e| AppError::ServiceUnavailable(format!("Token poll failed: {}", e)))?;

        if res.status().is_success() {
            break res
                .json()
                .await
                .map_err(|e| AppError::ServiceUnavailable(format!("Token parse failed: {}", e)))?;
        }
    };

    let refresh_token = token_resp.refresh_token;
    let user_id = token_resp
        .user
        .map(|u| u.user_id)
        .unwrap_or_else(|| "unknown".into());

    account_manager
        .add_account(
            format!("Tidal Account ({})", user_id),
            REQUEST_CLIENT_ID.to_string(),
            REQUEST_CLIENT_SECRET.to_string(),
            refresh_token,
            Some(user_id.to_string()),
        )
        .await?;

    info!("Tidal account {} added successfully", user_id);
    Ok(())
}
