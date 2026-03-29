# Phases 1-7: Implementation Specifications

> **Generated**: 2026-03-15
> **Source of truth**: `docs/IMPLEMENTATION-PHASES.md` for phase sequencing, plus this spec file for exact phase-1-7 implementation detail.
> **Codebase edition**: Rust 2024, workspace resolver v2

---

## Table of Contents

- [Phase 1 — Chat Page (UI)](#phase-1--chat-page-ui)
- [Phase 2 — WebSocket Gateway RPC Protocol](#phase-2--websocket-gateway-rpc-protocol)
- [Phase 3 — Multi-Channel Adapters](#phase-3--multi-channel-adapters)
- [Phase 4 — LaneQueue Concurrency Model](#phase-4--lanequeue-concurrency-model)
- [Phase 5 — Hot-Reloading Skills System](#phase-5--hot-reloading-skills-system)
- [Phase 6 — MCP Client](#phase-6--mcp-client)
- [Phase 7 — Expanded LLM Providers](#phase-7--expanded-llm-providers)

---

## Phase 1 — Chat Page (UI)

### 1.1 Overview

Build the admin chat page that connects to the existing backend endpoints:
- `POST /sessions/{id}/messages` (send a message, triggers turn execution)
- `GET /sessions/{id}/transcript` (load conversation history)
- `GET /ws` WebSocket (live event streaming via `session.send`, `session.transcript` RPC)

The UI already has React 19 + TanStack Router + Tailwind 4 + TanStack Query. The `ui/src/routes/_admin/chat.tsx` route file already exists. This phase fills in the real chat UI components.

### 1.2 NPM Dependencies

Already present in `package.json`:
- `marked` ^17.0.4
- `dompurify` ^3.3.3
- `@types/dompurify` ^3.0.5

No new dependencies required.

### 1.3 File Inventory

| File | Purpose |
|---|---|
| `ui/src/routes/_admin/chat.tsx` | Route component: session selector + chat thread + sidebar layout |
| `ui/src/components/chat/ChatThread.tsx` | Scrollable message list with role-based grouping and auto-scroll |
| `ui/src/components/chat/ChatMessage.tsx` | Single message bubble: avatar, role badge, timestamp, markdown body |
| `ui/src/components/chat/ChatInput.tsx` | Textarea: Enter to send, Shift+Enter newline, image paste, disabled while loading |
| `ui/src/components/chat/ToolCard.tsx` | Inline tool call/result display, collapsible accordion |
| `ui/src/components/chat/ThinkingBlock.tsx` | Collapsible `<thinking>` extraction with toggle |
| `ui/src/components/chat/MarkdownRenderer.tsx` | `marked` + `DOMPurify` sanitized rendering with code highlighting |
| `ui/src/components/chat/ChatSidebar.tsx` | Resizable split panel (0.4-0.7 ratio) for long tool outputs |
| `ui/src/components/chat/CopyMarkdown.tsx` | Copy-as-markdown button with "Copied!" feedback state |
| `ui/src/components/chat/ImageAttachment.tsx` | Clipboard paste preview with remove button |
| `ui/src/hooks/use-chat.ts` | Custom hook: transcript polling, send message, WS subscription |

### 1.4 TypeScript Types

```typescript
// ui/src/hooks/use-chat.ts

/** Matches the JSON shape returned by the `session.transcript` RPC method
 *  and `GET /sessions/{id}/transcript`. */
interface TranscriptEntry {
  id: string;
  turn_id: string | null;
  seq: number;
  kind:
    | "user_message"
    | "assistant_message"
    | "tool_request"
    | "tool_result"
    | "approval_request"
    | "approval_response"
    | "status_note"
    | "subagent_result";
  payload: Record<string, unknown>;
  created_at: string; // ISO-8601
}

/** Matches the JSON shape returned by `session.send` RPC. */
interface SendMessageResult {
  turn_id: string;
  assistant_reply: string | null;
  usage: {
    prompt_tokens: number;
    completion_tokens: number;
  };
  latency_ms: number;
}

/** Matches `session.list` RPC result items. */
interface SessionListItem {
  id: string;
  kind: string;
  status: string;
  channel: string | null;
  created_at: string;
}

/** WebSocket inbound event frame. */
interface WsEventFrame {
  type: "event";
  event: string;
  payload: Record<string, unknown>;
  seq: number;
  stateVersion: number;
}

/** WebSocket response frame. */
interface WsResFrame {
  type: "res";
  id: string;
  ok: boolean;
  stateVersion: number;
  payload?: Record<string, unknown>;
  error?: { code: string; message: string };
}

type WsFrame = WsEventFrame | WsResFrame;
```

### 1.5 Hook: `use-chat.ts`

```typescript
// ui/src/hooks/use-chat.ts

interface UseChatOptions {
  sessionId: string | null;
}

interface UseChatReturn {
  /** Current transcript entries, sorted by seq ascending. */
  transcript: TranscriptEntry[];
  /** True while the initial transcript is loading. */
  isLoading: boolean;
  /** True while a message send is in flight. */
  isSending: boolean;
  /** Error from the last operation, if any. */
  error: string | null;
  /** Send a user message. Returns the assistant reply or throws. */
  sendMessage: (content: string) => Promise<SendMessageResult>;
  /** Force refetch of transcript. */
  refetch: () => void;
}

export function useChat(options: UseChatOptions): UseChatReturn;
```

**Implementation requirements**:

1. Use `@tanstack/react-query` with key `["transcript", sessionId]`.
2. `queryFn` sends a WS RPC `session.transcript` request (if WS connected) or falls back to `GET /sessions/{sessionId}/transcript`.
3. On mount, send a WS RPC `subscribe` with `{"session_id": sessionId}`.
4. On unmount, send `unsubscribe` with `{"session_id": sessionId}`.
5. On receiving a `turn_completed` or `transcript_item` event for the subscribed session, call `queryClient.invalidateQueries(["transcript", sessionId])`.
6. `sendMessage` sends a WS RPC `session.send` with `{"session_id": sessionId, "content": content}`.
7. While `sendMessage` is in flight, `isSending` is true and the ChatInput is disabled.
8. Reconnection: if the WebSocket disconnects, fall back to polling every 3 seconds via `refetchInterval`. When WS reconnects, disable polling and re-subscribe.

### 1.6 Component Specifications

#### ChatThread.tsx

- Renders `TranscriptEntry[]` grouped by `turn_id`.
- For each entry, dispatch to `ChatMessage` (user/assistant), `ToolCard` (tool_request/tool_result), `ThinkingBlock` (assistant_message with `<thinking>` tags).
- Auto-scrolls to bottom on new entries. Uses `useRef` + `scrollIntoView({ behavior: "smooth" })`.
- Shows a "scroll to bottom" FAB when user scrolls up more than 200px from the bottom.
- Empty state: centered "Send a message to start" text when transcript is empty.

#### ChatMessage.tsx

Props:
```typescript
interface ChatMessageProps {
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: string;
  isStreaming?: boolean;
}
```

- User messages: right-aligned, primary color background.
- Assistant messages: left-aligned, muted background. Content rendered via `MarkdownRenderer`.
- Shows relative timestamp (e.g., "2m ago") via `date-fns.formatDistanceToNow`.

#### ChatInput.tsx

Props:
```typescript
interface ChatInputProps {
  onSend: (content: string) => void;
  disabled: boolean;
  placeholder?: string;
}
```

- `<textarea>` with auto-resize (min 1 row, max 8 rows).
- Enter sends (calls `onSend` with trimmed content, then clears).
- Shift+Enter inserts a newline.
- Image paste: intercept `onPaste`, extract `image/*` from `clipboardData.items`, show `ImageAttachment` preview. (Image sending is a future feature; for now, show a toast "Image attachments coming soon").
- Disabled state: reduced opacity, no interaction.

#### ToolCard.tsx

Props:
```typescript
interface ToolCardProps {
  toolName: string;
  arguments: Record<string, unknown>;
  result?: string;
  isError?: boolean;
  isCollapsed?: boolean;
}
```

- Renders as an accordion: header shows tool name + status icon (spinner while pending, check for success, X for error).
- Collapsed by default if result length > 500 characters.
- Arguments rendered as syntax-highlighted JSON.
- Result rendered as preformatted text. If `isError`, red border and error icon.

#### ThinkingBlock.tsx

- Parses `<thinking>...</thinking>` tags from assistant content.
- Renders thinking content in a collapsible block with a "brain" icon.
- Collapsed by default.
- Remaining content (outside thinking tags) rendered normally via MarkdownRenderer.

#### MarkdownRenderer.tsx

```typescript
interface MarkdownRendererProps {
  content: string;
}
```

- Uses `marked.parse(content)` to produce HTML.
- Sanitizes with `DOMPurify.sanitize(html)`.
- Renders via `dangerouslySetInnerHTML`.
- CSS classes for: headings, code blocks (with copy button), inline code, links (open in new tab), lists, blockquotes, tables.

#### ChatSidebar.tsx

- Resizable panel using CSS `resize: horizontal` or a drag handle.
- Default width ratio: 0.4 of the chat area.
- Min ratio: 0.3, max ratio: 0.7.
- Contains the session list, session status card, or expanded tool output.

#### CopyMarkdown.tsx

```typescript
interface CopyMarkdownProps {
  content: string;
}
```

- Button with clipboard icon.
- On click: `navigator.clipboard.writeText(content)`.
- Shows "Copied!" tooltip for 2 seconds, then reverts to clipboard icon.

#### ImageAttachment.tsx

```typescript
interface ImageAttachmentProps {
  file: File;
  onRemove: () => void;
}
```

- Shows a thumbnail preview via `URL.createObjectURL(file)`.
- "X" button to remove.
- Cleans up object URL on unmount.

### 1.7 Wire Protocol (reuses existing)

The chat page uses only existing RPC methods. No new backend endpoints are needed.

**Send a message via WebSocket RPC**:

Request:
```json
{
  "type": "req",
  "id": "msg-001",
  "method": "session.send",
  "params": {
    "session_id": "01958d2a-3b4c-7def-8a90-123456789abc",
    "content": "Hello, what can you do?",
    "model": null
  }
}
```

Response (success):
```json
{
  "type": "res",
  "id": "msg-001",
  "ok": true,
  "stateVersion": 42,
  "payload": {
    "turn_id": "01958d2a-4444-7def-8a90-aabbccddeeff",
    "assistant_reply": "I can help you with file operations, code execution, and more.",
    "usage": {
      "prompt_tokens": 1200,
      "completion_tokens": 85
    },
    "latency_ms": 3200
  }
}
```

Response (error - session not found):
```json
{
  "type": "res",
  "id": "msg-001",
  "ok": false,
  "stateVersion": 42,
  "error": {
    "code": "not_found",
    "message": "session not found: 01958d2a-0000-0000-0000-000000000000"
  }
}
```

**Load transcript via WebSocket RPC**:

Request:
```json
{
  "type": "req",
  "id": "tx-001",
  "method": "session.transcript",
  "params": {
    "session_id": "01958d2a-3b4c-7def-8a90-123456789abc"
  }
}
```

Response:
```json
{
  "type": "res",
  "id": "tx-001",
  "ok": true,
  "stateVersion": 42,
  "payload": [
    {
      "id": "01958d2a-5555-7def-8a90-000000000001",
      "turn_id": "01958d2a-4444-7def-8a90-aabbccddeeff",
      "seq": 0,
      "kind": "user_message",
      "payload": {
        "message": { "role": "user", "content": "Hello" }
      },
      "created_at": "2026-03-15T10:00:00Z"
    },
    {
      "id": "01958d2a-5555-7def-8a90-000000000002",
      "turn_id": "01958d2a-4444-7def-8a90-aabbccddeeff",
      "seq": 1,
      "kind": "assistant_message",
      "payload": {
        "content": "Hello! How can I help you?"
      },
      "created_at": "2026-03-15T10:00:03Z"
    }
  ]
}
```

**Event frame (live update)**:
```json
{
  "type": "event",
  "event": "turn_completed",
  "payload": {
    "session_id": "01958d2a-3b4c-7def-8a90-123456789abc",
    "kind": "turn_completed",
    "data": {
      "session_id": "01958d2a-3b4c-7def-8a90-123456789abc",
      "turn_id": "01958d2a-4444-7def-8a90-aabbccddeeff",
      "assistant_reply": "Done!",
      "prompt_tokens": 500,
      "completion_tokens": 20
    }
  },
  "seq": 17,
  "stateVersion": 43
}
```

### 1.8 Edge Cases

1. **Empty session**: Show centered placeholder text, not an error.
2. **WS disconnect mid-send**: `sendMessage` promise rejects with a network error. Show a toast: "Connection lost. Retrying..." and fall back to HTTP `POST /sessions/{id}/messages`.
3. **Rapid message sends**: Disable input while `isSending` is true. Queue is not needed because the backend serializes turns per session.
4. **Very long messages**: Textarea auto-grows up to 8 rows, then scrolls internally. No client-side length limit (the backend enforces model context limits).
5. **Malformed transcript payload**: If `payload` is missing expected fields, render a gray "Unknown message format" placeholder instead of crashing.
6. **Stale transcript after reconnect**: On WS reconnect, invalidate the transcript query to force a full refetch.
7. **XSS in markdown**: DOMPurify strips all dangerous tags/attributes. Test with `<img onerror=alert(1)>` and `<script>` inputs.
8. **Session deleted while chatting**: If `session.transcript` returns `not_found`, show "This session no longer exists" and redirect to session list.

### 1.9 Integration Test Scenarios

These are Playwright E2E tests in `ui/tests/`:

```typescript
// ui/tests/chat.spec.ts

test("chat page loads transcript for selected session", async ({ page }) => {
  // Precondition: a session with at least one turn exists
  // Action: navigate to /chat?session=<id>
  // Assert: transcript entries visible in the thread
});

test("sending a message appends user bubble and assistant reply", async ({ page }) => {
  // Action: type "Hello" in input, press Enter
  // Assert: user bubble with "Hello" appears
  // Assert: after backend responds, assistant bubble appears
  // Assert: input is re-enabled
});

test("tool card renders and collapses", async ({ page }) => {
  // Precondition: session with a tool_request + tool_result in transcript
  // Assert: ToolCard header visible with tool name
  // Action: click to expand
  // Assert: arguments JSON and result text visible
});

test("thinking block is collapsed by default", async ({ page }) => {
  // Precondition: assistant message containing <thinking>...</thinking>
  // Assert: thinking content is hidden
  // Action: click toggle
  // Assert: thinking content becomes visible
});

test("markdown renderer sanitizes XSS", async ({ page }) => {
  // Precondition: assistant message with "<img onerror=alert(1)>"
  // Assert: no alert dialog, sanitized output rendered
});

test("copy markdown button copies content", async ({ page }) => {
  // Action: click copy button on a message
  // Assert: clipboard contains the message content
  // Assert: button shows "Copied!" feedback
});

test("empty session shows placeholder", async ({ page }) => {
  // Precondition: session with zero transcript items
  // Assert: "Send a message to start" placeholder visible
});

test("websocket disconnect falls back to polling", async ({ page }) => {
  // Action: simulate WS disconnect
  // Assert: chat still loads transcript (via HTTP polling)
  // Assert: reconnection toast appears
});
```

### 1.10 Acceptance Criteria

- [ ] Chat route loads at `/chat` and shows a session selector in the sidebar
- [ ] Selecting a session loads its transcript with all message types rendered
- [ ] User messages render right-aligned; assistant messages render left-aligned with markdown
- [ ] Typing a message and pressing Enter sends via WS RPC and shows the reply
- [ ] Tool calls render as collapsible ToolCards with arguments and results
- [ ] `<thinking>` blocks are extracted and shown in a collapsible ThinkingBlock
- [ ] Markdown is rendered with sanitization (no XSS)
- [ ] Copy-as-markdown button works with feedback state
- [ ] Auto-scroll to bottom on new messages; scroll-to-bottom FAB when scrolled up
- [ ] WS disconnect falls back to HTTP polling; reconnect re-subscribes
- [ ] Empty session shows a placeholder, not an error
- [ ] Chat entry appears as first item in `AdminNavbar` and `AdminBottomNav`

---

## Phase 2 — WebSocket Gateway RPC Protocol

### 2.1 Overview

Phase 2 is **already implemented**. The existing `crates/rune-gateway/src/ws.rs` and `crates/rune-gateway/src/ws_rpc.rs` already provide:

- Request frame: `{"type":"req","id":"<uuid>","method":"<string>","params":{...}}`
- Response frame: `{"type":"res","id":"<uuid>","ok":true|false,"stateVersion":<n>,"payload":{...},"error":{...}}`
- Event frame: `{"type":"event","event":"<string>","payload":{...},"seq":<n>,"stateVersion":<n>}`
- Sequence numbering via `EVENT_SEQ` atomic counter
- `stateVersion` tracking via `STATE_VERSION` atomic counter
- `system.lagged` event for gap detection
- RPC method dispatch for: `subscribe`, `unsubscribe`, `session.list`, `session.get`, `session.create`, `session.send`, `session.transcript`, `session.status`, `cron.list`, `cron.status`, `runtime.lanes`, `skills.list`, `skills.reload`, `skills.enable`, `skills.disable`, `health`, `status`

This spec documents the protocol for reference by other phases.

### 2.2 Exact Rust Types (existing)

```rust
// crates/rune-gateway/src/ws.rs — already implemented

/// Inbound frame from a WebSocket client.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum InboundFrame {
    /// RPC request.
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
    frame_type: &'static str, // always "res"
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
    frame_type: &'static str, // always "event"
    event: String,
    payload: Value,
    seq: u64,
    #[serde(rename = "stateVersion")]
    state_version: u64,
}

/// Per-connection subscription state.
#[derive(Default)]
pub struct ConnState {
    subscribed_sessions: HashSet<String>,
    subscribed_events: HashSet<String>,
    subscribe_all: bool,
}
```

```rust
// crates/rune-gateway/src/ws_rpc.rs — already implemented

/// Error returned from an RPC method.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

/// Trait for RPC dispatch (allows test stubs).
#[async_trait]
pub trait RpcDispatch: Send + Sync {
    async fn dispatch(&self, method: &str, params: Value) -> Result<Value, RpcError>;
}
```

### 2.3 Wire Protocol Reference

#### RPC Methods

| Method | Params | Returns | Error codes |
|---|---|---|---|
| `subscribe` | `session_id?`, `event?`, `all?` | `{subscribed: {session_id, event, all}}` | `bad_request` |
| `unsubscribe` | `session_id?`, `event?`, `all?` | `{unsubscribed: {session_id, event, all}}` | `bad_request` |
| `session.list` | `limit?` (u64, max 500), `channel?`, `active?` (minutes) | `[{id, kind, status, channel, created_at}]` | `internal` |
| `session.get` | `session_id` (required UUID) | `{id, kind, status, ...}` | `bad_request`, `not_found`, `internal` |
| `session.create` | `kind?`, `workspace_root?`, `requester_session_id?`, `channel_ref?` | `{id, kind, status, created_at}` | `bad_request`, `internal` |
| `session.send` | `session_id` (required UUID), `content` (required string), `model?` | `{turn_id, assistant_reply, usage, latency_ms}` | `bad_request`, `not_found`, `internal` |
| `session.transcript` | `session_id` (required UUID) | `[{id, turn_id, seq, kind, payload, created_at}]` | `bad_request`, `not_found`, `internal` |
| `session.status` | `session_id` (required UUID) | Session status card JSON | `bad_request`, `not_found`, `internal` |
| `cron.list` | `include_disabled?` (bool) | `[{id, name, enabled, ...}]` | `internal` |
| `cron.status` | (none) | `{total_jobs, enabled_jobs, due_jobs}` | `internal` |
| `runtime.lanes` | (none) | `{enabled, lanes: {main, subagent, cron}}` | (never fails) |
| `skills.list` | (none) | `[{name, description, enabled, source_dir, binary_path}]` | (never fails) |
| `skills.reload` | (none) | `{success, discovered, loaded, removed}` | (never fails) |
| `skills.enable` | `name` (required string) | `{name, enabled: true}` | `not_found` |
| `skills.disable` | `name` (required string) | `{name, enabled: false}` | `not_found` |
| `health` | (none) | `{status, service, version, uptime_seconds, ...}` | `internal` |
| `status` | (none) | Full daemon status JSON | `internal` |

#### Error Codes

| Code | Meaning |
|---|---|
| `parse_error` | Frame is not valid JSON |
| `bad_request` | Missing or invalid required parameter |
| `not_found` | Referenced entity does not exist |
| `method_not_found` | Unknown RPC method name |
| `internal` | Server-side error (store, runtime) |

#### Event Types

| Event | Payload | state_changed |
|---|---|---|
| `session_created` | `{session_id, kind, status}` | true |
| `turn_completed` | `{session_id, turn_id, assistant_reply, prompt_tokens, completion_tokens}` | true |
| `transcript_item` | `{session_id, kind, data}` | true |
| `system.lagged` | `{missed: <count>}` | false |

### 2.4 Acceptance Criteria

- [x] Request/response framing with `type`, `id`, `ok`, `stateVersion`
- [x] Event framing with monotonic `seq` counter
- [x] `stateVersion` increments on state-changing operations
- [x] Gap detection via `system.lagged` event
- [x] Per-connection subscription filtering (session, event, all)
- [x] Legacy `subscribe` frame backward compatibility
- [x] RPC dispatch to all session/cron/skills/system methods
- [x] Malformed frame returns `parse_error` response
- [x] Unknown method returns `method_not_found` response

---

## Phase 3 — Multi-Channel Adapters

### 3.1 Overview

Extend `crates/rune-channels/` with real adapter implementations for Discord, Slack, WhatsApp, and Signal. The `ChannelAdapter` trait, types (`InboundEvent`, `OutboundAction`, `ChannelError`, `DeliveryReceipt`, `ChannelMessage`), and factory function (`create_adapter`) already exist. The adapter stubs for all four channels already exist. This phase replaces the stubs with real implementations.

### 3.2 Exact Rust Types (existing trait — no changes)

```rust
// crates/rune-channels/src/lib.rs — already defined

#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    /// Receive the next inbound event from the channel.
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError>;
    /// Send an outbound action to the channel.
    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError>;
}
```

```rust
// crates/rune-channels/src/types.rs — already defined

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelMessage {
    pub channel_id: ChannelId,
    pub raw_chat_id: String,
    pub sender: String,
    pub content: String,
    pub attachments: Vec<AttachmentRef>,
    pub timestamp: DateTime<Utc>,
    pub provider_message_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    pub provider_message_id: String,
    pub delivered_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboundEvent {
    Message(ChannelMessage),
    Reaction { channel_id: ChannelId, message_id: String, emoji: String, user: String },
    Edit { channel_id: ChannelId, message_id: String, new_content: String },
    Delete { channel_id: ChannelId, message_id: String },
    MemberJoin { channel_id: ChannelId, user: String },
    MemberLeave { channel_id: ChannelId, user: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundAction {
    Send { channel_id: ChannelId, chat_id: String, content: String },
    Reply { channel_id: ChannelId, chat_id: String, reply_to: String, content: String },
    Edit { channel_id: ChannelId, chat_id: String, message_id: String, new_content: String },
    React { channel_id: ChannelId, message_id: String, emoji: String },
    Delete { channel_id: ChannelId, chat_id: String, message_id: String },
    SendTypingIndicator { channel_id: ChannelId, chat_id: String },
    SendInlineKeyboard { channel_id: ChannelId, chat_id: String, content: String, buttons: Vec<(String, String)> },
}

#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("provider error: {message}")]
    Provider { message: String },
    #[error("not implemented")]
    NotImplemented,
    #[error("connection lost: {reason}")]
    ConnectionLost { reason: String },
}
```

### 3.3 Config (existing — no changes needed)

```rust
// crates/rune-config/src/lib.rs — already defined

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    pub enabled: Vec<String>,
    pub telegram_token: Option<String>,
    pub discord_token: Option<String>,
    pub discord_guild_id: Option<String>,
    pub discord_channel_ids: Vec<String>,
    pub slack_bot_token: Option<String>,
    pub slack_app_token: Option<String>,
    pub slack_listen_addr: Option<String>,
    pub whatsapp_access_token: Option<String>,
    pub whatsapp_phone_number_id: Option<String>,
    pub whatsapp_verify_token: Option<String>,
    pub whatsapp_listen_addr: Option<String>,
    pub signal_number: Option<String>,
    pub signal_api_url: Option<String>,
}
```

### 3.4 Discord Adapter

**File**: `crates/rune-channels/src/discord.rs`

```rust
/// Discord adapter using the Gateway WebSocket (for receiving) and REST API (for sending).
///
/// Connection flow:
/// 1. Connect to `wss://gateway.discord.gg/?v=10&encoding=json`
/// 2. Receive Hello (op 10), extract heartbeat_interval
/// 3. Send Identify (op 2) with bot token and intents (GUILD_MESSAGES | MESSAGE_CONTENT)
/// 4. Enter heartbeat loop + message receive loop
///
/// Intents required: `GUILD_MESSAGES` (1 << 9) | `MESSAGE_CONTENT` (1 << 15) = 33280
pub struct DiscordAdapter {
    /// Bot token for authentication.
    token: String,
    /// Guild ID to filter events (empty = all guilds).
    guild_id: String,
    /// Channel IDs to listen on (empty = all channels in guild).
    channel_ids: Vec<String>,
    /// Receiving channel for inbound events from the gateway task.
    rx: tokio::sync::mpsc::Receiver<InboundEvent>,
    /// HTTP client for REST API calls.
    http: reqwest::Client,
}
```

**Discord REST API endpoints used**:

| Operation | Method | URL | Body |
|---|---|---|---|
| Send message | POST | `https://discord.com/api/v10/channels/{channel_id}/messages` | `{"content": "..."}` |
| Edit message | PATCH | `https://discord.com/api/v10/channels/{channel_id}/messages/{message_id}` | `{"content": "..."}` |
| Delete message | DELETE | `https://discord.com/api/v10/channels/{channel_id}/messages/{message_id}` | (none) |
| Add reaction | PUT | `https://discord.com/api/v10/channels/{channel_id}/messages/{message_id}/reactions/{emoji}/@me` | (none) |

**Discord Gateway events mapped**:

| Gateway event | Maps to |
|---|---|
| `MESSAGE_CREATE` | `InboundEvent::Message` |
| `MESSAGE_UPDATE` | `InboundEvent::Edit` |
| `MESSAGE_DELETE` | `InboundEvent::Delete` |
| `MESSAGE_REACTION_ADD` | `InboundEvent::Reaction` |
| `GUILD_MEMBER_ADD` | `InboundEvent::MemberJoin` |
| `GUILD_MEMBER_REMOVE` | `InboundEvent::MemberLeave` |

**Edge cases**:
- Bot ignores its own messages (check `author.bot` field).
- Rate limiting: Discord returns 429 with `retry_after`. The adapter must sleep and retry.
- Gateway disconnect (op 7 Reconnect or op 9 Invalid Session): reconnect with resume if session_id available, otherwise re-identify.
- Heartbeat ACK timeout: if no ACK within `heartbeat_interval * 1.5`, reconnect.
- Message content over 2000 chars: split into multiple messages.

### 3.5 Slack Adapter

**File**: `crates/rune-channels/src/slack.rs`

```rust
/// Slack adapter using Socket Mode (for receiving) and Web API (for sending).
///
/// Socket Mode flow:
/// 1. Connect to `wss://wss-primary.slack.com/link?ticket=<ticket>&app_id=<app_id>`
///    (ticket obtained via `POST https://slack.com/api/apps.connections.open`)
/// 2. Receive `hello` event
/// 3. For each event envelope, send `{"envelope_id": "...", "payload": {}}` acknowledgement
/// 4. Map Slack events to InboundEvent
///
/// Fallback: if `slack_listen_addr` is set, use Events API HTTP server instead of Socket Mode.
pub struct SlackAdapter {
    /// Bot OAuth token (xoxb-...).
    bot_token: String,
    /// App-level token (xapp-...) for Socket Mode.
    app_token: String,
    /// Optional HTTP listener address for Events API fallback.
    listen_addr: Option<String>,
    /// Receiving channel for inbound events.
    rx: tokio::sync::mpsc::Receiver<InboundEvent>,
    /// HTTP client for Web API calls.
    http: reqwest::Client,
}
```

**Slack Web API endpoints used**:

| Operation | Method | URL | Body |
|---|---|---|---|
| Send message | POST | `https://slack.com/api/chat.postMessage` | `{"channel": "...", "text": "..."}` |
| Reply in thread | POST | `https://slack.com/api/chat.postMessage` | `{"channel": "...", "text": "...", "thread_ts": "..."}` |
| Edit message | POST | `https://slack.com/api/chat.update` | `{"channel": "...", "ts": "...", "text": "..."}` |
| Delete message | POST | `https://slack.com/api/chat.delete` | `{"channel": "...", "ts": "..."}` |
| Add reaction | POST | `https://slack.com/api/reactions.add` | `{"channel": "...", "timestamp": "...", "name": "..."}` |

**Slack events mapped**:

| Slack event type | Maps to |
|---|---|
| `message` (subtype null) | `InboundEvent::Message` |
| `message` (subtype `message_changed`) | `InboundEvent::Edit` |
| `message` (subtype `message_deleted`) | `InboundEvent::Delete` |
| `reaction_added` | `InboundEvent::Reaction` |
| `member_joined_channel` | `InboundEvent::MemberJoin` |
| `member_left_channel` | `InboundEvent::MemberLeave` |

**Edge cases**:
- Bot ignores messages from itself (check `bot_id` or `user` == bot user ID).
- Socket Mode disconnects: reconnect by calling `apps.connections.open` again.
- Slack API rate limits (HTTP 429): respect `Retry-After` header.
- URL verification challenge: if using Events API HTTP, respond to `{"type": "url_verification", "challenge": "..."}` with `{"challenge": "..."}`.
- Message content over 4000 chars: split into multiple messages or use a file upload.

### 3.6 WhatsApp Adapter

**File**: `crates/rune-channels/src/whatsapp.rs`

```rust
/// WhatsApp Cloud API adapter.
///
/// Inbound: webhook HTTP server receives POST from Meta.
/// Outbound: REST API calls to `https://graph.facebook.com/v21.0/{phone_number_id}/messages`.
pub struct WhatsAppAdapter {
    /// Permanent access token.
    access_token: String,
    /// Phone number ID from the Business dashboard.
    phone_number_id: String,
    /// Token for webhook verification.
    verify_token: String,
    /// Local address for the webhook listener.
    listen_addr: Option<String>,
    /// Receiving channel for inbound events.
    rx: tokio::sync::mpsc::Receiver<InboundEvent>,
    /// HTTP client for outbound API calls.
    http: reqwest::Client,
}
```

**WhatsApp Cloud API endpoints used**:

| Operation | Method | URL | Body |
|---|---|---|---|
| Send text message | POST | `https://graph.facebook.com/v21.0/{phone_number_id}/messages` | `{"messaging_product":"whatsapp","to":"...","type":"text","text":{"body":"..."}}` |
| Send reaction | POST | same | `{"messaging_product":"whatsapp","to":"...","type":"reaction","reaction":{"message_id":"...","emoji":"..."}}` |
| Mark as read | POST | same | `{"messaging_product":"whatsapp","status":"read","message_id":"..."}` |

**Webhook verification** (GET):
```
GET /webhook?hub.mode=subscribe&hub.verify_token=<verify_token>&hub.challenge=<challenge>
Response: 200 with body = challenge
```

**Webhook event** (POST):
```json
{
  "object": "whatsapp_business_account",
  "entry": [{
    "changes": [{
      "value": {
        "messages": [{
          "from": "15551234567",
          "id": "wamid.xxx",
          "timestamp": "1710000000",
          "type": "text",
          "text": { "body": "Hello" }
        }]
      }
    }]
  }]
}
```

**Edge cases**:
- Webhook signature verification: validate `X-Hub-Signature-256` header using HMAC-SHA256 with app secret.
- Duplicate webhook deliveries: deduplicate by `message_id`. Keep an LRU cache of 1000 recent message IDs.
- Media messages (images, audio): extract media URL from webhook, download via `GET https://graph.facebook.com/v21.0/{media_id}`, create `AttachmentRef`.
- Message templates: required for initiating conversations outside the 24-hour window.

### 3.7 Signal Adapter

**File**: `crates/rune-channels/src/signal.rs`

```rust
/// Signal adapter via signal-cli REST API.
///
/// Inbound: polls `GET /v1/receive/{number}` for new messages.
/// Outbound: `POST /v2/send` to send messages.
pub struct SignalAdapter {
    /// Signal phone number (e.g. "+15551234567").
    number: String,
    /// Base URL of the signal-cli REST daemon (e.g. "http://localhost:8080").
    api_url: String,
    /// Receiving channel for inbound events.
    rx: tokio::sync::mpsc::Receiver<InboundEvent>,
    /// HTTP client for API calls.
    http: reqwest::Client,
}
```

**signal-cli REST API endpoints used**:

| Operation | Method | URL | Body |
|---|---|---|---|
| Receive messages | GET | `{api_url}/v1/receive/{number}` | (none) |
| Send message | POST | `{api_url}/v2/send` | `{"number":"{number}","recipients":["{recipient}"],"message":"..."}` |
| Send reaction | POST | `{api_url}/v1/reactions/{number}` | `{"recipient":"{recipient}","emoji":"...","target_author":"{author}","target_sent_timestamp":{ts}}` |

**Edge cases**:
- signal-cli daemon not running: `receive` returns `ChannelError::ConnectionLost`.
- Poll interval: 1 second between polls. Use `tokio::time::interval`.
- Group messages: `recipient` is the group ID (base64-encoded).
- Attachments: signal-cli returns attachments as file paths; read and create `AttachmentRef`.
- Rate limiting: signal-cli has no rate limit, but Signal servers may throttle. Back off exponentially on 5xx responses.

### 3.8 Crate Dependencies

```toml
# crates/rune-channels/Cargo.toml — dependencies section

[dependencies]
async-trait.workspace = true
chrono.workspace = true
reqwest.workspace = true
rune-config = { path = "../rune-config" }
rune-core = { path = "../rune-core" }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true
tracing.workspace = true
# New: for WebSocket connections (Discord, Slack Socket Mode)
tokio-tungstenite = { version = "0.26", features = ["rustls-tls-native-roots"] }
# New: for HMAC signature verification (WhatsApp webhooks)
hmac = "0.12"
sha2 = "0.10"
# New: for LRU deduplication cache (WhatsApp)
lru = "0.14"
```

### 3.9 Integration Test Scenarios

```rust
// crates/rune-channels/tests/adapter_tests.rs

/// Verify Discord adapter maps a MESSAGE_CREATE gateway event to InboundEvent::Message.
#[tokio::test]
async fn test_discord_message_create_maps_to_inbound_message() {}

/// Verify Discord adapter sends a message via REST and returns a DeliveryReceipt.
#[tokio::test]
async fn test_discord_send_message_returns_receipt() {}

/// Verify Discord adapter ignores its own bot messages.
#[tokio::test]
async fn test_discord_ignores_own_messages() {}

/// Verify Slack adapter maps a Socket Mode message event to InboundEvent::Message.
#[tokio::test]
async fn test_slack_socket_mode_message_maps_to_inbound() {}

/// Verify Slack adapter sends acknowledgement for every envelope.
#[tokio::test]
async fn test_slack_socket_mode_ack() {}

/// Verify Slack adapter handles url_verification challenge.
#[tokio::test]
async fn test_slack_url_verification() {}

/// Verify WhatsApp adapter verifies webhook GET challenge.
#[tokio::test]
async fn test_whatsapp_webhook_verification() {}

/// Verify WhatsApp adapter deduplicates repeated webhook POSTs.
#[tokio::test]
async fn test_whatsapp_deduplication() {}

/// Verify WhatsApp adapter validates X-Hub-Signature-256.
#[tokio::test]
async fn test_whatsapp_signature_validation() {}

/// Verify Signal adapter polls and maps text messages to InboundEvent::Message.
#[tokio::test]
async fn test_signal_receive_text_message() {}

/// Verify Signal adapter sends message via POST /v2/send.
#[tokio::test]
async fn test_signal_send_message() {}

/// Verify factory function `create_adapter` returns the correct adapter type.
#[tokio::test]
async fn test_create_adapter_factory() {}

/// Verify factory rejects unknown adapter kind.
#[tokio::test]
async fn test_create_adapter_unknown_kind() {}
```

All tests use `wiremock` to mock the external APIs. No real API credentials needed.

### 3.10 Acceptance Criteria

- [ ] `create_adapter("discord", &config)` returns a `DiscordAdapter` that connects to the Gateway WebSocket
- [ ] Discord adapter maps MESSAGE_CREATE to `InboundEvent::Message` and ignores bot messages
- [ ] Discord adapter sends messages via REST API with rate-limit retry
- [ ] `create_adapter("slack", &config)` returns a `SlackAdapter` using Socket Mode
- [ ] Slack adapter acknowledges every event envelope
- [ ] Slack adapter sends messages via `chat.postMessage`
- [ ] `create_adapter("whatsapp", &config)` returns a `WhatsAppAdapter` with webhook server
- [ ] WhatsApp adapter verifies webhook challenges and validates signatures
- [ ] WhatsApp adapter deduplicates repeated webhook deliveries
- [ ] `create_adapter("signal", &config)` returns a `SignalAdapter` polling signal-cli
- [ ] Signal adapter polls for messages and sends via `/v2/send`
- [ ] All adapters handle disconnection gracefully with reconnection
- [ ] All adapters run under `cargo test` with wiremock (no real credentials)

---

## Phase 4 — LaneQueue Concurrency Model

### 4.1 Overview

Phase 4 is **already implemented**. The `LaneQueue` with FIFO semaphore-based concurrency control exists in `crates/rune-runtime/src/lane_queue.rs`. The `TurnExecutor` already integrates with it via `with_lane_queue()`. Configuration is in `LaneQueueConfig`.

This spec documents the implementation for reference.

### 4.2 Exact Rust Types (existing)

```rust
// crates/rune-runtime/src/lane_queue.rs — already implemented

/// Task classification that determines which concurrency lane a turn uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// Direct user sessions and channel sessions. Default cap: 4.
    Main,
    /// Subagent sessions. Default cap: 8.
    Subagent,
    /// Scheduled / cron jobs. Default cap: 1024 (effectively uncapped).
    Cron,
}

impl Lane {
    pub fn from_session_kind(kind: &SessionKind) -> Self {
        match kind {
            SessionKind::Direct | SessionKind::Channel => Lane::Main,
            SessionKind::Subagent => Lane::Subagent,
            SessionKind::Scheduled => Lane::Cron,
        }
    }
}

/// Central lane-based concurrency controller.
pub struct LaneQueue {
    main: LaneSemaphore,
    subagent: LaneSemaphore,
    cron: LaneSemaphore,
}

impl LaneQueue {
    pub fn new() -> Self; // default capacities: 4, 8, 1024
    pub fn with_capacities(main: usize, subagent: usize, cron: usize) -> Self;
    pub async fn acquire(self: &Arc<Self>, lane: Lane) -> LanePermit;
    pub async fn acquire_for_session(self: &Arc<Self>, kind: &SessionKind) -> LanePermit;
    pub fn stats(&self) -> LaneStats;
}

/// A held lane permit. Dropping it releases the lane slot.
pub struct LanePermit {
    _permit: OwnedSemaphorePermit,
    lane: Lane,
    queue: Arc<LaneQueue>,
}

/// Snapshot of lane utilisation.
#[derive(Clone, Debug)]
pub struct LaneStats {
    pub main_active: usize,
    pub main_capacity: usize,
    pub subagent_active: usize,
    pub subagent_capacity: usize,
    pub cron_active: usize,
    pub cron_capacity: usize,
}
```

```rust
// crates/rune-config/src/lib.rs — already defined

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneQueueConfig {
    pub main_capacity: usize,      // default: 4
    pub subagent_capacity: usize,   // default: 8
    pub cron_capacity: usize,       // default: 1024
}
```

### 4.3 Configuration

```toml
# config.toml

[runtime.lanes]
main_capacity = 4
subagent_capacity = 8
cron_capacity = 1024
global_tool_capacity = 32
project_tool_capacity = 4
```

Environment overrides:
```bash
RUNE_RUNTIME__LANES__MAIN_CAPACITY=6
RUNE_RUNTIME__LANES__SUBAGENT_CAPACITY=12
RUNE_RUNTIME__LANES__CRON_CAPACITY=2048
RUNE_RUNTIME__LANES__GLOBAL_TOOL_CAPACITY=48
RUNE_RUNTIME__LANES__PROJECT_TOOL_CAPACITY=6
```

### 4.4 Wire Protocol (existing — `runtime.lanes` RPC)

Request:
```json
{"type": "req", "id": "lanes-1", "method": "runtime.lanes", "params": {}}
```

Response:
```json
{
  "type": "res",
  "id": "lanes-1",
  "ok": true,
  "stateVersion": 5,
  "payload": {
    "enabled": true,
    "lanes": {
      "main": { "active": 2, "capacity": 4 },
      "subagent": { "active": 1, "capacity": 8 },
      "cron": { "active": 0, "capacity": 1024 }
    }
  }
}
```

### 4.5 Acceptance Criteria

- [x] `Lane::from_session_kind` maps Direct/Channel to Main, Subagent to Subagent, Scheduled to Cron
- [x] `LaneQueue::acquire` blocks when the lane is at capacity and resumes in FIFO order
- [x] Lanes are independent: saturating Main does not block Subagent
- [x] Cancelled waiters do not block subsequent waiters
- [x] `LaneStats` accurately reflects current utilisation
- [x] Configurable capacities via `LaneQueueConfig`
- [x] `TurnExecutor` acquires a lane permit before executing a turn
- [x] Lane stats exposed via `runtime.lanes` RPC and HTTP status endpoint

---

## Phase 5 — Hot-Reloading Skills System

### 5.1 Overview

Phase 5 is **already implemented**. The skill system consists of:

- `crates/rune-runtime/src/skill.rs` — `Skill` struct, `SkillFrontmatter`, `SkillRegistry`, YAML frontmatter parser
- `crates/rune-runtime/src/skill_loader.rs` — `SkillLoader` with directory scanning, reconciliation, background watcher
- Integration: `TurnExecutor.with_skill_registry()` injects enabled skills into the system prompt
- Gateway: `skills.list`, `skills.reload`, `skills.enable`, `skills.disable` RPC methods
- HTTP routes: `GET /skills`, `POST /skills/{name}/enable`, `POST /skills/{name}/disable`

This spec documents the design for reference.

### 5.2 Exact Rust Types (existing)

```rust
// crates/rune-runtime/src/skill.rs — already implemented

/// A single skill parsed from a SKILL.md file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub binary_path: Option<PathBuf>,
    pub source_dir: PathBuf,
    pub enabled: bool,
}

/// YAML frontmatter parsed from a SKILL.md file.
#[derive(Clone, Debug, Deserialize)]
pub struct SkillFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub binary: Option<String>,
    pub parameters: Option<serde_json::Value>,
    pub enabled: Option<bool>,
}

/// Thread-safe dynamic skill registry.
#[derive(Clone)]
pub struct SkillRegistry {
    inner: Arc<RwLock<HashMap<String, Skill>>>,
}

impl SkillRegistry {
    pub fn new() -> Self;
    pub async fn register(&self, skill: Skill);
    pub async fn remove(&self, name: &str) -> Option<Skill>;
    pub async fn list(&self) -> Vec<Skill>;
    pub async fn list_enabled(&self) -> Vec<Skill>;
    pub async fn enable(&self, name: &str) -> bool;
    pub async fn disable(&self, name: &str) -> bool;
    pub async fn get(&self, name: &str) -> Option<Skill>;
    pub async fn len(&self) -> usize;
    pub async fn system_prompt_fragment(&self) -> Option<String>;
}
```

```rust
// crates/rune-runtime/src/skill_loader.rs — already implemented

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillScanSummary {
    pub discovered: usize,
    pub loaded: usize,
    pub removed: usize,
}

pub struct SkillLoader {
    skills_dir: PathBuf,
    registry: Arc<SkillRegistry>,
}

impl SkillLoader {
    pub fn new(skills_dir: impl Into<PathBuf>, registry: Arc<SkillRegistry>) -> Self;
    pub async fn scan(&self) -> usize;
    pub async fn scan_summary(&self) -> SkillScanSummary;
    pub fn start_watcher(self: Arc<Self>, interval_secs: u64) -> JoinHandle<()>;
    pub fn skills_dir(&self) -> &Path;
}
```

### 5.3 SKILL.md Format

```markdown
---
name: web-search
description: Search the web using a search engine API
binary: ./search.sh
parameters: {"type":"object","properties":{"query":{"type":"string","description":"Search query"},"max_results":{"type":"integer","default":5}}}
enabled: true
---

# Web Search Skill

This skill searches the web and returns structured results.

## Usage

The agent can invoke this skill by calling the `web-search` tool with a query parameter.
```

### 5.4 Wire Protocol (existing)

**List skills**:
```json
// Request
{"type": "req", "id": "sk-1", "method": "skills.list", "params": {}}

// Response
{
  "type": "res", "id": "sk-1", "ok": true, "stateVersion": 10,
  "payload": [
    {
      "name": "web-search",
      "description": "Search the web using a search engine API",
      "enabled": true,
      "source_dir": "/data/skills/web-search",
      "binary_path": "/data/skills/web-search/search.sh"
    }
  ]
}
```

**Reload skills**:
```json
// Request
{"type": "req", "id": "sk-2", "method": "skills.reload", "params": {}}

// Response
{
  "type": "res", "id": "sk-2", "ok": true, "stateVersion": 10,
  "payload": { "success": true, "discovered": 3, "loaded": 2, "removed": 1 }
}
```

**Enable/disable skill**:
```json
// Request
{"type": "req", "id": "sk-3", "method": "skills.enable", "params": {"name": "web-search"}}

// Response (success)
{"type": "res", "id": "sk-3", "ok": true, "stateVersion": 11, "payload": {"name": "web-search", "enabled": true}}

// Response (not found)
{"type": "res", "id": "sk-3", "ok": false, "stateVersion": 11, "error": {"code": "not_found", "message": "unknown skill: nonexistent"}}
```

### 5.5 Acceptance Criteria

- [x] `SkillLoader` scans `skills/*/SKILL.md` and populates `SkillRegistry`
- [x] YAML frontmatter parser handles name, description, binary, parameters, enabled
- [x] Background watcher re-scans on interval and reconciles (adds new, removes missing)
- [x] Reload preserves runtime enabled/disabled state across rescans
- [x] Invalid SKILL.md files are skipped with a warning (do not block other skills)
- [x] `SkillRegistry.system_prompt_fragment()` returns only enabled skills
- [x] `TurnExecutor` injects skill prompt fragment before each model call
- [x] RPC methods `skills.list`, `skills.reload`, `skills.enable`, `skills.disable` work
- [x] HTTP routes `GET /skills`, `POST /skills/{name}/enable`, `POST /skills/{name}/disable` work

---

## Phase 6 — MCP Client

### 6.1 Overview

Implement the Model Context Protocol (MCP) client as a new crate `crates/rune-mcp/`. This enables Rune to connect to external MCP servers (via STDIO subprocess or HTTP+SSE) and expose their tools alongside built-in tools in the `ToolRegistry`.

### 6.2 Crate Structure

```
crates/rune-mcp/
  Cargo.toml
  src/
    lib.rs              — McpClientManager: manages multiple MCP server connections
    transport_stdio.rs  — STDIO transport: spawn subprocess, JSON-RPC over stdin/stdout
    transport_http.rs   — HTTP+SSE transport: POST for requests, SSE for notifications
    protocol.rs         — MCP JSON-RPC type definitions
    discovery.rs        — Config parsing for [[mcp_servers]] entries
```

### 6.3 Cargo.toml

```toml
[package]
name = "rune-mcp"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
async-trait.workspace = true
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio = { workspace = true, features = ["process", "io-util"] }
tracing.workspace = true
uuid.workspace = true

# For SSE streaming
reqwest-eventsource = "0.7"
# For JSON-RPC framing
futures = "0.3"
```

### 6.4 Exact Rust Types

```rust
// crates/rune-mcp/src/protocol.rs

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str, // always "2.0"
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC 2.0 notification (no id).
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

// ── MCP Protocol Types ──────────────────────────────────────────────

/// MCP server capabilities returned in `initialize` response.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerCapabilities {
    #[serde(default)]
    pub tools: Option<ToolsCapability>,
    #[serde(default)]
    pub resources: Option<ResourcesCapability>,
    #[serde(default)]
    pub prompts: Option<PromptsCapability>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolsCapability {
    #[serde(default)]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResourcesCapability {
    #[serde(default)]
    pub subscribe: bool,
    #[serde(default)]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptsCapability {
    #[serde(default)]
    pub list_changed: bool,
}

/// MCP tool definition returned by `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDefinition {
    /// Unique tool name.
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// JSON Schema for the tool's input parameters.
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Value,
}

/// MCP tool call result returned by `tools/call`.
#[derive(Debug, Clone, Deserialize)]
pub struct McpToolCallResult {
    #[serde(default)]
    pub content: Vec<McpContent>,
    #[serde(default, rename = "isError")]
    pub is_error: bool,
}

/// Content block in an MCP tool result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpContent {
    Text {
        text: String,
    },
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    Resource {
        resource: McpResourceRef,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceRef {
    pub uri: String,
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
    pub text: Option<String>,
}

/// MCP initialize request params.
#[derive(Debug, Clone, Serialize)]
pub struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String, // "2024-11-05"
    pub capabilities: ClientCapabilities,
    #[serde(rename = "clientInfo")]
    pub client_info: ClientInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientCapabilities {
    // Empty for now — Rune does not expose resources/prompts to MCP servers
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// MCP initialize response result.
#[derive(Debug, Clone, Deserialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo")]
    pub server_info: ServerInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
}
```

### 6.5 Transport Trait

```rust
// crates/rune-mcp/src/lib.rs

use async_trait::async_trait;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Errors from MCP client operations.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("transport error: {0}")]
    Transport(String),

    #[error("JSON-RPC error {code}: {message}")]
    JsonRpc {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("server not initialized")]
    NotInitialized,

    #[error("server process exited: {0}")]
    ProcessExited(String),

    #[error("timeout waiting for response")]
    Timeout,

    #[error("serialization error: {0}")]
    Serialization(String),
}

/// Transport abstraction for MCP communication.
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Send a JSON-RPC request and receive the response.
    async fn request(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, McpError>;

    /// Send a JSON-RPC notification (no response expected).
    async fn notify(&self, method: &str, params: Option<serde_json::Value>) -> Result<(), McpError>;

    /// Shut down the transport gracefully.
    async fn shutdown(&self) -> Result<(), McpError>;
}
```

### 6.6 STDIO Transport

```rust
// crates/rune-mcp/src/transport_stdio.rs

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, oneshot};

/// STDIO transport: spawns a child process and communicates via stdin/stdout JSON-RPC.
///
/// Each line on stdout is a complete JSON-RPC message (response or notification).
/// Each line on stdin is a complete JSON-RPC message (request or notification).
pub struct StdioTransport {
    /// Child process handle.
    child: Arc<Mutex<Child>>,
    /// Stdin writer (line-delimited JSON).
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    /// Pending request ID -> response sender.
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
    /// Monotonic request ID counter.
    next_id: AtomicU64,
}

impl StdioTransport {
    /// Spawn a subprocess and start the stdout reader task.
    ///
    /// # Arguments
    /// * `command` - Path to the MCP server binary
    /// * `args` - Command-line arguments
    /// * `env` - Extra environment variables
    /// * `cwd` - Working directory for the subprocess
    pub async fn spawn(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        cwd: Option<&str>,
    ) -> Result<Self, McpError> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        for (key, value) in env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().map_err(|e| McpError::Transport(
            format!("failed to spawn MCP server '{command}': {e}")
        ))?;

        let stdout = child.stdout.take()
            .ok_or_else(|| McpError::Transport("no stdout".into()))?;
        let stdin = child.stdin.take()
            .ok_or_else(|| McpError::Transport("no stdin".into()))?;

        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Spawn stdout reader task
        let pending_clone = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&line) {
                            if let Some(id) = response.id {
                                if let Some(tx) = pending_clone.lock().await.remove(&id) {
                                    let _ = tx.send(response);
                                }
                            }
                            // Notifications (no id) are logged but not dispatched yet
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
            pending,
            next_id: AtomicU64::new(1),
        })
    }
}
```

### 6.7 HTTP+SSE Transport

```rust
// crates/rune-mcp/src/transport_http.rs

/// HTTP+SSE transport for MCP servers that expose HTTP endpoints.
///
/// Requests are sent as POST to the server URL.
/// Server-sent events (SSE) provide notifications.
pub struct HttpTransport {
    /// Base URL of the MCP server (e.g., "http://localhost:3001").
    base_url: String,
    /// HTTP client.
    http: reqwest::Client,
    /// Monotonic request ID counter.
    next_id: AtomicU64,
}

impl HttpTransport {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
            next_id: AtomicU64::new(1),
        }
    }
}
```

### 6.8 MCP Client Manager

```rust
// crates/rune-mcp/src/lib.rs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::protocol::*;

