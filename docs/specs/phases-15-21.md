# Phases 15-21: Implementation Specification

> Generated 2026-03-15. Authoritative reference for implementing phases 15 through 21.
> Every type, endpoint, wire example, error case, and acceptance criterion is defined
> here so that implementation can proceed without guessing.

---

## Table of Contents

1. [Phase 15 — Log Viewer (Backend + UI)](#phase-15--log-viewer-backend--ui)
2. [Phase 16 — Debug Page (UI)](#phase-16--debug-page-ui)
3. [Phase 17 — Agents & Skills Pages (UI)](#phase-17--agents--skills-pages-ui)
4. [Phase 18 — Semantic Browser Snapshots (Backend)](#phase-18--semantic-browser-snapshots-backend)
5. [Phase 19 — A2UI Protocol (Backend + UI)](#phase-19--a2ui-protocol-backend--ui)
6. [Phase 20 — Enhanced Existing Pages (UI)](#phase-20--enhanced-existing-pages-ui)
7. [Phase 21 — UI Polish](#phase-21--ui-polish)

---

## Phase 15 — Log Viewer (Backend + UI)

### 15.1 Overview

Add a ring-buffer tracing subscriber layer in the gateway that captures structured
log records and exposes them over a dedicated WebSocket endpoint (`/ws/logs`). The
existing `ui/src/routes/_admin/logs.tsx` page already connects to `/ws/logs` and
renders the stream — this phase adds the **backend** that the page depends on.

### 15.2 Rust Types

File: `crates/rune-gateway/src/log_layer.rs`

```rust
use std::collections::VecDeque;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};

/// Single structured log record stored in the ring buffer and sent over the wire.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogRecord {
    /// ISO-8601 timestamp of the log event.
    pub timestamp: DateTime<Utc>,
    /// Tracing level: "ERROR", "WARN", "INFO", "DEBUG", "TRACE".
    pub level: String,
    /// Tracing target (module path), e.g. "rune_gateway::routes".
    pub target: String,
    /// The formatted log message.
    pub message: String,
    /// Structured span/event fields as key-value pairs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<serde_json::Value>,
}

/// Configuration for the ring buffer layer.
#[derive(Clone, Debug, Deserialize)]
pub struct LogLayerConfig {
    /// Maximum entries retained in-memory. Default: 5000.
    pub ring_capacity: usize,
    /// Broadcast channel capacity for live subscribers. Default: 512.
    pub broadcast_capacity: usize,
    /// Minimum level to capture. Default: "DEBUG".
    pub min_level: String,
}

impl Default for LogLayerConfig {
    fn default() -> Self {
        Self {
            ring_capacity: 5000,
            broadcast_capacity: 512,
            min_level: "DEBUG".to_string(),
        }
    }
}

/// Shared state backing the log ring buffer + broadcast.
#[derive(Clone)]
pub struct LogBuffer {
    ring: Arc<RwLock<VecDeque<LogRecord>>>,
    capacity: usize,
    tx: broadcast::Sender<LogRecord>,
}

impl LogBuffer {
    pub fn new(config: &LogLayerConfig) -> Self {
        let (tx, _) = broadcast::channel(config.broadcast_capacity);
        Self {
            ring: Arc::new(RwLock::new(VecDeque::with_capacity(config.ring_capacity))),
            capacity: config.ring_capacity,
            tx,
        }
    }

    /// Push a record into the ring buffer and broadcast to live subscribers.
    pub async fn push(&self, record: LogRecord) {
        {
            let mut ring = self.ring.write().await;
            if ring.len() >= self.capacity {
                ring.pop_front();
            }
            ring.push_back(record.clone());
        }
        // Ignore send errors (no receivers).
        let _ = self.tx.send(record);
    }

    /// Snapshot the current ring buffer contents.
    pub async fn snapshot(&self) -> Vec<LogRecord> {
        self.ring.read().await.iter().cloned().collect()
    }

    /// Subscribe to live log records.
    pub fn subscribe(&self) -> broadcast::Receiver<LogRecord> {
        self.tx.subscribe()
    }

    /// Number of records currently buffered.
    pub async fn len(&self) -> usize {
        self.ring.read().await.len()
    }
}
```

File: `crates/rune-gateway/src/log_layer.rs` (tracing layer implementation)

```rust
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Tracing subscriber layer that feeds log records into a [`LogBuffer`].
pub struct RingBufferLayer {
    buffer: LogBuffer,
    min_level: tracing::Level,
    runtime_handle: tokio::runtime::Handle,
}

impl RingBufferLayer {
    pub fn new(buffer: LogBuffer, min_level: tracing::Level) -> Self {
        Self {
            buffer,
            min_level,
            runtime_handle: tokio::runtime::Handle::current(),
        }
    }
}

/// Visitor that collects span/event fields into a `serde_json::Map`.
struct FieldVisitor {
    fields: serde_json::Map<String, serde_json::Value>,
    message: Option<String>,
}

impl FieldVisitor {
    fn new() -> Self {
        Self {
            fields: serde_json::Map::new(),
            message: None,
        }
    }
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let val = format!("{:?}", value);
        if field.name() == "message" {
            self.message = Some(val);
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::String(val));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Bool(value),
        );
    }
}

impl<S> Layer<S> for RingBufferLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().level() > &self.min_level {
            return;
        }

        let mut visitor = FieldVisitor::new();
        event.record(&mut visitor);

        let record = LogRecord {
            timestamp: chrono::Utc::now(),
            level: event.metadata().level().to_string().to_uppercase(),
            target: event.metadata().target().to_string(),
            message: visitor.message.unwrap_or_default(),
            fields: if visitor.fields.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(visitor.fields))
            },
        };

        let buffer = self.buffer.clone();
        self.runtime_handle.spawn(async move {
            buffer.push(record).await;
        });
    }
}
```

### 15.3 AppState Changes

File: `crates/rune-gateway/src/state.rs`

Add one field to `AppState`:

```rust
/// Ring-buffer log store for the /ws/logs endpoint.
pub log_buffer: Arc<LogBuffer>,
```

### 15.4 WebSocket Endpoint: `/ws/logs`

File: `crates/rune-gateway/src/ws_logs.rs`

```rust
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;

use crate::log_layer::LogBuffer;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct LogStreamQuery {
    /// If true, replay the current ring buffer snapshot before streaming live.
    #[serde(default)]
    pub replay: bool,
    /// Optional minimum level filter: "ERROR", "WARN", "INFO", "DEBUG", "TRACE".
    pub level: Option<String>,
}

pub async fn ws_logs_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<LogStreamQuery>,
) -> Response {
    let buffer = Arc::clone(&state.log_buffer);
    ws.on_upgrade(move |socket| handle_log_socket(socket, buffer, query))
}

async fn handle_log_socket(
    mut socket: WebSocket,
    buffer: Arc<LogBuffer>,
    query: LogStreamQuery,
) {
    // ...replay + live stream implementation...
}
```

### 15.5 HTTP Endpoint: `GET /api/logs/snapshot`

Returns the current ring buffer as a JSON array for non-WebSocket consumers.

```rust
/// Response for GET /api/logs/snapshot.
#[derive(Serialize)]
pub struct LogSnapshotResponse {
    /// Total records in the ring buffer.
    pub total: usize,
    /// Records matching the query (after level/search filter).
    pub records: Vec<LogRecord>,
}

/// Query parameters for GET /api/logs/snapshot.
#[derive(Debug, Deserialize)]
pub struct LogSnapshotQuery {
    /// Minimum level: "ERROR", "WARN", "INFO", "DEBUG", "TRACE". Default: all.
    pub level: Option<String>,
    /// Substring search across message, target, and fields.
    pub search: Option<String>,
    /// Maximum records to return. Default: 500, max: 5000.
    pub limit: Option<usize>,
    /// Offset for pagination. Default: 0.
    pub offset: Option<usize>,
}

pub async fn logs_snapshot(
    State(state): State<AppState>,
    Query(query): Query<LogSnapshotQuery>,
) -> Result<Json<LogSnapshotResponse>, GatewayError> { ... }
```

### 15.6 Wire Protocol

#### WebSocket `/ws/logs` — Outbound Frame (server to client)

Each message is a single JSON object:

```json
{
  "timestamp": "2026-03-15T14:30:22.456Z",
  "level": "INFO",
  "target": "rune_gateway::routes",
  "message": "Session created",
  "fields": {
    "session_id": "01953a2b-...",
    "kind": "interactive"
  }
}
```

#### WebSocket `/ws/logs` — Inbound Frame (client to server, optional)

The client may send filter commands to adjust the stream server-side:

```json
{
  "type": "set_filter",
  "level": "WARN"
}
```

```json
{
  "type": "set_filter",
  "level": null
}
```

Response acknowledged with:

```json
{
  "type": "filter_ack",
  "level": "WARN"
}
```

#### HTTP `GET /api/logs/snapshot?level=ERROR&limit=100`

```json
{
  "total": 4823,
  "records": [
    {
      "timestamp": "2026-03-15T14:29:01.001Z",
      "level": "ERROR",
      "target": "rune_models::provider::anthropic",
      "message": "API rate limit exceeded",
      "fields": { "status": 429, "retry_after_ms": 5000 }
    }
  ]
}
```

### 15.7 Error Cases

| Condition | HTTP Status / WS Behavior | Error Code | Message |
|---|---|---|---|
| Invalid `level` query param | 400 | `bad_request` | `"invalid log level: must be ERROR, WARN, INFO, DEBUG, or TRACE"` |
| `limit` exceeds 5000 | 400 | `bad_request` | `"limit must not exceed 5000"` |
| Log buffer not initialized | 500 | `internal_error` | `"log buffer unavailable"` |
| WebSocket upgrade fails | Connection dropped | N/A | Client sees `onclose` |
| Broadcast receiver lag (too slow consumer) | Dropped messages | N/A | Gap in sequence; client can request `/api/logs/snapshot` to catch up |

### 15.8 Edge Cases

- **Empty ring buffer**: `/api/logs/snapshot` returns `{"total": 0, "records": []}`. WebSocket sends nothing until new records arrive.
- **Ring buffer wrap**: Oldest entries silently discarded. No notification to clients.
- **Reconnection**: Client (logs.tsx) already implements exponential backoff reconnection with `RECONNECT_DELAY_MS = 3000`. On reconnect, if `replay=true` was set, the snapshot replays again, which may produce duplicate entries. Client should deduplicate by timestamp or accept duplicates (current UI appends without dedup).
- **Large fields**: Fields object is unbounded. Tracing events with large debug output (e.g., full request bodies) are captured as-is. The ring buffer config `ring_capacity` bounds total count, not byte size. Future enhancement: add a `max_field_bytes` config option.
- **High throughput**: At TRACE level in a busy system, the broadcast channel may lag. The `broadcast::Receiver` returns `RecvError::Lagged(n)` — the WebSocket handler should log this and continue from the latest record.

### 15.9 TypeScript Types

File: `ui/src/lib/api-types.ts` (additions)

```typescript
// Log Viewer
export interface LogRecord {
  timestamp: string;
  level: string;
  target: string;
  message: string;
  fields?: Record<string, unknown>;
}

export interface LogSnapshotResponse {
  total: number;
  records: LogRecord[];
}

export interface LogStreamFilter {
  type: "set_filter";
  level: string | null;
}

export interface LogFilterAck {
  type: "filter_ack";
  level: string | null;
}
```

### 15.10 Integration Test Scenarios

File: `crates/rune-gateway/tests/log_tests.rs`

```rust
/// Verify that the ring buffer captures and replays records.
#[tokio::test]
async fn test_log_buffer_push_and_snapshot() { ... }

/// Verify that the ring buffer evicts oldest entries when capacity is reached.
#[tokio::test]
async fn test_log_buffer_ring_eviction() { ... }

/// Verify that broadcast subscribers receive live records.
#[tokio::test]
async fn test_log_buffer_broadcast() { ... }

/// Verify that the HTTP snapshot endpoint returns filtered records.
#[tokio::test]
async fn test_logs_snapshot_endpoint_level_filter() { ... }

/// Verify that invalid level query param returns 400.
#[tokio::test]
async fn test_logs_snapshot_invalid_level() { ... }

/// Verify that limit > 5000 returns 400.
#[tokio::test]
async fn test_logs_snapshot_limit_exceeds_max() { ... }

/// Verify the WebSocket /ws/logs endpoint delivers records in real time.
#[tokio::test]
async fn test_ws_logs_live_stream() { ... }

/// Verify that replay=true sends the snapshot before live records.
#[tokio::test]
async fn test_ws_logs_replay_then_live() { ... }

/// Verify that the set_filter inbound frame narrows the stream.
#[tokio::test]
async fn test_ws_logs_set_filter() { ... }
```

### 15.11 Config Additions

File: `crates/rune-config/src/lib.rs` — add to `AppConfig`:

```rust
/// Log viewer ring buffer configuration.
#[serde(default)]
pub log_viewer: LogViewerConfig,
```

```rust
#[derive(Clone, Debug, Deserialize)]
pub struct LogViewerConfig {
    /// Maximum number of log records in the ring buffer. Default: 5000.
    #[serde(default = "default_ring_capacity")]
    pub ring_capacity: usize,
    /// Broadcast channel capacity for live WebSocket subscribers. Default: 512.
    #[serde(default = "default_broadcast_capacity")]
    pub broadcast_capacity: usize,
    /// Minimum tracing level captured. Default: "DEBUG".
    #[serde(default = "default_min_level")]
    pub min_level: String,
}
```

Env var overrides: `RUNE_LOG_VIEWER__RING_CAPACITY`, `RUNE_LOG_VIEWER__BROADCAST_CAPACITY`, `RUNE_LOG_VIEWER__MIN_LEVEL`.

### 15.12 Acceptance Criteria

- [ ] `LogBuffer` ring buffer stores up to `ring_capacity` `LogRecord` entries
- [ ] `RingBufferLayer` captures tracing events at or above `min_level` and pushes them into `LogBuffer`
- [ ] `GET /ws/logs` upgrades to WebSocket and streams `LogRecord` JSON objects
- [ ] `GET /ws/logs?replay=true` replays buffered entries before switching to live
- [ ] Client can send `{"type": "set_filter", "level": "ERROR"}` to narrow the stream
- [ ] `GET /api/logs/snapshot` returns `LogSnapshotResponse` with level/search/limit/offset filtering
- [ ] Existing `ui/src/routes/_admin/logs.tsx` connects and renders the stream without code changes
- [ ] Ring buffer evicts oldest entries when full — no unbounded memory growth
- [ ] Broadcast lag is handled gracefully (dropped frames, no crash)
- [ ] All 9 integration tests pass

### 15.13 Dependencies

**Rust (existing workspace deps, no new crates needed)**:
- `tracing = "0.1"` (already in workspace)
- `tracing-subscriber = "0.3"` (already in workspace)
- `tokio = "1"` (already in workspace)
- `chrono = "0.4"` (already in workspace)
- `serde = "1"` / `serde_json = "1"` (already in workspace)

**npm** (no new packages):
- Existing `logs.tsx` already works with this wire format

---

## Phase 16 — Debug Page (UI)

### 16.1 Overview

The debug page at `ui/src/routes/_admin/debug.tsx` already exists with three tabs
(Status, API Tester, WebSocket). This phase enhances it with:

1. **Full `/status` JSON tree view** with collapsible sections and value highlighting
2. **`/health` JSON tree view** alongside status
3. **Improved API tester** with response headers display, history, and request presets
4. **WebSocket event log ring buffer** with seq/stateVersion display and gap detection

No new backend endpoints are required. All enhancements are purely UI-side.

### 16.2 TypeScript Types

File: `ui/src/lib/api-types.ts` (additions)

```typescript
// Debug Page
export interface ApiTestHistoryEntry {
  id: string;
  method: string;
  path: string;
  body: string | null;
  status: number;
  statusText: string;
  responseBody: unknown;
  responseHeaders: Record<string, string>;
  latency_ms: number;
  timestamp: string;
}

export interface ApiPreset {
  label: string;
  method: string;
  path: string;
  body?: string;
}

export interface WsEventLogEntry {
  /** Monotonic local index for key stability */
  idx: number;
  /** Raw frame type: "event", "res" */
  frame_type: string;
  /** Event name or method for res frames */
  event_or_method: string;
  /** Full parsed JSON payload */
  payload: unknown;
  /** Server-sent seq number (event frames only) */
  seq: number | null;
  /** Server-sent stateVersion */
  state_version: number | null;
  /** Whether a gap was detected in the seq */
  gap_detected: boolean;
  /** Local receive timestamp */
  received_at: string;
}
```

### 16.3 Component Tree

```
DebugPage
├── PageHeader ("Debug" title + description)
├── Tabs
│   ├── TabsTrigger "Status"
│   ├── TabsTrigger "API Tester"
│   └── TabsTrigger "WebSocket"
│
├── TabsContent "status"
│   ├── div.grid.lg:grid-cols-2
│   │   ├── Card "/health"
│   │   │   └── JsonTreeView { data: health, defaultExpanded: true }
│   │   └── Card "/status"
│   │       └── JsonTreeView { data: status, defaultExpanded: true }
│
├── TabsContent "api"
│   ├── Card "Request"
│   │   ├── div.flex
│   │   │   ├── Select method (GET/POST/PUT/DELETE/PATCH)
│   │   │   ├── Input path
│   │   │   └── Button "Send"
│   │   ├── Textarea body (shown for POST/PUT/PATCH)
│   │   └── PresetSelector { presets: API_PRESETS, onSelect }
│   │
│   ├── Card "Response" (shown when result exists)
│   │   ├── div.flex (status badge + latency badge + copy button)
│   │   ├── Tabs
│   │   │   ├── TabsContent "Body"
│   │   │   │   └── JsonTreeView { data: result.body }
│   │   │   └── TabsContent "Headers"
│   │   │       └── HeaderTable { headers: result.headers }
│   │   └── (empty)
│   │
│   └── Card "History" (collapsible)
│       └── ApiTestHistoryList { entries, onReplay, onClear }
│
└── TabsContent "ws"
    └── Card "WebSocket Events"
        ├── div.flex (session ID input + subscribe-all toggle + clear button + connected badge)
        ├── WsStatsBar { totalEvents, gaps, latestSeq, latestStateVersion }
        └── WsEventLogList { entries: WsEventLogEntry[], maxEntries: 500 }
            └── WsEventLogRow { entry }
                ├── Badge frame_type
                ├── Badge event_or_method
                ├── span seq (orange if gap_detected)
                ├── span stateVersion
                └── pre payload (collapsed by default, click to expand)
```

### 16.4 New Shared Component: `JsonTreeView`

File: `ui/src/components/ui/json-tree-view.tsx`

```typescript
interface JsonTreeViewProps {
  data: unknown;
  /** If true, the root object is expanded on mount. Default: false. */
  defaultExpanded?: boolean;
  /** Max depth to auto-expand. Default: 2. */
  autoExpandDepth?: number;
  /** Classname applied to the root element. */
  className?: string;
}
```

Renders a recursive collapsible tree of JSON data. Leaf values are color-coded:
- Strings: green
- Numbers: blue
- Booleans: purple
- Null: gray italic

### 16.5 API Presets

Hard-coded in `debug.tsx`:

```typescript
const API_PRESETS: ApiPreset[] = [
  { label: "Health",       method: "GET",  path: "/health" },
  { label: "Status",       method: "GET",  path: "/status" },
  { label: "Sessions",     method: "GET",  path: "/sessions" },
  { label: "Cron Jobs",    method: "GET",  path: "/cron/jobs" },
  { label: "Agents",       method: "GET",  path: "/agents" },
  { label: "Skills",       method: "GET",  path: "/skills" },
  { label: "Tools",        method: "GET",  path: "/tools" },
  { label: "Reminders",    method: "GET",  path: "/reminders" },
  { label: "Approvals",    method: "GET",  path: "/approvals" },
  { label: "Dashboard",    method: "GET",  path: "/api/dashboard/summary" },
  { label: "Log Snapshot", method: "GET",  path: "/api/logs/snapshot?limit=50" },
];
```

### 16.6 WebSocket Event Log Gap Detection

The client tracks the last seen `seq` value. When a new event arrives with `seq > last_seq + 1`, the entry is marked `gap_detected: true` and the gap count in `WsStatsBar` increments. This indicates the broadcast channel lagged and messages were dropped.

### 16.7 Edge Cases

- **Health/Status endpoints down**: Show error card with retry button, not a crash.
- **API tester non-JSON response**: Display raw text in a `<pre>` block.
- **API tester invalid JSON body input**: Show inline validation error below the textarea.
- **WebSocket disconnect during event logging**: Show "Disconnected" badge, auto-reconnect, and append a synthetic `{frame_type: "system", event_or_method: "reconnected"}` entry.
- **History overflow**: Keep last 50 `ApiTestHistoryEntry` items in React state. Oldest are silently dropped.
- **Large JSON response in tree view**: Collapse nodes beyond depth 3 by default. Nodes with more than 100 keys show a "Show N more..." button.

### 16.8 Integration Test Scenarios

No backend tests needed. UI tests (Playwright or manual):

```typescript
/** Status tab: both /health and /status JSON trees render with correct top-level keys. */
test("debug_status_tab_renders_health_and_status_trees");

/** API Tester: selecting a preset fills method + path and clicking Send shows a response card. */
test("debug_api_tester_preset_and_send");

/** API Tester: POST with invalid JSON body shows validation error inline. */
test("debug_api_tester_invalid_json_body");

/** API Tester: history list grows on each request and Replay button refills the form. */
test("debug_api_tester_history_replay");

/** WebSocket tab: subscribing to a session shows events with seq numbers. */
test("debug_ws_tab_event_display");

/** WebSocket tab: gap detection highlights entries with missing seq values. */
test("debug_ws_tab_gap_detection");
```

### 16.9 Acceptance Criteria

- [ ] `/health` and `/status` JSON trees render with collapsible nodes and color-coded values
- [ ] API tester supports GET, POST, PUT, DELETE, PATCH methods
- [ ] API tester shows response headers in a dedicated sub-tab
- [ ] API tester has a preset dropdown that fills method + path
- [ ] API tester maintains a history of the last 50 requests
- [ ] API tester history entries can be replayed (re-fill form and execute)
- [ ] WebSocket tab shows all frame types (event, res) with seq and stateVersion
- [ ] WebSocket tab detects and highlights sequence gaps
- [ ] WebSocket stats bar shows total events, gap count, latest seq, latest stateVersion
- [ ] JsonTreeView component handles null, empty objects, empty arrays, and deeply nested data

### 16.10 Dependencies

**npm** (no new packages):
- All UI primitives already available via `radix-ui`, `lucide-react`, `class-variance-authority`

---

## Phase 17 — Agents & Skills Pages (UI)

### 17.1 Overview

Enhance the existing `agents.tsx` and `skills.tsx` pages with richer detail views,
inline editing, and status indicators. The backend endpoints (`GET /agents`,
`GET /skills`, `POST /skills/:name/enable`, `POST /skills/:name/disable`) already
exist.

### 17.2 Backend: New `GET /agents` Response Shape

The existing endpoint returns `AgentItem[]`. Extend the response struct:

File: `crates/rune-gateway/src/routes.rs`

```rust
/// Single agent in the `GET /agents` list response.
#[derive(Serialize)]
pub struct AgentResponse {
    /// Agent identifier.
    pub id: String,
    /// Whether this is the default agent.
    pub default: bool,
    /// Model identifier, e.g. "anthropic:claude-sonnet-4-20250514".
    pub model: Option<String>,
    /// Workspace root path.
    pub workspace: Option<String>,
    /// Full system prompt text.
    pub system_prompt: Option<String>,
    /// Number of sessions currently using this agent.
    pub active_session_count: usize,
    /// Number of tools available to this agent.
    pub tool_count: usize,
    /// Allowed tool names (empty = all tools allowed).
    pub allowed_tools: Vec<String>,
    /// Denied tool names.
    pub denied_tools: Vec<String>,
    /// Agent-specific model parameters (temperature, max_tokens, etc).
    pub model_params: Option<serde_json::Value>,
}
```

### 17.3 Backend: New `GET /agents/:id` Detail Endpoint

```rust
pub async fn get_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentDetailResponse>, GatewayError> { ... }

#[derive(Serialize)]
pub struct AgentDetailResponse {
    #[serde(flatten)]
    pub agent: AgentResponse,
    /// Recent sessions for this agent (last 10).
    pub recent_sessions: Vec<AgentSessionSummary>,
    /// System prompt token count estimate.
    pub system_prompt_tokens: Option<usize>,
}

#[derive(Serialize)]
pub struct AgentSessionSummary {
    pub session_id: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub turn_count: usize,
}
```

### 17.4 Backend: New `GET /skills/:name` Detail Endpoint

```rust
pub async fn get_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<SkillDetailResponse>, GatewayError> { ... }

#[derive(Serialize)]
pub struct SkillDetailResponse {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub binary_path: Option<String>,
    pub source_dir: String,
    pub parameters: serde_json::Value,
    /// Raw SKILL.md file content.
    pub raw_markdown: Option<String>,
    /// Whether the binary (if specified) exists on disk.
    pub binary_exists: bool,
    /// Last modification time of the SKILL.md file (ISO-8601).
    pub last_modified_at: Option<String>,
}
```

### 17.5 Wire Protocol

#### `GET /agents`

```json
[
  {
    "id": "default",
    "default": true,
    "model": "anthropic:claude-sonnet-4-20250514",
    "workspace": "/home/user/project",
    "system_prompt": "You are a helpful assistant...",
    "active_session_count": 3,
    "tool_count": 12,
    "allowed_tools": [],
    "denied_tools": ["dangerous_tool"],
    "model_params": { "temperature": 0.7, "max_tokens": 4096 }
  }
]
```

#### `GET /agents/default`

```json
{
  "id": "default",
  "default": true,
  "model": "anthropic:claude-sonnet-4-20250514",
  "workspace": "/home/user/project",
  "system_prompt": "You are a helpful assistant...",
  "active_session_count": 3,
  "tool_count": 12,
  "allowed_tools": [],
  "denied_tools": ["dangerous_tool"],
  "model_params": { "temperature": 0.7, "max_tokens": 4096 },
  "recent_sessions": [
    {
      "session_id": "01953a2b-1234-7def-abcd-000000000001",
      "status": "active",
      "created_at": "2026-03-15T10:00:00Z",
      "turn_count": 5
    }
  ],
  "system_prompt_tokens": 342
}
```

#### `GET /skills/code-runner`

```json
{
  "name": "code-runner",
  "description": "Runs code in a sandboxed environment",
  "enabled": true,
  "binary_path": "./run.sh",
  "source_dir": "/home/user/.rune/skills/code-runner",
  "parameters": {
    "type": "object",
    "properties": {
      "code": { "type": "string" },
      "language": { "type": "string" }
    }
  },
  "raw_markdown": "---\nname: code-runner\n...",
  "binary_exists": true,
  "last_modified_at": "2026-03-14T18:00:00Z"
}
```

### 17.6 Error Cases

| Condition | HTTP Status | Error Code | Message |
|---|---|---|---|
| Agent not found | 404 | `agent_not_found` | `"agent not found: {id}"` |
| Skill not found | 404 | `skill_not_found` | `"skill not found: {name}"` |
| Skill enable/disable when already in desired state | 200 | N/A | Returns success (idempotent) |

Add to `GatewayError`:

```rust
/// Agent not found.
#[error("agent not found: {0}")]
AgentNotFound(String),

/// Skill not found.
#[error("skill not found: {0}")]
SkillNotFound(String),
```

With corresponding `IntoResponse` mapping:

```rust
Self::AgentNotFound(_) => (StatusCode::NOT_FOUND, "agent_not_found", false, false, self.to_string()),
Self::SkillNotFound(_) => (StatusCode::NOT_FOUND, "skill_not_found", false, false, self.to_string()),
```

### 17.7 TypeScript Types

File: `ui/src/lib/api-types.ts` (additions)

```typescript
// Agents (enhanced)
export interface AgentResponse {
  id: string;
  default: boolean;
  model: string | null;
  workspace: string | null;
  system_prompt: string | null;
  active_session_count: number;
  tool_count: number;
  allowed_tools: string[];
  denied_tools: string[];
  model_params: Record<string, unknown> | null;
}

export interface AgentSessionSummary {
  session_id: string;
  status: string;
  created_at: string;
  turn_count: number;
}

export interface AgentDetailResponse extends AgentResponse {
  recent_sessions: AgentSessionSummary[];
  system_prompt_tokens: number | null;
}

// Skills (enhanced)
export interface SkillDetailResponse {
  name: string;
  description: string;
  enabled: boolean;
  binary_path: string | null;
  source_dir: string;
  parameters: unknown;
  raw_markdown: string | null;
  binary_exists: boolean;
  last_modified_at: string | null;
}
```

### 17.8 Component Tree — Agents Page

```
AgentsPage
├── PageHeader ("Agents" title + agent count badge)
├── Card "Agent List"
│   └── Table
│       ├── TableHeader (ID, Model, Workspace, Sessions, Tools, Default)
│       └── TableBody
│           └── AgentRow[] (clickable, expands inline or navigates to detail)
│               ├── TableCell id (font-medium)
│               ├── TableCell model (Badge variant="outline")
│               ├── TableCell workspace (mono, truncated)
│               ├── TableCell active_session_count
│               ├── TableCell tool_count
│               └── TableCell default (Star icon if true)
│
└── AgentDetailDrawer (Sheet component, opens on row click)
    ├── SheetHeader (agent id + default badge)
    ├── Section "Model Configuration"
    │   ├── LabelValue "Model" model
    │   ├── LabelValue "Temperature" model_params.temperature
    │   └── LabelValue "Max Tokens" model_params.max_tokens
    ├── Section "Tool Access"
    │   ├── Badge[] allowed_tools (green)
    │   └── Badge[] denied_tools (red)
    ├── Section "System Prompt" (collapsible)
    │   ├── span "{system_prompt_tokens} tokens"
    │   └── pre system_prompt (mono, max-h-[300px], overflow-auto)
    └── Section "Recent Sessions"
        └── Table (session_id link, status badge, created_at, turn_count)
```

### 17.9 Component Tree — Skills Page

```
SkillsPage
├── PageHeader ("Skills" title + skill count badge + "Rescan" button)
├── EmptyState (if no skills: Wrench icon + message)
└── div.grid.sm:grid-cols-2.lg:grid-cols-3
    └── SkillCard[] (one per skill)
        ├── CardHeader
        │   ├── CardTitle (Wrench icon + name)
        │   └── Switch (enabled toggle)
        ├── CardContent
        │   ├── p description
        │   ├── div.flex (enabled badge, "Has binary" badge if binary_path)
        │   ├── div (FolderOpen icon + source_dir)
        │   └── Button "Details" (opens SkillDetailDrawer)
        └── (empty)

SkillDetailDrawer (Sheet component)
├── SheetHeader (skill name + enabled badge)
├── Section "Configuration"
│   ├── LabelValue "Source Directory" source_dir
│   ├── LabelValue "Binary Path" binary_path
│   ├── LabelValue "Binary Exists" binary_exists (green check / red x)
│   └── LabelValue "Last Modified" last_modified_at
├── Section "Parameters Schema"
│   └── JsonTreeView { data: parameters }
└── Section "Raw SKILL.md" (collapsible)
    └── pre raw_markdown (mono, max-h-[400px], overflow-auto)
```

### 17.10 Hooks

File: `ui/src/hooks/use-agents.ts`

```typescript
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type { AgentResponse, AgentDetailResponse } from "@/lib/api-types";

export function useAgents() {
  return useQuery({
    queryKey: ["agents"],
    queryFn: () => api.get<AgentResponse[]>("/agents"),
    refetchInterval: 30_000,
  });
}

export function useAgentDetail(agentId: string | null) {
  return useQuery({
    queryKey: ["agents", agentId],
    queryFn: () => api.get<AgentDetailResponse>(`/agents/${agentId}`),
    enabled: !!agentId,
    staleTime: 10_000,
  });
}
```

File: `ui/src/hooks/use-skills.ts`

```typescript
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api-client";
import type { SkillDetailResponse } from "@/lib/api-types";

interface SkillItem {
  name: string;
  description: string;
  enabled: boolean;
  binary_path: string | null;
  source_dir: string;
  parameters: unknown;
}

export function useSkills() {
  return useQuery({
    queryKey: ["skills"],
    queryFn: () => api.get<SkillItem[]>("/skills"),
    refetchInterval: 15_000,
  });
}

export function useSkillDetail(name: string | null) {
  return useQuery({
    queryKey: ["skills", name],
    queryFn: () => api.get<SkillDetailResponse>(`/skills/${name}`),
    enabled: !!name,
    staleTime: 10_000,
  });
}

export function useToggleSkill() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ name, enable }: { name: string; enable: boolean }) =>
      api.post(`/skills/${name}/${enable ? "enable" : "disable"}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
    },
  });
}

export function useRescanSkills() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => api.post<{ added: number; removed: number }>("/skills/rescan"),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills"] });
    },
  });
}
```

### 17.11 Edge Cases

- **No agents configured**: Table shows empty state message: "No agents configured. Add agents in your configuration file."
- **Agent with null model**: Model cell shows a muted dash.
- **Skill with missing binary**: `binary_exists: false` — detail drawer shows a red warning badge "Binary not found".
- **Skill rescan while drawer is open**: Drawer data may become stale. The drawer's query refetches on invalidation.
- **Agent with very long system prompt**: Capped at `max-h-[300px]` with overflow scroll. Token count shown above.
- **Agent model_params is null**: Model Configuration section shows "Using defaults".

### 17.12 Integration Test Scenarios

Backend:

```rust
/// GET /agents returns all configured agents with session counts.
#[tokio::test]
async fn test_list_agents() { ... }

