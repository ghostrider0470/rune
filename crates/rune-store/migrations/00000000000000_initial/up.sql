CREATE TABLE sessions (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    workspace_root TEXT,
    channel_ref TEXT,
    requester_session_id UUID,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    last_activity_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE turns (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    trigger_kind TEXT NOT NULL,
    status TEXT NOT NULL,
    model_ref TEXT,
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    usage_prompt_tokens INTEGER,
    usage_completion_tokens INTEGER
);

CREATE TABLE transcript_items (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id UUID REFERENCES turns(id) ON DELETE SET NULL,
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    UNIQUE (session_id, seq)
);

CREATE TABLE jobs (
    id UUID PRIMARY KEY,
    job_type TEXT NOT NULL,
    schedule TEXT,
    due_at TIMESTAMPTZ,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_run_at TIMESTAMPTZ,
    next_run_at TIMESTAMPTZ,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE approvals (
    id UUID PRIMARY KEY,
    subject_type TEXT NOT NULL,
    subject_id UUID NOT NULL,
    reason TEXT NOT NULL,
    decision TEXT,
    decided_by TEXT,
    decided_at TIMESTAMPTZ,
    presented_payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE tool_executions (
    id UUID PRIMARY KEY,
    tool_call_id UUID NOT NULL,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id UUID NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    arguments JSONB NOT NULL,
    status TEXT NOT NULL,
    result_summary TEXT,
    error_summary TEXT,
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ
);

CREATE INDEX idx_sessions_updated_at ON sessions (updated_at DESC);
CREATE INDEX idx_turns_session_started_at ON turns (session_id, started_at ASC);
CREATE INDEX idx_transcript_items_session_seq ON transcript_items (session_id, seq ASC);
CREATE INDEX idx_jobs_enabled_next_run_at ON jobs (enabled, next_run_at);
CREATE INDEX idx_tool_executions_session_turn ON tool_executions (session_id, turn_id);
