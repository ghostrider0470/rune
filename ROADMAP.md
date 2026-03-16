# Rune — Full OpenClaw Parity + Superior Multi-Agent Orchestration & Context Management

## Goal

Build a single Rust binary that does everything OpenClaw does — messaging-first AI agent across 12+ channels, skills ecosystem, file/shell/browser tools, TTS/STT, MCP, device pairing, cron/heartbeat automation — but with two critical advantages that none of the competition (OpenClaw, OpenCode, Kilo Code) have:

1. **Multi-agent orchestration that actually works.** An orchestrator that decomposes complex goals into a dependency graph of subtasks, spawns specialist sub-agents (Architect, Coder, Debugger) in isolated git worktrees, runs them in parallel where possible, handles partial failures, and synthesizes results. OpenClaw has basic sub-agents with no coordination. Kilo has orchestrator mode but it's IDE-only and single-repo. Rune's orchestrator works across any channel and any project.

2. **Context management that doesn't waste tokens.** Dynamic priority-based context budgeting that allocates the model's context window intelligently — more transcript budget in conversation-heavy sessions, more memory budget in knowledge-heavy ones. Automatic compression of older turns into structured summaries (not just truncation). Cross-session context sharing so orchestrator sub-agents get exactly the context they need, nothing more. A memory bank that auto-maintains architectural knowledge and project conventions.

3. **A better admin UI.** OpenClaw has a dashboard but it's built on a legacy Node.js stack. Rune's admin panel is built on React + TanStack Router + Tailwind — modern, fast, type-safe, mobile-responsive. Full feature set: live chat with streaming + tool cards + thinking blocks, session management, usage analytics with cost tracking, config editor with validation and diff view, log viewer with streaming tail, debug/API tester, agents & skills management, cron job builder, channel status cards, and A2UI for agent-rendered interactive components. Better DX, better UX, better stack.

The result: a self-hosted AI agent that is more capable, more secure (Rust, no 430K-line JS attack surface, no 9 CVEs), more durable (PostgreSQL, not JSONL), more observable (full admin UI), and smarter at complex multi-step work than anything else available.

## How to Start

Give your AI agent this prompt:

```
Read ROADMAP.md in full. Then:

0. Run `git status` and `cargo build`. If there are uncommitted changes, read them and understand
   what's in progress before touching anything. If the build is broken, fix it first. Do not
   discard or overwrite uncommitted work — it may be partially completed phase work from a
   previous session. If unsure, ask.
1. Read the Current State Audit table to find the first phase whose pillar is ❌ or ⚠️.
2. Check GitHub issues: `gh issue list --label "rune-phase"`. If no issue exists for this phase,
   create one: `gh issue create --title "Phase N — <name>" --label "rune-phase" --body "<phase summary>"`.
   Assign yourself to it.
3. Read that phase's detailed spec in docs/specs/phases-*.md.
4. Read the existing code it depends on (the files listed under "Modify" in the phase).
5. Implement the phase. Write real code, not stubs. Every public function needs at least one test.
6. Run `cargo build` and `cargo test` — fix until both pass.
7. Commit your work with a clear message referencing the phase number.
8. Update ROADMAP.md: change the audit table status to ✅ with today's date and a one-line
   summary of what you built.
9. Close the GitHub issue with a comment summarizing what was built:
   `gh issue close <number> --comment "Completed: <summary>. Commit: <hash>"`
10. Move to the next phase. Repeat from step 1.

If a phase's spec is missing or incomplete, write the spec first, then implement.
If you find a bug in the existing code while implementing, fix it and note what you fixed in the roadmap.
If a phase depends on a previous phase that isn't done, do the dependency first.
Do one phase at a time. Don't skip ahead.
Do NOT use git worktrees during development. Work directly on the main branch. Worktree isolation
is a product feature (Phase 23) for Rune's end users, not a development workflow for building Rune.
```

## Instructions for AI Agents

**This document is your source of truth. You are expected to edit it.**