/// GET /agents/:id returns detail including recent sessions.
#[tokio::test]
async fn test_get_agent_detail() { ... }

/// GET /agents/:id with unknown id returns 404.
#[tokio::test]
async fn test_get_agent_not_found() { ... }

/// GET /skills/:name returns full detail including raw markdown.
#[tokio::test]
async fn test_get_skill_detail() { ... }

/// GET /skills/:name with unknown name returns 404.
#[tokio::test]
async fn test_get_skill_not_found() { ... }

/// POST /skills/rescan triggers a scan and returns added/removed counts.
#[tokio::test]
async fn test_rescan_skills() { ... }
```

### 17.13 Acceptance Criteria

- [ ] `GET /agents` returns `AgentResponse[]` with `active_session_count`, `tool_count`, `allowed_tools`, `denied_tools`
- [ ] `GET /agents/:id` returns `AgentDetailResponse` with recent sessions and token count
- [ ] `GET /agents/:id` returns 404 for unknown agent
- [ ] `GET /skills/:name` returns `SkillDetailResponse` with raw markdown and binary existence check
- [ ] `GET /skills/:name` returns 404 for unknown skill
- [ ] `POST /skills/rescan` triggers a filesystem scan and returns counts
- [ ] Agents page table shows all agent fields with clickable rows opening detail drawer
- [ ] Agent detail drawer shows model config, tool access, system prompt, and recent sessions
- [ ] Skills page cards show description, enabled toggle, and "Details" button
- [ ] Skill detail drawer shows parameters schema tree view and raw SKILL.md content
- [ ] Empty states render correctly for both pages

### 17.14 Dependencies

**Rust** (no new crates)

**npm** (no new packages):
- Sheet/Drawer component from `radix-ui` (already available)

---

## Phase 18 — Semantic Browser Snapshots (Backend)

### 18.1 Overview

New crate `crates/rune-browser/` that launches headless Chromium, extracts the
accessibility tree of a URL, converts it to a compact text representation with
numeric `[ref=N]` annotations, and exposes it as a tool (`browse`) in the runtime.

### 18.2 Crate Structure

```
crates/rune-browser/
├── Cargo.toml
├── src/
│   ├── lib.rs           # Public API
│   ├── browser.rs       # Chromium launch/pool management
│   ├── ax_tree.rs       # Accessibility tree extraction
│   ├── snapshot.rs      # AX tree → compact text conversion
│   ├── tool.rs          # ToolExecutor implementation
│   └── error.rs         # Error types
```

### 18.3 Rust Types

File: `crates/rune-browser/src/lib.rs`

```rust
pub mod browser;
pub mod ax_tree;
pub mod snapshot;
pub mod tool;
pub mod error;

