//! WebSocket endpoint for subscribing to session events.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::state::{AppState, SessionEvent};

/// `GET /ws` — upgrade to WebSocket for live session event streaming.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state.event_tx.subscribe()))
}

/// Client-to-server subscribe command.
#[derive(Deserialize)]
struct SubscribeCommand {
    /// `"subscribe"`
    #[serde(rename = "type")]
    _type: String,
    /// Session ID to subscribe to.
    session_id: String,
}

async fn handle_socket(mut socket: WebSocket, mut rx: broadcast::Receiver<SessionEvent>) {
    // Wait for a subscribe command from the client.
    let subscribed_session = match wait_for_subscribe(&mut socket).await {
        Some(id) => id,
        None => return,
    };

    debug!(session_id = %subscribed_session, "ws client subscribed");

    // Fan out matching events.
    loop {
        match rx.recv().await {
            Ok(event) if event.session_id == subscribed_session => {
                let payload = match serde_json::to_string(&event) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
            Ok(_) => { /* different session, skip */ }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!(missed = n, "ws client lagged, dropped events");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn wait_for_subscribe(socket: &mut WebSocket) -> Option<String> {
    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(text) = msg {
            if let Ok(cmd) = serde_json::from_str::<SubscribeCommand>(&text) {
                return Some(cmd.session_id);
            }
        }
    }
    None
}
