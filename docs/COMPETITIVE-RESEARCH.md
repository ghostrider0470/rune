# Competitive Research — OpenClaw Rust Rewrite

## Purpose

This document surveys Rust and adjacent open-source projects that are relevant to an OpenClaw-compatible rewrite.

The target is not to copy another project’s shape blindly. The target is to identify:

- what each project actually is
- what is worth borrowing
- what should be avoided
- how relevant it is to this rewrite specifically

The main compatibility constraint remains unchanged:

- **behavioral parity with OpenClaw**
- **full Azure compatibility where provider/runtime integrations matter**

This means a project can be technically impressive and still be the wrong architectural template if it optimizes for a different problem.

---

## Executive take

If this rewrite is serious about being OpenClaw-compatible rather than merely “Rust agent software,” the most useful external reference set is:

1. **zeroclaw** — best direct conceptual comparison for a Rust-first personal assistant runtime
2. **ironclaw** — useful security and isolation reference, but more opinionated and heavier in some areas than this rewrite likely needs at phase 1
3. **goose** — strong reference for agent/tool UX, diagnostics, and extensibility patterns, though developer-agent-centric rather than personal-assistant-centric
4. **rig** — useful as a library design reference for model/provider abstraction, not as the overall system template
5. **wasmtime / Spin** — plugin/runtime isolation references, especially if native plugins are deferred in favor of WASI/process boundaries
6. **tantivy / qdrant** — retrieval/search subsystem references, not core system architecture references

My direct opinion:

- **zeroclaw is the most relevant project to study first**, because it is trying to be a Rust-native assistant runtime rather than just an LLM framework.
- **Do not overfit to zeroclaw’s marketing claims or edge-device posture.** The rewrite’s success criterion is OpenClaw parity, not winning a RAM benchmark chart.
- **The strongest design choice to borrow across the set is trait-driven subsystem boundaries with strict capability gating.**
- **The strongest design choice to avoid is premature over-modularization or over-distribution before session/tool/channel semantics are stable.**

---

## Evaluation criteria used here

Projects were judged against these questions:

1. Does it resemble an assistant runtime, or only an LLM library/framework?
2. Does it have a credible architecture for channels, tools, memory, scheduling, and operator control?
3. Does it help with OpenClaw-style workflows: sessions, approvals, background jobs, gateway, CLI, channels?
4. Does it help or hurt Azure compatibility?
5. Does it push toward a practical monorepo shape in Rust?

---

## 1. zeroclaw

**Repo:** `zeroclaw-labs/zeroclaw`

### What it is

ZeroClaw presents itself as a Rust-native AI assistant runtime / operating system with a trait-driven architecture, swappable providers/channels/tools, secure-by-default runtime ideas, and strong focus on low resource usage and portability.

This is the closest conceptual cousin in the set to an OpenClaw rewrite.

It is not just an LLM library. It is aiming at a whole runtime surface:

- agent infrastructure
- providers
- tools
- channels
- security boundaries
- deploy-anywhere positioning

### What to borrow

#### 1. Trait-driven subsystem boundaries

This is the strongest overlap.

For this rewrite, the equivalent boundaries should be explicit traits/interfaces for:

- model providers
- channels
- tool executors
- memory backends
- schedulers / job runners
- media providers
- auth / approval backends

That maps cleanly to the existing `PLAN.md` direction.

#### 2. Binary-first operational mindset

A Rust daemon that starts fast, stays lean, and runs cleanly on small hardware is a real advantage for OpenClaw-style personal deployment.

Worth borrowing:

- low-runtime-overhead discipline
- single-binary bias where practical
- avoiding Node-style dependency sprawl
- clean operator install story

#### 3. Swappable everything, but through stable contracts

The “provider/channel/tool swappable” stance is correct for this rewrite.

OpenClaw compatibility will require multiple channels, multiple model providers, and likely multiple execution/security modes over time. Designing subsystem seams early is good.

#### 4. Security framing around workspace scoping and explicit allowlists

This is relevant because OpenClaw has real tool power. Workspace scoping, outbound allowlists, and explicit approvals should be first-class domain objects rather than ad hoc middleware.

#### 5. Documentation posture as an operator product

ZeroClaw appears to think in terms of docs, operations, security, and hardware guidance rather than only crate APIs. That is the correct mentality for this rewrite too.

### What to avoid

#### 1. Optimizing around benchmark theater

Low memory and fast startup are good. Designing the system around headline benchmark optics is not.

For this rewrite, the key metric is:

- does it preserve OpenClaw behavior cleanly?

Not:

- can it run in absurdly tiny RAM at any architectural cost?

If forcing everything into an ultra-minimal shape degrades channel adapters, scheduling semantics, transcript fidelity, or Azure integration, that is the wrong trade.

