# OpenClaw vs Rune — Parity Matrix

_Generated 2026-03-20 from code audit of both codebases. Updated 2026-04-10 for replacement-readiness truth._

## Summary

| Metric | OpenClaw | Rune | Gap |
|--------|----------|------|-----|
| Language | TypeScript | Rust | Design choice |
| LLM Providers | 40+ | 10 | -30 (most are niche) |
| Channel Adapters | 21+ | 5 | -16 |
| Built-in Tools (real) | 20+ | 11 | -9 |
| Built-in Skills | 53 | 8 templates | -45 |
| Extensions/Plugins | 65+ | 0 (CLI stubs only) | -65 |
| CLI Commands | 172+ files | 35+ families / 200+ variants | Comparable |
| Gateway RPC Methods | 95+ | 66+ HTTP routes | -29 |
| Database | JSON/file-based | PostgreSQL + SQLite | Rune ahead |
| Docker | Yes | Yes | Parity |
| TTS Providers | Edge TTS, Whisper | OpenAI, ElevenLabs | Comparable |
| STT Providers | Whisper, Sherpa-ONNX | Whisper | -1 |
| Browser Automation | Full Playwright | Snapshot engine | Rune simpler |
| Memory/Vector Search | LanceDB + 5 embedding providers | pgvector + keyword | -4 embedding providers |
| Subagent/Multi-agent | Yes | Yes | Parity |
| Approval System | Basic | Rich (risk classification, policies) | Rune ahead |
| Session Persistence | File/memory-based | RDBMS (PostgreSQL/SQLite) | Rune ahead |

---

## Detailed Comparison by Category

### 1. Model Providers

| Provider | OpenClaw | Rune |
|----------|----------|------|
| OpenAI | Yes | Yes |
| Anthropic (direct) | Yes | Yes |
| Azure OpenAI | Yes | Yes |
| Azure AI Foundry | No | Yes |
| Google Gemini | Yes | Yes |
| Ollama | Yes | Yes |
| AWS Bedrock | Yes | Yes |
| Groq | Yes | Yes |
| DeepSeek | Yes | Yes |
| Mistral | Yes | Yes |
| Perplexity | Yes | No |
| OpenRouter | Yes | No |
| HuggingFace | Yes | No |
| Together AI | Yes | No |
| VLLM/SGLang | Yes | No |
| GitHub Copilot | Yes | No |
| Cloudflare AI | Yes | No |
| XAI/Grok | Yes | No |
| 20+ regional (Moonshot, Minimax, Qwen, etc.) | Yes | No |

**Rune advantage:** Azure AI Foundry (dual-mode Anthropic/OpenAI router).
**Gap:** 30 niche/regional providers. Most users need only 3-5.

### 2. Channel Adapters

| Channel | OpenClaw | Rune |
|---------|----------|------|
| Telegram | Yes | Yes |
| Discord | Yes | Yes |
| Slack | Yes | Yes |
| WhatsApp | Yes | Yes |
| Signal | Yes | Yes |
| iMessage | Yes | No |
| Microsoft Teams | Yes | No |
| Google Chat | Yes | No |
| Matrix | Yes | No |
| IRC | Yes | No |
| LINE | Yes | No |
| Feishu/Lark | Yes | No |
| Mattermost | Yes | No |
| Twitch | Yes | No |
| Nostr | Yes | No |
| WebChat (in-browser) | Yes | Yes |
| 5+ others | Yes | No |

**Remaining gap:** Teams/Google Chat for enterprise plus the broader long-tail channel adapters.

### 3. Tool System

| Tool | OpenClaw | Rune |
|------|----------|------|
| File read/write/edit/list/delete | Yes | Yes (REAL) |
| File search (regex) | Yes | Yes (REAL) |
| Command exec (shell) | Yes | Yes (REAL) |
| Background processes | Yes | Yes (REAL) |
| Browser automation | Yes (full Playwright) | Yes (snapshot engine) |
| Memory search | Yes | Yes (REAL) |
| Session management | Yes | Yes (REAL) |
| Subagent spawn/steer/kill | Yes | Yes (REAL) |
| Cron management | Yes | Yes (REAL) |
| Message send (channel) | Yes | Yes (REAL) |
| Gateway control | Yes | Yes (REAL) |
| Web fetch/search | Yes | No (stub) |
| Image generation | Yes | No (stub) |
| Git operations | Yes | No (stub) |
| Docker operations | Yes | No (stub) |
| AWS/cloud operations | Yes | No (stub) |
| Database query | Yes | No (stub) |
| Canvas/A2UI rendering | Yes | No |
| PDF extraction | Yes | No |
| Video frame extraction | Yes | No |

**Rune has 11 real tools + 30 stubs.** OpenClaw has 20+ real tools.
**Critical gap:** web-fetch, git, and image generation are the most commonly used missing tools.

