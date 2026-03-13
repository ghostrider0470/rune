//! Gateway-specific error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

/// Errors that can occur in the gateway layer.
#[derive(Debug, Error)]
pub enum GatewayError {
    /// Session not found.
    #[error("session not found: {0}")]
    SessionNotFound(String),

    /// Invalid request payload.
    #[error("bad request: {0}")]
    BadRequest(String),

    /// Authentication failure.
    #[error("unauthorized")]
    Unauthorized,

    /// Internal error forwarded from runtime or store.
    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::SessionNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = axum::Json(json!({ "error": message }));
        (status, body).into_response()
    }
}