#### 2. Over-general “runtime OS” abstraction too early

The phrase is directionally useful, but dangerous. If taken too literally, it encourages inventing a giant generalized platform before reproducing actual user-visible behavior.

For phase 1, the rewrite should stay concrete:

- gateway
- sessions
- tools
- approvals
- status
- one channel
- one memory path

#### 3. Assuming portability matters more than compatibility

OpenClaw parity includes lots of subtle workflow expectations. A hyper-portable abstraction layer that sands off provider/channel specifics can become counterproductive.

Azure support is a good example: generic OpenAI compatibility is not enough.

### Relevance to this rewrite

**Very high.**

ZeroClaw is the most relevant comparison because it validates that a Rust-native assistant runtime is a sensible target, and it appears to share several key instincts:

- trait-driven architecture
- security-aware runtime design
- deploy-anywhere posture
- modular providers/channels/tools

### Bottom line

Borrow the architectural instincts, not the branding narrative.

**Most worth borrowing:** trait boundaries, binary-first ops, swappable subsystem design, security-first capability boundaries.

**Most worth resisting:** premature platformization, benchmark-driven design, and over-optimizing for tiny hardware before parity.

---

## 2. ironclaw

**Repo:** `nearai/ironclaw`

### What it is

IronClaw is another OpenClaw-inspired Rust implementation, but its visible posture is more security-heavy and infrastructure-opinionated:

- WASM sandboxing
- credential isolation
- endpoint allowlisting
- routines / heartbeat / background execution
- hybrid search
- local-first trust model

This makes it a strong reference for safe assistant runtime design.

### What to borrow

#### 1. Security as a product-level feature, not a patch

IronClaw treats prompt injection resistance, secret protection, outbound control, and sandboxing as system concerns. That is the correct instinct for any assistant that can run tools or touch personal data.

Borrow:

- explicit capability model
- host-side secret injection rather than tool-visible raw secrets
- outbound network allowlisting
- isolated execution for untrusted extensions

#### 2. Heartbeat / routines as first-class runtime concepts

This aligns strongly with OpenClaw behavior. Scheduled and proactive execution should not be bolted on later. They should be modeled in the job system from day one.

#### 3. Hybrid retrieval mindset

Full-text plus vector retrieval is a better long-term direction than vector-only thinking. For a personal assistant workspace, exact match and structured recall often matter more than embeddings alone.

#### 4. Parallel job handling with isolation

Useful reference for modeling concurrent background tasks without letting everything share one mutable global runtime state.

### What to avoid

#### 1. Pulling in phase-1 infrastructure heaviness

IronClaw’s public posture suggests a heavier base stack in some areas, including more advanced sandbox and database assumptions.

This rewrite should be careful not to require too much from day one if SQLite + files + bounded process isolation can get to parity faster.

#### 2. Letting security maximalism damage usability

Security matters, but OpenClaw works partly because it is operationally convenient. Overly rigid sandbox boundaries or too many approval chokepoints can make the system feel worse than the original.

The rewrite should aim for:

- secure defaults
- explicit escalation paths
- good auditability

not constant friction.

#### 3. Designing primarily around dynamic tool generation

Interesting idea, but not core to OpenClaw parity. This can wait.

### Relevance to this rewrite

**High.**

IronClaw is especially relevant for:

- approvals/security design
- heartbeat/routine design
- isolated execution strategy
- memory retrieval posture

### Bottom line

Use IronClaw as a **security architecture reference**, not as the exact phase-1 platform template.

---

## 3. goose

**Repo:** `block/goose`

### What it is

Goose is an open-source Rust AI agent focused heavily on engineering workflows: code generation, code execution, debugging, orchestration, extensions, and diagnostics. It is not an OpenClaw equivalent, but it is a serious Rust agent product with strong operator and developer ergonomics.

### What to borrow

#### 1. Strong CLI and operator experience

This is a major area where many agent systems are weak. Goose appears to care about:

- installation
- diagnostics
- troubleshooting
- custom distributions
- docs for real users

This rewrite should absolutely borrow that seriousness for:

- `status`
- `doctor`
- logs
- config inspection
- environment validation
- support bundles

#### 2. Extensibility without pretending everything is core

Goose’s extensibility posture is useful because OpenClaw-compatible systems will inevitably need custom providers, tools, and integrations.

#### 3. Multi-model/provider pragmatism

Good reference for not hard-coding a single vendor worldview.

### What to avoid

#### 1. Over-centering coding-agent assumptions

Goose is developer-agent-heavy. OpenClaw is broader:

- personal assistant workflows
- channels
- cron/heartbeat
- memory files
- approvals
- media
- messaging platform semantics

This rewrite should not let coding-agent affordances dominate the base architecture.

#### 2. UI/desktop-first assumptions bleeding into core runtime