- Before starting a phase, read its spec in `docs/specs/` and this roadmap. If anything is unclear, ambiguous, or wrong — fix it here before writing code.
- After completing a phase, update the **Current State Audit** table (change ❌ to ✅, update notes with what was built and the date).
- If you discover a better approach during implementation, update the relevant phase description to reflect what you actually built and why. Future agents reading this document should see the real architecture, not the original plan.
- If a phase turns out to need sub-phases or additional work not listed, add them. If a phase is unnecessary, mark it as skipped with a reason.
- Add implementation notes inline (like the existing `docs/FUNCTIONALITY-CHECKLIST.md` does) with dates so there's a clear audit trail.
- Reference the detailed specs in `docs/specs/phases-*.md` for exact types, wire protocols, error cases, edge cases, and test scenarios. Those files are also yours to edit and improve.
- Do not treat this roadmap as immutable. It is a living document. The goal above is fixed. Everything below it is a plan that should evolve as you learn more.

### GitHub Issue Tracking

All phase work MUST be tracked via GitHub issues. This is how the project owner monitors progress.

- **Label:** Every phase issue gets the `rune-phase` label. Create it if it doesn't exist: `gh label create rune-phase --color 0E8A16 --description "Rune roadmap phase work"`
- **Creating issues:** Before starting a phase, check if an issue exists (`gh issue list --label rune-phase`). If not, create one with the title format `Phase N — <Phase Name>` and a body that includes: phase summary, key deliverables as a checklist, and files to create/modify.
- **Progress updates:** As you work, post comments on the issue with progress updates — especially when hitting blockers or making design decisions that deviate from the spec.
- **Closing issues:** When a phase is complete (build passes, tests pass, roadmap updated), close the issue with a summary comment including the commit hash.
- **Sub-issues:** If a phase is large, break it into sub-issues linked to the parent. Use `gh issue create --title "Phase N.1 — <subtask>" --label rune-phase --body "Part of #<parent>"`.
- **Bug issues:** If you find and fix a bug while working on a phase, create a separate issue for it with the `bug` label, fix it, and close it. Reference it in the phase issue.

---

Context: Full rewrite of OpenClaw's architecture in Rust + comprehensive admin UI, with multi-agent orchestration and context management that surpasses OpenClaw, OpenCode, and Kilo Code. Rune already has several pillars implemented; this plan covers everything missing to reach full OpenClaw parity and then exceed it with smarter agent coordination and richer context intelligence.

---

## Current State Audit

| Pillar | Status | Notes |
|---|---|---|
| WebSocket Gateway | ✅ Basic | Req/res/event framing, sequence numbering, gap detection, and RPC dispatch landed; broader parity work remains |
| Multi-channel Routing | ✅ 2026-03-16 | Telegram plus Discord/Slack/WhatsApp/Signal adapters landed with factory wiring, inbound normalization, outbound delivery flows, and adapter test coverage |
| Device Pairing | ✅ 2026-03-16 | Ed25519 challenge-response pairing, PostgreSQL-backed device/request persistence, SHA-256 token storage, supervisor pruning, and route/integration coverage landed |
| LaneQueue Concurrency | ✅ 2026-03-16 | LaneQueue implemented with per-lane caps, TurnExecutor integration, runtime.lanes visibility, and FIFO/cancellation coverage |
| File-based Identity | ✅ Done | SOUL.md, USER.md, AGENTS.md, TOOLS.md, IDENTITY.md |
| Session History | ✅ Done | PostgreSQL-based (not JSONL) |
| Hybrid Memory Search | ✅ 2026-03-16 | Persisted pgvector + tsvector hybrid search landed with MemoryEmbeddingRepo, RRF retrieval, startup bootstrap indexing, and change-driven workspace reindexing |
| Hot-reloading Skills | ✅ 2026-03-14 | SkillLoader + SkillRegistry landed with SKILL.md scanning, runtime enable/disable, reload reconciliation, prompt injection, and gateway/RPC controls |
| MCP Client | ✅ 2026-03-16 | Config-driven STDIO/HTTP MCP client landed with initialize handshake, tool discovery, `server__tool` registry bridging, and gateway startup integration |
| PTY Execution Sandbox | ✅ Basic | Via Unix script, no advanced PTY |
| Heartbeat + Silent Eval | ✅ Done | HEARTBEAT_OK suppression works |
| Sub-agents | ✅ Done | Session spawning + manager trait |
| Semantic Browser Snapshots | ❌ Missing | No accessibility tree parsing |
| A2UI Protocol | ❌ Missing | No streaming UI components |
| TTS | ❌ Missing | No text-to-speech providers |
| STT | ❌ Missing | No speech-to-text providers |
| LLM Providers | ✅ Partial | Anthropic, OpenAI, Azure only — missing Google, Ollama, Bedrock, Groq, DeepSeek, Mistral |
| Admin UI | ✅ Basic | Shell + basic pages exist, missing chat, usage, debug, config, logs, agents, skills |
| Agent Modes | ❌ Missing | No Orchestrator/Architect/Coder/Debugger modes — beyond OpenClaw |
| Git Worktree Isolation | ❌ Missing | No isolated agent execution environments — beyond OpenClaw |
| Context Compression | ❌ Missing | No intelligent context windowing or priority-based assembly — beyond OpenClaw |
| Memory Bank | ❌ Missing | No architectural decision records or project knowledge base — beyond OpenClaw |
| Extended Channels | ❌ Missing | No LINE/Mattermost/Matrix/Feishu/iMessage — OpenClaw breadth |
| Calendar/Email | ❌ Missing | No calendar or email integration — OpenClaw productivity |

