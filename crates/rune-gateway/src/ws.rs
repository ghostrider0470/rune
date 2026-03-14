//! WebSocket endpoint with req/res/event framing for RPC and live event streaming.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use once_cell::sync::Lazy;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::state::{AppState, SessionEvent};
use crate::ws_rpc::RpcDispatcher;

/// Global monotonic sequence counter for event frames (gap detection).
static EVENT_SEQ: AtomicU64 = AtomicU64::new(1);

/// Global monotonic state version, bumped whenever connection-visible state changes.
static STATE_VERSION: Lazy<Arc<AtomicU64>> = Lazy::new(|| Arc::new(AtomicU64::new(1)));

// ── Wire frame types ─────────────────────────────────────────────────────────

/// Inbound frame from a WebSocket client.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum InboundFrame {
    /// RPC request: `{"type": "req", "id": "<uuid>", "method": "<string>", "params": {...}}`
    Req {
        id: String,
        method: String,
        #[serde(default)]
        params: Value,
    },
    /// Legacy subscribe shorthand (backward-compatible).
    Subscribe { session_id: String },
}

/// Outbound response frame.
#[derive(Debug, Serialize)]
struct ResFrame {
    #[serde(rename = "type")]
    frame_type: &'static str,
    id: String,
    ok: bool,
    #[serde(rename = "stateVersion")]
    state_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ResError>,
}

/// Error detail inside a response frame.
#[derive(Debug, Serialize)]
struct ResError {
    code: String,
    message: String,
}

/// Outbound event frame.
#[derive(Debug, Serialize)]
struct EventFrame {
    #[serde(rename = "type")]
    frame_type: &'static str,
    event: String,
    payload: Value,
    seq: u64,
    #[serde(rename = "stateVersion")]
    state_version: u64,
}

// ── Handler ──────────────────────────────────────────────────────────────────

/// `GET /ws` -- upgrade to WebSocket for RPC and live event streaming.
pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    let rx = state.event_tx.subscribe();
    let dispatcher = RpcDispatcher::new(state);
    let state_version = Arc::clone(&STATE_VERSION);
    ws.on_upgrade(move |socket| handle_socket(socket, rx, dispatcher, state_version))
}

/// Per-connection state tracking subscribed sessions.
pub struct ConnState {
    subscribed_sessions: HashSet<String>,
    subscribed_events: HashSet<String>,
    subscribe_all: bool,
}

impl ConnState {
    pub fn new() -> Self {
        Self {
            subscribed_sessions: HashSet::new(),
            subscribed_events: HashSet::new(),
            subscribe_all: false,
        }
    }

    fn subscribes_to(&self, event: &SessionEvent) -> bool {
        self.subscribe_all
            || self.subscribed_sessions.contains(&event.session_id)
            || self.subscribed_events.contains(&event.kind)
    }
}

