# CLI Parity Audit Matrix

**Issue:** #74 — CLI surface parity sweep
**Generated:** 2026-03-20
**Source:** PARITY-INVENTORY.md §4.2 (OpenClaw census) cross-referenced against `rune-cli/src/cli.rs` on main at `e60cdba` and follow-on merged surfaces through `3c62e0a`.

---

## How to read this matrix

| Status | Meaning |
|--------|---------|
| **Shipped** | Rune subcommand exists, wired through gateway client, has test coverage |
| **Partial** | Family exists but not all parity-required verbs are implemented |
| **Not started** | No Rune CLI surface exists yet |
| **N/A** | Explicitly not applicable or intentionally deferred with rationale |
| **Covered elsewhere** | Tracked under a different issue |

**Rune name** shows the actual Rune command when it differs from the OpenClaw name.
**Tracking** links to the GitHub issue that owns the work.

---

## Tier 0 — Release blockers

| OpenClaw family | Rune equivalent | Status | Subcommand coverage | Tracking |
|-----------------|----------------|--------|---------------------|----------|
| `gateway` | `rune gateway` | **Shipped** | `status`, `health`, `probe`, `discover`, `call`, `usage-cost`, `start`, `stop`, `restart`, `run` | #39 |
| `daemon` | `rune gateway` (alias) | **Shipped** | Collapsed into `gateway`; same verbs apply | #39 |
| `doctor` | `rune doctor` | **Shipped** | Single top-level command with check families | #40 |
| `health` | `rune health` | **Shipped** | Standalone top-level command | #40 |
| `status` | `rune status` | **Shipped** | Standalone top-level command | #40 |
| `cron` | `rune cron` | **Shipped** | `status`, `list`, `add`, `show`, `edit`, `enable`, `disable`, `rm`, `run`, `runs`, `wake` | #43 |
| `channels` | `rune channels` | **Partial** | `list`, `status`, `capabilities`, `resolve`, `logs` shipped. Missing: `add`, `remove`, `login`, `logout` | #41 |
| `models` | `rune models` | **Shipped** | `list`, `status`, `set`, `set-image`, `aliases`, `auth`, `fallbacks`, `image-fallbacks`, `scan` | #69 |
| `memory` | `rune memory` | **Shipped** | `status`, `search`, `get` | — |
| `approvals` | `rune approvals` | **Shipped** | `list`, `decide`, `policies`, `get`, `set`, `clear` | #42 |
| `sessions` | `rune sessions` | **Shipped** | `list`, `show`, `status` | #38 |
| `config` | `rune config` + `rune gateway config` | **Partial** | local `show`, `file`, `get`, `set`, `unset`, `validate` shipped; live gateway `config show/apply` shipped. Interactive `configure` still missing. | #40/#30 |
| `configure` | — | **Not started** | Interactive setup wizard — no Rune surface yet | #61 |
| `secrets` | `rune secrets` | **Shipped** | `reload`, `audit`, `configure`, `apply` | #67 |
| `security` | `rune security` | **Shipped** | `audit` | #64 |
| `system` | `rune system` | **Shipped** | `event inject`, `event schedule`, `event list`, `heartbeat presence/last/enable/disable/status` | #43 |
| `sandbox` | `rune sandbox` | **Shipped** | `list`, `recreate`, `explain` | #64 |
| `logs` | `rune logs` + `rune gateway logs` | **Partial** | query/list surface with `--level`, `--source`, `--limit`, `--since` shipped against current gateway stub. Follow/tail and richer filtering still missing. | #40 |
| `dashboard` | `rune dashboard` | **Shipped** | Compact operator summary | #65 |
| `completion` | `rune completion` | **Shipped** | `generate` for bash/zsh/fish/elvish/powershell | #74 (PR #143) |

### Tier 0 summary

- **Shipped:** 16 / 20 families
- **Partial:** 3 (`channels` — missing mutation verbs; `config` — interactive configure still missing; `logs` — query surface landed, follow/tail breadth still missing)
- **Not started:** 1 (`configure`)

---

## Tier 1 — Must-follow closely