If any desktop or developer UX choices are tightly coupled in Goose, that is not the right model here. Runtime core should remain headless and API-driven.

### Relevance to this rewrite

**Medium-high.**

Most useful for:

- CLI UX
- diagnostics
- extension workflow
- operator documentation and packaging

Less useful for:

- channel abstractions
- OpenClaw-style memory conventions
- personal assistant semantics

### Bottom line

Borrow the operational product quality, not the developer-agent center of gravity.

---

## 4. rig

**Repo:** `0xPlaygrounds/rig`

### What it is

Rig is a Rust library/framework for building LLM-powered applications. It is not an assistant runtime. It is closer to a composable library ecosystem for providers, agents, embeddings, vector stores, and media-related model capabilities.

### What to borrow

#### 1. Provider abstraction patterns

Rig is useful as a reference for how to organize:

- provider clients
- model capability differences
- unified abstractions without total least-common-denominator collapse

#### 2. Modular crate decomposition

As a Rust ecosystem project, Rig is a useful reference for crate boundaries and public API hygiene.

#### 3. Observability-aware AI abstractions

Its mention of semantic conventions and broader integrations suggests a more mature view of instrumentation than many agent libraries.

### What to avoid

#### 1. Treating an application runtime like a library API

This rewrite is not just an SDK. It needs:

- daemon lifecycle
- session persistence
- job execution
- approvals
- channels
- CLI
- operator UX

A clean library abstraction is necessary but not sufficient.

#### 2. Depending on a third-party framework in a way that constrains Azure-specific behavior

If using Rig helps internally, fine. If it obscures or complicates Azure deployment IDs, auth/header patterns, or provider-specific behavior, it becomes a liability.

My bias here: **use it only as inspiration, not as a foundational dependency for the core runtime.**

### Relevance to this rewrite

**Medium.**

Very useful for provider-layer ideas; not useful as the top-level architecture template.

### Bottom line

Study Rig to improve the **models/provider crate design**, not to shape the whole system.

---

## 5. Wasmtime

**Repo:** `bytecodealliance/wasmtime`

### What it is

Wasmtime is a mature WebAssembly runtime with strong security and configurability. For this rewrite, it matters mainly as a building block for safe plugin execution.

### What to borrow

- proven sandbox/runtime foundation
- configurable resource controls
- security-focused execution model
- good fit if plugin isolation matters early

### What to avoid

- making WASM/plugin infrastructure a prerequisite for shipping parity
- building a complex component-model platform before the simpler process-plugin path is validated

### Relevance to this rewrite

**Medium-high.**

Important if the extension strategy becomes:

- prompt skills for simple behavior
- WASI plugins for untrusted executable extensions

### Bottom line

Wasmtime is a good **implementation option** for plugin isolation, not the architecture itself.

---

## 6. Spin

**Repo:** `spinframework/spin`

### What it is

Spin is a WebAssembly application framework on top of Wasmtime. It is useful as an example of packaging, running, and distributing Wasm components with a good developer experience.

### What to borrow

- plugin/app packaging ideas
- distribution/install ergonomics for Wasm components
- a practical example of language-agnostic extension surfaces

### What to avoid

- adopting Spin’s application model wholesale for assistant plugins
- letting the plugin framework dictate runtime architecture

This rewrite’s plugin model should be shaped by OpenClaw-compatible tool/channel/processor needs, not by generic serverless component concepts.

### Relevance to this rewrite

**Medium.**

Mostly useful as a reference for later plugin lifecycle design.

### Bottom line

Useful inspiration for **how** to package isolated extensions, not **what** the extension model should be.

---

## 7. Tantivy

**Repo:** `quickwit-oss/tantivy`

### What it is

Tantivy is a Rust full-text search engine library, effectively the Lucene-style search building block in Rust.

### What to borrow

- strong full-text retrieval path
- low startup and local indexing fit
- good option if SQLite FTS becomes limiting

### What to avoid

- introducing a dedicated search engine too early
- solving retrieval scale before proving the recall semantics needed by OpenClaw workflows

### Relevance to this rewrite

**Medium.**

Important for memory search evolution, but not phase-1 architecture.

### Bottom line

Use as a **phase-2/3 escalation path** if SQLite FTS is insufficient.

---

## 8. Qdrant

**Repo:** `qdrant/qdrant`

### What it is

Qdrant is a vector database/search engine. High quality, production-grade, and Rust-based — but it solves a narrower problem than this rewrite.

### What to borrow

- proof that Rust can support serious vector retrieval infrastructure
- a clean separation between retrieval service and application runtime if remote semantic search is later needed

### What to avoid

- making vector DB infrastructure mandatory from the start
- assuming semantic search is the center of assistant memory

For OpenClaw-like workflows, memory often needs:

- file-backed conventions
- exact recall
- metadata filters
- human-readable persistence

