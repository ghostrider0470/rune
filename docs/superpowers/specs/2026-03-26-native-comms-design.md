# Native Comms System — Design Spec

**Date:** 2026-03-26
**Goal:** Replace LLM-driven filesystem mailbox polling with native Rust code in Rune's supervisor, enabling reliable bi-directional communication with OpenClaw (Horizon AI).

## Problem

Both Rune and OpenClaw use the `.comms/` filesystem mailbox for inter-agent communication, but neither has code-level integration. Both rely on LLM tool calls to read/write JSON files via cron jobs. Result: 13 messages sent, only 2 produced real action. Acks say "no executable handler implemented." Routing bugs persist.

## Protocol (existing, unchanged)

Path: `/home/hamza/Development/rune.worktrees/.comms/`

```
.comms/
  rune/inbox/          ← Horizon AI writes here for Rune
  horizon-ai/inbox/    ← Rune writes here for Horizon AI
  .archive/            ← processed messages
  .board/              ← shared task artifacts
```

Message format (JSON):
```json
{
  "id": "msg-{uuid}",
  "from": "rune",
  "to": "horizon-ai",
  "type": "task|result|status|ack|question|directive",
  "subject": "short description",
  "body": "detailed content",
  "priority": "p0|p1|p2",
  "refs": { "issue": "#123", "pr": null },
  "created_at": "ISO-8601",
  "expires_at": null
}
```

## Design — Rune Side

### 1. Supervisor Inbox Check

Add a `check_comms_inbox()` step to the supervisor loop, running every tick (10s). Scans `{comms_dir}/rune/inbox/` for `*.json` files.

For each message file:
1. Parse JSON envelope
2. Validate required fields (id, from, to, type, body)
3. Dispatch by type (see below)
4. Move processed file to `.archive/` with timestamp prefix

**Dispatch by message type:**

| Type | Action |
|------|--------|
| `ack` | Archive immediately, no further action |
| `result` | Archive (response to something we asked) |
| `status` | Respond with gateway status (active sessions, cron jobs, tool count, uptime), write to `horizon-ai/inbox/`, archive |
| `directive` | Write ack to `horizon-ai/inbox/`, log the directive, archive. The directive content is available to the LLM through workspace/memory context on future turns. |
| `task` | Create an isolated agent turn with the message body as prompt. Prepend comms context header. Write the agent's response as a `result` message to `horizon-ai/inbox/`. Archive original. |
| `question` | Same as `task` — create agent turn, respond with result. |

**Comms context header prepended to task/question agent turns:**
```
[Inter-Agent Comms] This message is from Horizon AI via the .comms/ mailbox.
Priority: {priority}
Subject: {subject}

{body}

Respond with a clear, actionable answer. Your response will be sent back to Horizon AI as a result message.
```

### 2. Outbound from Cron Results

After any cron job completes with `delivery_mode = "announce"`, write a result message to `horizon-ai/inbox/`:

```json
{
  "id": "msg-cron-{job_name}-{timestamp}",
  "from": "rune",
  "to": "horizon-ai",
  "type": "result",
  "subject": "[cron:{job_name}] {status}",
  "body": "{output truncated to 3000 chars}",
  "priority": "p2",
  "created_at": "ISO-8601"
}
```

This reuses the existing `deliver_result` path in the supervisor — adding a comms write alongside the existing Telegram delivery.

### 3. Comms Send Tool

A new tool `comms_send` available to the agent during any session:

```
Tool: comms_send
Parameters:
  to: string (default: "horizon-ai")
  type: string (task|question|status|result)
  subject: string
  body: string
  priority: string (default: "p1")
```

This lets the LLM proactively send messages to Horizon AI — asking questions, reporting status, or dispatching tasks.

### 4. Config

```toml
[comms]
enabled = true
comms_dir = "/home/hamza/Development/rune.worktrees/.comms"
agent_id = "rune"
peer_id = "horizon-ai"
```

## Files Changed

| File | Change |
|------|--------|
| `crates/rune-config/src/lib.rs` | Add `CommsConfig` struct |
| `crates/rune-runtime/src/comms.rs` | New — comms client (read inbox, write outbox, archive) |
| `crates/rune-tools/src/comms_tool.rs` | New — `comms_send` tool executor |
| `crates/rune-gateway/src/supervisor.rs` | Add `check_comms_inbox()` step + cron result outbound |
| `apps/gateway/src/main.rs` | Wire comms config, pass to supervisor |
| `config.toml` | Add `[comms]` section |

## Out of Scope (This Spec)

- OpenClaw TypeScript comms client (separate spec/plan)
- Shared task board `.board/` integration
- Message expiry enforcement
- HTTP API comms endpoint (future enhancement)

## Success Criteria

1. Messages in `rune/inbox/` are processed within 10 seconds
2. `ack`/`status`/`directive` messages handled at code level (no LLM involved)
3. `task`/`question` messages create isolated agent turns and write results back
4. Cron results auto-written to `horizon-ai/inbox/`
5. `comms_send` tool available during agent sessions
6. Processed messages archived with timestamp prefix
7. `from`/`to` fields always correct (no self-routing bugs)