| OpenClaw family | Rune equivalent | Status | Subcommand coverage | Tracking |
|-----------------|----------------|--------|---------------------|----------|
| `message` | `rune message` | **Partial** | `send`, `read`, `edit`, `delete`, `react`, `pin`, `search`, `broadcast`, `thread list/reply`, `voice send/status`, `tag`, `ack`, `list-reactions` shipped | #74 |
| `agent` | `rune agent` | **Shipped** | `run`, `result` | #70 |
| `agents` | `rune agents` | **Partial** | `list`, `show`, `status`, `tree`, `templates`, `start --template` shipped. `steer`/`kill` remain blocked because no client-facing transport surface exists yet to send those control actions, even though internal lifecycle/session logic already exists. | #63/#70 |
| `acp` | `acp_dispatch` tool | **Partial** | Tool dispatches tasks to Claude Code / Codex CLIs as subprocesses. CLI `rune acp send/inbox/ack` surface not yet wired. | #70 |
| `devices` | — | **Not started** | `list`, `remove`, `clear`, `approve`, `reject`, `rotate`, `revoke` | — |
| `pairing` | — | **Not started** | `list`, `approve` | — |
| `node` | — | **Not started** | `run`, `status`, `install`, `uninstall`, `stop`, `restart` | — |
| `nodes` | — | **Not started** | 20+ subcommands (remote exec, camera, screen, canvas, location) | — |
| `skills` | `rune skills` | **Partial** | `list`, `info`, `check`, `enable`, `disable` shipped. Plugins/hooks lifecycle and broader extension management still missing. | #71 |
| `plugins` | `rune plugins` | **Shipped** | `list`, `info`, `enable`, `disable`, `install`, `uninstall`, `update`, `doctor` | #68 |
| `hooks` | `rune hooks` | **Shipped** | `list`, `info`, `check`, `enable`, `disable`, `install`, `update` | #68 |
| `webhooks` | — | **Not started** | `setup`, `run` | — |
| `backup` | `rune backup` | **Shipped** | `create`, `restore`, `list` | #67 |
| `update` | `rune update` | **Partial** | `check`, `apply`, `status` shipped; `wizard` still missing | — |
| `setup` | — | **Not started** | Interactive setup wizard | #61 |
| `onboard` | — | **Not started** | First-run onboarding | #61 |
| `uninstall` | — | **Not started** | Clean removal | — |

### Tier 1 summary

- **Shipped:** 4
- **Partial:** 4 (`message` — breadth verbs remain; `agents` — inspect/start/spawn/steer/kill surface exists but transport/runtime parity still needs validation; `skills` — plugins/hooks lifecycle still missing; `update` — wizard still missing)
- **Not started:** 9

---

## Tier 2 — Breadth parity

| OpenClaw family | Rune equivalent | Status | Subcommand coverage | Tracking |
|-----------------|----------------|--------|---------------------|----------|
| `browser` | — | **Not started** | 28+ subcommands for browser automation | #14 |
| `tui` | — | **Not started** | Terminal UI | — |
| `qr` | — | **Not started** | QR code generation for mobile pairing | #60 |
| `dns` | — | **Not started** | DNS/discovery management | — |
| `docs` | — | **Not started** | Open documentation | — |
| `clawbot` | — | **N/A** | Legacy/compat — may not be needed in Rune | — |
| `voicecall` | — | **Not started** | Voice/telephony integration | — |
| `reset` | — | **Not started** | Factory reset | — |
| `directory` | — | **Not started** | `self`, `peers`, `groups` | — |

### Tier 2 summary

- **Shipped:** 4
- **N/A:** 1 (`clawbot`)
- **Not started:** 8

---

## Rune-only families (no OpenClaw equivalent)

| Rune family | Purpose | Notes |
|-------------|---------|-------|
| `rune init` | Initialize a new workspace with default files | Rune-specific onboarding helper |
| `rune reminders` | `add`, `list`, `cancel` | Rune exposes reminders as a first-class CLI family separate from cron |

---

## Message family detail (Tier 1 deep-dive)

