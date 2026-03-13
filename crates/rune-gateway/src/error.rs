//! Gateway-specific error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

/// Stable JSON error body for HTTP clients.
#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
    retriable: bool,
    approval_required: bool,
    request_id: Uuid,
}

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
        let (status, code, retriable, approval_required, message) = match &self {
            Self::SessionNotFound(_) => (
                StatusCode::NOT_FOUND,
                "session_not_found",
                false,
                false,
                self.to_string(),
            ),
            Self::BadRequest(_) => (
                StatusCode::BAD_REQUEST,
                "bad_request",
                false,
                false,
                self.to_string(),
            ),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                false,
                false,
                self.to_string(),
            ),
            Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                true,
                false,
                self.to_string(),
            ),
        };

        let body = axum::Json(ErrorBody {
            code,
            message,
            retriable,
            approval_required,
            request_id: Uuid::now_v7(),
        });
        (status, body).into_response()
    }
}
