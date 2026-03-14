# Rune — Full OpenClaw Parity (Backend + UI)

Context: Full rewrite of OpenClaw's architecture in Rust + comprehensive admin UI. Rune already has several pillars implemented; this plan covers everything missing to reach parity.

---

## Current State Audit

| Pillar | Status | Notes |
|---|---|---|
| WebSocket Gateway | ✅ Basic | Event broadcast only, no req/res/RPC framing |
| Multi-channel Routing | ✅ Telegram only | Adapter trait exists; Discord/Slack/WhatsApp/Signal missing |
| Device Pairing | ❌ Missing | No Ed25519, no crypto |
| LaneQueue Concurrency | ❌ Missing | Sequential per-session, no lane caps |
| File-based Identity | ✅ Done | SOUL.md, USER.md, AGENTS.md, TOOLS.md, IDENTITY.md |
| Session History | ✅ Done | PostgreSQL-based (not JSONL) |
| Hybrid Memory Search | ⚠️ Keyword only | No vector embeddings, no FTS5 |
| Hot-reloading Skills | ❌ Missing | Static tool registry, no SKILL.md scanning |
| MCP Client | ❌ Missing | No STDIO/HTTP MCP transport |
| PTY Execution Sandbox | ✅ Basic | Via Unix script, no advanced PTY |
| Heartbeat + Silent Eval | ✅ Done | HEARTBEAT_OK suppression works |
| Sub-agents | ✅ Done | Session spawning + manager trait |
| Semantic Browser Snapshots | ❌ Missing | No accessibility tree parsing |
| A2UI Protocol | ❌ Missing | No streaming UI components |
| TTS | ❌ Missing | No text-to-speech providers |
| STT | ❌ Missing | No speech-to-text providers |
| LLM Providers | ✅ Partial | Anthropic, OpenAI, Azure only — missing Google, Ollama, Bedrock, Groq, DeepSeek, Mistral |
| Admin UI | ✅ Basic | Shell + basic pages exist, missing chat, usage, debug, config, logs, agents, skills |

---

## Reference Note

We can also inspect other community Rust rewrites of OpenClaw — Panther, ZeroClaw, and ZeptoClaw — to compare implementation choices around safety, single-binary distribution, and Tokio-based async performance.

Core architectural pillars to recreate:

1. **WebSocket Gateway & Routing Engine** — Async WebSocket server (default port 18789) as the central control plane. Strict JSON framing: requests `{type:"req", id, method, params}` and events `{type:"event", event, payload}`. Multi-channel routing (Telegram, Discord, Slack, WhatsApp). Device pairing with Ed25519 challenge nonces.
2. **LaneQueue Concurrency Model** — FIFO queue enforcing “Default Serial, Explicit Parallel”. Per-lane caps: `main=4`, `subagent=8`, `cron=independent`, `nested=recursive`.
3. **State, Memory, and Identity Management** — File-based identity via `SOUL.md`, `USER.md`, `AGENTS.md`. Session history in plain `.jsonl` in OpenClaw; Rune currently uses PostgreSQL. Hybrid memory search should combine vector embeddings with keyword matching (Rune should use PostgreSQL `pgvector` + `tsvector`).
4. **Extensibility and Tooling** — Hot-reloading skills via `SKILL.md` directory scanning with fast parsing and dynamic system prompt injection. MCP client with STDIO + HTTP transports. PTY execution sandbox with `allowlist` / `deny` / `full` security modes for interactive CLI support.
5. **Proactive Autonomy** — Heartbeat cron loop (30 min default) with silent evaluation via `HEARTBEAT.md` checklist and `HEARTBEAT_OK` suppression. Sub-agent spawning in isolated sessions for parallel tasks.
6. **Advanced Interfaces and Browsing** — Semantic browser snapshots via accessibility tree parsing with numeric `[ref=N]` annotations. A2UI protocol with streaming JSONL transport for declarative UI components rendered progressively by frontends.

---

## Implementation Phases

### Phase 1 — Chat Page (UI only, backend exists)

Highest-impact missing UI feature. Backend already has `POST /sessions/{id}/messages`, `GET /sessions/{id}/transcript`, and WebSocket event streaming.

