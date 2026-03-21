//! Embedded WebChat UI served at `GET /chat`.
//!
//! Provides a minimal browser-based chat interface that communicates with Rune
//! over the existing WebSocket RPC protocol (`/ws`).

use axum::response::{Html, IntoResponse};

/// The embedded chat HTML page.
const CHAT_HTML: &str = include_str!("webchat.html");

/// Serve the WebChat single-page interface.
pub async fn webchat_handler() -> impl IntoResponse {
    Html(CHAT_HTML)
}