/// Manages multiple MCP server connections and provides a unified tool interface.
pub struct McpClientManager {
    /// Connected MCP servers keyed by server name.
    servers: Arc<RwLock<HashMap<String, McpServerConnection>>>,
}

/// A single connected MCP server.
pub struct McpServerConnection {
    /// Server name from config.
    pub name: String,
    /// Transport used for communication.
    transport: Box<dyn McpTransport>,
    /// Server info from initialize response.
    pub server_info: ServerInfo,
    /// Server capabilities.
    pub capabilities: ServerCapabilities,
    /// Cached tool definitions.
    tools: Vec<McpToolDefinition>,
}

impl McpClientManager {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Connect to an MCP server and perform initialization handshake.
    ///
    /// Sequence:
    /// 1. Send `initialize` request with client capabilities
    /// 2. Receive `InitializeResult` with server capabilities
    /// 3. Send `notifications/initialized` notification
    /// 4. If server supports tools, send `tools/list` to cache tool definitions
    pub async fn connect(
        &self,
        name: &str,
        transport: Box<dyn McpTransport>,
    ) -> Result<(), McpError> {
        // ... implementation
        todo!()
    }

    /// Disconnect from a named MCP server.
    pub async fn disconnect(&self, name: &str) -> Result<(), McpError> {
        // ... implementation
        todo!()
    }