pub use browser::BrowserPool;
pub use snapshot::{SemanticSnapshot, SnapshotConfig};
pub use tool::BrowseTool;
pub use error::BrowserError;
```

File: `crates/rune-browser/src/error.rs`

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrowserError {
    /// Chromium binary not found or not executable.
    #[error("chromium not found at {path}: {reason}")]
    ChromiumNotFound { path: String, reason: String },

    /// Timed out waiting for the page to load.
    #[error("page load timed out after {timeout_ms}ms for {url}")]
    PageLoadTimeout { url: String, timeout_ms: u64 },

    /// Navigation to URL failed (DNS, TLS, HTTP error).
    #[error("navigation failed for {url}: {reason}")]
    NavigationFailed { url: String, reason: String },

    /// Accessibility tree extraction failed.
    #[error("accessibility tree extraction failed: {0}")]
    AxTreeFailed(String),

    /// The page returned an error HTTP status.
    #[error("HTTP {status} for {url}")]
    HttpError { url: String, status: u16 },

    /// All browser instances in the pool are busy.
    #[error("browser pool exhausted (capacity: {capacity})")]
    PoolExhausted { capacity: usize },

    /// Internal CDP protocol error.
    #[error("CDP error: {0}")]
    CdpError(String),

    /// URL is blocked by policy.
    #[error("URL blocked by policy: {url}")]
    UrlBlocked { url: String },
}
```