not just embedding similarity.

### Relevance to this rewrite

**Low-medium.**

Useful only if the system later outgrows local retrieval paths.

### Bottom line

Keep optional. Do not center the architecture around it.

---

## Cross-project patterns worth borrowing

### 1. Trait/interface boundaries between core subsystems

This shows up directly or indirectly across the strongest references.

For this rewrite, stable boundaries should exist around:

- models
- channels
- tools
- jobs/scheduler
- memory
- media
- auth/approvals
- plugins

### 2. Security as capabilities, not ad hoc conditionals

The better projects treat permissions and outbound controls as runtime policy, not scattered `if` statements.

This rewrite should model capabilities explicitly for:

- filesystem scope
- network egress
- process execution
- secret access
- channel send/reply/edit/react rights
- plugin entitlements

### 3. Strong operator UX

Projects that survive operationally usually care about:

- install
- logs
- diagnostics
- docs
- supportability

This is especially important because OpenClaw-compatible users are effectively self-hosting an always-on assistant runtime.

### 4. Local-first pragmatism

Fast startup, small footprint, file-based persistence where it makes sense, and clean Docker support are all worth keeping.

### 5. Isolation of untrusted extensions

Whether the answer is process-based first or WASI-based first, the direction is correct: untrusted executable extensions should not start in-process by default.

---

## Cross-project mistakes to avoid

### 1. Building a generic AI platform before reproducing OpenClaw behavior

This is the biggest risk.

The rewrite should not become:

- a generic multi-agent framework
- a general plugin OS
- a vector-search product
- a coding-agent shell

before it becomes a credible OpenClaw-compatible runtime.

### 2. Over-engineering phase 1 storage

SQLite + files is the right starting bias unless hard evidence disproves it.

### 3. Over-committing to one plugin strategy too early

The likely best sequence is:

1. prompt/resource skills
2. out-of-process executable helpers
3. WASI plugin path if justified
4. in-process native plugins only for trusted/internal cases later

### 4. Letting framework dependencies own the design

Using crates is fine. Letting a framework dictate session semantics, channel abstractions, or provider behavior is not.

### 5. Ignoring Azure-specific sharp edges

Most open-source Rust AI projects are not optimized for Azure-first correctness.

That means this rewrite must deliberately protect:

- Azure endpoint patterns
- deployment-name semantics
- API versions
- headers/auth differences
- Azure Document Intelligence integration
- Azure-friendly container/runtime config

This will need custom handling, regardless of what external projects do.

---

## Practical conclusions for this rewrite

### Recommended architecture stance

- **Primary system model:** closer to **zeroclaw** than to Rig or Goose
- **Security model inspiration:** selectively borrow from **ironclaw**
- **CLI/diagnostics inspiration:** borrow from **goose**
- **provider abstraction inspiration:** borrow from **rig**, but keep core-owned abstractions
- **plugin isolation path:** start process-first, keep **Wasmtime/WASI** as the likely next step
- **retrieval path:** start SQLite FTS + files, keep **Tantivy** as the stronger local-search upgrade path before introducing Qdrant

### Recommended default decisions implied by this research

#### 1. Keep the core runtime application-owned

Do not build the rewrite on top of an agent framework that becomes the real center of gravity.

#### 2. Model channels, sessions, tools, jobs, and approvals as first-class domains

That is what makes OpenClaw OpenClaw.

#### 3. Treat plugins as a layered system

- prompt skills
- safe external helpers
- isolated plugins later

#### 4. Keep Azure compatibility explicit in the provider layer

Do not assume generic “OpenAI-compatible” abstractions will cover the important Azure differences.

#### 5. Resist premature distributed complexity

Single-node, local-first, container-friendly architecture is the correct base.

---

## Suggested follow-up research

If deeper research is needed next, the most valuable follow-up documents would be:

1. **Plugin isolation decision memo**
   - process vs WASI vs in-process trusted plugins
2. **Provider abstraction memo**
   - OpenAI vs Azure OpenAI vs Anthropic vs local models
3. **Channel architecture memo**
   - Telegram-first adapter and normalized event model
4. **Job/scheduler state machine memo**
   - cron, heartbeat, reminders, long-running tasks
5. **Memory retrieval memo**
   - files + SQLite FTS vs Tantivy vs vector layer introduction criteria

---

## Final recommendation

If one external project deserves to influence the rewrite’s overall shape, it is **zeroclaw**.

But the rewrite should still be more conservative than zeroclaw in one important way:

- **parity first, platform second**

That means:

- borrow zeroclaw’s Rust-native modularity
- borrow ironclaw’s security seriousness
- borrow goose’s operator UX discipline
- borrow rig’s provider abstraction ideas
- keep the actual runtime behavior anchored to OpenClaw, not to any one Rust competitor