    /// List all tools from all connected MCP servers.
    /// Tool names are prefixed with the server name: `servername__toolname`.
    pub async fn list_tools(&self) -> Vec<McpToolDefinition> {
        // ... implementation
        todo!()
    }

    /// Call a tool on the appropriate MCP server.
    /// The tool name must be in `servername__toolname` format.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallResult, McpError> {
        // ... implementation
        todo!()
    }

    /// Refresh tool list for a specific server.
    pub async fn refresh_tools(&self, name: &str) -> Result<(), McpError> {
        // ... implementation
        todo!()
    }

    /// List connected server names.
    pub async fn list_servers(&self) -> Vec<String> {
        self.servers.read().await.keys().cloned().collect()
    }
}
```

### 6.9 MCP Tool Executor (bridge to ToolRegistry)

```rust
// crates/rune-mcp/src/lib.rs (continued)

use rune_tools::{ToolCall, ToolResult, ToolError, ToolDefinition as RuneToolDefinition};
use rune_core::ToolCategory;
use async_trait::async_trait;

/// Wraps McpClientManager as a ToolExecutor for integration with the runtime.
pub struct McpToolExecutor {
    manager: Arc<McpClientManager>,
}

impl McpToolExecutor {
    pub fn new(manager: Arc<McpClientManager>) -> Self {
        Self { manager }
    }

