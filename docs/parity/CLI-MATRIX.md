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
| `secrets` | — | **Not started** | `reload`, `audit`, `configure`, `apply` — no Rune surface yet | #67 |
| `security` | — | **Not started** | `audit` — no Rune surface yet | #64 |
| `system` | `rune system` | **Shipped** | `event inject`, `event schedule`, `event list`, `heartbeat presence/last/enable/disable/status` | #43 |
| `sandbox` | — | **Not started** | `list`, `recreate`, `explain` — no Rune surface yet | #64 |
| `logs` | `rune logs` + `rune gateway logs` | **Partial** | query/list surface with `--level`, `--source`, `--limit`, `--since` shipped against current gateway stub. Follow/tail and richer filtering still missing. | #40 |
| `dashboard` | `rune dashboard` | **Shipped** | Compact operator summary | #65 |
| `completion` | `rune completion` | **Shipped** | `generate` for bash/zsh/fish/elvish/powershell | #74 (PR #143) |

### Tier 0 summary

- **Shipped:** 13 / 20 families
- **Partial:** 3 (`channels` — missing mutation verbs; `config` — interactive configure still missing; `logs` — query surface landed, follow/tail breadth still missing)
- **Not started:** 4 (`configure`, `secrets`, `security`, `sandbox`)

---

## Tier 1 — Must-follow closely

| OpenClaw family | Rune equivalent | Status | Subcommand coverage | Tracking |
|-----------------|----------------|--------|---------------------|----------|
| `message` | `rune message` | **Partial** | `send`, `read`, `edit`, `delete`, `react`, `pin`, `search`, `broadcast`, `thread list/reply`, `voice send/status`, `tag`, `ack`, `list-reactions` shipped | #74 |
| `agent` | — | **Not started** | Direct agent-turn invocation | #70 |
| `agents` | `rune agents` | **Partial** | `list`, `show`, `status`, `tree`, `templates`, `start --template` shipped. `steer`/`kill` remain blocked because no client-facing transport surface exists yet to send those control actions, even though internal lifecycle/session logic already exists. | #63/#70 |
| `acp` | — | **Not started** | ACP bridge/client | #70 |
| `devices` | — | **Not started** | `list`, `remove`, `clear`, `approve`, `reject`, `rotate`, `revoke` | — |
| `pairing` | — | **Not started** | `list`, `approve` | — |
| `node` | — | **Not started** | `run`, `status`, `install`, `uninstall`, `stop`, `restart` | — |
| `nodes` | — | **Not started** | 20+ subcommands (remote exec, camera, screen, canvas, location) | — |
| `skills` | `rune skills` | **Partial** | `list`, `info`, `check`, `enable`, `disable` shipped. Plugins/hooks lifecycle and broader extension management still missing. | #71 |
| `plugins` | — | **Not started** | `list`, `info`, `enable`, `disable`, `install`, `update`, `doctor` | #68 |
| `hooks` | — | **Not started** | `list`, `info`, `check`, `enable`, `disable`, `install`, `update` | #68 |
| `webhooks` | — | **Not started** | `setup`, `run` | — |
| `backup` | — | **Not started** | `create`, `restore`, `list` | #67 |
| `update` | — | **Not started** | `wizard`, `status` | — |
| `setup` | — | **Not started** | Interactive setup wizard | #61 |
| `onboard` | — | **Not started** | First-run onboarding | #61 |
| `uninstall` | — | **Not started** | Clean removal | — |

### Tier 1 summary

- **Shipped:** 0
- **Partial:** 3 (`message` — breadth verbs remain; `agents` — inspect/start flows ship but `steer`/`kill` are still blocked on missing client-facing transport; `skills` — plugins/hooks lifecycle still missing)
- **Not started:** 14

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

- **Shipped:** 0
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
| 0 — Release blockers | 20 | 13 | 3 | 4 | 0 |
| 1 — Must-follow | 17 | 0 | 3 | 14 | 0 |
| 2 — Breadth | 9 | 0 | 0 | 8 | 1 |
| **Total** | **46** | **13** | **6** | **26** | **1** |

**Parity coverage: 13 shipped + 6 partial out of 46 families (28% fully shipped, 41% with partial credit).**

---

## Key gaps by urgency

### Immediate (Tier 0, not started)
1. `configure` — interactive setup wizard (#61)
2. `secrets` — secret management surface (#67)
3. `security` — security audit surface (#64)
4. `sandbox` — sandbox inspection (#64)

### Near-term (Tier 0, partial)
5. `logs` — add follow/tail/richer filtering on top of the shipped query surface (#40)
6. `channels` — add `add`, `remove`, `login`, `logout` verbs (#41)
7. `config` — bridge from shipped local/gateway config surfaces to true interactive configure parity (#40/#61)

### Medium-term (Tier 1, highest value)
8. `agent` / `acp` — agent orchestration CLI breadth beyond current `agents` inspect/start surfaces, including the missing client-facing transport needed for `steer` / `kill` parity (#70)
9. `skills` / `plugins` / `hooks` — complete extension lifecycle beyond the shipped `skills` family core (#71/#68)
10. `backup` — backup/restore workflow (#67)
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