The `message` family is the most actively developed #74 artifact. Current verb coverage:

| OpenClaw verb | Rune verb | Status | PR |
|---------------|-----------|--------|----|
| `send` | `rune message send` | **Shipped** | #145 |
| `read` | `rune message read` | **Shipped** | #152 |
| `edit` | `rune message edit` | **Shipped** | #153 |
| `delete` | `rune message delete` | **Shipped** | #150 |
| `react` | `rune message react` | **Shipped** | #148 |
| `pin` | `rune message pin` | **Shipped** | #149 |
| `search` | `rune message search` | **Shipped** | #146 |
| `broadcast` | `rune message broadcast` | **Shipped** | #147 |
| `thread` | `rune message thread list/reply` | **Shipped** | #151 |
| `voice` | `rune message voice send/status` | **Shipped** | #155 |
| `tag` | `rune message tag add/remove/list` | **Shipped** | #157 |
| `ack` | `rune message ack` | **Shipped** | #162 |
| `reactions` | `rune message list-reactions` | **Shipped** | #163 |
| `poll` | — | **Not started** | Poll creation/management |
| `channel` | — | **Not started** | Channel-scoped message operations |
| `event` | — | **Not started** | Message event subscriptions |
| `member` / `role` | — | **Not started** | Member/role management via message family |
| `emoji` | — | **Not started** | Custom emoji operations |
| `permissions` | — | **Not started** | Permission management |
| `sticker` | — | **Not started** | Sticker operations |
| `ban` / `kick` / `timeout` | — | **Not started** | Moderation actions |

---

## Overall scorecard

| Tier | Total families | Shipped | Partial | Not started | N/A |
|------|---------------|---------|---------|-------------|-----|
| 0 — Release blockers | 20 | 16 | 3 | 1 | 0 |
| 1 — Must-follow | 17 | 4 | 4 | 9 | 0 |
| 2 — Breadth | 9 | 0 | 0 | 8 | 1 |
| **Total** | **46** | **20** | **7** | **18** | **1** |

**Parity coverage: 20 shipped + 7 partial out of 46 families (43% fully shipped, 59% with partial credit).**

---

## Key gaps by urgency

### Immediate (Tier 0, not started)
1. `configure` — interactive setup wizard (#61)
2. `update` wizard — lifecycle UX parity on top of shipped `check`/`apply`/`status` surface
3. `secrets` / `security` / `sandbox` were previously marked missing in this audit but now ship via first-class CLI families

### Near-term (Tier 0, partial)
5. `logs` — add follow/tail/richer filtering on top of the shipped query surface (#40)
6. `channels` — add `add`, `remove`, `login`, `logout` verbs (#41)
7. `config` — bridge from shipped local/gateway config surfaces to true interactive configure parity (#40/#61)

### Medium-term (Tier 1, highest value)
8. `agent` / `acp` — direct agent-turn CLI now ships via `rune agent`; ACP CLI/gateway route wiring still needs parity work (#70).
9. `skills` / `plugins` / `hooks` — `plugins` and `hooks` CLI families now ship, but broader runtime lifecycle parity still needs backing implementations where applicable (#71/#68)
10. `backup` — CLI family now ships; end-to-end durable backup workflow depth still needs validation (#67)
11. `devices` / `pairing` / `node` / `nodes` — multi-node surface (no issue yet)

---

## Cross-reference: issue #74 acceptance criteria

| Criterion | Status |
|-----------|--------|
| Every OpenClaw CLI family has a Rune decision | **Done** — this matrix |
| Shell completion generation for bash, zsh, fish | **Shipped** — PR #143 |
| Operator workflow families have working equivalents | **Partial** — `sessions`, `approvals`, `system`, substantial `message` surfaces, and `agents` inspect/start flows shipped, plus first gateway-backed `logs`/`doctor` admin surfaces; subagent `steer` / `kill` still lack a client-facing transport and `secrets` is still not started |
| Lifecycle families have working equivalents | **Not started** — `setup`, `update`, `uninstall`, `reset` all missing |
| Audit matrix produced | **Done** — this document |
