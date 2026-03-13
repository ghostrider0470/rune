# Parity Spec

This rewrite is not an OpenClaw-inspired redesign.
It is a parity-targeted replacement.

The governing standard is:

- preserve OpenClaw’s user-visible and operator-visible behavior
- preserve Azure compatibility as a first-class requirement
- preserve Docker-first mountable persistence as a first-class requirement
- allow internal reimplementation only where it does not change behavioral contracts

---

## 1. Hard constraints

The Rust rewrite must be:

1. **Functionally identical to OpenClaw** from the user/operator perspective
2. **Fully Azure compatible** across relevant provider and deployment surfaces

These are not stretch goals.
They are release gates.

---

## 2. What parity means

Parity means that a user, operator, channel client, or automated test interacting through supported surfaces should observe the same practical outcomes for the same intent.

Parity is judged at these layers:

1. **workflow parity**
   - the same operational tasks can be completed with the same practical expectations
2. **behavior parity**
   - the same action produces materially equivalent state changes, outputs, and side effects
3. **protocol parity**
   - the same subsystem boundaries and machine-readable semantics exist where clients depend on them
4. **safety/policy parity**
   - approval gates, privacy boundaries, and access rules remain intact

Parity does **not** require:

- identical source code structure
- identical crate/module/class layout
- identical storage schema
- identical HTTP route names if higher-level contracts remain intact

---

## 3. Parity surfaces

The following are parity-critical surfaces.
Changing them requires an explicit compatibility decision.
`PARITY-INVENTORY.md` is the anchor surface census and command/resource/event inventory; this document defines the release rule applied to that inventory.

## 3.1 Gateway and daemon behavior

Must preserve behavior in:

- daemon lifecycle and control model
- local/remote gateway assumptions where supported
- status and health workflows
- auth/token or equivalent operator access controls
- background process supervision visibility

Acceptance expectation:
- operators should be able to start, stop, inspect, and diagnose the runtime with equivalent confidence and ergonomics

## 3.2 CLI workflows

Must preserve:

- practical command coverage for status, gateway, daemon, sessions, cron, channels, config/configure, sandbox, skills/plugins, logs, health, approvals
- broader command-family inventory tracked in `PARITY-INVENTORY.md`, including node/nodes, devices/pairing, browser, ACP, message, setup/update/backup/reset breadth
- stable operator mental model
- structured/machine-readable outputs where applicable
- expected failure semantics and diagnostics
- explicit defer-vs-ship decisions for non-tier-0 command families rather than silent omission

Acceptance expectation:
- an experienced OpenClaw operator should not need to relearn the product to perform normal tasks

## 3.3 Session and runtime behavior

Must preserve:

- session lifecycle
- transcript ordering and persistence
- turn execution model
- context assembly boundaries
- model invocation abstraction semantics
- compaction/pruning behavior at the behavioral level
- usage/cost accounting behavior where visible

Acceptance expectation:
- equivalent triggers and inputs should produce equivalent turn lifecycle behavior and inspectable history

## 3.4 Tool semantics

Must preserve:

- first-class tool identities and intent
- argument validation behavior
- approval gating behavior
- foreground/background execution semantics
- process handle semantics for long-running work
- auditability and transcript integration

This is a strict surface.
Silent tool semantic drift is not acceptable.

## 3.5 Scheduler and automation behavior

Must preserve:

- cron scheduling concepts
- reminder behavior
- heartbeat behavior and quiet/no-op semantics
- isolated scheduled runs
- job history and inspection workflows

## 3.6 Memory behavior

Must preserve:

- workspace memory conventions
- long-term vs daily memory distinction
- safe retrieval/snippet behavior
- privacy boundaries by session type/context
- update workflows for memory artifacts

## 3.7 Skills behavior

Must preserve:

- prompt-triggered skill concept
- selection behavior and specificity rules
- packaging of instructions/resources
- operator understanding of what skill was loaded and why

## 3.8 Channel behavior

Must preserve:

- normalized inbound/outbound semantics
- reply/edit/react behavior where channel supports it
- media attachment flow concepts
- direct vs group routing behavior
- channel-specific privacy/participation boundaries

## 3.9 Media behavior

Must preserve at the conceptual level:

- inbound audio transcription flow
- attachment/media ingestion
- image understanding handoff to model providers
- TTS reply workflow where configured

## 3.10 Observability and diagnostics

Must preserve:

- logs/events sufficient to diagnose runtime behavior
- inspectability of sessions, jobs, tools, approvals, and processes
- operator-facing health and doctor-style workflows

---