---

## Reference Note

We can also inspect other community Rust rewrites of OpenClaw — Panther, ZeroClaw, and ZeptoClaw — to compare implementation choices around safety, single-binary distribution, and Tokio-based async performance.

### Competitive Landscape

| Project | Language | Stars | Core Strength | Rune Advantage |
|---|---|---|---|---|
| **OpenClaw** | Node.js | 100K+ | Messaging-first AI agent, 50+ channel integrations, skills ecosystem (2,857+ on ClawHub) | Rust single-binary, PostgreSQL-backed durability, no 430K-line JS security surface (9 CVEs in OpenClaw), superior multi-agent orchestration |
| **OpenCode** | Go | 120K+ | TUI (Bubble Tea), LSP integration, plan mode, SQLite storage, SSE streaming | Rune adds multi-channel routing, admin web UI, skills, sub-agents, TTS/STT, orchestrator mode |
| **Kilo Code** | TypeScript | N/A (1.5M users) | IDE extensions (VS Code/JetBrains), orchestrator mode, code review agent, MCP marketplace, cloud agents | Rust performance, self-hosted by default, multi-channel messaging (Kilo is IDE-only), no subscription lock-in |

**Rune's unique position:** OpenClaw-complete messaging-first agent with multi-agent orchestration and context management that exceeds what any of the three offers individually. Rust single-binary, PostgreSQL durability, and security-first design.

Core architectural pillars to recreate:

1. **WebSocket Gateway & Routing Engine** — Async WebSocket server (default port 18789) as the central control plane. Strict JSON framing: requests `{type:"req", id, method, params}` and events `{type:"event", event, payload}`. Multi-channel routing (Telegram, Discord, Slack, WhatsApp). Device pairing with Ed25519 challenge nonces.
2. **LaneQueue Concurrency Model** — FIFO queue enforcing “Default Serial, Explicit Parallel”. Per-lane caps: `main=4`, `subagent=8`, `cron=independent`, `nested=recursive`.
3. **State, Memory, and Identity Management** — File-based identity via `SOUL.md`, `USER.md`, `AGENTS.md`. Session history in plain `.jsonl` in OpenClaw; Rune currently uses PostgreSQL. Hybrid memory search should combine vector embeddings with keyword matching (Rune should use PostgreSQL `pgvector` + `tsvector`).
4. **Extensibility and Tooling** — Hot-reloading skills via `SKILL.md` directory scanning with fast parsing and dynamic system prompt injection. MCP client with STDIO + HTTP transports. PTY execution sandbox with `allowlist` / `deny` / `full` security modes for interactive CLI support.
5. **Proactive Autonomy** — Heartbeat cron loop (30 min default) with silent evaluation via `HEARTBEAT.md` checklist and `HEARTBEAT_OK` suppression. Sub-agent spawning in isolated sessions for parallel tasks.
6. **Advanced Interfaces and Browsing** — Semantic browser snapshots via accessibility tree parsing with numeric `[ref=N]` annotations. A2UI protocol with streaming JSONL transport for declarative UI components rendered progressively by frontends.

**Beyond-parity pillars (superior orchestration & context):**