    /// Convert MCP tool definitions to Rune ToolDefinitions for the ToolRegistry.
    pub async fn tool_definitions(&self) -> Vec<RuneToolDefinition> {
        self.manager
            .list_tools()
            .await
            .into_iter()
            .map(|mcp_tool| RuneToolDefinition {
                name: mcp_tool.name.clone(),
                description: mcp_tool.description.unwrap_or_default(),
                parameters: mcp_tool.input_schema,
                category: ToolCategory::External,
                requires_approval: false,
            })
            .collect()
    }
}

#[async_trait]
impl rune_tools::ToolExecutor for McpToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let result = self
            .manager
            .call_tool(&call.tool_name, call.arguments)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: call.tool_name.clone(),
                reason: e.to_string(),
            })?;

        // Concatenate all text content blocks
        let output = result
            .content
            .iter()
            .filter_map(|c| match c {
                McpContent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output,
            is_error: result.is_error,
            tool_execution_id: None,
        })
    }

    fn definition(&self) -> RuneToolDefinition {
        // This is a multi-tool executor; individual definitions come from tool_definitions()
        unreachable!("McpToolExecutor serves multiple tools; use tool_definitions() instead")
    }
}
```

### 6.10 Configuration

Add to `crates/rune-config/src/lib.rs`:

```rust
/// MCP server configuration entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique name for this MCP server connection.
    pub name: String,
    /// Transport type: "stdio" or "http".
    pub transport: McpTransportKind,
    /// For STDIO: path to the server binary.
    #[serde(default)]
    pub command: Option<String>,
    /// For STDIO: command-line arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// For STDIO: extra environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// For STDIO: working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// For HTTP: base URL of the MCP server.
    #[serde(default)]
    pub url: Option<String>,
    /// Whether this server is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