File: `crates/rune-browser/src/snapshot.rs`

```rust
use serde::{Deserialize, Serialize};

/// Configuration for snapshot generation.
#[derive(Clone, Debug, Deserialize)]
pub struct SnapshotConfig {
    /// Maximum characters in the output snapshot text. Default: 30_000.
    pub max_chars: usize,
    /// Whether to include ARIA descriptions. Default: true.
    pub include_aria_descriptions: bool,
    /// Whether to include form field values. Default: true.
    pub include_form_values: bool,
    /// Whether to include link href text. Default: true.
    pub include_link_targets: bool,
    /// CSS selectors to exclude from the snapshot (e.g. "nav", "footer").
    pub exclude_selectors: Vec<String>,
    /// Page load timeout in milliseconds. Default: 15_000.
    pub page_load_timeout_ms: u64,
    /// Wait for this CSS selector to appear before extracting. Optional.
    pub wait_for_selector: Option<String>,
    /// Additional wait after page load in milliseconds. Default: 0.
    pub extra_wait_ms: u64,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            max_chars: 30_000,
            include_aria_descriptions: true,
            include_form_values: true,
            include_link_targets: true,
            exclude_selectors: vec![],
            page_load_timeout_ms: 15_000,
            wait_for_selector: None,
            extra_wait_ms: 0,
        }
    }
}

/// A single node in the extracted accessibility tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxNode {
    /// Numeric reference for the model to interact with this element.
    pub ref_id: u32,
    /// ARIA role: "button", "link", "textbox", "heading", "img", etc.
    pub role: String,
    /// Accessible name (label, text content, alt text).
    pub name: String,
    /// ARIA description, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Current value for form fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Href for links.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    /// Whether the element is disabled.
    pub disabled: bool,
    /// Whether the element is hidden/offscreen.
    pub hidden: bool,
    /// Nesting depth in the tree (for indentation).
    pub depth: u32,
    /// Children nodes.
    pub children: Vec<AxNode>,
}

/// The final snapshot produced by the browser crate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticSnapshot {
    /// The URL that was browsed.
    pub url: String,
    /// Page title from the document.
    pub title: String,
    /// Compact text representation of the page for model consumption.
    pub text: String,
    /// Number of interactive elements (links, buttons, inputs) annotated.
    pub ref_count: u32,
    /// Total characters in the text representation.
    pub text_length: usize,
    /// Whether the snapshot was truncated to max_chars.
    pub truncated: bool,
    /// Page load duration in milliseconds.
    pub load_time_ms: u64,
    /// Structured accessibility tree (optional, for tool callers that want it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ax_tree: Option<Vec<AxNode>>,
}
```

File: `crates/rune-browser/src/browser.rs`

```rust
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Manages a pool of headless Chromium instances.
pub struct BrowserPool {
    /// Semaphore limiting concurrent browser instances.
    semaphore: Arc<Semaphore>,
    /// Path to the Chromium executable.
    chromium_path: String,
    /// Maximum concurrent instances.
    capacity: usize,
    /// Launch arguments for Chromium.
    launch_args: Vec<String>,
}

/// Configuration for the browser pool.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct BrowserPoolConfig {
    /// Path to the Chromium binary. Default: auto-detect.
    pub chromium_path: Option<String>,
    /// Maximum concurrent browser instances. Default: 3.
    pub max_instances: usize,
    /// Extra Chromium launch flags.
    pub extra_args: Vec<String>,
    /// Blocked URL patterns (glob syntax).
    pub blocked_urls: Vec<String>,
}

impl Default for BrowserPoolConfig {
    fn default() -> Self {
        Self {
            chromium_path: None,
            max_instances: 3,
            extra_args: vec![],
            blocked_urls: vec![],
        }
    }
}

impl BrowserPool {
    pub fn new(config: &BrowserPoolConfig) -> Result<Self, crate::error::BrowserError> { ... }

    /// Browse a URL and return a semantic snapshot.
    pub async fn browse(
        &self,
        url: &str,
        config: &crate::snapshot::SnapshotConfig,
    ) -> Result<crate::snapshot::SemanticSnapshot, crate::error::BrowserError> { ... }
}
```

File: `crates/rune-browser/src/tool.rs`

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use rune_tools::{ToolCall, ToolDefinition, ToolError, ToolExecutor, ToolResult};

use crate::browser::BrowserPool;
use crate::snapshot::SnapshotConfig;

/// Parameters for the `browse` tool.
#[derive(Debug, Deserialize)]
pub struct BrowseParams {
    /// URL to browse.
    pub url: String,
    /// Optional CSS selector to wait for before extracting.
    pub wait_for: Option<String>,
    /// Optional maximum characters in the snapshot. Default: 30_000.
    pub max_chars: Option<usize>,
}

/// The `browse` tool that returns semantic snapshots.
pub struct BrowseTool {
    pool: BrowserPool,
    default_config: SnapshotConfig,
}

impl BrowseTool {
    pub fn new(pool: BrowserPool, default_config: SnapshotConfig) -> Self {
        Self { pool, default_config }
    }
}

#[async_trait]
impl ToolExecutor for BrowseTool {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let params: BrowseParams = serde_json::from_value(call.arguments)
            .map_err(|e| ToolError::InvalidArguments(e.to_string()))?;

        let mut config = self.default_config.clone();
        if let Some(wait_for) = params.wait_for {
            config.wait_for_selector = Some(wait_for);
        }
        if let Some(max_chars) = params.max_chars {
            config.max_chars = max_chars;
        }

        let snapshot = self.pool.browse(&params.url, &config).await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ToolResult {
            tool_use_id: call.id,
            content: snapshot.text,
            is_error: false,
        })
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "browse".to_string(),
            description: "Browse a URL and return a semantic text snapshot of the page content. \
                Returns a compact text representation of the page's accessibility tree with \
                numeric [ref=N] annotations for interactive elements.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["url"],
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to browse"
                    },
                    "wait_for": {
                        "type": "string",
                        "description": "CSS selector to wait for before extracting the snapshot"
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": "Maximum characters in the snapshot output (default: 30000)"
                    }
                }
            }),
        }
    }
}
```

### 18.4 Compact Text Format

The snapshot `text` field uses the following format:

```
Page: Example Domain
URL: https://example.com

[1] heading (h1): "Example Domain"

paragraph: "This domain is for use in illustrative examples in documents."

paragraph: "More information..."

[2] link: "More information..." → https://www.iana.org/domains/example
```

Rules:
- Interactive elements (links, buttons, inputs, selects, textareas) get `[ref=N]` annotations
- Headings are prefixed with their level: `heading (h1):`
- Non-interactive text blocks are rendered as `paragraph:`, `list-item:`, etc.
- Form fields show their current value: `[5] textbox "Email": "user@example.com"`
- Disabled elements are suffixed with `(disabled)`
- Hidden elements are omitted entirely
- Images show alt text: `[3] img: "Company Logo"`
- Depth is indicated by 2-space indentation per level
- Sections excluded by `exclude_selectors` are omitted

### 18.5 Wire Protocol

The `browse` tool is invoked as part of the normal TurnExecutor tool call flow. The
model sees it as a standard tool:

#### Tool Definition (in API messages)

```json
{
  "name": "browse",
  "description": "Browse a URL and return a semantic text snapshot...",
  "input_schema": {
    "type": "object",
    "required": ["url"],
    "properties": {
      "url": { "type": "string", "description": "The URL to browse" },
      "wait_for": { "type": "string", "description": "CSS selector to wait for..." },
      "max_chars": { "type": "integer", "description": "Maximum characters..." }
    }
  }
}
```

#### Tool Call (model output)

```json
{
  "id": "toolu_01abc123",
  "name": "browse",
  "arguments": {
    "url": "https://docs.rs/tokio/latest/tokio/",
    "max_chars": 20000
  }
}
```

#### Tool Result (returned to model)

```json
{
  "tool_use_id": "toolu_01abc123",
  "content": "Page: tokio - Rust\nURL: https://docs.rs/tokio/latest/tokio/\n\n[1] heading (h1): \"Crate tokio\"\n\nparagraph: \"A runtime for writing reliable, asynchronous applications...\"\n\n[2] link: \"Runtime\" → https://docs.rs/tokio/latest/tokio/runtime/...",
  "is_error": false
}
```

#### Tool Error (URL blocked)

```json
{
  "tool_use_id": "toolu_01abc123",
  "content": "Error: URL blocked by policy: https://internal.corp.example.com",
  "is_error": true
}
```

### 18.6 Error Cases

| Condition | Behavior | ToolError variant |
|---|---|---|
| Chromium binary not found | Tool registration fails at startup, logged as ERROR | N/A (tool not registered) |
| URL blocked by policy | Returns `ToolResult { is_error: true }` | `ExecutionFailed` |
| Page load timeout | Returns `ToolResult { is_error: true }` | `ExecutionFailed` |
| DNS resolution failure | Returns `ToolResult { is_error: true }` | `ExecutionFailed` |
| TLS certificate error | Returns `ToolResult { is_error: true }` | `ExecutionFailed` |
| Browser pool exhausted | Waits up to 30s, then returns error | `ExecutionFailed` |
| Invalid URL (not http/https) | Returns `ToolResult { is_error: true }` immediately | `InvalidArguments` |
| Empty page (no accessibility tree) | Returns snapshot with `text: "Page: (title)\nURL: ...\n\n(empty page)"` | N/A (success) |
| Snapshot exceeds max_chars | Truncated with `"...\n[snapshot truncated at N chars]"` | N/A (success, `truncated: true`) |

### 18.7 Edge Cases

- **JavaScript-heavy SPAs**: The `wait_for_selector` parameter allows waiting for dynamic content. Default behavior waits for `load` event plus `extra_wait_ms`.
- **Very large DOM**: Snapshot text is capped at `max_chars`. The AX tree is traversed depth-first and output stops at the limit.
- **Iframes**: Only the main frame's accessibility tree is extracted. Iframes are noted as `frame: "(iframe title or src)"` but not traversed.
- **PDF/binary content**: Navigation succeeds but AX tree is empty. Returns empty page snapshot.
- **Redirect chains**: The final URL is recorded in the snapshot. The tool does not expose intermediate redirects.
- **Concurrent browsing**: The `BrowserPool` semaphore limits concurrency. Multiple tool calls queue behind the semaphore.
- **Chromium crashes**: The pool detects the crash and creates a new instance for the next request. The current request returns `BrowserError::CdpError`.

### 18.8 Config Additions

File: `crates/rune-config/src/lib.rs`

```rust
/// Browser snapshot configuration.
#[serde(default)]
pub browser: BrowserConfig,
```

```rust
#[derive(Clone, Debug, Deserialize)]
pub struct BrowserConfig {
    /// Enable the browse tool. Default: false (must opt in).
    #[serde(default)]
    pub enabled: bool,
    /// Path to Chromium binary. Default: auto-detect.
    pub chromium_path: Option<String>,
    /// Maximum concurrent browser instances. Default: 3.
    #[serde(default = "default_max_browser_instances")]
    pub max_instances: usize,
    /// Maximum characters in snapshots. Default: 30_000.
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
    /// Page load timeout in ms. Default: 15_000.
    #[serde(default = "default_page_load_timeout")]
    pub page_load_timeout_ms: u64,
    /// URL patterns to block (glob syntax).
    #[serde(default)]
    pub blocked_urls: Vec<String>,
}