**New files**
- `ui/src/routes/_admin/chat.tsx` — Chat page with session selector + message thread + sidebar
- `ui/src/components/chat/ChatThread.tsx` — Message list with role-based grouping
- `ui/src/components/chat/ChatMessage.tsx` — Message bubble with markdown rendering
- `ui/src/components/chat/ChatInput.tsx` — Enter to send, Shift+Enter newline, image paste
- `ui/src/components/chat/ToolCard.tsx` — Inline tool call/result display, collapsible
- `ui/src/components/chat/ThinkingBlock.tsx` — Collapsible `<thinking>` block extraction
- `ui/src/components/chat/MarkdownRenderer.tsx` — `marked` + `DOMPurify` sanitized rendering
- `ui/src/components/chat/ChatSidebar.tsx` — Resizable split panel for long tool outputs (0.4–0.7 ratio)
- `ui/src/components/chat/CopyMarkdown.tsx` — Copy-as-markdown with feedback state
- `ui/src/components/chat/ImageAttachment.tsx` — Clipboard paste preview
- `ui/src/hooks/use-chat.ts` — Transcript polling, send message, WS subscription

**NPM deps**
- `marked`
- `dompurify`
- `@types/dompurify`

**Nav**
- Add `Chat` as first item in `AdminNavbar` + `AdminBottomNav`

---

### Phase 2 — WebSocket Gateway RPC Protocol (Backend)

Upgrade from simple event broadcast to full req/res/event framing.

**Modify** `crates/rune-gateway/src/ws.rs`
- Add request frame handling: `{"type":"req","id":"<uuid>","method":"<string>","params":{...}}`
- Add response frame: `{"type":"res","id":"<uuid>","ok":true|false,"payload":{...},"error":{...}}`
- Keep event frame: `{"type":"event","event":"<string>","payload":{...},"seq":<number>}`
- Add sequence numbering for gap detection
- Route RPC methods to handler functions (reusing existing route handlers)
- Support `stateVersion` tracking for presence/health

**New file**
- `crates/rune-gateway/src/ws_rpc.rs` — RPC method dispatcher mapping WS methods to existing service calls

---

### Phase 3 — Multi-Channel Adapters (Backend)

Extend `crates/rune-channels/` with new adapters.

**New files**
- `crates/rune-channels/src/discord.rs` — Discord bot via Gateway WebSocket + REST API
- `crates/rune-channels/src/slack.rs` — Slack bot via Socket Mode + Web API
- `crates/rune-channels/src/whatsapp.rs` — WhatsApp Cloud API webhook receiver + send REST
- `crates/rune-channels/src/signal.rs` — Signal via `signal-cli` REST API or linked device

**Modify**
- `crates/rune-channels/src/lib.rs` — Register new adapters in factory
- `crates/rune-config/src/lib.rs` — Add channel config sections for each

---

### Phase 4 — LaneQueue Concurrency Model (Backend)

Replace sequential per-session execution with lane-based FIFO queuing.

**New file**
- `crates/rune-runtime/src/lane_queue.rs`
  - `Lane` enum: `Main`, `Subagent`, `Cron`, `Nested`
  - Per-lane semaphore caps: `Main=4`, `Subagent=8`, `Cron=independent`, `Nested=recursive`
  - FIFO queue per lane using `tokio::sync::Semaphore` + `VecDeque`
  - Task submission returning a handle/future
  - Lane routing based on session kind

**Modify**
- `crates/rune-runtime/src/executor.rs` — Route turn execution through `LaneQueue`

---

### Phase 5 — Hot-Reloading Skills System (Backend)

**New files**
- `crates/rune-runtime/src/skill_loader.rs`
  - Directory scanner for `skills/*/SKILL.md`
  - YAML frontmatter parser
  - File watcher via `notify`
  - Dynamic injection into system prompt per turn
  - Cached fast directory scanning
- `crates/rune-runtime/src/skill.rs`
  - `Skill` struct: `name`, `description`, `parameters_schema`, `binary_path`, `enabled`
  - `SkillRegistry`: add/remove/list/toggle

