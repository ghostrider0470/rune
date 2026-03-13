//! Bearer-token authentication middleware.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::error::GatewayError;

/// Middleware that validates a `Bearer` token against the configured auth token.
///
/// If no auth token is configured, all requests are allowed through.
pub async fn bearer_auth(
    request: Request,
    next: Next,
    expected_token: Option<String>,
) -> Result<Response, GatewayError> {
    let Some(expected) = expected_token else {
        return Ok(next.run(request).await);
    };

    let auth_header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());

    match auth_header {
        Some(value)
            if value
                .strip_prefix("Bearer ")
                .is_some_and(|token| token == expected) =>
        {
            Ok(next.run(request).await)
        }
        _ => Err(GatewayError::Unauthorized),
    }
}
