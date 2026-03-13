# Confirmed Stack

## Backend / Runtime

Azure compatibility is mandatory.

| Component | Choice | Notes |
|-----------|--------|-------|
| Language | **Rust** | — |
| Async runtime | **Tokio** | — |
| HTTP / WS framework | **Axum** | REST API + WebSocket for realtime |
| CLI | **Clap** | — |
| Serialization | **Serde** / serde_json / serde_yaml | — |
| Config | **figment** | Layered: defaults → file → env → CLI args |
| Logging | **tracing** + tracing-subscriber | — |
| Metrics | **prometheus** + OpenTelemetry | — |
| Scheduling | Tokio tasks + custom scheduler or cron parser crate | — |
| Process supervision | Tokio process + explicit state machine | — |

## Database & Storage

| Component | Choice | Notes |
|-----------|--------|-------|
| Primary DB | **PostgreSQL** via **Diesel** + **diesel-async** | Full ORM with async support |
| Embedded fallback | **postgresql_embedded** | Auto-managed Postgres when no connection string is configured |
| Full-text search | **PostgreSQL FTS** (tsvector/tsquery) | Built-in, powerful ranking and language support |
| Vector search | **pgvector** | PostgreSQL extension for vector similarity search |
| Embeddings | **Remote only** (Azure OpenAI / OpenAI API) | No local embedding models in phase 1 |
| File storage | **Filesystem** | Sessions, memory docs, media, logs — all on mounted paths |
| Secrets | OS keychain or encrypted local store | — |

Diesel ORM with diesel-async for async query execution. Schema-first design with compile-time checked queries and migrations.

When no `DATABASE_URL` is configured, the system automatically starts an embedded PostgreSQL instance via `postgresql_embedded`, storing data under `/data/db`. When a connection string is provided, it connects to the external Postgres server directly.

## LLM Providers (all in phase 1)

- **Azure OpenAI** / Azure AI Foundry (hard requirement)
- **OpenAI**
- **Anthropic**
- **AWS Bedrock**

Provider abstraction trait with per-provider implementations. Azure-specific handling (deployment IDs, API versions, custom headers) must be first-class.

## Messaging Channels

Phase 1:
- **Telegram** (primary, simplest API — validates the core flow)
- **Discord** (validates the abstraction layer)

Later phases:
- WhatsApp, Signal, Slack, Teams

Channels are provider adapters with a shared normalized message/event model and retry/backoff policy per provider.

## API Architecture

- **REST API** for CRUD operations (sessions, config, skills, jobs, channels, etc.)
- **WebSocket** for live streaming (session turns, logs, events, channel messages)
- Both served from the Axum gateway daemon

## Frontend

Target style: Horizon Tech design system.

| Component | Choice | Notes |
|-----------|--------|-------|
| Framework | **React 19 + Vite** | Pure Vite SPA — NOT Next.js/SSR |
| Routing | **TanStack Router** | — |
| Data fetching | **TanStack Query** | — |
| Forms | **TanStack React Form** | — |
| Tables | **TanStack React Table** | — |
| Styling | **Tailwind CSS 4** | — |
| Components | **shadcn/ui + Radix UI** | Horizon Tech design system |
| Icons | **Lucide React** | — |
| Animation | **Motion** (Framer Motion successor) | — |
| Validation | **Zod** | — |
| HTTP client | **Axios** | — |
| Charts | **ECharts** (echarts-for-react) | — |
| Rich text | **TipTap** | — |
| i18n | **i18next + react-i18next** | — |
| Realtime | WS client for live sessions/logs/events | — |
| Auth | Local gateway token/session auth | — |
| Testing | **Vitest** + Testing Library | — |

The backend serves the API; the frontend is a separate build artifact.

## Plugin / Skill System

Two-layer model matching current OpenClaw:

1. **Prompt skills** — `skill.md` markdown files with instructions/context the AI loads
2. **CLI tool skills** — executable tools the AI builds and invokes as subprocess commands (process isolation)

No WASI/WASM complexity. The AI itself is the primary skill author. Process isolation keeps the host safe.

## Memory / Retrieval

- PostgreSQL (via Diesel) for metadata
- Local files for workspace memory
- Remote embeddings only (Azure OpenAI / OpenAI API) — no local embedding models in phase 1
- pgvector for vector search

## Media

- Transcription via provider abstraction
- TTS via provider abstraction
- File/media pipeline isolated from chat runtime

## Azure-Specific Expectations

- First-class support for Azure OpenAI / Azure AI Foundry style providers
- Provider abstraction must handle Azure endpoint shapes, deployment IDs, API versions, and custom headers cleanly
- Document/OCR pipeline should support Azure Document Intelligence as a native integration
- Config model should support Azure resources without hacks or provider-specific leakage into business logic
- Deployment model should not block Azure Container Apps / AKS / App Service / VM-based hosting later

## Deployment

**Docker and bare-metal (systemd) equally from day one.**

- Single container with mounted persistent volumes
- All critical paths configurable via env/config
- Identical behavior on host or in container
- macOS launchd and Windows service support in later phases

### Persistent mount layout

```
/data/db        — Embedded PostgreSQL data directory (when no external connection string)
/data/sessions  — transcripts / session files
/data/memory    — memory docs and derived artifacts
/data/media     — attachments, inbound media, exports
/data/skills    — installed skills/plugins if user-managed at runtime
/data/logs      — structured logs and debug bundles
/data/backups   — backups
/config         — config overlays
/secrets        — mounted secret references if file-based
```

## Key Architectural Decisions

1. **PostgreSQL from day one** — Diesel ORM with diesel-async; embedded Postgres when no connection string configured
2. **Diesel ORM** — schema-first, compile-time checked queries, migrations
3. **All providers in phase 1** — provider abstraction trait with Azure/OpenAI/Anthropic/Bedrock implementations
4. **Process isolation for plugins** — subprocess execution, no in-process loading
5. **Vite SPA frontend** — not SSR/Next.js; the backend serves the API, frontend is a separate build artifact
6. **REST + WS dual API** — standard pattern, easy to test and debug
7. **figment for config** — layered config with file/env/CLI override support
8. **Embedded Postgres fallback** — zero-config local dev/deployment; external Postgres for production

## Recommended Philosophy

- **Rust backend for performance-critical and operationally critical paths**
- **Type-safe web frontend** for operator UX
- **Protocol stability** before optimization of internals
