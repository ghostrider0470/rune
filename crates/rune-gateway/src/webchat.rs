//! Embedded WebChat UI served at `GET /webchat`.
//!
//! Provides a minimal browser-based chat interface that communicates with Rune
//! over the existing WebSocket RPC protocol (`/ws`).

use axum::{
    extract::Query,
    response::{Html, IntoResponse, Redirect},
};
use std::collections::HashMap;

/// The embedded chat HTML page.
const CHAT_HTML: &str = include_str!("webchat.html");

/// Redirect legacy `/chat` traffic into the embedded WebChat entrypoint.
pub async fn legacy_chat_redirect(
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let mut target = "/webchat".to_string();
    let forwarded = [
        "api_key",
        "auth",
        "session_token",
        "browser_session",
        "session_id",
    ];
    let query = forwarded
        .iter()
        .filter_map(|key| {
            params
                .get(*key)
                .map(|value| format!("{}={}", key, urlencoding::encode(value)))
        })
        .collect::<Vec<_>>()
        .join("&");
    if !query.is_empty() {
        target.push('?');
        target.push_str(&query);
    }
    Redirect::temporary(&target)
}

/// Serve the WebChat single-page interface.
pub async fn webchat_handler() -> impl IntoResponse {
    Html(CHAT_HTML)
}