7. **Agent Modes & Orchestration** — Multiple specialized modes (Orchestrator, Architect, Coder, Debugger, Ask) with custom user-defined modes. Orchestrator decomposes complex tasks and delegates to specialist sub-agents in parallel. OpenClaw has basic sub-agents; Rune's orchestrator is smarter about task decomposition, dependency ordering, and result synthesis.
8. **Git Worktree Isolation** — Parallel agent execution in isolated git worktrees. Each sub-agent gets its own workspace, preventing conflicts during multi-task work. LLM-assisted conflict resolution on merge.
9. **Intelligent Context Management** — Priority-based context assembly that dynamically selects what goes into the prompt window based on task relevance. Context compression for long sessions. Cross-session context sharing between related agents. Smarter than OpenClaw's static file-loading approach.
10. **Memory Bank & Architectural Knowledge** — Structured project knowledge base (architecture, conventions, decisions) that auto-updates and provides rich context to all agents. Goes beyond OpenClaw's basic `MEMORY.md` with semantic indexing and team onboarding.

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

Status: ✅ Completed 2026-03-16

Rune now ships real Discord, Slack, WhatsApp, and Signal adapters in `crates/rune-channels/`, wired through the adapter factory and channel config. The delivered implementation favors durable REST/webhook/polling flows over idealized gateway/socket integrations where that keeps the single-binary runtime simpler and testable.

**Landed work**
- `crates/rune-channels/src/discord.rs` — Discord REST adapter with inbound polling, outbound send/edit/delete/react, and rate-limit retry handling
- `crates/rune-channels/src/slack.rs` — Slack Web API adapter with local Events API listener, outbound messaging flows, and retry handling
- `crates/rune-channels/src/whatsapp.rs` — WhatsApp Cloud API sender + webhook verification/signature validation/deduplication flow
- `crates/rune-channels/src/signal.rs` — Signal `signal-cli` REST adapter with polling receive loop and outbound send path
- `crates/rune-channels/src/lib.rs` — factory registration for all adapters
- `crates/rune-config/src/lib.rs` — channel config coverage for the new adapters

**Validation**
- `cargo test -p rune-channels --lib --tests`
- adapter factory coverage plus provider-specific unit tests for inbound normalization, outbound delivery, retries, verification, and deduplication

Implementation note (2026-03-16): the roadmap originally called for Discord Gateway and Slack Socket Mode specifically. The current shipped adapters use REST + polling/webhook mechanisms where appropriate to keep the runtime robust and fully testable in CI. If native gateway/socket parity becomes product-critical later, treat it as an enhancement pass on top of the now-working adapter surface.

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

Status: ✅ Completed 2026-03-14

The hot-reloading skills system is already in place.

**Landed work**
- `crates/rune-runtime/src/skill_loader.rs`
  - scans `skills/*/SKILL.md`
  - parses YAML frontmatter
  - reconciles additions/removals across reloads
  - provides background watcher-style rescanning
- `crates/rune-runtime/src/skill.rs`
  - `Skill` / `SkillFrontmatter`
  - `SkillRegistry` with add/remove/list/toggle APIs
- `crates/rune-runtime/src/executor.rs`
  - injects enabled skills into the system prompt before model calls
- gateway + WS RPC controls
  - `GET /skills`
  - `POST /skills/reload`
  - `POST /skills/{name}/enable`
  - `POST /skills/{name}/disable`
  - `skills.list`, `skills.reload`, `skills.enable`, `skills.disable`

Implementation note (2026-03-14): the current watcher uses periodic rescanning rather than a hard dependency on `notify`, but the externally visible hot-reload behavior, registry reconciliation, and runtime prompt injection are already delivered and tested.

---

### Phase 6 — MCP Client (Backend)

Status: ✅ Completed 2026-03-16

Rune now ships a config-driven MCP client that can connect to external tool servers over STDIO or HTTP, complete the initialize handshake, discover tools, and expose them through the existing tool registry.

**Landed work**
- `crates/rune-mcp/`
  - STDIO and HTTP transports, including `notifications/initialized`
  - multi-server manager with initialize + `tools/list` handshake flow
  - `server__tool` namespacing and tool invocation routing
  - `McpToolExecutor` bridge into Rune's tool registry/executor surface
- `crates/rune-config/src/lib.rs`
  - `mcp_servers` config entries with transport, args, env, cwd, URL, and enabled flag support
