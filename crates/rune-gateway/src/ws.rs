//! WebSocket endpoint with req/res/event framing for RPC and live event streaming.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use once_cell::sync::Lazy;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::state::{AppState, SessionEvent};
use crate::ws_rpc::{RpcDispatch, RpcDispatcher};

/// Global monotonic sequence counter for event frames (gap detection).
static EVENT_SEQ: AtomicU64 = AtomicU64::new(1);

/// Global monotonic state version, bumped whenever connection-visible state changes.
static STATE_VERSION: Lazy<Arc<AtomicU64>> = Lazy::new(|| Arc::new(AtomicU64::new(1)));
/// Number of active upgraded WebSocket connections.
static ACTIVE_WS_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

fn next_state_version(state_version: &AtomicU64) -> u64 {
    state_version.fetch_add(1, Ordering::Relaxed) + 1
}

fn current_state_version(state_version: &AtomicU64) -> u64 {
    state_version.load(Ordering::Relaxed)
}

pub fn active_ws_connections() -> usize {
    ACTIVE_WS_CONNECTIONS.load(Ordering::Relaxed)
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
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
#[derive(Default)]
pub struct ConnState {
    subscribed_sessions: HashSet<String>,
    subscribed_events: HashSet<String>,
    subscribe_all: bool,
}

impl ConnState {
    pub fn new() -> Self {
        Self::default()
    }

    fn subscribes_to(&self, event: &SessionEvent) -> bool {
        self.subscribe_all
            || self.subscribed_sessions.contains(&event.session_id)
            || self.subscribed_events.contains(&event.kind)
            || self.subscribed_events.iter().any(|sub| {
                // Family-prefix match: subscribing to "turn" matches "turn.started" etc.
                !sub.contains('.')
                    && event.kind.starts_with(sub.as_str())
                    && event.kind.as_bytes().get(sub.len()) == Some(&b'.')
            })
    }
}

struct ActiveConnectionGuard;

impl ActiveConnectionGuard {
    fn new() -> Self {
        ACTIVE_WS_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        ACTIVE_WS_CONNECTIONS.fetch_sub(1, Ordering::Relaxed);
    }
}

async fn handle_socket<D>(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<SessionEvent>,
    dispatcher: D,
    state_version: Arc<AtomicU64>,
) where
    D: RpcDispatch,
{
    let _connection_guard = ActiveConnectionGuard::new();
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
                            next_state_version(state_version.as_ref())
                        } else {
                            current_state_version(state_version.as_ref())
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
                        let current_state_version = current_state_version(state_version.as_ref());
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
pub async fn handle_text_message<D>(
    text: &str,
    conn: &mut ConnState,
    dispatcher: &D,
    state_version: &Arc<AtomicU64>,
) -> Option<String>
where
    D: RpcDispatch,
{
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
                state_version: current_state_version(state_version.as_ref()),
                payload: None,
                error: Some(ResError {
                    code: "parse_error".to_string(),
                    message: format!("invalid frame: {e}"),
                    data: None,
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
                            Some((
                                "bad_request",
                                "missing subscription target: session_id, event, or all",
                                None,
                            )),
                            current_state_version(state_version.as_ref()),
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

                    let current_state_version = next_state_version(state_version.as_ref());
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
                            Some((
                                "bad_request",
                                "missing subscription target: session_id, event, or all",
                                None,
                            )),
                            current_state_version(state_version.as_ref()),
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

                    let current_state_version = next_state_version(state_version.as_ref());
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
                            current_state_version(state_version.as_ref()),
                        )),
                        Err(rpc_err) => Some(encode_res(
                            &id,
                            false,
                            None,
                            Some((&rpc_err.code, &rpc_err.message, rpc_err.data.as_ref())),
                            current_state_version(state_version.as_ref()),
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
            let current_state_version = next_state_version(state_version.as_ref());
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
    error: Option<(&str, &str, Option<&Value>)>,
    state_version: u64,
) -> String {
    let frame = ResFrame {
        frame_type: "res",
        id: id.to_string(),
        ok,
        state_version,
        payload,
        error: error.map(|(code, message, data)| ResError {
            code: code.to_string(),
            message: message.to_string(),
            data: data.cloned(),
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

    struct StubDispatcher {
        result: Result<Value, crate::ws_rpc::RpcError>,
    }

    #[async_trait::async_trait]
    impl RpcDispatch for StubDispatcher {
        async fn dispatch(
            &self,
            _method: &str,
            _params: Value,
        ) -> Result<Value, crate::ws_rpc::RpcError> {
            self.result.clone()
        }
    }

    fn ok_dispatcher(payload: Value) -> StubDispatcher {
        StubDispatcher {
            result: Ok(payload),
        }
    }

    fn err_dispatcher(code: &str, message: &str) -> StubDispatcher {
        StubDispatcher {
            result: Err(crate::ws_rpc::RpcError {
                code: code.to_string(),
                message: message.to_string(),
                data: None,
            }),
        }
    }

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

    #[test]
    fn conn_state_family_prefix_matches_dotted_event_kinds() {
        let mut conn = ConnState::new();
        conn.subscribed_events.insert("turn".to_string());

        let started = SessionEvent {
            session_id: "sess-1".to_string(),
            kind: "turn.started".to_string(),
            payload: json!({}),
            state_changed: true,
        };
        let completed = SessionEvent {
            session_id: "sess-1".to_string(),
            kind: "turn.completed".to_string(),
            payload: json!({}),
            state_changed: true,
        };
        let tool_event = SessionEvent {
            session_id: "sess-1".to_string(),
            kind: "tool.requested".to_string(),
            payload: json!({}),
            state_changed: true,
        };
        let turnstile_event = SessionEvent {
            session_id: "sess-1".to_string(),
            kind: "turnstile".to_string(),
            payload: json!({}),
            state_changed: false,
        };

        assert!(conn.subscribes_to(&started));
        assert!(conn.subscribes_to(&completed));
        assert!(!conn.subscribes_to(&tool_event));
        // "turn" should NOT match "turnstile" (no dot separator).
        assert!(!conn.subscribes_to(&turnstile_event));
    }

    #[tokio::test]
    async fn subscribe_req_updates_connection_state_and_returns_ack() {
        let mut conn = ConnState::new();
        let dispatcher = ok_dispatcher(json!({"ignored": true}));
        let state_version = Arc::new(AtomicU64::new(41));

        let reply = handle_text_message(
            r#"{"type":"req","id":"1","method":"subscribe","params":{"session_id":"sess-1","event":"turn_completed"}}"#,
            &mut conn,
            &dispatcher,
            &state_version,
        )
        .await
        .unwrap();

        let decoded: JsonValue = serde_json::from_str(&reply).unwrap();
        assert_eq!(decoded["type"], "res");
        assert_eq!(decoded["id"], "1");
        assert_eq!(decoded["ok"], true);
        assert_eq!(decoded["payload"]["subscribed"]["session_id"], "sess-1");
        assert_eq!(decoded["payload"]["subscribed"]["event"], "turn_completed");
        assert_eq!(decoded["stateVersion"], 42);
        assert_eq!(state_version.load(Ordering::Relaxed), 42);
        assert!(conn.subscribed_sessions.contains("sess-1"));
        assert!(conn.subscribed_events.contains("turn_completed"));
    }

    #[tokio::test]
    async fn legacy_subscribe_frame_returns_ack_and_updates_state() {
        let mut conn = ConnState::new();
        let dispatcher = ok_dispatcher(json!({"ignored": true}));
        let state_version = Arc::new(AtomicU64::new(41));

        let reply = handle_text_message(
            r#"{"type":"subscribe","session_id":"sess-legacy"}"#,
            &mut conn,
            &dispatcher,
            &state_version,
        )
        .await
        .unwrap();

        let decoded: JsonValue = serde_json::from_str(&reply).unwrap();
        assert_eq!(decoded["type"], "res");
        assert_eq!(decoded["id"], "legacy");
        assert_eq!(decoded["ok"], true);
        assert_eq!(decoded["payload"]["subscribed"], "sess-legacy");
        assert_eq!(decoded["stateVersion"], 42);
        assert_eq!(state_version.load(Ordering::Relaxed), 42);
        assert!(conn.subscribed_sessions.contains("sess-legacy"));
    }

    #[tokio::test]
    async fn subscribe_all_flag_controls_event_delivery_for_any_session() {
        let mut conn = ConnState::new();
        let dispatcher = ok_dispatcher(json!({"ignored": true}));
        let state_version = Arc::new(AtomicU64::new(50));

        let subscribe_reply = handle_text_message(
            r#"{"type":"req","id":"all-on","method":"subscribe","params":{"all":true}}"#,
            &mut conn,
            &dispatcher,
            &state_version,
        )
        .await
        .unwrap();
        let subscribe_decoded: JsonValue = serde_json::from_str(&subscribe_reply).unwrap();
        assert_eq!(subscribe_decoded["ok"], true);
        assert_eq!(subscribe_decoded["payload"]["subscribed"]["all"], true);
        assert!(conn.subscribe_all);

        let unrelated_event = SessionEvent {
            session_id: "sess-unrelated".to_string(),
            kind: "turn_completed".to_string(),
            payload: json!({"ok": true}),
            state_changed: true,
        };
        assert!(conn.subscribes_to(&unrelated_event));

        let unsubscribe_reply = handle_text_message(
            r#"{"type":"req","id":"all-off","method":"unsubscribe","params":{"all":true}}"#,
            &mut conn,
            &dispatcher,
            &state_version,
        )
        .await
        .unwrap();
        let unsubscribe_decoded: JsonValue = serde_json::from_str(&unsubscribe_reply).unwrap();
        assert_eq!(unsubscribe_decoded["ok"], true);
        assert_eq!(unsubscribe_decoded["payload"]["unsubscribed"]["all"], true);
        assert!(!conn.subscribe_all);
        assert!(!conn.subscribes_to(&unrelated_event));
        assert_eq!(state_version.load(Ordering::Relaxed), 52);
    }

    #[tokio::test]
    async fn unsubscribe_req_updates_connection_state_and_returns_ack() {
        let mut conn = ConnState::new();
        conn.subscribed_sessions.insert("sess-1".to_string());
        conn.subscribed_events.insert("turn_completed".to_string());
        let dispatcher = ok_dispatcher(json!({"ignored": true}));
        let state_version = Arc::new(AtomicU64::new(100));

        let reply = handle_text_message(
            r#"{"type":"req","id":"2","method":"unsubscribe","params":{"session_id":"sess-1","event":"turn_completed"}}"#,
            &mut conn,
            &dispatcher,
            &state_version,
        )
        .await
        .unwrap();

        let decoded: JsonValue = serde_json::from_str(&reply).unwrap();
        assert_eq!(decoded["type"], "res");
        assert_eq!(decoded["id"], "2");
        assert_eq!(decoded["ok"], true);
        assert_eq!(decoded["payload"]["unsubscribed"]["session_id"], "sess-1");
        assert_eq!(
            decoded["payload"]["unsubscribed"]["event"],
            "turn_completed"
        );
        assert_eq!(decoded["stateVersion"], 101);
        assert_eq!(state_version.load(Ordering::Relaxed), 101);
        assert!(!conn.subscribed_sessions.contains("sess-1"));
        assert!(!conn.subscribed_events.contains("turn_completed"));
    }

    #[tokio::test]
    async fn rpc_error_is_encoded_as_error_response() {
        let mut conn = ConnState::new();
        let dispatcher = err_dispatcher("method_not_found", "unknown method: nope");
        let state_version = Arc::new(AtomicU64::new(9));

        let reply = handle_text_message(
            r#"{"type":"req","id":"2","method":"nope","params":{}}"#,
            &mut conn,
            &dispatcher,
            &state_version,
        )
        .await
        .unwrap();

        let decoded: JsonValue = serde_json::from_str(&reply).unwrap();
        assert_eq!(decoded["type"], "res");
        assert_eq!(decoded["id"], "2");
        assert_eq!(decoded["ok"], false);
        assert_eq!(decoded["error"]["code"], "method_not_found");
        assert_eq!(decoded["error"]["message"], "unknown method: nope");
        assert!(decoded["error"]["data"].is_null());
        assert_eq!(decoded["stateVersion"], 9);
        assert_eq!(state_version.load(Ordering::Relaxed), 9);
    }

    #[tokio::test]
    async fn malformed_frame_returns_parse_error_response() {
        let mut conn = ConnState::new();
        let dispatcher = ok_dispatcher(json!({}));
        let state_version = Arc::new(AtomicU64::new(5));

        let reply = handle_text_message("not-json", &mut conn, &dispatcher, &state_version)
            .await
            .unwrap();

        let decoded: JsonValue = serde_json::from_str(&reply).unwrap();
        assert_eq!(decoded["type"], "res");
        assert_eq!(decoded["id"], "unknown");
        assert_eq!(decoded["ok"], false);
        assert_eq!(decoded["error"]["code"], "parse_error");
        assert_eq!(decoded["stateVersion"], 5);
        assert_eq!(state_version.load(Ordering::Relaxed), 5);
    }

    #[tokio::test]
    async fn missing_subscription_target_does_not_bump_state_version() {
        let mut conn = ConnState::new();
        let dispatcher = ok_dispatcher(json!({"ignored": true}));
        let state_version = Arc::new(AtomicU64::new(77));

        let reply = handle_text_message(
            r#"{"type":"req","id":"bad-sub","method":"subscribe","params":{}}"#,
            &mut conn,
            &dispatcher,
            &state_version,
        )
        .await
        .unwrap();

        let decoded: JsonValue = serde_json::from_str(&reply).unwrap();
        assert_eq!(decoded["ok"], false);
        assert_eq!(decoded["error"]["code"], "bad_request");
        assert_eq!(decoded["stateVersion"], 77);
        assert_eq!(state_version.load(Ordering::Relaxed), 77);
    }

    #[tokio::test]
    async fn legacy_subscribe_bumps_state_version_once() {
        let mut conn = ConnState::new();
        let dispatcher = ok_dispatcher(json!({"ignored": true}));
        let state_version = Arc::new(AtomicU64::new(13));

        let reply = handle_text_message(
            r#"{"type":"subscribe","session_id":"legacy-sess"}"#,
            &mut conn,
            &dispatcher,
            &state_version,
        )
        .await
        .unwrap();

        let decoded: JsonValue = serde_json::from_str(&reply).unwrap();
        assert_eq!(decoded["ok"], true);
        assert_eq!(decoded["stateVersion"], 14);
        assert_eq!(state_version.load(Ordering::Relaxed), 14);
        assert!(conn.subscribed_sessions.contains("legacy-sess"));
    }
}
