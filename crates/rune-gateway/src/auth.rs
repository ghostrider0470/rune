//! Bearer-token authentication middleware.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

/// Middleware that validates a `Bearer` token against the configured auth token.
///
/// If no auth token is configured, all requests are allowed through.
pub async fn bearer_auth(
    request: Request,
    next: Next,
    expected_token: Option<String>,
) -> Result<Response, StatusCode> {
    let Some(expected) = expected_token else {
        // No auth configured — pass through.
        return Ok(next.run(request).await);
    };

    let auth_header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(value) if value.strip_prefix("Bearer ").is_some_and(|t| t == expected) => {
            Ok(next.run(request).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