**Modify**
- `crates/rune-runtime/src/executor.rs` — Inject active skills into system prompt before model call
- `crates/rune-tools/src/lib.rs` — Make `ToolRegistry` support dynamic registration

**Gateway routes**
- `GET /skills`
- `POST /skills/{name}/enable`
- `POST /skills/{name}/disable`

---

### Phase 6 — MCP Client (Backend)

**New crate** `crates/rune-mcp/`
- `src/lib.rs` — MCP client manager
- `src/transport_stdio.rs` — STDIO transport via subprocess stdin/stdout JSON-RPC
- `src/transport_http.rs` — HTTP/SSE transport
- `src/protocol.rs` — MCP JSON-RPC types
- `src/discovery.rs` — MCP server config parsing
- `Cargo.toml` — `tokio`, `serde`, `serde_json`, `reqwest`, `tokio-process`

**Integration**
- MCP tools appear in the tool registry alongside built-ins
- Tool calls route transparently through MCP client
- Config via `[[mcp_servers]]` in `config.toml`

**Modify**
- `crates/rune-config/src/lib.rs`
- `crates/rune-runtime/src/executor.rs`

---

### Phase 7 — Expanded LLM Providers (Backend)

**New files in `crates/rune-models/src/`**
- `google.rs` — Gemini
- `ollama.rs` — Ollama local models + discovery
- `bedrock.rs` — AWS Bedrock ConverseStream
- `groq.rs` — Groq
- `deepseek.rs` — DeepSeek
- `mistral.rs` — Mistral

**Modify**
- `crates/rune-config/src/lib.rs` — Extend provider kind enum
- `crates/rune-models/src/lib.rs` — Register providers

**Gateway routes**
- `POST /models/scan`
- `GET /models`

---

### Phase 8 — TTS Backend + UI

**New crate** `crates/rune-tts/`
- `src/lib.rs` — TTS engine + provider trait
- `src/openai.rs` — OpenAI TTS
- `src/elevenlabs.rs` — ElevenLabs
- `src/config.rs` — provider, voice, model, auto mode (`off|always|inbound|tagged`)

**Gateway routes**
- `GET /tts/status`
- `POST /tts/enable`
- `POST /tts/disable`
- `POST /tts/convert`

**Config**
- Add `[tts]` to `AppConfig`

**UI**
- TTS controls in settings page

---

### Phase 9 — STT Backend + UI

**New crate** `crates/rune-stt/`
- `src/lib.rs` — STT engine + provider trait
- `src/openai.rs` — Whisper transcription
- `src/config.rs` — provider, api key, model

**Gateway routes**
- `GET /stt/status`
- `POST /stt/transcribe`

**Integration**
- Auto-transcribe audio attachments before processing

**Config/UI**
- Add `[stt]` to `AppConfig`
- STT controls in settings page

---

### Phase 10 — Hybrid Memory Search (Backend)

Upgrade keyword search to hybrid vector + PostgreSQL full-text search using `pgvector` + `tsvector`.

**New migration**
```sql
CREATE EXTENSION IF NOT EXISTS vector;
CREATE TABLE memory_embeddings (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  file_path TEXT NOT NULL,
  chunk_text TEXT NOT NULL,
  embedding vector(1536),
  updated_at TIMESTAMPTZ DEFAULT now()
);
CREATE INDEX ON memory_embeddings USING ivfflat (embedding vector_cosine_ops);
CREATE INDEX ON memory_embeddings USING gin (to_tsvector('english', chunk_text));
```

**Modify** `crates/rune-tools/src/memory_tool.rs`
- Replace word-hit scoring with hybrid PostgreSQL query
- Combine `ts_rank(...)` + `1 - (embedding <=> query_embedding)`
- Use Reciprocal Rank Fusion (RRF)
- Re-index lazily on file change

**New file**
- `crates/rune-tools/src/memory_index.rs`
  - Embedding provider abstraction
  - File chunking (~512 tokens)
  - Batch embed + upsert
  - Background re-index on watcher events

**Modify**
- `crates/rune-store/` — Add `MemoryEmbeddingRepo` + Diesel model

**Deps**
- `pgvector`