### 4. Skills & Plugins

| Capability | OpenClaw | Rune |
|-----------|----------|------|
| Built-in skills | 53 | 8 templates |
| Plugin discovery/loading | Yes | No (stub) |
| Plugin marketplace | Yes | No |
| Hook system (pre/post events) | Yes (15+ hook types) | No (stub) |
| Dynamic tool registration | Yes | Partial (SkillRegistry) |
| Skill hot-reload | Yes | Yes |

**Critical gap:** Plugin/hook execution engine is the biggest extensibility gap. Without it, Rune can't load community extensions.

### 5. Session & Memory

| Feature | OpenClaw | Rune |
|---------|----------|------|
| Session persistence | File/memory | PostgreSQL + SQLite |
| Session state machine | Implicit | Explicit 10-state FSM |
| Turn execution | Yes | Yes (with lane-queue concurrency) |
| Context assembly | Yes | Yes (transcript + memory + system prompt) |
| Memory (long-term) | MEMORY.md | MEMORY.md |
| Memory privacy boundaries | No | Yes (Direct-only for MEMORY.md) |
| Vector search | LanceDB | pgvector (optional) |
| Embedding providers | 5 (OpenAI, Gemini, Ollama, Mistral, Voyage) | 0 (keyword only without pgvector) |
| MMR/hybrid search | Yes | Partial (BM25 + vector if pgvector) |

**Rune advantage:** RDBMS-backed sessions survive crashes; explicit FSM; privacy boundaries.
**Gap:** Embedding provider variety.

### 6. Security & Approval

| Feature | OpenClaw | Rune |
|---------|----------|------|
| Tool approval | Basic allow/deny | Risk-classified (Low/Medium/High) |
| Approval policies (per-tool) | No | Yes |
| Allow-always persistence | No | Yes (durable) |
| Sandbox (filesystem) | Yes | Yes |
| Process audit trail | No | Yes |
| First-use bypass warning | No | Yes |
| Channel allowlists | Yes | Partial — provider/webhook authenticity checks exist where applicable, but no sender/user/chat allowlist policy surface yet; this remains a documented replacement-readiness blocker even though issue #898 is closed as a truth/documentation pass |
| Role-based access | Basic | No |

**Rune advantage:** Significantly more sophisticated approval system.

### 7. Deployment & Operations

| Feature | OpenClaw | Rune |
|---------|----------|------|
| Docker | Yes | Yes |
| systemd/launchd service | Yes | Yes (`rune service install`) |
| Zero-config startup | Yes (--allow-unconfigured) | Yes (Ollama auto-detect) |
| Config management | JSON | TOML + env overrides |
| Health checks | Yes | Yes |
| Doctor diagnostics | Yes (comprehensive) | Yes |
| Interactive setup wizard | Yes (onboard) | Yes (`rune setup` / `rune onboard` / `rune configure`) |
| Auto-update | Yes | Partial (`rune update check/apply/status/wizard`) |
| TLS/HTTPS | Yes | No |

**Critical gap:** TLS.

---

## Critical Path: Rune as OpenClaw Replacement

### Must-Have (historical gap list; not the canonical readiness verdict):
1. **web-fetch tool** — historical note; shipped in the current Horizon execution environment, so do not use this row as the operator truth source
2. **git tool** — historical note; shipped in the current Horizon execution environment, so do not use this row as the operator truth source
3. **WebChat channel** — operator needs browser-based interaction
4. **TLS/HTTPS** — production deployment requirement
5. **Plugin execution engine** — extensibility is core to the product

### Should-Have (blocks feature parity):
6. Embedding provider (at least OpenAI embeddings for vector search)
7. Image generation tool
8. systemd/launchd service management
9. Teams/Google Chat channels (enterprise adoption)
10. A2UI/Canvas rendering

### Nice-to-Have (OpenClaw has, but rarely used):
11. 30+ niche LLM providers
12. 16+ niche channel adapters
13. 53 built-in skills (most are thin wrappers)
14. PDF/video extraction tools

---

## Self-Maintaining Rune: What's Needed

For Rune to maintain its own code (self-coding agent):

1. **Working git tool** — read/write/commit/push/PR
2. **Working web-fetch** — read issues, documentation, APIs
3. **File tools** — already working
4. **Exec tool** — already working (cargo build, cargo test)
5. **Session persistence** — already working (survives across turns)
6. **Subagent orchestration** — already working (spawn coder agents)
7. **Approval system** — already working (operator reviews before destructive ops)

**Historical note:** this self-maintenance summary is stale if read outside its original generation context. For current operator truth about replacement claims, use `docs/operator/REPLACEMENT-READINESS.md` and `docs/operator/HEALTH-AND-DOCTOR.md` instead of this matrix.