/// MCP transport kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransportKind {
    Stdio,
    Http,
}
```

Add field to `AppConfig`:

```rust
pub struct AppConfig {
    // ... existing fields ...
    /// MCP server configurations.
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
}
```

Config file example:

```toml
[[mcp_servers]]
name = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"]
enabled = true

[[mcp_servers]]
name = "github"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "ghp_xxx" }
enabled = true

[[mcp_servers]]
name = "remote-tools"
transport = "http"
url = "http://localhost:3001"
enabled = true
```

### 6.11 Integration with TurnExecutor

Modify `crates/rune-runtime/src/executor.rs`:

```rust
impl TurnExecutor {
    /// Attach an MCP client manager whose tools are included in every turn.
    pub fn with_mcp_manager(mut self, manager: Arc<McpClientManager>) -> Self {
        self.mcp_manager = Some(manager);
        self
    }
}
```

In `run_turn_loop`, before building tool definitions:

```rust
// Merge MCP tools into the tool definitions
let mut tool_defs: Vec<rune_models::ToolDefinition> = self
    .tool_registry
    .list()
    .iter()
    .map(|t| /* existing mapping */)
    .collect();

if let Some(ref mcp) = self.mcp_manager {
    let mcp_tools = mcp.list_tools().await;
    for mcp_tool in mcp_tools {
        tool_defs.push(rune_models::ToolDefinition {
            tool_type: "function".to_string(),
            function: rune_models::FunctionDefinition {
                name: mcp_tool.name,
                description: mcp_tool.description.unwrap_or_default(),
                parameters: mcp_tool.input_schema,
            },
        });
    }
}
```

When dispatching tool calls, check if the tool name matches an MCP tool (contains `__` prefix) and route to the MCP executor instead of the built-in executor.

### 6.12 MCP Protocol Handshake Sequence

```
Client                          Server
  |                                |
  |--- initialize ----------------->|
  |    {protocolVersion: "2024-11-05",
  |     capabilities: {},
  |     clientInfo: {name: "rune", version: "0.1.0"}}
  |                                |
  |<-- initialize result -----------|
  |    {protocolVersion: "2024-11-05",
  |     capabilities: {tools: {listChanged: true}},
  |     serverInfo: {name: "fs-server", version: "1.0"}}
  |                                |
  |--- notifications/initialized -->|
  |                                |
  |--- tools/list ----------------->|
  |                                |
  |<-- tools/list result -----------|
  |    {tools: [{name: "read_file", description: "...", inputSchema: {...}}]}
  |                                |