fn default_max_browser_instances() -> usize { 3 }
fn default_max_chars() -> usize { 30_000 }
fn default_page_load_timeout() -> u64 { 15_000 }
```

Env overrides: `RUNE_BROWSER__ENABLED`, `RUNE_BROWSER__CHROMIUM_PATH`, etc.

### 18.9 Integration Test Scenarios

```rust
/// Snapshot of a simple static HTML page produces correct text format.
#[tokio::test]
async fn test_snapshot_simple_html() { ... }

/// Snapshot includes [ref=N] annotations for links and buttons.
#[tokio::test]
async fn test_snapshot_ref_annotations() { ... }

/// Snapshot respects max_chars and sets truncated=true.
#[tokio::test]
async fn test_snapshot_truncation() { ... }

/// Blocked URL returns an error ToolResult.
#[tokio::test]
async fn test_browse_blocked_url() { ... }

/// Invalid URL (ftp://) returns InvalidArguments error.
#[tokio::test]
async fn test_browse_invalid_scheme() { ... }

/// Pool exhaustion queues and eventually serves the request.
#[tokio::test]
async fn test_pool_concurrency_limit() { ... }

/// Page with no interactive elements produces a snapshot with ref_count=0.
#[tokio::test]
async fn test_snapshot_no_interactive_elements() { ... }

/// BrowseTool.definition() returns the correct JSON schema.
#[test]
fn test_browse_tool_definition() { ... }
```

### 18.10 Acceptance Criteria

- [ ] `crates/rune-browser/` compiles and passes `cargo test`
- [ ] `BrowserPool` launches headless Chromium and manages instance lifecycle
- [ ] `browse` tool is registered in the ToolRegistry when `browser.enabled = true`
- [ ] Semantic snapshot text format uses `[ref=N]` annotations for interactive elements
- [ ] Snapshots respect `max_chars` truncation with a `[snapshot truncated]` suffix
- [ ] `exclude_selectors` removes matching subtrees from the snapshot
- [ ] `wait_for_selector` delays extraction until the selector matches
- [ ] Blocked URLs return `is_error: true` tool results
- [ ] Pool exhaustion does not panic; requests queue and eventually complete or timeout
- [ ] Chromium crashes are recovered gracefully
- [ ] All 8 integration tests pass

### 18.11 Dependencies

**Rust** — new crate dependencies in `crates/rune-browser/Cargo.toml`:

```toml
[dependencies]
chromiumoxide = { version = "0.7", features = ["tokio-runtime"], default-features = false }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
async-trait = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
chrono = { workspace = true }
rune-tools = { path = "../rune-tools" }

[dev-dependencies]
tempfile = { workspace = true }
tokio = { workspace = true, features = ["test-util"] }
```

Add to workspace Cargo.toml `[workspace.dependencies]`:

```toml
chromiumoxide = { version = "0.7", features = ["tokio-runtime"], default-features = false }
```

**System requirement**: Chromium or Chrome installed. The crate auto-detects common paths:
`/usr/bin/chromium-browser`, `/usr/bin/chromium`, `/usr/bin/google-chrome`, `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`.

---

## Phase 19 — A2UI Protocol (Backend + UI)

### 19.1 Overview

Agent-to-UI (A2UI) protocol: a structured way for the agent to push declarative UI
components to the admin UI. The agent emits `a2ui.push` and `a2ui.reset` events via
the existing WebSocket event bus. The UI renders these components in a dedicated
panel or inline within the chat.

### 19.2 Rust Types

File: `crates/rune-gateway/src/a2ui.rs`

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A2UI component types that the agent can push to the UI.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum A2uiComponent {
    /// Markdown-rendered card with optional title and actions.
    Card(CardComponent),
    /// Tabular data display.
    Table(TableComponent),
    /// Ordered/unordered list.
    List(ListComponent),
    /// Interactive form with input fields.
    Form(FormComponent),
    /// Data visualization chart.
    Chart(ChartComponent),
    /// Key-value metadata display.
    Kv(KvComponent),
    /// Progress indicator.
    Progress(ProgressComponent),
    /// Code block with syntax highlighting.
    Code(CodeComponent),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CardComponent {
    /// Unique component ID for updates.
    pub id: String,
    /// Optional card title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Markdown body content.
    pub body: String,
    /// Optional action buttons.
    #[serde(default)]
    pub actions: Vec<A2uiAction>,
    /// Visual variant: "default", "info", "success", "warning", "error".
    #[serde(default = "default_variant")]
    pub variant: String,
}

fn default_variant() -> String { "default".to_string() }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableComponent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Column definitions.
    pub columns: Vec<TableColumn>,
    /// Row data. Each row is a map of column_key → value.
    pub rows: Vec<serde_json::Map<String, serde_json::Value>>,
    /// Whether to show row numbers. Default: false.
    #[serde(default)]
    pub show_row_numbers: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableColumn {
    /// Column key matching row data keys.
    pub key: String,
    /// Display header.
    pub label: String,
    /// Column type hint: "text", "number", "badge", "date", "code".
    #[serde(default = "default_col_type")]
    pub col_type: String,
}

fn default_col_type() -> String { "text".to_string() }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListComponent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// List items. Each item can be plain text or markdown.
    pub items: Vec<String>,
    /// Whether the list is ordered. Default: false.
    #[serde(default)]
    pub ordered: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FormComponent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Form field definitions.
    pub fields: Vec<FormField>,
    /// Submit button label. Default: "Submit".
    #[serde(default = "default_submit_label")]
    pub submit_label: String,
    /// Callback identifier — sent back to the agent when the form is submitted.
    pub callback_id: String,
}

fn default_submit_label() -> String { "Submit".to_string() }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FormField {
    /// Field key used in the submitted data.
    pub key: String,
    /// Display label.
    pub label: String,
    /// Field type: "text", "number", "select", "checkbox", "textarea".
    pub field_type: String,
    /// Whether the field is required. Default: false.
    #[serde(default)]
    pub required: bool,
    /// Default value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<serde_json::Value>,
    /// Options for select fields.
    #[serde(default)]
    pub options: Vec<SelectOption>,
    /// Placeholder text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChartComponent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Chart type: "bar", "line", "pie", "area".
    pub chart_type: String,
    /// Data series.
    pub series: Vec<ChartSeries>,
    /// X-axis labels (for bar/line/area).
    #[serde(default)]
    pub x_labels: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChartSeries {
    pub name: String,
    pub data: Vec<f64>,
    /// Optional color override (hex).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KvComponent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub entries: Vec<KvEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KvEntry {
    pub key: String,
    pub value: serde_json::Value,
    /// Optional label override (defaults to key).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgressComponent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 0.0 to 1.0.
    pub value: f64,
    /// Optional status message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Variant: "default", "success", "warning", "error".
    #[serde(default = "default_variant")]
    pub variant: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodeComponent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Code content.
    pub code: String,
    /// Language hint for syntax highlighting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// An action button on a card or component.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2uiAction {
    /// Button label.
    pub label: String,
    /// Action type: "callback" (sends to agent) or "link" (opens URL).
    pub action_type: String,
    /// Callback ID or URL depending on action_type.
    pub target: String,
    /// Visual variant: "default", "destructive", "outline".
    #[serde(default = "default_variant")]
    pub variant: String,
}

/// Events sent over the WebSocket for A2UI.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum A2uiEvent {
    /// Push a component to the UI (add or update by id).
    Push {
        session_id: String,
        component: A2uiComponent,
        /// Target panel: "inline" (in chat flow) or "panel" (side panel).
        target: String,
        /// Timestamp of the push.
        timestamp: DateTime<Utc>,
    },
    /// Remove a specific component by id.
    Remove {
        session_id: String,
        component_id: String,
        timestamp: DateTime<Utc>,
    },
    /// Reset all A2UI components for a session.
    Reset {
        session_id: String,
        timestamp: DateTime<Utc>,
    },
}
```

### 19.3 Integration with SessionEvent

A2UI events are broadcast as `SessionEvent` with `kind: "a2ui"`:

```rust
// When the agent pushes a component:
let event = SessionEvent {
    session_id: session_id.clone(),
    kind: "a2ui".to_string(),
    payload: serde_json::to_value(&a2ui_event).unwrap(),
    state_changed: false,
};
event_tx.send(event).ok();
```

### 19.4 Tool for Agent to Push A2UI Components

File: `crates/rune-gateway/src/a2ui.rs` (add to same file)

```rust
/// Tool that allows the agent to push UI components via A2UI protocol.
pub struct A2uiTool {
    event_tx: broadcast::Sender<SessionEvent>,
}

impl A2uiTool {
    pub fn new(event_tx: broadcast::Sender<SessionEvent>) -> Self {
        Self { event_tx }
    }
}

#[async_trait]
impl ToolExecutor for A2uiTool {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> { ... }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "a2ui_push".to_string(),
            description: "Push a declarative UI component to the user's admin panel. \
                Supported types: card, table, list, form, chart, kv, progress, code.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["session_id", "component"],
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session to push the component to"
                    },
                    "component": {
                        "type": "object",
                        "description": "The A2UI component to push. Must include 'type' and 'id' fields."
                    },
                    "target": {
                        "type": "string",
                        "enum": ["inline", "panel"],
                        "description": "Where to display: 'inline' in chat or 'panel' in side panel"
                    }
                }
            }),
        }
    }
}
```

### 19.5 Wire Protocol

#### WebSocket Event: `a2ui` (push)

```json
{
  "type": "event",
  "event": "a2ui",
  "payload": {
    "session_id": "01953a2b-...",
    "kind": "a2ui",
    "payload": {
      "action": "push",
      "session_id": "01953a2b-...",
      "component": {
        "type": "table",
        "id": "dep-analysis",
        "title": "Dependency Analysis",
        "columns": [
          { "key": "name", "label": "Package", "col_type": "text" },
          { "key": "version", "label": "Version", "col_type": "code" },
          { "key": "status", "label": "Status", "col_type": "badge" }
        ],
        "rows": [
          { "name": "tokio", "version": "1.37", "status": "up-to-date" },
          { "name": "serde", "version": "1.0.198", "status": "outdated" }
        ],
        "show_row_numbers": true
      },
      "target": "inline",
      "timestamp": "2026-03-15T14:30:22Z"
    }
  },
  "seq": 42,
  "stateVersion": 17
}
```

#### WebSocket Event: `a2ui` (reset)

```json
{
  "type": "event",
  "event": "a2ui",
  "payload": {
    "session_id": "01953a2b-...",
    "kind": "a2ui",
    "payload": {
      "action": "reset",
      "session_id": "01953a2b-...",
      "timestamp": "2026-03-15T14:35:00Z"
    }
  },
  "seq": 43,
  "stateVersion": 17
}
```

