//! Embedded WebChat UI served at `GET /webchat`.
//!
//! Provides a minimal browser-based chat interface that communicates with Rune
//! over the existing WebSocket RPC protocol (`/ws`).

use axum::response::{Html, IntoResponse, Redirect};

/// The embedded chat HTML page.
const CHAT_HTML: &str = include_str!("webchat.html");


/// Redirect legacy `/chat` traffic into the embedded WebChat entrypoint.
pub async fn legacy_chat_redirect() -> impl IntoResponse {
    Redirect::temporary("/webchat")
}

/// Serve the WebChat single-page interface.
pub async fn webchat_handler() -> impl IntoResponse {
    Html(CHAT_HTML)
}