```

### 6.13 Wire Examples

**STDIO transport — initialize request** (written to subprocess stdin):
```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"rune","version":"0.1.0"}}}
```

**STDIO transport — initialize response** (read from subprocess stdout):
```json
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"filesystem","version":"0.5.0"}}}
```

**STDIO transport — tools/call request**:
```json
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"read_file","arguments":{"path":"/etc/hostname"}}}
```

**STDIO transport — tools/call response**:
```json
{"jsonrpc":"2.0","id":5,"result":{"content":[{"type":"text","text":"my-hostname\n"}],"isError":false}}
```

**STDIO transport — tools/call error response**:
```json
{"jsonrpc":"2.0","id":5,"result":{"content":[{"type":"text","text":"Error: ENOENT: no such file or directory, open '/nonexistent'"}],"isError":true}}
```

### 6.14 Error Cases

| Scenario | Error type | Behavior |
|---|---|---|
| Server binary not found | `McpError::Transport` | Logged, server skipped during startup |
| Server crashes mid-request | `McpError::ProcessExited` | Pending requests receive error, server marked disconnected |
| Initialize timeout (>10s) | `McpError::Timeout` | Server skipped, warning logged |
| Server returns JSON-RPC error | `McpError::JsonRpc` | Converted to `ToolError::ExecutionFailed` |
| Malformed JSON on stdout | Logged + skipped | Line skipped, does not crash the reader |
| tools/call timeout (>30s) | `McpError::Timeout` | Tool result is_error=true with timeout message |
| Server signals `tools/list_changed` | Automatic refresh | Call `tools/list` again and update cached tools |
| HTTP transport: 4xx/5xx response | `McpError::Transport` | Converted to tool error |
| HTTP transport: SSE disconnect | Reconnect | Exponential backoff, max 5 retries |

### 6.15 Edge Cases

1. **Tool name collision**: If MCP server "fs" exposes `read_file` and a built-in tool also has `read_file`, the MCP tool is registered as `fs__read_file`. Built-in tools always take precedence for unqualified names.
2. **Multiple MCP servers**: Tools from different servers are namespaced by server name. `tools/call` routes to the correct server based on the `servername__` prefix.
3. **Server restart**: If the subprocess exits, the transport detects EOF on stdout and marks the server as disconnected. A background task attempts to respawn every 30 seconds.
4. **Concurrent tool calls**: Multiple tool calls to the same MCP server are serialized via the request ID mechanism. STDIO transport can handle multiple in-flight requests (each identified by `id`).
5. **Large tool output**: If tool output exceeds 1MB, it is truncated with a `[truncated]` suffix.
6. **Subprocess environment**: The subprocess inherits the parent environment plus any `env` overrides. `PATH` is preserved so `npx` and other tools work.

### 6.16 Integration Test Scenarios

```rust
// crates/rune-mcp/tests/mcp_tests.rs

/// Test STDIO transport spawns a subprocess and exchanges initialize handshake.
#[tokio::test]
async fn test_stdio_transport_initialize() {}

/// Test STDIO transport handles tools/list and returns tool definitions.
#[tokio::test]
async fn test_stdio_transport_tools_list() {}

/// Test STDIO transport handles tools/call and returns result.
#[tokio::test]
async fn test_stdio_transport_tools_call() {}

/// Test STDIO transport handles subprocess exit gracefully.
#[tokio::test]
async fn test_stdio_transport_process_exit() {}

/// Test STDIO transport handles malformed JSON on stdout without crashing.
#[tokio::test]
async fn test_stdio_transport_malformed_json() {}

/// Test HTTP transport sends POST request and parses response.
#[tokio::test]
async fn test_http_transport_request() {}

/// Test McpClientManager connects to multiple servers.
#[tokio::test]
async fn test_manager_multi_server() {}

/// Test McpClientManager tool name prefixing avoids collisions.
#[tokio::test]
async fn test_manager_tool_name_prefixing() {}

/// Test McpToolExecutor bridges MCP tools to Rune ToolResult.
#[tokio::test]
async fn test_mcp_tool_executor_bridge() {}

/// Test McpServerConfig deserialization from TOML.
#[test]
fn test_mcp_config_deserialization() {}
```

For STDIO tests, use a simple Rust test binary that implements the MCP server protocol minimally (respond to `initialize` + `tools/list` + `tools/call`), compiled as a test fixture.

### 6.17 Acceptance Criteria

- [ ] `rune-mcp` crate compiles and all tests pass
- [ ] STDIO transport spawns subprocess, performs initialize handshake, and caches tools
- [ ] HTTP transport sends POST requests and parses JSON-RPC responses
- [ ] `McpClientManager` manages multiple server connections with connect/disconnect
- [ ] MCP tools appear in the tool registry alongside built-in tools
- [ ] Tool calls route transparently through MCP client (server name prefix)
- [ ] Subprocess exit is detected and reported as `McpError::ProcessExited`
- [ ] `[[mcp_servers]]` config entries are parsed from `config.toml`
- [ ] MCP servers are connected on startup and disconnected on shutdown
- [ ] `TurnExecutor` includes MCP tools in the tool definitions sent to the model
- [ ] Tool name collision is handled via server name prefixing

---

## Phase 7 — Expanded LLM Providers

### 7.1 Overview

Add provider implementations for Google Gemini, Ollama, AWS Bedrock, Groq, DeepSeek, and Mistral. All implement the existing `ModelProvider` trait. The provider stubs already exist as files; this phase fills them with real HTTP client code.

### 7.2 Existing ModelProvider Trait (no changes)

```rust
// crates/rune-models/src/provider/mod.rs — already defined

#[async_trait]
pub trait ModelProvider: Send + Sync + std::fmt::Debug {
    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, ModelError>;
}
```

```rust
// crates/rune-models/src/types.rs — already defined

pub struct CompletionRequest {
    pub messages: Vec<ChatMessage>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub tools: Option<Vec<ToolDefinition>>,
}

pub struct CompletionResponse {
    pub content: Option<String>,
    pub usage: Usage,
    pub finish_reason: Option<FinishReason>,
    pub tool_calls: Vec<ToolCallRequest>,
}

pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
```

### 7.3 Google Gemini Provider

**File**: `crates/rune-models/src/provider/google.rs`

```rust
/// Google Gemini provider via the Generative Language API.
///
/// API base: `https://generativelanguage.googleapis.com/v1beta`
/// Auth: API key as query parameter `?key={api_key}`
///
/// Endpoint: `POST /models/{model}:generateContent`
#[derive(Debug)]
pub struct GoogleProvider {
    /// API key for authentication.
    api_key: String,
    /// Model name (e.g., "gemini-2.0-flash", "gemini-2.5-pro").
    model: String,
    /// HTTP client.
    http: reqwest::Client,
}

