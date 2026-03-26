//! Bearer-token authentication middleware.

use std::sync::Arc;

use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;

use crate::error::GatewayError;
use crate::pairing::DeviceRegistry;

/// Middleware that validates a `Bearer` token against either:
/// - the configured gateway auth token, or
/// - a paired device token issued by the device registry.
///
/// If no gateway auth token is configured, all requests are allowed through.
pub async fn bearer_auth(
    request: Request,
    next: Next,
    expected_token: Option<String>,
    device_registry: Arc<DeviceRegistry>,
) -> Result<Response, GatewayError> {
    let Some(expected) = expected_token else {
        return Ok(next.run(request).await);
    };

    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    let query_token = request.uri().query().and_then(extract_query_bearer_token);
    let websocket_protocol_token = request
        .headers()
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .and_then(extract_websocket_protocol_bearer_token);

    let Some(token) = auth_header.or(query_token).or(websocket_protocol_token) else {
        return Err(GatewayError::Unauthorized);
    };

    if token == expected || device_registry.validate_token(token).await.is_some() {
        Ok(next.run(request).await)
    } else {
        Err(GatewayError::Unauthorized)
    }
}

fn extract_query_bearer_token(query: &str) -> Option<&str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(key, value)| match key {
            "api_key" | "auth" | "session_token" => Some(value),
            _ => None,
        })
}

fn extract_websocket_protocol_bearer_token(value: &str) -> Option<&str> {
    value
        .split(',')
        .map(str::trim)
        .find_map(|protocol| protocol.strip_prefix("Bearer "))
}

#[cfg(test)]
mod tests {

    #[test]
    fn extracts_session_token_query_alias() {
        assert_eq!(
            super::extract_query_bearer_token("session_token=browser-secret"),
            Some("browser-secret")
        );
    }

    #[test]
    fn extracts_websocket_protocol_bearer_token() {
        assert_eq!(
            super::extract_websocket_protocol_bearer_token("rune-ws, Bearer shared-secret"),
            Some("shared-secret")
        );
    }

    #[test]
    fn ignores_non_bearer_websocket_protocol_values() {
        assert_eq!(
            super::extract_websocket_protocol_bearer_token("rune-ws, chat"),
            None
        );
    }
}
