# Rune — Full OpenClaw Parity + Superior Multi-Agent Orchestration & Context Management

> Status: Legacy roadmap/planning artifact retained for provenance during the planning-canonicalization transition.
>
> Canonical product strategy now lives in `rune-plan.md`.
> Use `docs/IMPLEMENTATION-PHASES.md` for parity-phase sequencing and acceptance criteria.
> Use GitHub Project 2 for live execution state.

## Goal

Build a single Rust binary that does everything OpenClaw does — messaging-first AI agent across 12+ channels, skills ecosystem, file/shell/browser tools, TTS/STT, MCP, device pairing, cron/heartbeat automation — but with two critical advantages that none of the competition (OpenClaw, OpenCode, Kilo Code) have:

1. **Multi-agent orchestration that actually works.** An orchestrator that decomposes complex goals into a dependency graph of subtasks, spawns specialist sub-agents (Architect, Coder, Debugger) in isolated git worktrees, runs them in parallel where possible, handles partial failures, and synthesizes results. OpenClaw has basic sub-agents with no coordination. Kilo has orchestrator mode but it's IDE-only and single-repo. Rune's orchestrator works across any channel and any project.

2. **Context management that doesn't waste tokens.** Dynamic priority-based context budgeting that allocates the model's context window intelligently — more transcript budget in conversation-heavy sessions, more memory budget in knowledge-heavy ones. Automatic compression of older turns into structured summaries (not just truncation). Cross-session context sharing so orchestrator sub-agents get exactly the context they need, nothing more. A memory bank that auto-maintains architectural knowledge and project conventions.

3. **A better admin UI.** OpenClaw has a dashboard but it's built on a legacy Node.js stack. Rune's admin panel is built on React + TanStack Router + Tailwind — modern, fast, type-safe, mobile-responsive. Full feature set: live chat with streaming + tool cards + thinking blocks, session management, usage analytics with cost tracking, config editor with validation and diff view, log viewer with streaming tail, debug/API tester, agents & skills management, cron job builder, channel status cards, and A2UI for agent-rendered interactive components. Better DX, better UX, better stack.

The result: a self-hosted AI agent that is more capable, more secure (Rust, no 430K-line JS attack surface, no 9 CVEs), more durable (PostgreSQL, not JSONL), more observable (full admin UI), and smarter at complex multi-step work than anything else available.

## Historical implementation prompt

This preserved prompt reflects an earlier planning-stage execution style.
It is kept for provenance, not as the current branch/PR workflow authority.

At minimum, interpret it through the current accepted model:
- do **not** implement directly on `main`
- do **not** use git worktrees by default
- use one active coding branch and one active PR per coherent batch
- use GitHub Project 2, linked issues, and PRs as the live execution control plane

## Instructions for AI Agents

**This document is historical planning context, not the current execution authority.**

- Before starting a phase, read its spec in `docs/specs/` and this roadmap. If anything is unclear, ambiguous, or wrong — fix it here before writing code.
- After completing a phase, update the **Current State Audit** table (change ❌ to ✅, update notes with what was built and the date).
- If you discover a better approach during implementation, update the relevant phase description to reflect what you actually built and why. Future agents reading this document should see the real architecture, not the original plan.
- If a phase turns out to need sub-phases or additional work not listed, add them. If a phase is unnecessary, mark it as skipped with a reason.
- Add implementation notes inline (like the existing `docs/FUNCTIONALITY-CHECKLIST.md` does) with dates so there's a clear audit trail.
- Reference the detailed specs in `docs/specs/phases-*.md` for exact types, wire protocols, error cases, edge cases, and test scenarios. Those files are also yours to edit and improve.
- Do not treat this roadmap as immutable. It is a living document. The goal above is fixed. Everything below it is a plan that should evolve as you learn more.

### GitHub Issue Tracking