- `apps/gateway/src/main.rs`
  - startup bootstrap for configured MCP servers
  - MCP tool registration alongside built-ins
  - execution routing for MCP-prefixed tool names

**Validation**
- `cargo test -p rune-config`
- `cargo test -p rune-mcp`
- `cargo build -p rune-gateway-app`

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

Status: ✅ Completed 2026-03-16

Keyword-only memory search has been upgraded to persisted hybrid retrieval using PostgreSQL full-text ranking plus pgvector-style vector search, fused through Reciprocal Rank Fusion (RRF).

**Landed work**
- migration for `memory_embeddings`
- `crates/rune-store/`
  - `MemoryEmbeddingRepo`
  - Diesel/raw-SQL backing types and `PgMemoryEmbeddingRepo`
- `crates/rune-tools/src/memory_tool.rs`
  - persisted hybrid backend integration
  - local keyword fallback when embeddings/config are unavailable
- `crates/rune-tools/src/memory_index.rs`
  - embedding provider abstraction
  - file chunking
  - batch embedding
  - RRF merge logic
  - repo-backed helpers for reindex/remove flows
- `apps/gateway/src/main.rs`
  - startup bootstrap of persisted workspace memory index
  - change-driven workspace memory reindexing/removal sync

**Landed commits**
- `5282e64` — `feat(memory): persist hybrid memory search`
- `defa456` — `Bootstrap persisted hybrid memory index`
- `49be041` — `feat(memory): reindex persisted workspace changes`

**Validation**
- `cargo build`
- `cargo test -p rune-tools memory --lib --tests`
- `cargo test -p rune-gateway-app sync_workspace_memory_index`

Implementation note (2026-03-16): the original roadmap described a direct PostgreSQL hybrid query and lazy reindex-on-change behavior. The shipped implementation matches that outcome via persisted embedding storage, startup bootstrap indexing, and workspace change reconciliation, with graceful fallback to local keyword search whenever semantic indexing cannot be configured.

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
- RTL detection via `dir=”rtl”` on RTL text
- Settings persistence for theme, focus mode, split ratio, show-thinking

---

## Beyond-Parity Phases (Superior Orchestration & Context)

### Phase 22 — Agent Modes & Orchestration (Backend)

Multiple specialized agent modes with an orchestrator that decomposes complex tasks into coordinated sub-agent work. OpenClaw has basic sub-agents; Rune's orchestrator is smarter about task decomposition, dependency ordering, parallel execution, and result synthesis.

**New files**
- `crates/rune-runtime/src/agent_mode.rs`
  - `AgentMode` enum: `Orchestrator`, `Architect`, `Coder`, `Debugger`, `Ask`, `Custom(String)`
  - Per-mode system prompt templates
  - Per-mode tool permission sets (Architect/Ask = read-only, Coder = full, Debugger = read + exec)
  - Mode switching mid-session via message prefix or API
- `crates/rune-runtime/src/orchestrator.rs`
  - Task decomposition: break complex goals into subtasks via LLM planning step
  - Subtask delegation: spawn sub-agent sessions with appropriate mode
  - Progress tracking: monitor subtask completion, aggregate results
  - Dependency graph: sequential vs parallel subtask execution based on data dependencies
  - Result synthesis: combine subtask outputs into coherent response
  - Failure recovery: retry failed subtasks, re-route to different mode, or escalate to user
- `crates/rune-runtime/src/custom_mode.rs`
  - User-defined modes via `modes/*.md` directory scanning
  - YAML frontmatter: `name`, `description`, `tools`, `system_prompt`, `read_only`
  - Hot-reload on file change (reuse `notify` watcher from skill_loader)

**Modify**
- `crates/rune-runtime/src/executor.rs` — Route through mode-specific system prompt + tool filtering
- `crates/rune-config/src/lib.rs` — Default mode, available modes config

**Gateway routes**
- `GET /modes` — List available modes
- `POST /sessions/{id}/mode` — Switch session mode
- `GET /modes/{name}` — Get mode definition

---

### Phase 23 — Git Worktree Isolation (Backend)

Parallel agent execution in isolated git worktrees so multiple orchestrator subtasks can work simultaneously without file conflicts. Each sub-agent gets its own workspace with LLM-assisted merge on completion.