#### Form Callback (client to server via WebSocket RPC)

When a user submits an A2UI form, the client sends an RPC request:

```json
{
  "type": "req",
  "id": "rpc-uuid-001",
  "method": "a2ui.form_submit",
  "params": {
    "session_id": "01953a2b-...",
    "callback_id": "deploy-config-form",
    "data": {
      "environment": "staging",
      "replicas": 3,
      "auto_scale": true
    }
  }
}
```

Response:

```json
{
  "type": "res",
  "id": "rpc-uuid-001",
  "ok": true,
  "stateVersion": 18,
  "payload": {
    "accepted": true,
    "message": "Form submitted to agent"
  }
}
```

#### Action Callback (client to server via WebSocket RPC)

```json
{
  "type": "req",
  "id": "rpc-uuid-002",
  "method": "a2ui.action",
  "params": {
    "session_id": "01953a2b-...",
    "component_id": "deploy-card",
    "action_target": "confirm-deploy"
  }
}
```

### 19.6 TypeScript Types

File: `ui/src/lib/api-types.ts` (additions)

```typescript
// A2UI Protocol
export type A2uiComponentType =
  | "card" | "table" | "list" | "form" | "chart" | "kv" | "progress" | "code";

export interface A2uiAction {
  label: string;
  action_type: "callback" | "link";
  target: string;
  variant: string;
}

export interface CardComponent {
  type: "card";
  id: string;
  title?: string;
  body: string;
  actions: A2uiAction[];
  variant: string;
}

export interface TableColumn {
  key: string;
  label: string;
  col_type: "text" | "number" | "badge" | "date" | "code";
}

export interface TableComponent {
  type: "table";
  id: string;
  title?: string;
  columns: TableColumn[];
  rows: Record<string, unknown>[];
  show_row_numbers: boolean;
}

export interface ListComponent {
  type: "list";
  id: string;
  title?: string;
  items: string[];
  ordered: boolean;
}

export interface SelectOption {
  value: string;
  label: string;
}

export interface FormField {
  key: string;
  label: string;
  field_type: "text" | "number" | "select" | "checkbox" | "textarea";
  required: boolean;
  default_value?: unknown;
  options: SelectOption[];
  placeholder?: string;
}

export interface FormComponent {
  type: "form";
  id: string;
  title?: string;
  fields: FormField[];
  submit_label: string;
  callback_id: string;
}

export interface ChartSeries {
  name: string;
  data: number[];
  color?: string;
}

export interface ChartComponent {
  type: "chart";
  id: string;
  title?: string;
  chart_type: "bar" | "line" | "pie" | "area";
  series: ChartSeries[];
  x_labels: string[];
}

export interface KvEntry {
  key: string;
  value: unknown;
  label?: string;
}

export interface KvComponent {
  type: "kv";
  id: string;
  title?: string;
  entries: KvEntry[];
}

export interface ProgressComponent {
  type: "progress";
  id: string;
  label?: string;
  value: number;
  status?: string;
  variant: string;
}

export interface CodeComponent {
  type: "code";
  id: string;
  title?: string;
  code: string;
  language?: string;
}

export type A2uiComponent =
  | CardComponent
  | TableComponent
  | ListComponent
  | FormComponent
  | ChartComponent
  | KvComponent
  | ProgressComponent
  | CodeComponent;

export interface A2uiPushEvent {
  action: "push";
  session_id: string;
  component: A2uiComponent;
  target: "inline" | "panel";
  timestamp: string;
}

export interface A2uiRemoveEvent {
  action: "remove";
  session_id: string;
  component_id: string;
  timestamp: string;
}

export interface A2uiResetEvent {
  action: "reset";
  session_id: string;
  timestamp: string;
}

export type A2uiEvent = A2uiPushEvent | A2uiRemoveEvent | A2uiResetEvent;

export interface A2uiFormSubmitRequest {
  session_id: string;
  callback_id: string;
  data: Record<string, unknown>;
}

export interface A2uiActionRequest {
  session_id: string;
  component_id: string;
  action_target: string;
}
```

### 19.7 Component Tree — UI Renderer

File: `ui/src/components/a2ui/`

```
a2ui/
├── A2uiPanel.tsx           # Side panel container for A2UI components
├── A2uiInline.tsx          # Inline renderer for A2UI components in chat
├── A2uiRenderer.tsx        # Dispatcher: routes component type to renderer
├── A2uiCard.tsx            # Card renderer
├── A2uiTable.tsx           # Table renderer
├── A2uiList.tsx            # List renderer
├── A2uiForm.tsx            # Form renderer with validation
├── A2uiChart.tsx           # Chart renderer (using native SVG)
├── A2uiKv.tsx              # Key-value renderer
├── A2uiProgress.tsx        # Progress bar renderer
├── A2uiCode.tsx            # Code block renderer
└── use-a2ui.ts             # Hook managing A2UI component state
```

Component hierarchy:

```
A2uiPanel
├── div.header ("Agent UI" title + component count + clear button)
└── div.component-list (scrollable)
    └── A2uiRenderer[] (one per component)
        ├── case "card" → A2uiCard { component, onAction }
        ├── case "table" → A2uiTable { component }
        ├── case "list" → A2uiList { component }
        ├── case "form" → A2uiForm { component, onSubmit }
        ├── case "chart" → A2uiChart { component }
        ├── case "kv" → A2uiKv { component }
        ├── case "progress" → A2uiProgress { component }
        └── case "code" → A2uiCode { component }
```

### 19.8 Hook: `use-a2ui.ts`

```typescript
interface UseA2uiOptions {
  sessionId: string;
  maxComponents?: number; // default: 50
}

interface UseA2uiReturn {
  /** Active components ordered by push time */
  components: A2uiComponent[];
  /** Submit a form callback */
  submitForm: (callbackId: string, data: Record<string, unknown>) => void;
  /** Trigger an action callback */
  triggerAction: (componentId: string, actionTarget: string) => void;
  /** Clear all components locally */
  clear: () => void;
}

export function useA2ui(options: UseA2uiOptions): UseA2uiReturn { ... }
```

The hook listens for `a2ui` events via the existing WebSocket connection and maintains
a local component map. On `push`, it upserts by component `id`. On `remove`, it
deletes by `id`. On `reset`, it clears all.

### 19.9 Chart Rendering

Charts are rendered using native SVG (no chart library dependency). Supported types:

- **Bar**: Vertical bars with x-axis labels
- **Line**: Line with dots at data points
- **Pie**: SVG circle with `stroke-dasharray` segments
- **Area**: Filled line chart

The chart component uses Tailwind colors for series (cycling through a 6-color palette).

### 19.10 Error Cases

| Condition | Behavior |
|---|---|
| Unknown component type | `A2uiRenderer` shows a fallback "Unsupported component type: X" card |
| Form submission with missing required fields | Client-side validation prevents submission; red error borders on fields |
| WebSocket disconnected when submitting form | Queued locally; toast notification "Reconnecting..." |
| Component id collision (push with existing id) | Old component replaced with new one (upsert semantics) |
| maxComponents exceeded | Oldest component (by push timestamp) is evicted |
| Chart with empty data series | Renders empty chart area with "No data" text |
| Card body with malicious HTML | Rendered via `marked` + `DOMPurify` (already in deps) |

### 19.11 Edge Cases

- **Large table (>100 rows)**: Virtualized rendering with "Show more" button. First 50 rows displayed.
- **Form with nested data**: Not supported. Only flat key-value form data. Complex structures should use multiple forms.
- **Multiple sessions pushing A2UI**: Each session has its own component namespace. The panel shows components for the currently viewed session only.
- **A2UI push before client connects**: Components are ephemeral (not persisted). Client only sees components pushed after connection.
- **Agent pushes progress then card**: Both components coexist. Agent should use `remove` to clean up progress before pushing results.

### 19.12 Integration Test Scenarios

Backend:

```rust
/// a2ui.push event is broadcast on the session event bus.
#[tokio::test]
async fn test_a2ui_push_broadcasts_event() { ... }

/// a2ui.reset clears the component store for a session.
#[tokio::test]
async fn test_a2ui_reset_broadcasts_event() { ... }

/// a2ui.form_submit RPC method delivers form data to the agent.
#[tokio::test]
async fn test_a2ui_form_submit_rpc() { ... }

/// a2ui.action RPC method delivers callback to the agent.
#[tokio::test]
async fn test_a2ui_action_rpc() { ... }

/// Unknown component type in push is accepted (forward compatible).
#[tokio::test]
async fn test_a2ui_push_unknown_type_accepted() { ... }
```

UI (Playwright or manual):

```typescript
/** A2uiCard renders title, markdown body, and action buttons. */
test("a2ui_card_renders_correctly");

/** A2uiTable renders columns, rows, and row numbers when enabled. */
test("a2ui_table_renders_correctly");

/** A2uiForm validates required fields and submits data. */
test("a2ui_form_validation_and_submit");

/** A2uiChart renders a bar chart with labels and series. */
test("a2ui_chart_bar_renders");

/** Components are evicted when maxComponents is exceeded. */
test("a2ui_component_eviction");

/** Reset event clears all components. */
test("a2ui_reset_clears_panel");
```

### 19.13 Acceptance Criteria

- [ ] `A2uiComponent` enum supports card, table, list, form, chart, kv, progress, code types
- [ ] `a2ui.push` WebSocket events create/update components in the UI
- [ ] `a2ui.remove` WebSocket events remove specific components by id
- [ ] `a2ui.reset` WebSocket events clear all components for a session
- [ ] Form components validate required fields client-side
- [ ] Form submission sends `a2ui.form_submit` RPC over WebSocket
- [ ] Action buttons send `a2ui.action` RPC over WebSocket
- [ ] Card body is rendered as sanitized markdown
- [ ] Table supports text, number, badge, date, code column types
- [ ] Charts render using native SVG (no external chart library)
- [ ] Component eviction works when max is exceeded
- [ ] Unknown component types show a graceful fallback
- [ ] All 5 backend integration tests pass
- [ ] UI renderers handle all component types correctly

### 19.14 Dependencies

**Rust** (no new crates beyond existing workspace deps)

**npm** (no new packages):
- `marked` (already in deps) for card body markdown
- `dompurify` (already in deps) for sanitization
- SVG charts are implemented directly, no chart library needed

---

## Phase 20 — Enhanced Existing Pages (UI)

### 20.1 Overview

Incremental improvements to four existing pages: Cron, Dashboard, Channels, and
Settings. All changes are UI-only; no new backend endpoints are needed.

### 20.2 Cron Page Enhancements

File: `ui/src/routes/_admin/cron.tsx`

#### 20.2.1 Schedule Mode Selector

Replace the raw JSON schedule editor with a tabbed mode selector:

```typescript
interface ScheduleEditorProps {
  value: CronSchedule;
  onChange: (schedule: CronSchedule) => void;
}
```

Component tree:

```
ScheduleEditor
├── Tabs
│   ├── TabsTrigger "At" (one-time schedule)
│   │   └── DateTimePicker (maps to CronScheduleAt)
│   ├── TabsTrigger "Every" (interval)
│   │   └── IntervalPicker
│   │       ├── Input "Interval value" (number)
│   │       ├── Select "Unit" (seconds/minutes/hours/days)
│   │       └── DateTimePicker "Anchor" (optional)
│   └── TabsTrigger "Cron" (cron expression)
│       ├── Input "Cron expression" (text, e.g. "0 */5 * * *")
│       ├── Input "Timezone" (optional, e.g. "America/New_York")
│       └── span "Next 3 runs: ..." (computed preview)
```

#### 20.2.2 Payload Mode Selector

```typescript
interface PayloadEditorProps {
  value: CronPayload;
  onChange: (payload: CronPayload) => void;
}
```

Component tree:

```
PayloadEditor
├── Tabs
│   ├── TabsTrigger "System Event"
│   │   └── Input "Event text"
│   └── TabsTrigger "Agent Turn"
│       ├── Input "Message"
│       ├── Input "Model" (optional)
│       └── Input "Timeout (seconds)" (optional number)
```