## 4. Behavioral invariants

These invariants define the minimum bar for parity.

1. **Same intent, same kind of outcome**
   - equivalent user/operator input must produce materially equivalent state transitions and outputs

2. **No hidden approval scope expansion**
   - `allow-once` cannot silently approve later commands

3. **No privacy-boundary regression**
   - files or memory restricted to main-session use must not leak into shared/external contexts

4. **No transcript ambiguity**
   - session history must remain ordered, attributable, and explainable after tool loops, compaction, retries, or background work

5. **No silent background loss**
   - backgrounded work must remain inspectable and controllable after the initiating turn ends

6. **No provider leakage into core abstractions**
   - channel/provider-specific quirks may exist, but core contracts must stay normalized

7. **No fake Azure support**
   - Azure compatibility must mean real endpoint/auth/version/deployment compatibility, not generic OpenAI compatibility with renamed fields

---

## 5. Allowed internal differences

The following may differ internally without breaking parity:

- implementation language
- crate/module boundaries
- internal async orchestration strategy
- database engine choices
- internal event bus implementation
- frontend implementation details
- plugin ABI design
- storage schema and indexing approach

These differences are allowed only if they do not alter parity surfaces.

---

## 6. Explicitly not allowed

The following are not acceptable without an explicit approved compatibility decision:

- breaking user-visible workflows
- changing tool semantics while keeping the same tool names
- reducing Azure compatibility to generic OpenAI compatibility
- weakening approval or privacy behavior
- removing inspectability of jobs/processes/sessions
- changing heartbeat or scheduler behavior in ways that create spam, silence, or missed expected runs
- replacing skill selection behavior with opaque or non-explainable heuristics

---

## 7. Azure compatibility scope

Azure compatibility must support, at minimum:

- Azure OpenAI / Azure AI Foundry model endpoints
- Azure-style auth headers and API-version handling
- deployment-specific naming/configuration
- Azure Document Intelligence integration paths
- Azure-friendly containerized deployment patterns
- config and secret patterns suitable for Azure infrastructure

Azure support is parity-critical because it is a stated requirement of the rewrite, not an optional adapter.

---

## 8. Practical parity thresholds by subsystem

A subsystem should only be called parity-complete when all three are true:

1. **observable behavior matches**
2. **failure behavior is acceptable and documented**
3. **black-box tests exist for both success and failure paths**

### 8.1 Gateway
- lifecycle workflows work
- errors are diagnosable
- live event streams are inspectable

### 8.2 Runtime
- sessions and turns are durable and attributable
- compaction does not break future behavior
- usage/accounting state is persisted where expected

### 8.3 Tools
- names, arguments, and result semantics match expectations
- approvals bind to exact payloads
- long-running work remains manageable

### 8.4 Scheduler
- schedules execute as expected
- quiet/no-op behavior is preserved
- run history is durable

### 8.5 Memory
- retrieval respects privacy boundaries
- sources are attributable
- updates follow current conventions

### 8.6 Channels
- normalization is stable
- reply/edit/react semantics are preserved where supported
- group/direct routing behavior remains correct

### 8.7 Skills
- selection remains inspectable and specific
- resource packaging works predictably
- operator can understand loaded behavior

---

## 9. How parity is measured

Parity should be measured using black-box evidence, not intuition.

Preferred evidence sources:

1. golden traces from current OpenClaw behavior
2. CLI command/response comparisons
3. event-sequence comparisons
4. transcript delta comparisons
5. approval prompt/result comparisons
6. scheduler run-history comparisons
7. Azure request-construction snapshots

If a behavior cannot be tested or inspected, it is not yet a trustworthy parity claim.

---

## 10. Compatibility test implications

The project should eventually have parity tests for:

- CLI command behavior
- gateway endpoints and protocol behavior
- session/tool orchestration behavior
- process/background execution behavior
- scheduled job behavior
- memory privacy-boundary behavior
- channel normalization and outbound action behavior
- skill selection behavior
- Azure provider request construction and response normalization

---

## 11. Decision rule for ambiguity

When documentation or existing behavior is ambiguous, prefer this order:

1. observed OpenClaw behavior
2. operator expectation and workflow continuity
3. written protocol contract
4. implementation convenience

Implementation convenience is last on purpose.

---

## 12. Release gating rule

Do not claim the Rust rewrite is functionally identical until:

- parity-critical subsystems have acceptance coverage
- known divergences are documented explicitly
- Azure compatibility has concrete validation evidence
- privacy and approval invariants are still intact

Until then, it is a parity-seeking rewrite, not a parity-complete replacement.
