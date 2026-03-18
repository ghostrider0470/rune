# ADR-0003: Source-of-Truth Model

- **Status:** Accepted
- **Date:** 2026-03-18

## Context

Rune documentation and execution tracking had started to overlap in ways that created avoidable ambiguity.

The same repo contained:
- public-facing entry docs
- long-form strategy and rationale
- parity-phase sequencing docs
- issue/PR execution trails
- legacy planning files retained for provenance

Without explicit source-of-truth boundaries, the project risks:
- conflicting guidance across docs
- planning files drifting into live execution tracking
- issue comments being treated like durable architecture records
- repeated re-decisions about where to find the current truth for a given concern

The docs reorganization proposal already defines the intended direction: one canonical home per concern, with GitHub Project 2 owning live execution state.

## Decision

Rune will use an explicit source-of-truth model where each concern has one canonical home.

### 1. Public product entry lives in `README.md`
Use `README.md` for:
- honest project positioning
- quick understanding
- quick-start orientation
- links into deeper docs

Do not use it for:
- detailed phase acceptance criteria
- fine-grained execution tracking
- deep implementation reference

### 2. Product strategy lives in `rune-plan.md`
Use `rune-plan.md` for:
- product direction
- confirmed architecture and stack choices
- high-level delivery shape
- planning boundaries

Do not use it for:
- live batch status
- issue-level execution updates
- detailed parity acceptance evidence

### 3. Parity phase definition lives in `docs/IMPLEMENTATION-PHASES.md`
Use `docs/IMPLEMENTATION-PHASES.md` for:
- parity rewrite sequencing
- phase-by-phase acceptance criteria
- execution philosophy for parity-critical work

Do not use it for:
- marketing language
- daily execution queue management
- durable architecture rationale that should live in ADRs

### 4. Live execution state lives in GitHub Project 2
Use GitHub Project 2, linked issues, and PRs for:
- active epics, features, and stories
- current execution order
- status flow
- linked implementation artifacts
- batch-level movement and review state

Do not use repo docs as a substitute for the live board.

### 5. Durable architectural decisions live in `docs/adr/`
Use ADRs for:
- decisions that should outlive chat, issues, and PR discussion
- rationale for important workflow, product-shape, and architecture choices
- stable reference points for future changes

### 6. Transitional planning files are provenance, not active control surfaces
Legacy planning files kept during cleanup may remain in the repo for history, but they are not equal peers with the canonical surfaces above unless explicitly re-designated.

## Consequences

### Positive
- lower ambiguity about where to look for current truth
- less duplicate planning language across files
- cleaner separation between strategy, execution, and decisions
- easier docs maintenance as the repo grows

### Negative / tradeoffs
- contributors must maintain discipline about where updates belong
- some older docs will remain present but intentionally secondary until cleanup is complete
- cross-linking matters more because information is intentionally separated

## Alternatives considered

### Let multiple planning files coexist informally
Rejected because it guarantees drift and repeated confusion.

### Put live execution state inside repo docs
Rejected because it goes stale faster than Project 2 and PR/issue trails.

### Keep durable decisions only in issues/PRs
Rejected because important rationale becomes hard to discover and easy to lose.

## Follow-up

This ADR should be reflected in:
- `rune-plan.md`
- `docs/INDEX.md`
- `docs/OPENCLAW-COVERAGE-MAP.md`
- legacy planning-file disclaimers
- future docs and issue workflow guidance