#### 20.2.3 Job Cloning

Each job row gets a "Clone" action button that copies the job's schedule + payload into the creation form with a generated name suffix `(copy)`.

#### 20.2.4 Advanced Filtering and Sorting

```typescript
interface CronFilterState {
  search: string;
  status: "all" | "enabled" | "disabled";
  scheduleType: "all" | "at" | "every" | "cron";
  sortBy: "name" | "created_at" | "next_run_at" | "run_count";
  sortDir: "asc" | "desc";
}
```

### 20.3 Dashboard Page Enhancements

File: `ui/src/routes/_admin/index.tsx`

#### 20.3.1 New Cards

```
DashboardPage (enhanced)
├── div.grid (summary cards)
│   ├── Card "Connection Status"
│   │   ├── Badge (WebSocket connected/disconnected)
│   │   ├── span "Last heartbeat: ..."
│   │   └── span "WS subscribers: N"
│   ├── Card "Auth Mode"
│   │   ├── Badge (auth_enabled ? "Token Auth" : "No Auth")
│   │   └── span description
│   ├── Card "Tools"
│   │   ├── span tool_count
│   │   └── Link "View tools →" → /skills
│   └── Card "Quick Actions"
│       ├── Button "New Session" → POST /sessions
│       ├── Button "Rescan Skills" → POST /skills/rescan
│       └── Button "Clear Diagnostics"
│
├── div.grid (existing sections - models, sessions, diagnostics)
│   └── ... (unchanged)
```

#### 20.3.2 TypeScript Types for Quick Actions

```typescript
interface QuickAction {
  label: string;
  icon: LucideIcon;
  action: () => Promise<void>;
  variant: "default" | "outline" | "destructive";
  /** If true, show a confirmation dialog before executing. */
  confirm?: boolean;
}
```

### 20.4 Channels Page Enhancements

File: `ui/src/routes/_admin/channels.tsx`

#### 20.4.1 New Backend Endpoint: `GET /api/channels/status`

While the roadmap says "no new backend", we need a richer response to show per-channel
status. If the backend already provides channel data via `/api/dashboard/summary`, we
extract from that. If not, add:

```rust
#[derive(Serialize)]
pub struct ChannelStatusResponse {
    pub name: String,
    /// "connected", "disconnected", "error", "initializing".
    pub status: String,
    /// Channel type: "discord", "slack", "whatsapp", "signal", "http".
    pub channel_type: String,
    /// Number of active sessions via this channel.
    pub active_sessions: usize,
    /// Last message received timestamp (ISO-8601).
    pub last_message_at: Option<String>,
    /// Error message if status is "error".
    pub error: Option<String>,
}
```

#### 20.4.2 Component Tree

```
ChannelsPage (enhanced)
├── PageHeader
├── div.grid.sm:grid-cols-2.lg:grid-cols-3
│   └── ChannelStatusCard[] (one per channel)
│       ├── div.flex (channel icon + name)
│       ├── ConnectionIndicator { status }
│       │   ├── span.animate-pulse (green dot if connected)
│       │   ├── span (red dot if error)
│       │   └── span (yellow dot if initializing)
│       ├── div "Type: {channel_type}"
│       ├── div "Sessions: {active_sessions}"
│       ├── div "Last message: {relative_time}"
│       └── div.text-red-500 error (if any)
```

```typescript
interface ChannelStatusCardProps {
  channel: ChannelStatusResponse;
}

interface ConnectionIndicatorProps {
  status: "connected" | "disconnected" | "error" | "initializing";
}
```

### 20.5 Settings Page Enhancements

File: `ui/src/routes/_admin/settings.tsx`

#### 20.5.1 New Sections

```
SettingsPage (enhanced)
├── ... (existing Heartbeat and Gateway sections)
│
├── Card "Text-to-Speech"
│   ├── Select "TTS Provider" (none/system/elevenlabs/openai)
│   ├── Select "Voice" (provider-specific voice list)
│   ├── Slider "Speed" (0.5 - 2.0)
│   └── Button "Test Voice"
│
├── Card "Speech-to-Text"
│   ├── Select "STT Provider" (none/whisper/deepgram)
│   ├── Select "Language" (auto/en/es/fr/...)
│   └── Switch "Continuous listening"
│
├── Card "Device Pairing"
│   ├── div "Paired Devices"
│   │   └── PairedDeviceList
│   │       └── PairedDeviceRow[] { device_name, paired_at, last_seen, "Unpair" button }
│   └── Button "Pair New Device" → opens PairingDialog
│       ├── QR code display (device scans to pair)
│       └── Manual code entry input
```

#### 20.5.2 TypeScript Types

```typescript
export interface TtsSettings {
  provider: "none" | "system" | "elevenlabs" | "openai";
  voice: string | null;
  speed: number;
}

export interface SttSettings {
  provider: "none" | "whisper" | "deepgram";
  language: string;
  continuous: boolean;
}

export interface PairedDeviceInfo {
  device_id: string;
  device_name: string;
  paired_at: string;
  last_seen_at: string | null;
  public_key_fingerprint: string;
}
```

### 20.6 Edge Cases

- **Cron: Invalid cron expression**: Inline validation with red border and error message. "Next 3 runs" shows "Invalid expression".
- **Cron: Timezone not found**: Show warning "Unknown timezone, will use UTC".
- **Dashboard: Quick Action fails**: Toast notification with error message. Button shows loading spinner during execution.
- **Channels: No status endpoint available**: Fall back to current behavior (just channel names from `/api/dashboard/summary`).
- **Settings: TTS test fails**: Toast "Voice test failed: {error}". Button re-enables.
- **Settings: Unpair device confirmation**: Show dialog "Unpair {device_name}? This device will need to re-pair to connect."
- **Settings: No paired devices**: Empty state "No devices paired. Click 'Pair New Device' to get started."

### 20.7 Integration Test Scenarios

UI tests (Playwright):

```typescript
/** Cron: Schedule mode selector switches between At/Every/Cron tabs correctly. */
test("cron_schedule_mode_selector");

/** Cron: Cron expression preview shows next 3 run times. */
test("cron_expression_preview");

/** Cron: Clone button duplicates job config into creation form. */
test("cron_clone_job");

/** Cron: Column sorting works for all sortable columns. */
test("cron_column_sorting");

/** Dashboard: Quick Actions execute and show success/error toast. */
test("dashboard_quick_actions");

/** Dashboard: Connection status card reflects WS connection state. */
test("dashboard_connection_status");

/** Channels: Per-channel status cards render with correct indicators. */
test("channels_status_cards");

/** Settings: TTS provider selection shows provider-specific voice options. */
test("settings_tts_provider_voices");

/** Settings: Device pairing list shows paired devices with unpair action. */
test("settings_device_pairing_list");
```

### 20.8 Acceptance Criteria

- [ ] Cron page has a visual schedule mode selector with At/Every/Cron tabs
- [ ] Cron page has a payload mode selector with System Event/Agent Turn tabs
- [ ] Cron expression tab shows next 3 run times as preview
- [ ] Cron jobs can be cloned via a "Clone" button
- [ ] Cron job list supports filtering by status, schedule type, and text search
- [ ] Cron job list supports sorting by name, created_at, next_run_at, run_count
- [ ] Dashboard shows Connection Status, Auth Mode, Tools, and Quick Actions cards
- [ ] Dashboard Quick Actions include New Session, Rescan Skills, and Clear Diagnostics
- [ ] Channels page shows per-channel status cards with connection indicators
- [ ] Settings page has TTS section with provider/voice/speed controls and test button
- [ ] Settings page has STT section with provider/language/continuous controls
- [ ] Settings page has Device Pairing section with paired device list and pair button

### 20.9 Dependencies

**npm** (no new packages):
- Date pickers use native HTML `<input type="datetime-local">` — no library needed
- QR code for device pairing: use inline SVG generation (or add `qrcode` package if needed)

**Optional new npm package**:
```json
"qrcode.react": "^4.2.0"
```

---

## Phase 21 — UI Polish

### 21.1 Overview

Final polish pass across the entire admin UI. All changes are client-side. No backend
modifications.

### 21.2 Feature: Focus Mode Toggle

Hides the sidebar navigation to give the chat or current page full width.

```typescript
// State persisted in localStorage under key "rune:focus-mode"
interface FocusModeState {
  enabled: boolean;
}
```

Implementation:
- Toggle button in the top-right header area (Maximize2 / Minimize2 icon)
- When enabled: sidebar collapses to 0 width with CSS transition
- `localStorage.getItem("rune:focus-mode")` read on mount
- `localStorage.setItem("rune:focus-mode", JSON.stringify(state))` on toggle

Component:

```
FocusModeToggle
└── Button variant="ghost" size="icon"
    └── icon: enabled ? Minimize2 : Maximize2
```

### 21.3 Feature: Smart Scroll + "New Messages" Button

For the chat page (`ui/src/routes/_admin/chat.tsx`):

```typescript
interface SmartScrollState {
  /** Whether auto-scroll is active (user is at bottom). */
  isAtBottom: boolean;
  /** Number of new messages since the user scrolled away. */
  unreadCount: number;
}
```

Logic:
1. Track scroll position via `IntersectionObserver` on a sentinel `<div>` at the bottom of the message list
2. If `isAtBottom`, auto-scroll on new messages
3. If not `isAtBottom`, increment `unreadCount` on each new message
4. Show floating "N new messages" button at the bottom of the chat area
5. Clicking the button scrolls to bottom and resets `unreadCount`

Component:

```
ChatMessageList
├── div.message-container (overflow-y-auto)
│   ├── ChatMessage[]
│   └── div ref={sentinelRef} (invisible, 1px, at bottom)
└── NewMessagesButton { count: unreadCount, onClick: scrollToBottom }
    └── Button variant="secondary" className="fixed bottom-20 ..."
        └── "↓ {count} new messages"
```

### 21.4 Feature: Copy-as-Markdown per Message

Each assistant message bubble gets a copy button that extracts the message content
as markdown and copies it to the clipboard.

```typescript
interface MessageActionBarProps {
  content: string;
  role: "user" | "assistant";
}
```

Component:

```
MessageActionBar (shown on hover)
├── Button "Copy" (clipboard icon) → navigator.clipboard.writeText(content)
└── Button "Copy as Markdown" (file-text icon) → navigator.clipboard.writeText(markdownContent)
```

The markdown content is the raw assistant reply before HTML rendering. If the message
was already in markdown format (which it typically is), this is a direct copy.

### 21.5 Feature: Image Paste with Preview

When a user pastes an image (Ctrl+V or Cmd+V) into the chat input area:

```typescript
interface PastedImage {
  /** Object URL for preview. */
  previewUrl: string;
  /** The original File object. */
  file: File;
  /** MIME type. */
  mimeType: string;
  /** Size in bytes. */
  sizeBytes: number;
}

interface ImagePastePreviewProps {
  images: PastedImage[];
  onRemove: (index: number) => void;
  onSend: () => void;
}
```

Flow:
1. Listen for `paste` event on the chat input container
2. Check `event.clipboardData.items` for image MIME types
3. Create `PastedImage` with `URL.createObjectURL`
4. Show preview thumbnails above the input area
5. User can remove individual images or send all with the message
6. On send, images are included as attachments in the `SendMessageRequest`
7. Clean up object URLs on unmount or removal

Component:

```
ChatInput (enhanced)
├── ImagePastePreview { images, onRemove, onSend }
│   └── div.flex.gap-2 (horizontal scroll)
│       └── ImageThumbnail[] { image, onRemove }
│           ├── img src={previewUrl} className="h-20 w-20 object-cover rounded"
│           └── Button "×" (remove, absolute top-right)
├── Textarea (existing input)
└── Button "Send"
```

### 21.6 Feature: Theme Transitions via View Transitions API

When toggling between light/dark themes, use the View Transitions API for a smooth
crossfade instead of an instant swap.