All current execution work should be tracked via GitHub Project 2, linked issues, and PRs. This section is retained as historical planning guidance rather than the primary live control plane.

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
| WebSocket Gateway | ✅ 2026-03-28 | Req/res/event framing, sequence numbering, gap detection, stateVersion tracking, and RPC dispatch landed; broader parity work remains |
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
| Semantic Browser Snapshots | ⚠️ 2026-03-16 | `rune-browser` now emits semantic snapshots, exposes a real `browse` tool, and is wired through gateway/config against the existing CDP snapshot path; Chromium launch/pool lifecycle and selector-aware extraction still need follow-up |
| A2UI Protocol | ✅ 2026-03-28 | A2UI event bus, tool/RPC wiring, and admin UI renderers landed with inline/panel component rendering plus form/action callbacks |
| WebSocket Gateway note | ✅ 2026-03-28 | Phase 2 WebSocket gateway RPC protocol slice tracked in #538 is shipped; issue checklist completed and verified on current main lineage |
| TTS | ❌ Missing | No text-to-speech providers |
| STT | ❌ Missing | No speech-to-text providers |
| LLM Providers | ✅ Partial | Anthropic, OpenAI, Azure only — missing Google, Ollama, Bedrock, Groq, DeepSeek, Mistral |
| Admin UI | ✅ 2026-03-28 | Admin shell now includes shipped chat, usage, debug, config, logs, agents, and skills pages; broader UX polish and deeper parity still remain |
| Agent Modes | ❌ Missing | No Orchestrator/Architect/Coder/Debugger modes — beyond OpenClaw |
| Git Worktree Isolation | ❌ Missing | No isolated agent execution environments — beyond OpenClaw |
| Context Compression | ⚠️ 2026-03-29 | Context tier budgeting, compaction diagnostics, checkpoint metadata, and delegated context handoff for subagents are shipped; transcript summarization/checkpoint persistence still need follow-up |
| Memory Bank | ❌ Missing | No architectural decision records or project knowledge base — beyond OpenClaw |
| Extended Channels | ❌ Missing | No LINE/Mattermost/Matrix/Feishu/iMessage — OpenClaw breadth |
| Calendar/Email | ✅ 2026-03-26 | Microsoft 365 calendar, mail, files, users, Planner, and To-Do routes/services landed in gateway with auth and integration coverage |

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

Status: ✅ Completed 2026-03-28

The admin chat page and its supporting component set are now shipped in the UI, backed by the existing session message/transcript APIs and WebSocket event stream.

**Landed work**
- `ui/src/routes/_admin/chat.tsx` — chat workspace with session selector, live transcript state, inspector panel, mobile drawer handling, and A2UI integration
- `ui/src/components/chat/ChatThread.tsx` — grouped transcript rendering with tool inspection hooks
- `ui/src/components/chat/ChatMessage.tsx` — role-aware message bubbles with markdown and thinking/tool affordances
- `ui/src/components/chat/ChatInput.tsx` — send UX with multiline compose and attachment handling
- `ui/src/components/chat/ToolCard.tsx` — collapsible inline tool call/result cards with inspector selection
- `ui/src/components/chat/ThinkingBlock.tsx` — collapsible thinking block extraction and rendering
- `ui/src/components/chat/MarkdownRenderer.tsx` — sanitized markdown rendering via `marked` + `DOMPurify`
- `ui/src/components/chat/ChatSidebar.tsx` — resizable side inspector for tool details
- `ui/src/components/chat/CopyMarkdown.tsx` — copy-as-markdown affordance
- `ui/src/components/chat/ImageAttachment.tsx` — attachment preview support
- `ui/src/hooks/use-chat.ts` — chat session loading, transcript merge logic, send mutation, and WS subscription plumbing
- `ui/src/components/layout/AdminNavbar.tsx` + `AdminBottomNav.tsx` — Chat promoted as the first primary navigation destination

**Validation**
- `cd ui && npm run build`
- `cargo check`

Implementation note (2026-03-28): the roadmap entry was stale relative to the codebase. This pass verified the shipped UI surface, validated the build, and updated roadmap bookkeeping to reflect reality rather than re-implementing already-landed chat functionality.

---

### Phase 2 — WebSocket Gateway RPC Protocol (Backend)

Status: ✅ Completed 2026-03-28

Rune already ships the req/res/event WebSocket RPC protocol, sequence numbering, and connection-visible `stateVersion` tracking described in this roadmap item. This pass verified the implementation on current `main` and updated roadmap bookkeeping to match shipped reality.

**Landed work**
- `crates/rune-gateway/src/ws.rs`
  - request frame handling for `{"type":"req","id":"<uuid>","method":"<string>","params":{...}}`
  - response frames for success/error with `stateVersion`
  - event frames with monotonic `seq` numbering for gap detection
  - subscription management plus state-change-aware version bumps
- `crates/rune-gateway/src/ws_rpc.rs`
  - RPC dispatcher mapping WS methods onto existing gateway/runtime services
  - broad parity coverage for session, agent, tools, approvals, process, memory, dashboard, and doctor methods
- `crates/rune-gateway/src/lib.rs`
  - module wiring for the WS RPC dispatcher

**Validation**
- `cargo check -p rune-gateway`
- `cargo test -p rune-gateway ws:: -- --nocapture`

Implementation note (2026-03-28): the roadmap entry was stale relative to the codebase. Instead of re-implementing already-landed functionality, this pass verified the shipped backend behavior and marked the item complete so the roadmap reflects actual repo state.

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

