# Rune Docs / README / Planning Reorganization Proposal

> Status: **Proposal for Hamza verification**
> 
> This document is a planning/review artifact only. It does **not** authorize automatic
> rewrites, deletions, or moves by itself. Structural/content changes should begin only after
> explicit verification from Hamza.
>
> Verification note (2026-03-17): `rune-plan.md` has now been introduced at repo root as the
> canonical strategy file. `docs/PLAN.md`, `docs/STACK.md`, and `docs/WORKPLAN.md` remain in
> place as transitional provenance while the wider docs/README cleanup remains pending.

---

## 1. Why this lane exists

Rune currently has real product momentum, but its documentation and planning surfaces are split across:

- a public-facing `README.md`
- a root-level `ROADMAP.md`
- a root-level `rune-plan.md`
- `docs/IMPLEMENTATION-PHASES.md`
- a large number of parity/spec/strategy/reference docs in `docs/`
- live execution state in GitHub Project 2

That creates four problems:

1. **Too many sources of truth**
2. **README positioning is too absolute / ahead of shipped reality**
3. **Planning docs overlap in purpose**
4. **Audience boundaries are unclear** (user/operator/contributor/reference)

This proposal defines a detailed target structure that separates:

- public product positioning
- operator documentation
- contributor architecture/reference
- long-form design rationale
- live execution planning

---

## 2. Design principles

### 2.1 One source of truth per concern

Each concern needs one canonical home.

### 2.2 Audience-first docs

Docs should be organized by **who is reading** and **what they need to do**, not by historical file accretion.

### 2.3 Public honesty over aspirational wording

The README should describe what Rune **is today**, what it **targets**, and what is **still in progress**.

### 2.4 Project 2 is live execution truth

GitHub Project 2 should own:
- active execution queue
- Epic / Feature / Story hierarchy
- current progress state

Docs should explain the system, not replace the board.

### 2.5 Strategy, execution, and decisions must be distinct

- **Strategy** = where Rune is going
- **Execution** = what is being worked now
- **Decisions** = why key architectural choices were made

---

## 3. Current-state assessment

## 3.1 README problems

Current README strengths:
- strong visual identity
- clear ambition
- decent architecture overview
- useful commands/examples

Current README problems:
- claims are too broad / too absolute for a public landing page
  - e.g. "Drop-in replacement for OpenClaw"
  - e.g. "full Azure compatibility"
- blends product positioning, runtime behavior, operator details, and contributor workflows into one long surface
- public docs links still reference docs being retired (`PLAN.md` etc.)
- puts too much implementation detail into the landing page instead of pushing that into deeper docs

## 3.2 Planning/strategy problems

Current planning surfaces include:
- `ROADMAP.md`
- `rune-plan.md`
- `docs/IMPLEMENTATION-PHASES.md`
- `docs/STANDALONE-STRATEGY.md`
- legacy planning files still present during transition (`docs/PLAN.md`, `docs/STACK.md`, `docs/WORKPLAN.md`)

Problems:
- overlap between product roadmap and implementation-phase plans
- no simple table saying which file is authoritative for what
- strategy docs and execution docs are mixed together

## 3.3 docs/ structure problems

Current docs set includes:
- parity docs
- specs/phases docs
- operator policy/deployment docs
- competitive research
- architecture-like crate/subsystem docs
- standalone strategy

Problems:
- no clear docs front door (`docs/INDEX.md` missing)
- file set is understandable for an insider, but not clean for a new contributor/operator
- no audience-based grouping
- no documented hierarchy between long-form rationale and canonical operational docs

---

## 4. Proposed source-of-truth model

| Concern | Canonical source | Purpose |
|---|---|---|
| Public product entry | `README.md` | Honest landing page, quick understanding, quick start |
| Docs front door | `docs/INDEX.md` | Navigation entrypoint by audience/use-case |
| Product strategy | `rune-plan.md` | Medium/long-term product direction, target-state decisions |
| Parity/execution phase spec | `docs/IMPLEMENTATION-PHASES.md` | Rewrite/parity execution framework |
| Live execution queue | GitHub Project 2 | What is active now (Epic/Feature/Story) |
| Architecture decisions | `docs/adr/` | Durable design decisions with rationale |
| Long-form design rationale | `docs/strategy/` | Deep rationale/reference docs like standalone strategy |
| Operator runtime docs | `docs/operator/` | Deployment, health, configuration, runtime use |
| Contributor/reference docs | `docs/reference/` + `docs/contributor/` | Technical reference and contribution guidance |

---

## 5. Proposed documentation information architecture

## 5.1 Target top-level docs structure