**New file**
- `crates/rune-runtime/src/worktree.rs`
  - `WorktreeManager`: create, list, merge, cleanup worktrees
  - Auto-create worktree for orchestrator subtasks
  - Branch naming: `rune/agent/<session-id>/<task-slug>`
  - Worktree path: `.rune/worktrees/<session-id>/`
  - Auto-exclude from git via `.git/info/exclude`
  - Diff reviewer: unified/split view of all changes per worktree
  - Merge strategy: auto-merge non-conflicting, flag conflicts for human review
  - Cleanup: auto-delete worktree after merge or on session end

**Integration**
- Orchestrator (Phase 22) spawns subtasks in isolated worktrees
- Each sub-agent's file tools operate within its worktree
- Results are merged back to the main branch on completion
- Conflict resolution via LLM-assisted merge

**Modify**
- `crates/rune-runtime/src/executor.rs` — Set working directory per session based on worktree
- `crates/rune-tools/src/lib.rs` — Scope file tools to worktree root

---

### Phase 24 — Intelligent Context Management (Backend)

Smarter context assembly than OpenClaw's static file-loading. Priority-based context windowing, compression for long sessions, and cross-session context sharing between related agents.

**New files**
- `crates/rune-runtime/src/context_manager.rs`
  - `ContextBudget`: allocate token budget across system prompt, memory, tools, transcript, and user message
  - Priority scoring: rank context items by relevance to current task (recency, semantic similarity, explicit references)
  - Dynamic allocation: more budget to transcript in conversation-heavy sessions, more to memory in knowledge-heavy ones
  - Hard limit enforcement: never exceed model's context window, with clear truncation strategy
- `crates/rune-runtime/src/context_compression.rs`
  - Transcript summarization: compress older turns into summaries when approaching context limit
  - Tool output compression: collapse verbose tool results into key findings
  - Progressive detail: recent turns at full fidelity, older turns summarized, oldest turns as bullet points
  - Checkpoint system: save compressed state to DB so sessions can resume without re-summarizing
- `crates/rune-runtime/src/context_sharing.rs`
  - Cross-session context: orchestrator shares relevant context with sub-agents
  - Scoped sharing: only share what's relevant to the subtask, not the entire parent context
  - Context inheritance: sub-agents inherit parent's memory bank + task-specific context
  - Result propagation: sub-agent findings automatically flow back to parent context

**Modify**
- `crates/rune-runtime/src/executor.rs` — Replace static context assembly with `ContextManager`
- `crates/rune-store/src/repos.rs` — Persist compressed context checkpoints

---

### Phase 25 — Memory Bank & Architectural Knowledge (Backend)

Structured project knowledge base that auto-updates and provides rich context to all agents. Goes beyond OpenClaw's basic `MEMORY.md` with semantic indexing, architectural decision records, and team onboarding.

**New file**
- `crates/rune-runtime/src/memory_bank.rs`
  - `ARCHITECTURE.md` — Auto-maintained architectural overview
  - `DECISIONS.md` — Architectural decision records (ADRs) with date, context, decision, consequences
  - `CONVENTIONS.md` — Code style, naming patterns, project conventions
  - `DEPENDENCIES.md` — Key dependency inventory with rationale
  - Auto-update on significant code changes (new files, major refactors)
  - Team onboarding: `/onboard` command generates project briefing from memory bank
  - Context injection: memory bank is loaded into system prompt for all sessions
  - Stale detection: flag knowledge that may be outdated based on recent file changes

**Integration**
- Memory bank files live in `.rune/knowledge/` directory
- Auto-indexed by hybrid memory search (Phase 10)
- `memory_bank` tool: read, update, search the knowledge base
- `/onboard` command: generate a project briefing from the memory bank
- Orchestrator (Phase 22) uses memory bank to inform task decomposition

**Modify**
- `crates/rune-runtime/src/executor.rs` — Load memory bank into context
- `crates/rune-tools/src/lib.rs` — Register memory bank tools

---

### Phase 26 — Extended Channel Support (Backend)

Additional messaging platforms beyond the core five (Telegram, Discord, Slack, WhatsApp, Signal) to match full OpenClaw breadth.

**New files in `crates/rune-channels/src/`**
- `line.rs` — LINE Messaging API
- `mattermost.rs` — Mattermost Bot API + WebSocket
- `matrix.rs` — Matrix client-server API (via `matrix-sdk`)
- `feishu.rs` — Feishu/Lark Bot API
- `irc.rs` — IRC client (via `irc` crate)
- `google_chat.rs` — Google Chat API
- `teams.rs` — Microsoft Teams Bot Framework