Status: ✅ Re-verified 2026-03-29

LaneQueue remains shipped and correct on the current main lineage. The implementation provides independent FIFO-backed concurrency lanes for direct/channel, subagent, and scheduled sessions, plus project-scoped tool concurrency limits used by the runtime tool executor path.

**Landed work**
- `crates/rune-runtime/src/lane_queue.rs`
  - `Lane` enum: `Main`, `Subagent`, `Cron`
  - Per-lane semaphore caps: `Main=4`, `Subagent=8`, `Cron=1024`
  - FIFO waiter queues per lane using `tokio::sync::Semaphore` + `VecDeque`
  - cancellation-safe wakeup behavior covered by tests
  - global + per-project tool concurrency caps via `ToolConcurrencyQueue`
  - lane routing helpers from `SessionKind`
- `crates/rune-runtime/src/executor.rs`
  - turn execution acquires a lane permit before session transition/model loop
  - runtime surfaces expose lane stats for operators

Implementation note (2026-03-29): audited against issue #617. No code changes were required; targeted `lane_queue` tests and full `cargo check` passed on the current branch, and this roadmap entry was updated to match the implementation that actually shipped.

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

Status: ✅ Completed 2026-03-28

Rune already ships the expanded provider surface from this phase, including Gemini, Ollama, Bedrock, Groq, DeepSeek, and Mistral provider implementations, config wiring, and gateway model inventory endpoints. This pass closed the remaining roadmap bookkeeping gap by validating the current implementation and tightening the model scan route to reuse the provider-native Ollama discovery path.

**Landed work**
- `crates/rune-models/src/provider/`
  - `google.rs` — Gemini provider
  - `ollama.rs` — Ollama OpenAI-compatible provider plus `/api/tags` discovery
  - `bedrock.rs` — AWS Bedrock provider
  - `groq.rs` — Groq provider
  - `deepseek.rs` — DeepSeek provider
  - `mistral.rs` — Mistral provider
- `crates/rune-models/src/lib.rs`
  - provider registration and config-driven provider factory coverage for the expanded backend set
- `crates/rune-gateway/src/routes.rs`
  - `GET /models` inventory listing
  - `POST /models/scan` local model discovery using `rune_models::OllamaProvider::list_models()`
  - explicit `discovered` flag on configured model inventory rows so configured-vs-discovered surfaces stay distinguishable
- `crates/rune-gateway/tests/route_tests.rs`
  - coverage for `GET /models`
  - coverage for Ollama-backed `POST /models/scan`

**Validation**
- `cargo test -p rune-gateway list_models_marks_configured_inventory_as_not_discovered -- --nocapture`
- `cargo test -p rune-gateway scan_models_uses_ollama_provider_discovery -- --nocapture`
- `cargo check`

Implementation note (2026-03-28): most of this phase was already present in the codebase. The shipped delta here was consolidating gateway-side Ollama scan behavior onto the model provider's native discovery implementation and updating the roadmap to reflect actual repo state.

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

Status: ✅ Completed 2026-03-28

Rune already had the STT backend crate, gateway status/transcribe/toggle routes, and runtime-side inbound audio transcription wiring in place. This pass closed the remaining operator-surface gap by shipping the missing Settings UI toggle hooks for runtime STT enable/disable so the page now matches the backend capability.

**Landed work**
- `crates/rune-stt/`
  - provider trait, engine, config, and OpenAI transcription backend
- `crates/rune-gateway/src/routes.rs` + `crates/rune-gateway/src/server.rs`
  - `GET /stt/status`
  - `POST /stt/transcribe`
  - `POST /stt/enable`
  - `POST /stt/disable`
- `crates/rune-runtime/src/session_loop.rs`
  - inbound audio attachment enrichment via STT before turn execution
- `ui/src/hooks/use-system.ts`
  - added `useSttEnable` and `useSttDisable` mutations
- `ui/src/routes/_admin/settings.tsx`
  - added runtime STT toggle control and status card parity with TTS

**Validation**
- `cd ui && npm run build`
- `cargo check`

Implementation note (2026-03-28): the roadmap entry was partially stale. Backend STT and runtime transcription plumbing were already shipped; the unfinished slice was the admin Settings control surface for toggling STT at runtime. This pass shipped that missing UI layer and updated roadmap bookkeeping to reflect the actual delivered state.

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

Status: ✅ Completed 2026-03-28

Rune already ships the backend usage aggregation endpoint and the admin Usage page with range filters, grouped breakdowns, summary cards, and CSV export. This pass verified the implementation against the roadmap and updated the roadmap status to match the shipped code.