async fn handle_socket(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<SessionEvent>,
    dispatcher: RpcDispatcher,
    state_version: Arc<AtomicU64>,
) {
    let mut conn = ConnState::new();

    loop {
        tokio::select! {
            // Inbound client message
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(reply) = handle_text_message(&text, &mut conn, &dispatcher, &state_version).await {
                            if socket.send(Message::Text(reply.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => { /* binary/ping/pong -- ignore */ }
                    Some(Err(_)) => break,
                }
            }
            // Outbound broadcast event
            event = rx.recv() => {
                match event {
                    Ok(ev) if conn.subscribes_to(&ev) => {
                        let seq = EVENT_SEQ.fetch_add(1, Ordering::Relaxed);
                        let current_state_version = if ev.state_changed {
                            state_version.fetch_add(1, Ordering::Relaxed) + 1
                        } else {
                            state_version.load(Ordering::Relaxed)
                        };
                        let frame = EventFrame {
                            frame_type: "event",
                            event: ev.kind.clone(),
                            payload: json!({
                                "session_id": ev.session_id,
                                "kind": ev.kind,
                                "data": ev.payload,
                            }),
                            seq,
                            state_version: current_state_version,
                        };
                        let payload = match serde_json::to_string(&frame) {
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
                        // Send a gap notification so the client can re-sync
                        let seq = EVENT_SEQ.fetch_add(1, Ordering::Relaxed);
                        let current_state_version = state_version.load(Ordering::Relaxed);
                        let frame = EventFrame {
                            frame_type: "event",
                            event: "system.lagged".to_string(),
                            payload: json!({ "missed": n }),
                            seq,
                            state_version: current_state_version,
                        };
                        if let Ok(payload) = serde_json::to_string(&frame) {
                            let _ = socket.send(Message::Text(payload.into())).await;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

/// Process a single inbound text message and optionally produce a reply.
pub async fn handle_text_message(
    text: &str,
    conn: &mut ConnState,
    dispatcher: &RpcDispatcher,
    state_version: &Arc<AtomicU64>,
) -> Option<String> {
    // Try to parse as an inbound frame.
    let frame: InboundFrame = match serde_json::from_str(text) {
        Ok(f) => f,
        Err(e) => {
            // If the message doesn't parse at all, send an error response with a
            // synthetic id so the client can correlate.
            let res = ResFrame {
                frame_type: "res",
                id: "unknown".to_string(),
                ok: false,
                state_version: state_version.load(Ordering::Relaxed),
                payload: None,
                error: Some(ResError {
                    code: "parse_error".to_string(),
                    message: format!("invalid frame: {e}"),
                }),
            };
            return serde_json::to_string(&res).ok();
        }
    };

    match frame {
        InboundFrame::Req { id, method, params } => {
            // Handle subscribe/unsubscribe locally (connection state), delegate the rest.
            match method.as_str() {
                "subscribe" => {
                    let session_id = params
                        .get("session_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let event = params
                        .get("event")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let all = params.get("all").and_then(|v| v.as_bool()).unwrap_or(false);

                    if session_id.is_none() && event.is_none() && !all {
                        return Some(encode_res(
                            &id,
                            false,
                            None,
                            Some(("bad_request", "missing subscription target: session_id, event, or all")),
                            state_version.load(Ordering::Relaxed),
                        ));
                    }

                    if let Some(session_id) = session_id.as_ref() {
                        conn.subscribed_sessions.insert(session_id.clone());
                        debug!(session_id = %session_id, "ws client subscribed to session via RPC");
                    }
                    if let Some(event) = event.as_ref() {
                        conn.subscribed_events.insert(event.clone());
                        debug!(event = %event, "ws client subscribed to event via RPC");
                    }
                    if all {
                        conn.subscribe_all = true;
                        debug!("ws client subscribed to all events via RPC");
                    }

                    let current_state_version = STATE_VERSION.fetch_add(1, Ordering::Relaxed) + 1;
                    Some(encode_res(
                        &id,
                        true,
                        Some(json!({
                            "subscribed": {
                                "session_id": session_id,
                                "event": event,
                                "all": all,
                            }
                        })),
                        None,
                        current_state_version,
                    ))
                }
                "unsubscribe" => {
                    let session_id = params
                        .get("session_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let event = params
                        .get("event")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let all = params.get("all").and_then(|v| v.as_bool()).unwrap_or(false);

                    if session_id.is_none() && event.is_none() && !all {
                        return Some(encode_res(
                            &id,
                            false,
                            None,
                            Some(("bad_request", "missing subscription target: session_id, event, or all")),
                            state_version.load(Ordering::Relaxed),
                        ));
                    }

                    if let Some(session_id) = session_id.as_ref() {
                        conn.subscribed_sessions.remove(session_id);
                        debug!(session_id = %session_id, "ws client unsubscribed from session via RPC");
                    }
                    if let Some(event) = event.as_ref() {
                        conn.subscribed_events.remove(event);
                        debug!(event = %event, "ws client unsubscribed from event via RPC");
                    }
                    if all {
                        conn.subscribe_all = false;
                        debug!("ws client unsubscribed from all events via RPC");
                    }

                    let current_state_version = STATE_VERSION.fetch_add(1, Ordering::Relaxed) + 1;
                    Some(encode_res(
                        &id,
                        true,
                        Some(json!({
                            "unsubscribed": {
                                "session_id": session_id,
                                "event": event,
                                "all": all,
                            }
                        })),
                        None,
                        current_state_version,
                    ))
                }
                _ => {
                    // Delegate to the RPC dispatcher.
                    match dispatcher.dispatch(&method, params).await {
                        Ok(payload) => Some(encode_res(
                            &id,
                            true,
                            Some(payload),
                            None,
                            state_version.load(Ordering::Relaxed),
                        )),
                        Err(rpc_err) => Some(encode_res(
                            &id,
                            false,
                            None,
                            Some((&rpc_err.code, &rpc_err.message)),
                            state_version.load(Ordering::Relaxed),
                        )),
                    }
                }
            }
        }
        InboundFrame::Subscribe { session_id } => {
            // Backward-compatible legacy subscribe command.
            conn.subscribed_sessions.insert(session_id.clone());
            debug!(session_id = %session_id, "ws client subscribed (legacy)");
            // Legacy clients don't expect a response frame, but we send one for
            // consistency. They can ignore it.
            let current_state_version = STATE_VERSION.fetch_add(1, Ordering::Relaxed) + 1;
            let res = ResFrame {
                frame_type: "res",
                id: "legacy".to_string(),
                ok: true,
                state_version: current_state_version,
                payload: Some(json!({ "subscribed": session_id })),
                error: None,
            };
            serde_json::to_string(&res).ok()
        }
    }
}

/// Encode a response frame as a JSON string.
fn encode_res(
    id: &str,
    ok: bool,
    payload: Option<Value>,
    error: Option<(&str, &str)>,
    state_version: u64,
) -> String {
    let frame = ResFrame {
        frame_type: "res",
        id: id.to_string(),
        ok,
        state_version,
        payload,
        error: error.map(|(code, message)| ResError {
            code: code.to_string(),
            message: message.to_string(),
        }),
    };
    // Serialization of simple structs should not fail; unwrap_or provides a safe fallback.
    serde_json::to_string(&frame).unwrap_or_else(|_| {
        format!(
            r#"{{"type":"res","id":"error","ok":false,"stateVersion":{},"error":{{"code":"internal","message":"serialization failed"}}}}"#,
            state_version
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as JsonValue;

    #[test]
    fn encode_res_includes_state_version() {
        let encoded = encode_res("abc", true, Some(json!({"ok": true})), None, 7);
        let decoded: JsonValue = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded["type"], "res");
        assert_eq!(decoded["id"], "abc");
        assert_eq!(decoded["ok"], true);
        assert_eq!(decoded["stateVersion"], 7);
        assert_eq!(decoded["payload"]["ok"], true);
    }

    #[test]
    fn conn_state_matches_event_filters() {
        let mut conn = ConnState::new();
        let event = SessionEvent {
            session_id: "sess-1".to_string(),
            kind: "turn_completed".to_string(),
            payload: json!({"ok": true}),
            state_changed: true,
        };

        assert!(!conn.subscribes_to(&event));

        conn.subscribed_sessions.insert("sess-1".to_string());
        assert!(conn.subscribes_to(&event));

        conn.subscribed_sessions.clear();
        conn.subscribed_events.insert("turn_completed".to_string());
        assert!(conn.subscribes_to(&event));
    }
}
