-- Sessions: top-level conversation containers.
CREATE TABLE sessions (
    id              UUID PRIMARY KEY,
    kind            TEXT NOT NULL,
    status          TEXT NOT NULL,
    workspace_root  TEXT,
    channel_ref     TEXT,
    requester_session_id UUID REFERENCES sessions(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL,
    last_activity_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_sessions_status ON sessions (status);
CREATE INDEX idx_sessions_created_at ON sessions (created_at DESC);

-- Turns: individual request/response cycles within a session.
CREATE TABLE turns (
    id                      UUID PRIMARY KEY,
    session_id              UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    trigger_kind            TEXT NOT NULL,
    status                  TEXT NOT NULL,
    model_ref               TEXT,
    started_at              TIMESTAMPTZ NOT NULL,
    ended_at                TIMESTAMPTZ,
    usage_prompt_tokens     INTEGER,
    usage_completion_tokens INTEGER
);

CREATE INDEX idx_turns_session_id ON turns (session_id);
CREATE INDEX idx_turns_started_at ON turns (started_at);

-- Transcript items: ordered conversation history entries.
CREATE TABLE transcript_items (
    id          UUID PRIMARY KEY,
    session_id  UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id     UUID,
    seq         INTEGER NOT NULL,
    kind        TEXT NOT NULL,
    payload     JSONB NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_transcript_items_session_seq ON transcript_items (session_id, seq);

-- Jobs: scheduled work and cron definitions.
CREATE TABLE jobs (
    id          UUID PRIMARY KEY,
    job_type    TEXT NOT NULL,
    schedule    TEXT,
    due_at      TIMESTAMPTZ,
    enabled     BOOLEAN NOT NULL DEFAULT true,
    last_run_at TIMESTAMPTZ,
    next_run_at TIMESTAMPTZ,
    payload     JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_jobs_enabled ON jobs (enabled) WHERE enabled = true;

-- Approvals: human-in-the-loop approval gates.
CREATE TABLE approvals (
    id                UUID PRIMARY KEY,
    subject_type      TEXT NOT NULL,
    subject_id        UUID NOT NULL,
    reason            TEXT NOT NULL,
    decision          TEXT,
    decided_by        TEXT,
    decided_at        TIMESTAMPTZ,
    presented_payload JSONB NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL,
    handle_ref        TEXT,
    host_ref          TEXT
);

CREATE INDEX idx_approvals_subject ON approvals (subject_type, subject_id);

-- Tool executions: audit trail for tool invocations.
CREATE TABLE tool_executions (
    id              UUID PRIMARY KEY,
    tool_call_id    UUID NOT NULL,
    session_id      UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id         UUID NOT NULL,
    tool_name       TEXT NOT NULL,
    arguments       JSONB NOT NULL,
    status          TEXT NOT NULL,
    result_summary  TEXT,
    error_summary   TEXT,
    started_at      TIMESTAMPTZ NOT NULL,
    ended_at        TIMESTAMPTZ
);

CREATE INDEX idx_tool_executions_session_id ON tool_executions (session_id);
CREATE INDEX idx_tool_executions_turn_id ON tool_executions (turn_id);

-- Channel deliveries: outbound message tracking.
CREATE TABLE channel_deliveries (
    id                  UUID PRIMARY KEY,
    channel             TEXT NOT NULL,
    destination         TEXT NOT NULL,
    source_session_id   UUID REFERENCES sessions(id) ON DELETE SET NULL,
    message_kind        TEXT NOT NULL,
    provider_message_id TEXT,
    attempt_count       INTEGER NOT NULL DEFAULT 0,
    status              TEXT NOT NULL,
    sent_at             TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_channel_deliveries_status ON channel_deliveries (status);