**Landed work**
- `crates/rune-gateway/src/routes.rs`
  - `GET /api/dashboard/usage` with period/custom range filtering, token aggregation, cache-hit ratio reporting, and estimated cost formatting
- `crates/rune-gateway/src/server.rs`
  - route registration for `/api/dashboard/usage`
- `ui/src/hooks/use-operators.ts` + `ui/src/hooks/use-usage.ts`
  - typed React Query usage hook for analytics fetching
- `ui/src/routes/_admin/usage.tsx`
  - summary cards for prompt/completion/total tokens, estimated cost, cache hit ratio
  - preset/custom date range filtering
  - grouped breakdown table by model or date
  - CSV export of usage entries

**Validation**
- `cargo check`
- `cd ui && npm run build`

Implementation note (2026-03-28): the roadmap entry was stale relative to the repository state. No missing implementation remained for the phase acceptance bar, so this update records the actual shipped status instead of redoing already-landed functionality.

---

### Phase 14 — Config Editor (Backend + UI)

Status: ✅ Completed 2026-03-28

Rune already shipped the admin config surface with viewer/search, doctor integration, and raw JSON editing. This pass closes the remaining backend gap by exposing a typed config schema endpoint and wiring the UI hook/types for schema-aware editor follow-up work.

**Landed work**
- `crates/rune-gateway/src/routes.rs`
  - `GET /config/schema` returns a JSON-schema-like view of the current redacted config shape
- `crates/rune-gateway/src/server.rs`
  - route registration for `/config/schema`
- `crates/rune-config/src/lib.rs`
  - config-derived schema generation from the redacted effective config
- `ui/src/hooks/use-system.ts`
  - `useConfigSchema()` React Query hook
- `ui/src/lib/api-types.ts`
  - config schema response types for UI consumers

**Validation**
- `cargo check`
- `cd ui && npm run build`

Implementation note (2026-03-28): the schema endpoint is currently a lightweight JSON-schema-like structure inferred from the live redacted config rather than a full hand-authored schema contract. That is enough to unblock schema-aware editor UX without exposing secrets, and can be hardened later if strict validation metadata becomes necessary.

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

Implementation note (2026-03-16): this checkpoint lands semantic snapshot formatting, blocked/invalid URL handling, config-driven browse tool registration, and gateway execution wiring/tests around the `browse` tool. The backend still reuses the in-tree CDP snapshot engine rather than launching and recycling Chromium instances itself, and selector-aware extraction remains unimplemented, so this phase is still partial rather than complete.

---

### Phase 19 — A2UI Protocol (Backend + UI)

Status: ✅ Completed 2026-03-28

Rune now ships an end-to-end A2UI surface across the gateway WebSocket event bus and the admin chat UI. The backend exposes A2UI event types plus `a2ui_push`, `a2ui.form_submit`, and `a2ui.action` plumbing; the UI renders inline/panel components and sends callbacks back over WS RPC.

**Landed work**
- `crates/rune-gateway/src/a2ui.rs` — A2UI component/event model, broadcast helpers, and `a2ui_push` tool executor
- `crates/rune-gateway/src/ws_rpc.rs` — `a2ui.form_submit` and `a2ui.action` RPC handlers
- `ui/src/hooks/use-a2ui.ts` — session-scoped component state plus RPC callback helpers
- `ui/src/components/a2ui/*.tsx` — card/table/list/form/chart/kv/progress/code renderers with graceful fallback
- `ui/src/routes/_admin/chat.tsx` — inline/panel A2UI rendering integrated into the shipped chat workspace

**Validation**
- `cd ui && npm run build`
- `cargo check`

Implementation note (2026-03-28): the base A2UI skeleton already existed in-tree but did not satisfy the roadmap's interactive acceptance bar. This pass completed the missing UI interaction layer, added renderer coverage for the declared component set, and updated roadmap bookkeeping to reflect the shipped state.

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

**Implementation note (2026-03-29):** Core context-management groundwork is now partially shipped on current `main`: context tier budgeting/diagnostics, compaction-required metadata persistence, and delegated context handoff for sub-agents via `create_subagent_session_with_context` plus prompt injection of `delegation_context` and `shared_scratchpad`. The remaining gap in this roadmap item is true transcript/tool-output summarization with persisted checkpoints and broader cross-session relevance selection.

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
 Verification

```bash
# Full build: UI + Gateway
cd ui && npm run build && cd .. && cargo build --release --bin rune-gateway && systemctl --user restart rune-gateway
```
+ Gateway
cd ui && npm run build && cd .. && cargo build --release --bin rune-gateway && systemctl --user restart rune-gateway
```