**Modify**
- `crates/rune-channels/src/lib.rs` — Register new adapters
- `crates/rune-config/src/lib.rs` — Config sections for each

---

### Phase 27 — Calendar & Email Integration (Backend)

Personal productivity automation — the “personal assistant” side of OpenClaw.

**New crate** `crates/rune-productivity/`
- `src/lib.rs` — Productivity integrations manager
- `src/calendar.rs` — Calendar provider trait + Google Calendar + Outlook Calendar
- `src/email.rs` — Email provider trait + IMAP/SMTP + Gmail API
- `src/contacts.rs` — Contact lookup for email/calendar context

**Tools**
- `calendar_list` — List upcoming events
- `calendar_create` — Create event
- `calendar_update` — Update event
- `calendar_delete` — Delete event
- `email_search` — Search inbox
- `email_read` — Read email
- `email_send` — Send email (with approval gate)
- `email_reply` — Reply to email

**Config**
- Add `[calendar]` and `[email]` to `AppConfig`

---

## Files Summary

### New Crates

| Crate | Purpose |
|---|---|
| `crates/rune-tts/` | Text-to-speech providers |
| `crates/rune-stt/` | Speech-to-text providers |
| `crates/rune-mcp/` | Model Context Protocol client |
| `crates/rune-browser/` | Semantic browser snapshots |
| `crates/rune-productivity/` | Calendar + email integration |

### New Backend Files

| File | Purpose |
|---|---|
| `crates/rune-runtime/src/lane_queue.rs` | Lane-based FIFO concurrency |
| `crates/rune-runtime/src/skill_loader.rs` | `SKILL.md` scanner + hot-reload |
| `crates/rune-runtime/src/skill.rs` | Dynamic skill registry |
| `crates/rune-runtime/src/agent_mode.rs` | Agent mode definitions + switching |
| `crates/rune-runtime/src/orchestrator.rs` | Task decomposition + multi-agent coordination |
| `crates/rune-runtime/src/custom_mode.rs` | User-defined modes via `modes/*.md` |
| `crates/rune-runtime/src/worktree.rs` | Git worktree isolation for parallel agents |
| `crates/rune-runtime/src/context_manager.rs` | Priority-based context budget allocation |
| `crates/rune-runtime/src/context_compression.rs` | Transcript summarization + checkpoint system |
| `crates/rune-runtime/src/context_sharing.rs` | Cross-session context for orchestrator sub-agents |
| `crates/rune-runtime/src/memory_bank.rs` | Architectural knowledge base |
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
| `crates/rune-channels/src/line.rs` | LINE adapter |
| `crates/rune-channels/src/mattermost.rs` | Mattermost adapter |
| `crates/rune-channels/src/matrix.rs` | Matrix adapter |
| `crates/rune-channels/src/feishu.rs` | Feishu/Lark adapter |
| `crates/rune-channels/src/irc.rs` | IRC adapter |
| `crates/rune-channels/src/google_chat.rs` | Google Chat adapter |
| `crates/rune-channels/src/teams.rs` | Microsoft Teams adapter |

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
| `crates/rune-gateway/src/routes.rs` | Add new route handlers (modes, calendar, email) |
| `crates/rune-gateway/src/server.rs` | Register all new routes |
| `crates/rune-gateway/src/state.rs` | Add TTS/STT/MCP to `AppState` |
| `crates/rune-runtime/src/executor.rs` | LaneQueue routing, skill injection, MCP routing, mode dispatch, context manager, worktree scoping |
| `crates/rune-channels/src/lib.rs` | Register new channel adapters (core five + extended seven) |
| `crates/rune-config/src/lib.rs` | TTS, STT, MCP, providers, channels, calendar, email config |
| `crates/rune-models/src/lib.rs` | Register provider implementations |
| `crates/rune-tools/src/memory_tool.rs` | Hybrid search upgrade |
| `crates/rune-tools/src/lib.rs` | Dynamic tool registration + memory bank tools |
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
# Full build: UI + Gateway
cd ui && npm run build && cd .. && cargo build --release --bin rune-gateway && systemctl --user restart rune-gateway
```