```typescript
function toggleTheme(newTheme: "light" | "dark") {
  if (!document.startViewTransition) {
    // Fallback: instant swap
    applyTheme(newTheme);
    return;
  }

  document.startViewTransition(() => {
    applyTheme(newTheme);
  });
}
```

CSS:

```css
::view-transition-old(root),
::view-transition-new(root) {
  animation-duration: 200ms;
  animation-timing-function: ease-in-out;
}
```

### 21.7 Feature: RTL Detection

Detect right-to-left text in chat messages and apply `dir="rtl"` on the message
container.

```typescript
/** Returns true if the majority of characters in the text are RTL. */
function isRtlText(text: string): boolean {
  // Match Arabic, Hebrew, Syriac, Thaana, NKo ranges
  const rtlChars = text.match(/[\u0591-\u07FF\u200F\u202B\u202E\uFB1D-\uFDFD\uFE70-\uFEFC]/g);
  const ltrChars = text.match(/[A-Za-z\u00C0-\u024F\u1E00-\u1EFF]/g);

  const rtlCount = rtlChars?.length ?? 0;
  const ltrCount = ltrChars?.length ?? 0;

  return rtlCount > ltrCount;
}
```

Applied in message rendering:

```typescript
<div dir={isRtlText(message.content) ? "rtl" : "ltr"}>
  {renderedContent}
</div>
```

### 21.8 Feature: Settings Persistence

All user preferences are stored in `localStorage` and loaded on mount.

```typescript
interface UserPreferences {
  /** Light or dark theme. */
  theme: "light" | "dark" | "system";
  /** Focus mode (sidebar collapsed). */
  focusMode: boolean;
  /** Chat panel split ratio (0.3 to 0.7). */
  splitRatio: number;
  /** Show model thinking/reasoning in chat. */
  showThinking: boolean;
  /** Auto-scroll in chat. */
  autoScroll: boolean;
  /** Log viewer level filter. */
  logLevelFilter: string;
}

const STORAGE_KEY = "rune:preferences";

function loadPreferences(): UserPreferences {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_PREFERENCES;
    return { ...DEFAULT_PREFERENCES, ...JSON.parse(raw) };
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

function savePreferences(prefs: UserPreferences): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
}

const DEFAULT_PREFERENCES: UserPreferences = {
  theme: "system",
  focusMode: false,
  splitRatio: 0.5,
  showThinking: false,
  autoScroll: true,
  logLevelFilter: "all",
};
```

Hook: `ui/src/hooks/use-preferences.ts`

```typescript
import { useState, useCallback, useEffect } from "react";

export function usePreferences() {
  const [prefs, setPrefs] = useState<UserPreferences>(loadPreferences);

  const updatePreference = useCallback(
    <K extends keyof UserPreferences>(key: K, value: UserPreferences[K]) => {
      setPrefs((prev) => {
        const next = { ...prev, [key]: value };
        savePreferences(next);
        return next;
      });
    },
    [],
  );

  // Sync across tabs
  useEffect(() => {
    const handler = (e: StorageEvent) => {
      if (e.key === STORAGE_KEY && e.newValue) {
        try {
          setPrefs({ ...DEFAULT_PREFERENCES, ...JSON.parse(e.newValue) });
        } catch { /* ignore */ }
      }
    };
    window.addEventListener("storage", handler);
    return () => window.removeEventListener("storage", handler);
  }, []);

  return { prefs, updatePreference };
}
```

### 21.9 Edge Cases

- **Focus mode with small screen**: On viewports < 768px, the sidebar is already hidden (mobile layout). Focus mode toggle is hidden on mobile.
- **Smart scroll with very fast messages**: Batch scroll updates using `requestAnimationFrame` to avoid layout thrashing.
- **Image paste: non-image clipboard content**: Ignored. Only `image/*` MIME types trigger the preview.
- **Image paste: very large image**: Show file size in the thumbnail. Warn if > 10 MB. Do not block — let the backend reject if too large.
- **View Transitions API not supported**: Graceful fallback to instant theme swap. Check `document.startViewTransition` before calling.
- **RTL detection with mixed content**: If text is < 10 characters, skip RTL detection and default to `ltr`.
- **localStorage quota exceeded**: Catch the error in `savePreferences` and show a toast warning. Continue with in-memory state.
- **Settings sync across tabs**: The `storage` event listener keeps all open tabs in sync when preferences change.
- **Copy-as-markdown for messages with images**: Include image alt text as `![alt](url)` in the copied markdown.

### 21.10 Integration Test Scenarios

UI tests (Playwright):

```typescript
/** Focus mode toggle hides sidebar and persists to localStorage. */
test("focus_mode_toggle_persists");

/** Smart scroll: new messages button appears when scrolled up and shows count. */
test("smart_scroll_new_messages_button");

/** Smart scroll: clicking new messages button scrolls to bottom. */
test("smart_scroll_click_scrolls_to_bottom");

/** Copy-as-markdown copies message content to clipboard. */
test("copy_as_markdown");

/** Image paste shows preview thumbnails above input. */
test("image_paste_preview");

/** Image paste: remove button removes individual image. */
test("image_paste_remove");

/** Theme transition uses View Transitions API when available. */
test("theme_transition_smooth");

/** RTL detection applies dir=rtl to Arabic text messages. */
test("rtl_detection_arabic");

/** Settings persistence: preferences survive page reload. */
test("settings_persistence_reload");

/** Settings persistence: changes sync across tabs. */
test("settings_persistence_cross_tab");
```

### 21.11 Acceptance Criteria

- [ ] Focus mode toggle button exists in the header and toggles sidebar visibility
- [ ] Focus mode state persists in localStorage across page reloads
- [ ] Smart scroll auto-scrolls when user is at the bottom of the chat
- [ ] Smart scroll shows "N new messages" button when user scrolls up and new messages arrive
- [ ] Clicking "N new messages" button scrolls to bottom and resets counter
- [ ] Each assistant message has a "Copy" and "Copy as Markdown" action on hover
- [ ] Pasting an image into the chat input shows a preview thumbnail
- [ ] Preview thumbnails can be individually removed
- [ ] Images are sent as attachments when the user clicks Send
- [ ] Theme toggle uses View Transitions API for smooth crossfade (with fallback)
- [ ] RTL text (Arabic, Hebrew) is detected and `dir="rtl"` is applied to the message container
- [ ] All user preferences (theme, focusMode, splitRatio, showThinking, autoScroll, logLevelFilter) are persisted in localStorage
- [ ] Preferences sync across browser tabs via the `storage` event
- [ ] localStorage errors are caught gracefully without crashing

### 21.12 Dependencies

**npm** (no new packages):
- All features use existing browser APIs (`IntersectionObserver`, `View Transitions API`, `localStorage`, `navigator.clipboard`)
- `lucide-react` for icons (Maximize2, Minimize2, ArrowDown, Copy, FileText)

---

## Cross-Phase Summary

### New Files Created

| Phase | File | Type |
|---|---|---|
| 15 | `crates/rune-gateway/src/log_layer.rs` | Rust |
| 15 | `crates/rune-gateway/src/ws_logs.rs` | Rust |
| 15 | `crates/rune-gateway/tests/log_tests.rs` | Rust |
| 16 | `ui/src/components/ui/json-tree-view.tsx` | TypeScript |
| 17 | `ui/src/hooks/use-agents.ts` | TypeScript |
| 17 | `ui/src/hooks/use-skills.ts` | TypeScript |
| 18 | `crates/rune-browser/Cargo.toml` | TOML |
| 18 | `crates/rune-browser/src/lib.rs` | Rust |
| 18 | `crates/rune-browser/src/browser.rs` | Rust |
| 18 | `crates/rune-browser/src/ax_tree.rs` | Rust |
| 18 | `crates/rune-browser/src/snapshot.rs` | Rust |
| 18 | `crates/rune-browser/src/tool.rs` | Rust |
| 18 | `crates/rune-browser/src/error.rs` | Rust |
| 19 | `crates/rune-gateway/src/a2ui.rs` | Rust |
| 19 | `ui/src/components/a2ui/A2uiPanel.tsx` | TypeScript |
| 19 | `ui/src/components/a2ui/A2uiInline.tsx` | TypeScript |
| 19 | `ui/src/components/a2ui/A2uiRenderer.tsx` | TypeScript |
| 19 | `ui/src/components/a2ui/A2uiCard.tsx` | TypeScript |
| 19 | `ui/src/components/a2ui/A2uiTable.tsx` | TypeScript |
| 19 | `ui/src/components/a2ui/A2uiList.tsx` | TypeScript |
| 19 | `ui/src/components/a2ui/A2uiForm.tsx` | TypeScript |
| 19 | `ui/src/components/a2ui/A2uiChart.tsx` | TypeScript |
| 19 | `ui/src/components/a2ui/A2uiKv.tsx` | TypeScript |
| 19 | `ui/src/components/a2ui/A2uiProgress.tsx` | TypeScript |
| 19 | `ui/src/components/a2ui/A2uiCode.tsx` | TypeScript |
| 19 | `ui/src/components/a2ui/use-a2ui.ts` | TypeScript |
| 21 | `ui/src/hooks/use-preferences.ts` | TypeScript |

### Modified Files

| Phase | File | Change |
|---|---|---|
| 15 | `crates/rune-gateway/src/state.rs` | Add `log_buffer: Arc<LogBuffer>` field |
| 15 | `crates/rune-gateway/src/lib.rs` | Register `/ws/logs` and `/api/logs/snapshot` routes |
| 15 | `crates/rune-config/src/lib.rs` | Add `LogViewerConfig` |
| 15 | `ui/src/lib/api-types.ts` | Add `LogRecord`, `LogSnapshotResponse` types |
| 16 | `ui/src/routes/_admin/debug.tsx` | Rewrite with JsonTreeView, history, gap detection |
| 16 | `ui/src/lib/api-types.ts` | Add debug page types |
| 17 | `crates/rune-gateway/src/routes.rs` | Extend `AgentResponse`, add `/agents/:id`, `/skills/:name` |
| 17 | `crates/rune-gateway/src/error.rs` | Add `AgentNotFound`, `SkillNotFound` variants |
| 17 | `ui/src/routes/_admin/agents.tsx` | Add detail drawer, enhanced table |
| 17 | `ui/src/routes/_admin/skills.tsx` | Add detail drawer, rescan button |
| 17 | `ui/src/lib/api-types.ts` | Add agent/skill detail types |
| 18 | `Cargo.toml` | Add `chromiumoxide` to workspace deps, `rune-browser` to members |
| 18 | `crates/rune-config/src/lib.rs` | Add `BrowserConfig` |
| 18 | App startup code | Register `BrowseTool` when browser.enabled |
| 19 | `crates/rune-gateway/src/ws_rpc.rs` | Add `a2ui.form_submit` and `a2ui.action` RPC methods |
| 19 | `ui/src/lib/api-types.ts` | Add all A2UI types |
| 19 | `ui/src/routes/_admin/chat.tsx` | Integrate A2uiInline and A2uiPanel |
| 20 | `ui/src/routes/_admin/cron.tsx` | Add ScheduleEditor, PayloadEditor, clone, filtering, sorting |
| 20 | `ui/src/routes/_admin/index.tsx` | Add Connection Status, Auth Mode, Tools, Quick Actions cards |
| 20 | `ui/src/routes/_admin/channels.tsx` | Add per-channel status cards with connection indicators |
| 20 | `ui/src/routes/_admin/settings.tsx` | Add TTS, STT, Device Pairing sections |
| 21 | `ui/src/routes/_admin/chat.tsx` | Add smart scroll, copy-as-markdown, image paste, RTL detection |
| 21 | Layout component | Add focus mode toggle |
| 21 | `ui/src/components/theme-toggle.tsx` | Add View Transitions API support |

### Dependency Summary

**Rust workspace additions**:
- `chromiumoxide = { version = "0.7", features = ["tokio-runtime"], default-features = false }` (Phase 18 only)

**npm additions**:
- `qrcode.react = "^4.2.0"` (Phase 20, optional, for device pairing QR code)

All other dependencies are already present in the workspace.