---

### Phase 11 — Device Pairing (Backend)

**New file**
- `crates/rune-gateway/src/pairing.rs`
  - Ed25519 key generation
  - Challenge-response flow
  - Expiring token issuance
  - Device registry, roles, scopes
  - Approval/rejection flow

**Deps**
- `ed25519-dalek` or `ring`

**Gateway routes**
- `POST /devices/pair/request`
- `POST /devices/pair/approve`
- `POST /devices/pair/reject`
- `GET /devices`
- `DELETE /devices/{id}`
- `POST /devices/{id}/rotate-token`

---

### Phase 12 — Session Enhancements (Backend + UI)

**Backend**
- `DELETE /sessions/{id}` — Delete session + transcript
- `PATCH /sessions/{id}` — Update metadata (`label`, `thinking_level`, `verbose`, `reasoning`)
- Add repo methods: `delete_session()` and `update_session_metadata()`

**UI**
- Usage columns
- Delete button
- Metadata editing in sessions pages

---

### Phase 13 — Usage Analytics (Backend + UI)

**Backend**
- `GET /api/dashboard/usage` — Aggregate token usage by model/day

**UI**
- `ui/src/routes/_admin/usage.tsx` — Tokens, cost, trends, CSV export

---

### Phase 14 — Config Editor (Backend + UI)

**Backend**
- `GET /config`
- `PUT /config`
- `GET /config/schema`

**UI**
- `ui/src/routes/_admin/config.tsx` — Form mode + raw JSON editor, search, diff view

---

### Phase 15 — Log Viewer (Backend + UI)

**Backend**
- Ring buffer tracing subscriber layer + WebSocket/SSE log stream

**UI**
- `ui/src/routes/_admin/logs.tsx` — Streaming tail, level filter, search, JSONL export

---

### Phase 16 — Debug Page (UI)

**UI**
- `ui/src/routes/_admin/debug.tsx`
  - Full and `/health` JSON tree views
  - Manual API tester
  - WebSocket event log ring buffer

No new backend required beyond existing endpoints + arbitrary fetch.

---

### Phase 17 — Agents & Skills Pages (UI)

**UI**
- `ui/src/routes/_admin/agents.tsx` — Agent list, detail, model info
- `ui/src/routes/_admin/skills.tsx` — Skills/tools list, enable/disable, status badges

Uses routes from Phase 5 and a new `GET /agents` endpoint.

---

### Phase 18 — Semantic Browser Snapshots (Backend)

**New crate** `crates/rune-browser/`
- Accessibility tree parser via headless Chromium (`chromiumoxide` or `fantoccini`)
- DOM + ARIA to structured text
- Numeric `[ref=N]` annotations
- Compact snapshot format for model consumption

**Tool integration**
- `browse` tool returns semantic snapshot instead of screenshot

---

### Phase 19 — A2UI Protocol (Backend + UI)

**Backend**
- `crates/rune-gateway/src/a2ui.rs`
  - Streaming JSONL transport for declarative UI components
  - Component types: chart, form, card, table, list
  - `a2ui.push` and `a2ui.reset` events over WebSocket

**UI**
- `ui/src/components/a2ui/` renderer components

---

### Phase 20 — Enhanced Existing Pages (UI)

- **Cron** — Schedule mode selector (`at/every/cron`), payload modes, cloning, advanced filtering, column sorting
- **Dashboard** — Connection status, auth mode, tool count, quick actions
- **Channels** — Per-channel status cards, connection indicators
- **Settings** — TTS/STT sections, device pairing section

---

### Phase 21 — UI Polish

- Focus mode toggle with `localStorage`
- Smart scroll + “New messages” button
- Copy-as-markdown per message
- Image paste with preview + send flow
- Theme transitions via View Transitions API
- RTL detection via `dir="rtl"` on RTL text
- Settings persistence for theme, focus mode, split ratio, show-thinking

---

## Files Summary

### New Crates

| Crate | Purpose |
|---|---|
| `crates/rune-tts/` | Text-to-speech providers |
| `crates/rune-stt/` | Speech-to-text providers |
| `crates/rune-mcp/` | Model Context Protocol client |
| `crates/rune-browser/` | Semantic browser snapshots |

