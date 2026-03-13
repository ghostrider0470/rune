# Open Questions

## Resolved

1. **What level of CLI verb compatibility matters?**
   - **Decision:** Preserve recognizable command families, global CLI controls, and operator workflow coverage. Exact names are preferred for heavily used/runtime-critical surfaces (`gateway`, `daemon`, `doctor`, `cron`, `channels`, `models`, `memory`, `approvals`, `sessions`, `config`, `configure`, `system`, `sandbox`) and may be looser only on explicitly deferred breadth surfaces.

3. **Should skills be split into prompt skills (markdown bundles) and native plugins (Rust/WASI/process)?**
   - **Decision:** Yes — two-layer model. Prompt skills (markdown `skill.md` files) + CLI tool skills (subprocess execution, process isolation). No WASI/WASM complexity. The AI itself is the primary skill author.

5. **Which channels are phase-1 mandatory?**
   - **Decision:** Telegram (primary, simplest API) + Discord (validates the abstraction layer). WhatsApp, Signal, Slack, Teams in later phases.

6. **Should frontend be React/Next-based Horizon Tech style, or another Horizon Tech standard stack?**
   - **Decision:** React 19 + Vite (pure SPA, NOT Next.js) + TanStack Router/Query/Form/Table + Tailwind CSS 4 + shadcn/ui + Radix UI. Uses the Horizon Tech design system template.

7. **Should memory embeddings default to local for privacy/performance?**
   - **Decision:** Remote only (Azure OpenAI / OpenAI API) for phase 1. No local embedding models initially.

8. **Do we want full local-first operation, or is cloud dependency acceptable for some features?**
   - **Decision:** Local-first with remote embeddings only.

9. **Is SQLite enough for long-term scale, or should Postgres be supported from day one?**
   - **Decision:** PostgreSQL from day one via Diesel + diesel-async. Embedded Postgres (postgresql_embedded) when no `DATABASE_URL` is configured; external Postgres when connection string is provided.

10. **Should full-text search stay in SQLite FTS first, or move to Tantivy early?**
    - **Decision:** PostgreSQL FTS (tsvector/tsquery). Built-in, powerful ranking and language support.

11. **Do we want semantic retrieval to stay embedded/local first, or introduce Qdrant later?**
    - **Decision:** pgvector + remote embeddings (Azure OpenAI / OpenAI API). No Qdrant in phase 1.

12. **Do we want plugins to run in-process eventually, or keep them isolated out-of-process for safety?**
    - **Decision:** Process isolation (subprocess execution). No in-process plugin loading.

13. **What is the canonical runtime directory layout for host and Docker modes?**
    - **Decision:** `/data/db`, `/data/sessions`, `/data/memory`, `/data/media`, `/data/skills`, `/data/logs`, `/data/backups`, `/config`, `/secrets`. All paths configurable via env/config. Identical behavior on host or in container.

14. **Which state must always live on mounted storage, and which state can remain ephemeral?**
    - **Decision:** Everything parity-critical and operator-inspectable stays durable/mounted. Only scratch, transient staging, and rebuildable caches may remain ephemeral.

15. **Which Azure integrations are mandatory in phase 1 vs phase 2?**
    - **Decision:** Azure OpenAI model config (deployment-name handling, API-version handling) is a phase-1 hard requirement. Azure Document Intelligence is parity-inventoried as a hard compatibility surface but may phase behind the initial model-provider milestone if explicitly documented as a temporary divergence. Broader hosting/service conveniences can phase afterward.

## Still Open

2. **Do we want transcript/session file compatibility with OpenClaw, or only import tooling?**
   - Needs decision — import tooling vs direct compatibility.

4. **Do we require ACP-compatible external coding-agent harness support, or can this be redesigned?**
   - Deferred to phase 7.