```text
docs/
  INDEX.md

  getting-started/
    QUICKSTART.md
    INSTALL.md

  operator/
    CONFIGURATION.md
    PROVIDERS.md
    CHANNELS.md
    DEPLOYMENT.md
    HEALTH-AND-DOCTOR.md
    MEMORY.md

  contributor/
    CONTRIBUTING.md
    DEVELOPMENT.md
    TESTING.md
    WORKFLOW.md

  reference/
    ARCHITECTURE.md
    CRATE-LAYOUT.md
    SUBSYSTEMS.md
    API.md
    CLI.md

  parity/
    PARITY-INVENTORY.md
    PARITY-SPEC.md
    PARITY-CONTRACTS.md
    PROTOCOLS.md

  strategy/
    STANDALONE-STRATEGY.md
    strategy/COMPETITIVE-RESEARCH.md
    strategy/AZURE-DATA-OPTIONS.md

  adr/
    ADR-0001-execution-workflow-and-speed.md
    ADR-0002-standalone-first-product-shape.md
    ADR-0003-source-of-truth-model.md
    ADR-0004-project-2-execution-model.md

  specs/
    phases-01-07.md
    phases-08-14.md
    phases-15-21.md
    phases-22-24.md
    phases-25-27.md
```

## 5.2 Audience split

### Getting Started
For someone trying to install or try Rune quickly.

### Operator
For someone running Rune and configuring/deploying it.

### Contributor
For someone building/changing Rune.

### Reference
For stable technical facts.

### Parity
For the rewrite/parity framework and protocol alignment.

### Strategy
For deeper rationale and positioning context.

### ADR
For durable decisions that should not be buried in chat or issue comments.

---

## 6. README rewrite proposal

## 6.1 README objective

The README should answer, quickly:
1. What is Rune?
2. Why should I care?
3. How do I try it?
4. What does it do today?
5. Where do I go next?

## 6.2 Proposed README section order

1. Hero / logo
2. One-line description
3. Current status note
4. Why Rune
5. Quick Start
6. What Rune does
7. Core features (honest current-state list)
8. Standalone vs server runtime modes
9. Documentation links
10. For contributors
11. License / project ownership

## 6.3 Proposed README positioning language direction

### Replace
- "Drop-in replacement for OpenClaw"
- "full Azure compatibility"
- anything that sounds parity-complete or production-complete if it is not fully verified

### Prefer
- "Rust-based personal AI runtime"
- "designed as a high-performance OpenClaw-style runtime"
- "standalone-first with optional server-grade deployment paths"
- "strong Azure-oriented provider support"
- "active parity-seeking rewrite / runtime buildout"

## 6.4 README current-status block recommendation

Add a short explicit status block near the top, for example:

- standalone/server runtime modes are implemented
- dual-backend storage is in progress / available
- memory/browser/A2UI/quality lanes are actively being shipped
- GitHub Project 2 is the execution source of truth

This keeps the landing page honest without sounding weak.

## 6.5 README content that should move out

Move or reduce:
- excessive crate table detail
- long dev/service instructions
- detailed operator commands beyond a short sample
- public references to retired docs

Those belong in docs pages.

---

## 7. Planning canonicalization proposal

## 7.1 Recommended roles

### `rune-plan.md`
Use for:
- product direction
- stack/product decisions
- target-state roadmap
- high-level phases and goals

Do **not** use for:
- day-to-day task tracking
- fine-grained execution state

### `docs/IMPLEMENTATION-PHASES.md`
Use for:
- parity rewrite sequencing
- execution philosophy for parity-critical layers
- acceptance criteria per implementation phase

Do **not** use for:
- product marketing or release planning

### `ROADMAP.md`
Use for:
- historical roadmap and planning context retained for provenance
- older long-form planning detail that has not yet been folded elsewhere

Do **not** use for:
- live execution control
- current branch/PR workflow authority
- canonical product strategy

Current direction: keep it as a legacy/historical planning artifact unless and until a tighter fold/retire pass is executed.

### GitHub Project 2
Use for:
- active Epics / Features / Stories
- status flow
- linked PRs
- live execution order

### ADRs
Use for:
- durable architectural decisions that should outlive issues/PR chatter

---

## 8. Migration matrix

