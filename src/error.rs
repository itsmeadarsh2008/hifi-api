use std::fmt;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, Clone)]
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    UpstreamError(StatusCode, String),
    Timeout,
    ServiceUnavailable(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, detail) = match self {
            AppError::NotFound(d) => (StatusCode::NOT_FOUND, d),
            AppError::BadRequest(d) => (StatusCode::BAD_REQUEST, d),
            AppError::Unauthorized(d) => (StatusCode::UNAUTHORIZED, d),
            AppError::UpstreamError(s, d) => (s, d),
            AppError::Timeout => (StatusCode::TOO_MANY_REQUESTS, "Upstream timeout".into()),
            AppError::ServiceUnavailable(d) => (StatusCode::SERVICE_UNAVAILABLE, d),
            AppError::Internal(d) => (StatusCode::INTERNAL_SERVER_ERROR, d),
        };

        (status, Json(json!({"detail": detail}))).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!("Database error: {:?}", e);
        AppError::Internal("Database error".into())
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NotFound(d) => write!(f, "Not found: {}", d),
            AppError::BadRequest(d) => write!(f, "Bad request: {}", d),
            AppError::Unauthorized(d) => write!(f, "Unauthorized: {}", d),
            AppError::UpstreamError(s, d) => write!(f, "Upstream {}: {}", s, d),
            AppError::Timeout => write!(f, "Timeout"),
            AppError::ServiceUnavailable(d) => write!(f, "Service unavailable: {}", d),
            AppError::Internal(d) => write!(f, "Internal error: {}", d),
        }
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            AppError::Timeout
        } else if e.is_connect() {
            AppError::ServiceUnavailable("Connection error to Tidal".into())
        } else if let Some(status) = e.status() {
            AppError::UpstreamError(status, format!("Upstream API error: {}", status))
        } else {
            AppError::Internal(format!("Request error: {}", e))
        }
    }
}