### New Backend Files

| File | Purpose |
|---|---|
| `crates/rune-runtime/src/lane_queue.rs` | Lane-based FIFO concurrency |
| `crates/rune-runtime/src/skill_loader.rs` | `SKILL.md` scanner + hot-reload |
| `crates/rune-runtime/src/skill.rs` | Dynamic skill registry |
| `crates/rune-tools/src/memory_index.rs` | `pgvector` + `tsvector` hybrid index |
| `crates/rune-gateway/src/ws_rpc.rs` | WebSocket RPC dispatcher |
| `crates/rune-gateway/src/pairing.rs` | Ed25519 device pairing |
| `crates/rune-gateway/src/a2ui.rs` | A2UI streaming protocol |
| `crates/rune-models/src/google.rs` | Google Gemini provider |
| `crates/rune-models/src/ollama.rs` | Ollama provider + discovery |
| `crates/rune-models/src/bedrock.rs` | AWS Bedrock provider |
| `crates/rune-models/src/groq.rs` | Groq provider |
| `crates/rune-models/src/deepseek.rs` | DeepSeek provider |
| `crates/rune-models/src/mistral.rs` | Mistral provider |

### New UI Files

| File | Purpose |
|---|---|
| `ui/src/routes/_admin/chat.tsx` | Chat page |
| `ui/src/routes/_admin/usage.tsx` | Usage analytics |
| `ui/src/routes/_admin/debug.tsx` | Debug/API tester |
| `ui/src/routes/_admin/config.tsx` | Config editor |
| `ui/src/routes/_admin/logs.tsx` | Log viewer |
| `ui/src/routes/_admin/agents.tsx` | Agents management |
| `ui/src/routes/_admin/skills.tsx` | Skills/tools management |
| `ui/src/components/chat/*.tsx` | Chat UI components |
| `ui/src/components/a2ui/*.tsx` | A2UI renderers |
| `ui/src/hooks/use-chat.ts` | Chat hooks |
| `ui/src/hooks/use-usage.ts` | Usage hooks |
| `ui/src/hooks/use-models.ts` | Model management hooks |

### Key Files to Modify

| File | Change |
|---|---|
| `crates/rune-gateway/src/ws.rs` | Upgrade to req/res/event framing |
| `crates/rune-gateway/src/routes.rs` | Add new route handlers |
| `crates/rune-gateway/src/server.rs` | Register all new routes |
| `crates/rune-gateway/src/state.rs` | Add TTS/STT/MCP to `AppState` |
| `crates/rune-runtime/src/executor.rs` | LaneQueue routing, skill injection, MCP routing |
| `crates/rune-channels/src/lib.rs` | Register new channel adapters |
| `crates/rune-config/src/lib.rs` | TTS, STT, MCP, providers, channels config |
| `crates/rune-models/src/lib.rs` | Register provider implementations |
| `crates/rune-tools/src/memory_tool.rs` | Hybrid search upgrade |
| `crates/rune-tools/src/lib.rs` | Dynamic tool registration |
| `crates/rune-store/src/repos/session.rs` | Delete + metadata update |
| `Cargo.toml` | Add new crate members |
| `ui/src/lib/api-types.ts` | Add response types |
| `ui/src/components/layout/AdminNavbar.tsx` | Update nav items |
| `ui/src/components/layout/AdminBottomNav.tsx` | Update nav items |
| `ui/src/routes/_admin/sessions.tsx` | Usage columns, delete, metadata |
| `ui/src/routes/_admin/cron.tsx` | Schedule modes, cloning, filtering |
| `ui/src/routes/_admin/models.tsx` | Provider cards, scan, default selector |
| `ui/src/routes/_admin/settings.tsx` | TTS/STT/device sections |
| `ui/src/routes/_admin/index.tsx` | Connection status, quick actions |
| `ui/src/routes/_admin/channels.tsx` | Status cards, connection info |

---

## Verification

```bash
cd ui && npm run build && cd .. && cargo build --release --bin rune-gateway && systemctl --user restart rune-gateway
```
