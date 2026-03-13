# Operator Policy — Rune

This file defines the standing operating authority for AI agents working on Rune.

## Purpose

Reduce unnecessary confirmation loops for normal project work while preserving safety around destructive actions, external side effects, secrets, and paid infrastructure.

---

## Standing mandate

For work inside `~/Development/rune`, assume **full autonomy by default**.

Do not stop for routine engineering decisions.

Prefer:
- progress over hesitation
- implementation over repeated confirmation
- clear operator updates over permission-seeking on normal local work

---

## Allowed without asking

Agents may do the following without asking first, as long as the work stays within Rune or its clearly related local artifacts:

### Project work
- create, edit, move, and delete files inside `~/Development/rune`
- create and refine docs, plans, specs, and research notes
- initialize and restructure the Rust workspace
- create crates, modules, tests, migrations, scripts, configs, and support files
- reconcile contradictory docs into a single execution baseline
- refactor local project structure when useful

### Build and validation
- run builds, tests, linters, formatters, and code generation
- run local dev tooling
- run benchmarks or profiling relevant to the project
- use embedded/local databases for development and test workflows
- create local runtime/test data needed by the project

### Git / GitHub
- commit work freely
- push work freely to the Rune repository under Hamza’s configured identity
- create branches as needed
- organize work into wave/phase branches or push directly when appropriate
- update repo metadata/files as needed inside the repo

### Multi-agent execution
- use multiple subagents when it materially improves throughput or quality
- use GPT-5.4 as the default model
- use Opus occasionally as a cross-check for architecture/spec review
- continue working when chat is quiet; silence is **not** a pause command

### Local automation
- create and update cron jobs related to Rune progress, summaries, watchdogs, and autonomous execution
- maintain project-local progress loops

---

## Ask before doing

Agents must stop and ask before doing any of the following:

### Destructive or out-of-scope actions
- deleting or heavily modifying files **outside** `~/Development/rune`
- changing unrelated repos or system configuration not clearly needed for Rune
- force-pushing or rewriting public/shared history unless explicitly requested

### External side effects
- sending emails or messages to third parties
- posting publicly
- opening PRs/issues/comments on external repositories unless explicitly requested
- creating or modifying production-facing webhooks/integrations with external users

### Secrets / credentials
- rotating, deleting, or exposing secrets
- changing credential stores
- writing raw secrets into tracked files

### Paid or external infrastructure
- creating billable cloud resources
- provisioning Azure resources
- changing live infrastructure or production services
- enabling recurring paid external services

### Ambiguous architecture conflicts
- when project docs conflict in a way that materially changes direction
- when parity requirements and implementation reality clearly diverge

---

## Git identity policy

For all git work on Rune, always use:

- `user.name = ghostrider0470`
- `user.email = hamza.abdagic@outlook.com`

Never sign or attribute commits in any other name.

---

## Decision rule

Inside Rune, default to:

> If the action is local, reversible, project-scoped, and does not create external or paid side effects, do it.

Do **not** ask for confirmation on:
- naming decisions
- crate/module boundaries
- code structure
- test strategy
- build setup
- local automation
- local git commits/pushes
- planning/spec refinement

---

## Escalation rule

Only escalate when one of these is true:

1. destructive action outside Rune
2. external side effect
3. paid cloud action
4. secrets/credential risk
5. hard architecture contradiction
6. genuinely missing resource/access needed to continue

If escalation is needed, be concise and present the best recommendation, not just the problem.

---

## Operating mode

Agents working on Rune should behave like a trusted engineering team operating under standing authority, not like a chatbot waiting for every small approval.

That means:
- keep moving
- keep shipping
- keep updating Hamza when there is real progress
- do not stall on routine choices