impl GoogleProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            http: reqwest::Client::new(),
        }
    }
}
```

**Request mapping** (Rune `CompletionRequest` -> Gemini API):

```json
{
  "contents": [
    {"role": "user", "parts": [{"text": "Hello"}]},
    {"role": "model", "parts": [{"text": "Hi there!"}]}
  ],
  "systemInstruction": {
    "parts": [{"text": "You are a helpful assistant."}]
  },
  "tools": [{
    "functionDeclarations": [{
      "name": "read_file",
      "description": "Read a file",
      "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}
    }]
  }],
  "generationConfig": {
    "temperature": 0.7,
    "maxOutputTokens": 4096
  }
}
```

**Role mapping**:

| Rune Role | Gemini Role |
|---|---|
| `System` | Extracted to `systemInstruction` |
| `User` | `"user"` |
| `Assistant` | `"model"` |
| `Tool` | `"function"` (with function response) |

**Response mapping** (Gemini API -> Rune `CompletionResponse`):

| Gemini field | Rune field |
|---|---|
| `candidates[0].content.parts[*].text` | `content` (joined) |
| `candidates[0].content.parts[*].functionCall` | `tool_calls` |
| `candidates[0].finishReason` | `finish_reason` |
| `usageMetadata.promptTokenCount` | `usage.prompt_tokens` |
| `usageMetadata.candidatesTokenCount` | `usage.completion_tokens` |
| `usageMetadata.totalTokenCount` | `usage.total_tokens` |

**Error mapping**:

| Gemini HTTP status | ModelError variant |
|---|---|
| 400 | `ModelError::Provider` |
| 401, 403 | `ModelError::Auth` |
| 429 | `ModelError::RateLimited` (extract `retryDelay` from body) |
| 500, 503 | `ModelError::Transient` |

### 7.4 Ollama Provider

**File**: `crates/rune-models/src/provider/ollama.rs`

```rust
/// Ollama local model provider.
///
/// API base: `http://localhost:11434` (configurable).
/// Endpoint: `POST /api/chat`
///
/// Also supports model discovery via `GET /api/tags`.
#[derive(Debug)]
pub struct OllamaProvider {
    /// Base URL (default: http://localhost:11434).
    base_url: String,
    /// Model name (e.g., "llama3.2", "codellama:70b").
    model: String,
    /// HTTP client.
    http: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            http: reqwest::Client::new(),
        }
    }

    /// Discover available models via `GET /api/tags`.
    pub async fn list_models(&self) -> Result<Vec<OllamaModel>, ModelError> {
        // GET {base_url}/api/tags
        todo!()
    }
}

/// Model entry from Ollama's /api/tags endpoint.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OllamaModel {
    pub name: String,
    pub size: u64,
    pub modified_at: String,
}
```

**Request mapping**:

```json
{
  "model": "llama3.2",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello"}
  ],
  "tools": [{
    "type": "function",
    "function": {
      "name": "read_file",
      "description": "Read a file",
      "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}
    }
  }],
  "stream": false,
  "options": {
    "temperature": 0.7,
    "num_predict": 4096
  }
}
```

**Response mapping**:

| Ollama field | Rune field |
|---|---|
| `message.content` | `content` |
| `message.tool_calls` | `tool_calls` |
| `done_reason` (`"stop"` / `"length"`) | `finish_reason` |
| `prompt_eval_count` | `usage.prompt_tokens` |
| `eval_count` | `usage.completion_tokens` |

**Error mapping**:

| Scenario | ModelError variant |
|---|---|
| Connection refused | `ModelError::Transient("Ollama not running")` |
| Model not found (404) | `ModelError::Configuration("model not found")` |
| OOM / generation error | `ModelError::Provider` |

### 7.5 AWS Bedrock Provider

**File**: `crates/rune-models/src/provider/bedrock.rs`

```rust
/// AWS Bedrock provider using the ConverseStream API.
///
/// Endpoint: `POST https://bedrock-runtime.{region}.amazonaws.com/model/{model_id}/converse`
/// Auth: AWS Signature v4 (via `aws-sigv4` crate or manual signing).
///
/// The `deployment_name` field in config holds the AWS region.
/// The `api_key` field holds `{access_key_id}:{secret_access_key}` or uses default credential chain.
#[derive(Debug)]
pub struct BedrockProvider {
    /// AWS region (e.g., "us-east-1").
    region: String,
    /// Model ID (e.g., "anthropic.claude-3-5-sonnet-20241022-v2:0").
    model_id: String,
    /// AWS access key ID.
    access_key_id: String,
    /// AWS secret access key.
    secret_access_key: String,
    /// HTTP client.
    http: reqwest::Client,
}

impl BedrockProvider {
    pub fn new(
        region: impl Into<String>,
        model_id: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        Self {
            region: region.into(),
            model_id: model_id.into(),
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            http: reqwest::Client::new(),
        }
    }
}
```

**Bedrock Converse API request**:

```json
{
  "modelId": "anthropic.claude-3-5-sonnet-20241022-v2:0",
  "messages": [
    {"role": "user", "content": [{"text": "Hello"}]}
  ],
  "system": [{"text": "You are a helpful assistant."}],
  "toolConfig": {
    "tools": [{
      "toolSpec": {
        "name": "read_file",
        "description": "Read a file",
        "inputSchema": {"json": {"type": "object", "properties": {"path": {"type": "string"}}}}
      }
    }]
  },
  "inferenceConfig": {
    "temperature": 0.7,
    "maxTokens": 4096
  }
}
```

**Response mapping**:

| Bedrock field | Rune field |
|---|---|
| `output.message.content[*].text` | `content` |
| `output.message.content[*].toolUse` | `tool_calls` |
| `stopReason` | `finish_reason` |
| `usage.inputTokens` | `usage.prompt_tokens` |
| `usage.outputTokens` | `usage.completion_tokens` |

**Crate dependency for AWS SigV4**: Use `aws-sigv4 = "1"` crate for request signing:

```toml
# Add to workspace dependencies
aws-sigv4 = "1"
aws-credential-types = "1"
```

### 7.6 Groq Provider

**File**: `crates/rune-models/src/provider/groq.rs`

```rust
/// Groq provider — OpenAI-compatible API.
///
/// API base: `https://api.groq.com/openai/v1`
/// Auth: Bearer token in Authorization header.
/// Endpoint: `POST /chat/completions`
///
/// Uses the same request/response format as OpenAI.
#[derive(Debug)]
pub struct GroqProvider {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl GroqProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            http: reqwest::Client::new(),
        }
    }
}
```

Because Groq uses the OpenAI-compatible API, the implementation reuses the same request/response mapping as the existing `openai.rs` provider. The only differences are the base URL and the model names.

**Models**: `llama-3.3-70b-versatile`, `llama-3.1-8b-instant`, `mixtral-8x7b-32768`, `gemma2-9b-it`.

### 7.7 DeepSeek Provider

**File**: `crates/rune-models/src/provider/deepseek.rs`

```rust
/// DeepSeek provider — OpenAI-compatible API.
///
/// API base: `https://api.deepseek.com/v1`
/// Auth: Bearer token in Authorization header.
/// Endpoint: `POST /chat/completions`
#[derive(Debug)]
pub struct DeepSeekProvider {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl DeepSeekProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            http: reqwest::Client::new(),
        }
    }
}
```

OpenAI-compatible. Reuses OpenAI request/response mapping.

**Models**: `deepseek-chat`, `deepseek-coder`, `deepseek-reasoner`.

**Special**: DeepSeek-Reasoner returns `reasoning_content` alongside `content`. Extract both; put `reasoning_content` in a `<thinking>` block prefix for the UI ThinkingBlock to display.

### 7.8 Mistral Provider

**File**: `crates/rune-models/src/provider/mistral.rs`

```rust
/// Mistral provider — OpenAI-compatible API.
///
/// API base: `https://api.mistral.ai/v1`
/// Auth: Bearer token in Authorization header.
/// Endpoint: `POST /chat/completions`
#[derive(Debug)]
pub struct MistralProvider {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl MistralProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            http: reqwest::Client::new(),
        }
    }
}
```

OpenAI-compatible. Reuses OpenAI request/response mapping.

**Models**: `mistral-large-latest`, `mistral-small-latest`, `codestral-latest`, `open-mistral-nemo`.

### 7.9 Provider Factory Updates

Modify `crates/rune-models/src/lib.rs` to register new providers:

```rust
/// Create a ModelProvider from a config entry.
pub fn create_provider(config: &ModelProviderConfig) -> Result<Arc<dyn ModelProvider>, ModelError> {
    let api_key = resolve_api_key(config)?;

    match config.kind.as_str() {
        "openai" => Ok(Arc::new(OpenAiProvider::new(&config.base_url, &api_key, &model))),
        "anthropic" => Ok(Arc::new(AnthropicProvider::new(&config.base_url, &api_key, &model))),
        "azure-openai" => Ok(Arc::new(AzureProvider::new(/* ... */))),
        "azure-foundry" => Ok(Arc::new(AzureFoundryProvider::new(/* ... */))),

        // New providers
        "gemini" | "google" => {
            let model = config.models.first()
                .map(|m| m.id().to_string())
                .unwrap_or_else(|| "gemini-2.0-flash".to_string());
            Ok(Arc::new(GoogleProvider::new(&api_key, &model)))
        }
        "ollama" => {
            let base_url = if config.base_url.is_empty() {
                "http://localhost:11434".to_string()
            } else {
                config.base_url.clone()
            };
            let model = config.models.first()
                .map(|m| m.id().to_string())
                .unwrap_or_else(|| "llama3.2".to_string());
            Ok(Arc::new(OllamaProvider::new(&base_url, &model)))
        }
        "aws-bedrock" | "bedrock" => {
            let region = config.deployment_name.as_deref().unwrap_or("us-east-1");
            let model_id = config.models.first()
                .map(|m| m.id().to_string())
                .unwrap_or_default();
            let (access_key, secret_key) = parse_bedrock_credentials(&api_key)?;
            Ok(Arc::new(BedrockProvider::new(region, &model_id, access_key, secret_key)))
        }
        "groq" => {
            let model = config.models.first()
                .map(|m| m.id().to_string())
                .unwrap_or_else(|| "llama-3.3-70b-versatile".to_string());
            Ok(Arc::new(GroqProvider::new(&api_key, &model)))
        }
        "deepseek" => {
            let model = config.models.first()
                .map(|m| m.id().to_string())
                .unwrap_or_else(|| "deepseek-chat".to_string());
            Ok(Arc::new(DeepSeekProvider::new(&api_key, &model)))
        }
        "mistral" => {
            let model = config.models.first()
                .map(|m| m.id().to_string())
                .unwrap_or_else(|| "mistral-large-latest".to_string());
            Ok(Arc::new(MistralProvider::new(&api_key, &model)))
        }
        other => Err(ModelError::Configuration(format!("unknown provider kind: {other}"))),
    }
}

/// Parse "access_key_id:secret_access_key" format for Bedrock.
fn parse_bedrock_credentials(combined: &str) -> Result<(&str, &str), ModelError> {
    combined.split_once(':')
        .ok_or_else(|| ModelError::Configuration(
            "bedrock api_key must be in 'access_key_id:secret_access_key' format".into()
        ))
}
```

### 7.10 Gateway Routes for Model Discovery

Add to `crates/rune-gateway/src/routes.rs`:

```rust
/// Response for `GET /models`.
#[derive(Serialize)]
pub struct ModelsListResponse {
    pub models: Vec<ModelEntry>,
}

#[derive(Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub provider: String,
    pub kind: String,
}

/// List all configured models.
///
/// `GET /models`
///
/// Response 200:
/// ```json
/// {
///   "models": [
///     {"id": "google/gemini-2.0-flash", "provider": "google", "kind": "gemini"},
///     {"id": "ollama/llama3.2", "provider": "ollama", "kind": "ollama"}
///   ]
/// }
/// ```
pub async fn list_models(
    State(state): State<AppState>,
) -> Result<Json<ModelsListResponse>, GatewayError> {
    let entries = state.config.models.inventory()
        .into_iter()
        .map(|e| ModelEntry {
            id: e.model_id(),
            provider: e.provider_name.to_string(),
            kind: e.provider_kind.to_string(),
        })
        .collect();
    Ok(Json(ModelsListResponse { models: entries }))
}