| Current file | Proposed disposition | New canonical home / note |
|---|---|---|
| `README.md` | Rewrite | stay at repo root |
| `rune-plan.md` | Keep | canonical product strategy |
| `ROADMAP.md` | Keep as historical planning artifact for now | legacy roadmap/provenance; not current execution authority |
| `docs/IMPLEMENTATION-PHASES.md` | Keep | canonical parity execution spec |
| `docs/STANDALONE-STRATEGY.md` | Keep but re-home conceptually | `docs/strategy/STANDALONE-STRATEGY.md` |
| `docs/strategy/COMPETITIVE-RESEARCH.md` | Keep | `docs/strategy/` |
| `docs/reference/CRATE-LAYOUT.md` | Canonical reference doc | `docs/reference/` |
| `docs/reference/SUBSYSTEMS.md` | Canonical reference doc | `docs/reference/` |
| `docs/AZURE-COMPATIBILITY.md` | Keep | likely `docs/operator/` or `docs/reference/` depending scope |
| `docs/operator/DEPLOYMENT.md` | Canonical operator deployment doc | `docs/operator/` |
| `docs/PARITY-*` | Keep | `docs/parity/` |
| `docs/PROTOCOLS.md` | Keep | `docs/parity/PROTOCOLS.md` |
| `docs/WORKTREE-EXECUTION.md` | Verify | likely `docs/contributor/WORKFLOW.md` if still relevant |
| `docs/AGENT-ORCHESTRATION.md` | Verify | `docs/contributor/` or `docs/reference/` |
| `docs/PLAN.md` | Retire/fold | old duplicate planning doc |
| `docs/STACK.md` | Retire/fold | old duplicate planning doc |
| `docs/WORKPLAN.md` | Retire/fold | old duplicate planning doc |
| `notes/` | Verify | likely private/internal, not canonical public docs |

---

## 9. Proposed execution plan for this lane

This lane should be verification-gated.

### Phase A — planning only
- produce this review doc
- create Project 2 Epic / Features / Stories for docs lane
- no destructive moves yet

### Phase B — structure approval
- Hamza verifies IA + source-of-truth model
- resolve any remaining cleanup around `README.md` and strategy/reference placement after the roadmap authority decision already shipped

### Phase C — non-destructive scaffolding PR
- add `docs/INDEX.md`
- add target folder structure placeholders
- add ADR folder scaffold
- no major content deletion yet

### Phase D — README rewrite PR
- rewrite README according to approved spec
- update docs links
- keep wording honest

### Phase E — planning canonicalization PR
- fold/retire duplicate docs
- move/rename docs into approved structure
- update links and references

### Phase F — polish PR(s)
- contributor/reference/operator cleanup
- remaining redirects, link fixes, consistency passes

**Constraint:** one active PR at a time.

---

## 10. Proposed Project 2 structure for this lane

## Epic
- **Epic: Docs, Positioning & Information Architecture**

## Features
1. **Feature: README rewrite**
2. **Feature: Docs structure cleanup**
3. **Feature: Planning canonicalization**
4. **Feature: Architecture decision trail**

## Stories
### Under README rewrite
- Story: define honest public positioning language
- Story: rewrite README section structure
- Story: replace stale/retired doc links

### Under Docs structure cleanup
- Story: create docs front door (`docs/INDEX.md`)
- Story: define audience-based folder layout
- Story: move canonical docs into approved categories

### Under Planning canonicalization
- Story: define file roles and source-of-truth matrix
- Story: retire/fold duplicate planning docs
- Story: align Project 2 with docs references

### Under Architecture decision trail
- Story: add ADR scaffold
- Story: record execution workflow ADR
- Story: record standalone-first product-shape ADR
- Story: record source-of-truth model ADR
- Story: record Project 2 execution-model ADR

---

## 11. Verification gates

No major doc rewrite/reorg should happen without explicit verification of:

### Gate 1 — information architecture
Approve or change the target docs structure.

### Gate 2 — README/product positioning
Approve or change the public-facing language posture.

### Gate 3 — source-of-truth table
Approve what file owns what concern.

### Gate 4 — migration matrix
Approve what gets folded/retired/kept.

### Gate 5 — execution sequence
Approve PR order and one-active-PR sequencing.

---

## 12. Decisions/questions for Hamza

1. Should `rune-plan.md` remain at repo root, or move into `docs/strategy/` with a root pointer?
2. How aggressive should the README be about OpenClaw comparison?
3. Should `notes/` be treated as internal/private and excluded from canonical docs?
4. Do you want ADRs public in-repo from the start, or only after initial curation?
5. Should Azure-specific docs live under Operator docs or Reference docs?
6. Do you want release-style milestones renamed from `M4/M5/M6/M7` to versioned names later?

---

## 13. Recommended default answers

If no contrary preference is given, I recommend:

- keep `rune-plan.md` at repo root for now
- keep `ROADMAP.md` only as historical planning/provenance unless a later cleanup folds it away
- soften README replacement language until parity is fully verified
- add `docs/INDEX.md` as the canonical docs front door
- treat Project 2 as the live execution truth
- add ADRs in-repo early
- keep strategy docs separate from operator/reference docs

---

## 14. Immediate next step after verification

If Hamza verifies this proposal, the next implementation action should be a **non-destructive scaffolding PR**:

- add `docs/INDEX.md`
- create the target directory skeleton
- add ADR directory scaffold
- document source-of-truth roles
- do **not** yet delete/fold major docs without the next explicit pass
