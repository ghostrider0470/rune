-- Process handles: durable background process metadata.
CREATE TABLE process_handles (
    process_id  UUID PRIMARY KEY,
    tool_call_id UUID NOT NULL,
    session_id  UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    command     TEXT NOT NULL,
    cwd         TEXT NOT NULL,
    status      TEXT NOT NULL,
    exit_code   INTEGER,
    started_at  TIMESTAMPTZ NOT NULL,
    ended_at    TIMESTAMPTZ
);

CREATE INDEX idx_process_handles_session_id ON process_handles (session_id);
CREATE INDEX idx_process_handles_status ON process_handles (status);

-- Add approval linkage and execution mode to tool_executions.
ALTER TABLE tool_executions ADD COLUMN approval_id UUID;
ALTER TABLE tool_executions ADD COLUMN execution_mode TEXT;