/// Response for `POST /models/scan`.
#[derive(Serialize)]
pub struct ModelScanResponse {
    pub discovered: Vec<ModelEntry>,
}

/// Scan for available models (e.g., query Ollama for locally available models).
///
/// `POST /models/scan`
///
/// Response 200:
/// ```json
/// {
///   "discovered": [
///     {"id": "ollama/llama3.2:latest", "provider": "ollama", "kind": "ollama"},
///     {"id": "ollama/codellama:70b", "provider": "ollama", "kind": "ollama"}
///   ]
/// }
/// ```
pub async fn scan_models(
    State(state): State<AppState>,
) -> Result<Json<ModelScanResponse>, GatewayError> {
    // For now, only Ollama supports dynamic discovery.
    // Other providers return their statically configured models.
    let mut discovered = Vec::new();

    for provider_config in &state.config.models.providers {
        match provider_config.kind.as_str() {
            "ollama" => {
                let base_url = if provider_config.base_url.is_empty() {
                    "http://localhost:11434"
                } else {
                    &provider_config.base_url
                };
                // Call Ollama /api/tags
                match reqwest::get(format!("{base_url}/api/tags")).await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(body) = resp.json::<serde_json::Value>().await {
                            if let Some(models) = body.get("models").and_then(|v| v.as_array()) {
                                for model in models {
                                    if let Some(name) = model.get("name").and_then(|v| v.as_str()) {
                                        discovered.push(ModelEntry {
                                            id: format!("{}/{}", provider_config.name, name),
                                            provider: provider_config.name.clone(),
                                            kind: "ollama".into(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        // Ollama not reachable — skip silently
                    }
                }
            }
            _ => {
                // Static providers: list their configured models
                for model in &provider_config.models {
                    discovered.push(ModelEntry {
                        id: format!("{}/{}", provider_config.name, model.id()),
                        provider: provider_config.name.clone(),
                        kind: provider_config.kind.clone(),
                    });
                }
            }
        }
    }

    Ok(Json(ModelScanResponse { discovered }))
}
```

**Wire protocol — HTTP**:

`GET /models` response:
```json
{
  "models": [
    {"id": "google/gemini-2.0-flash", "provider": "google", "kind": "gemini"},
    {"id": "bedrock/anthropic.claude-3-5-sonnet-20241022-v2:0", "provider": "bedrock", "kind": "aws-bedrock"},
    {"id": "ollama/llama3.2", "provider": "ollama", "kind": "ollama"},
    {"id": "groq/llama-3.3-70b-versatile", "provider": "groq", "kind": "groq"},
    {"id": "deepseek/deepseek-chat", "provider": "deepseek", "kind": "deepseek"},
    {"id": "mistral/mistral-large-latest", "provider": "mistral", "kind": "mistral"}
  ]
}
```

`POST /models/scan` response:
```json
{
  "discovered": [
    {"id": "ollama/llama3.2:latest", "provider": "ollama", "kind": "ollama"},
    {"id": "ollama/codellama:70b", "provider": "ollama", "kind": "ollama"},
    {"id": "ollama/mistral:7b", "provider": "ollama", "kind": "ollama"}
  ]
}
```

**Error responses**:

`GET /models` — 500 (only if config is malformed):
```json
{
  "code": "internal_error",
  "message": "internal error: failed to read provider config",
  "retriable": true,
  "approval_required": false,
  "request_id": "01958d2a-6666-7def-8a90-ffffffffffff"
}
```

### 7.11 Config Examples

```toml
# config.toml

[models]
default_model = "google/gemini-2.0-flash"

[[models.providers]]
name = "google"
kind = "gemini"
base_url = ""
api_key_env = "GOOGLE_API_KEY"
models = ["gemini-2.0-flash", "gemini-2.5-pro"]

[[models.providers]]
name = "ollama"
kind = "ollama"
base_url = "http://localhost:11434"
models = ["llama3.2", "codellama:70b"]

[[models.providers]]
name = "bedrock"
kind = "aws-bedrock"
base_url = "https://bedrock-runtime.us-east-1.amazonaws.com"
deployment_name = "us-east-1"
api_key_env = "BEDROCK_COMBINED"
models = ["anthropic.claude-3-5-sonnet-20241022-v2:0"]

[[models.providers]]
name = "groq"
kind = "groq"
base_url = "https://api.groq.com/openai/v1"
api_key_env = "GROQ_API_KEY"
models = ["llama-3.3-70b-versatile", "mixtral-8x7b-32768"]

[[models.providers]]
name = "deepseek"
kind = "deepseek"
base_url = "https://api.deepseek.com/v1"
api_key_env = "DEEPSEEK_API_KEY"
models = ["deepseek-chat", "deepseek-reasoner"]

[[models.providers]]
name = "mistral"
kind = "mistral"
base_url = "https://api.mistral.ai/v1"
api_key_env = "MISTRAL_API_KEY"
models = ["mistral-large-latest", "codestral-latest"]
```

### 7.12 Crate Dependencies

```toml
# crates/rune-models/Cargo.toml — add to [dependencies]

# Existing
async-trait.workspace = true
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tracing.workspace = true

# New: for AWS Bedrock SigV4 signing
aws-sigv4 = "1"
aws-credential-types = "1"
http = "1"           # for constructing the signing request
```

Add to workspace `[workspace.dependencies]` in root `Cargo.toml`:
```toml
aws-sigv4 = "1"
aws-credential-types = "1"
http = "1"
```

### 7.13 Error Cases per Provider

| Provider | Scenario | HTTP Status | ModelError variant |
|---|---|---|---|
| Google | Invalid API key | 401 | `Auth("invalid API key")` |
| Google | Model not found | 404 | `Configuration("model not found: ...")` |
| Google | Safety filter triggered | 200 (finish_reason=SAFETY) | `ContentFiltered(...)` |
| Google | Rate limited | 429 | `RateLimited { retry_after_secs }` |
| Google | Quota exhausted | 429 (RESOURCE_EXHAUSTED) | `QuotaExhausted(...)` |
| Ollama | Server not running | Connection refused | `Transient("cannot connect to Ollama at ...")` |
| Ollama | Model not pulled | 404 | `Configuration("model not found; run 'ollama pull ...'")` |
| Ollama | OOM during generation | 500 | `Provider("Ollama generation failed: ...")` |
| Bedrock | Invalid credentials | 403 | `Auth("invalid AWS credentials")` |
| Bedrock | Throttled | 429 | `RateLimited { ... }` |
| Bedrock | Model not available in region | 400 | `Configuration("model not available in ...")` |
| Groq | Rate limited | 429 | `RateLimited { ... }` (extract `retry-after` header) |
| Groq | Invalid model | 404 | `Configuration("model not found")` |
| DeepSeek | Rate limited | 429 | `RateLimited { ... }` |
| DeepSeek | Context length exceeded | 400 | `ContextLengthExceeded(...)` |
| Mistral | Rate limited | 429 | `RateLimited { ... }` |
| Mistral | Invalid API key | 401 | `Auth("invalid API key")` |

### 7.14 OpenAI-Compatible Shared Implementation

Groq, DeepSeek, and Mistral all use the OpenAI chat completions API format. To avoid duplication, extract a shared helper:

```rust
// crates/rune-models/src/provider/response.rs (extend existing)

/// Send an OpenAI-compatible chat completion request and parse the response.
///
/// Used by: openai.rs, groq.rs, deepseek.rs, mistral.rs
pub async fn openai_compatible_complete(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &CompletionRequest,
    model: &str,
) -> Result<CompletionResponse, ModelError> {
    let url = format!("{base_url}/chat/completions");

    let body = build_openai_request_body(request, model);

    let resp = http
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        return Err(map_openai_error(status, resp).await);
    }

    let response_body: serde_json::Value = resp.json().await?;
    parse_openai_response(response_body)
}
```

### 7.15 Integration Test Scenarios

```rust
// crates/rune-models/tests/provider_tests.rs
// All tests use wiremock to mock the external APIs.

/// Google Gemini: successful completion with text response.
#[tokio::test]
async fn test_google_complete_text() {}

/// Google Gemini: successful completion with tool calls.
#[tokio::test]
async fn test_google_complete_tool_calls() {}

/// Google Gemini: safety filter returns ContentFiltered error.
#[tokio::test]
async fn test_google_safety_filter() {}

/// Google Gemini: 429 returns RateLimited error.
#[tokio::test]
async fn test_google_rate_limited() {}

/// Ollama: successful chat completion.
#[tokio::test]
async fn test_ollama_complete() {}

/// Ollama: model not found returns Configuration error.
#[tokio::test]
async fn test_ollama_model_not_found() {}

/// Ollama: list_models returns available models.
#[tokio::test]
async fn test_ollama_list_models() {}

/// Ollama: connection refused returns Transient error.
#[tokio::test]
async fn test_ollama_connection_refused() {}

/// Bedrock: successful converse API call.
#[tokio::test]
async fn test_bedrock_complete() {}

/// Bedrock: request is signed with AWS SigV4.
#[tokio::test]
async fn test_bedrock_sigv4_signing() {}

/// Groq: successful OpenAI-compatible completion.
#[tokio::test]
async fn test_groq_complete() {}

/// DeepSeek: reasoning_content is extracted to thinking block.
#[tokio::test]
async fn test_deepseek_reasoning_content() {}

/// Mistral: successful OpenAI-compatible completion.
#[tokio::test]
async fn test_mistral_complete() {}

/// Provider factory creates correct provider for each kind.
#[test]
fn test_create_provider_all_kinds() {}

/// GET /models returns all configured models.
#[tokio::test]
async fn test_list_models_endpoint() {}

/// POST /models/scan queries Ollama for available models.
#[tokio::test]
async fn test_scan_models_ollama() {}
```

### 7.16 SQL Migrations

No new tables are needed for Phase 7. Model provider selection is stored in session metadata (`metadata.selected_model`) which already exists as a JSONB column.

### 7.17 Acceptance Criteria

- [ ] `GoogleProvider` implements `ModelProvider` and handles text, tool calls, and safety filters
- [ ] `OllamaProvider` implements `ModelProvider` and supports model discovery via `/api/tags`
- [ ] `BedrockProvider` implements `ModelProvider` with AWS SigV4 request signing
- [ ] `GroqProvider` implements `ModelProvider` using OpenAI-compatible API
- [ ] `DeepSeekProvider` implements `ModelProvider` and extracts `reasoning_content`
- [ ] `MistralProvider` implements `ModelProvider` using OpenAI-compatible API
- [ ] Provider factory `create_provider` handles all kind strings: `gemini`, `google`, `ollama`, `aws-bedrock`, `bedrock`, `groq`, `deepseek`, `mistral`
- [ ] OpenAI-compatible providers share the helper function (no code duplication)
- [ ] All providers map HTTP errors to correct `ModelError` variants
- [ ] `GET /models` returns all configured models
- [ ] `POST /models/scan` queries Ollama for available models
- [ ] Config supports `[[models.providers]]` with `kind` field for all six new providers
- [ ] All tests pass with wiremock (no real API credentials needed)
- [ ] `cargo build` succeeds with all new providers compiled
