//! Hand-rolled SQLite migration runner.
//!
//! Uses a `_rune_migrations` table to track applied versions. Each migration
//! is a `(version, name, sql)` tuple executed in order.

use rusqlite::Connection;

use crate::error::StoreError;

/// A single migration entry.
struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

/// All SQLite schema migrations, applied in order.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: r#"
-- Sessions
CREATE TABLE IF NOT EXISTS sessions (
    id                   TEXT PRIMARY KEY,
    kind                 TEXT NOT NULL,
    status               TEXT NOT NULL,
    workspace_root       TEXT,
    channel_ref          TEXT,
    requester_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    metadata             TEXT NOT NULL DEFAULT '{}',
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    last_activity_at     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions (status);
CREATE INDEX IF NOT EXISTS idx_sessions_created_at ON sessions (created_at DESC);

-- Turns
CREATE TABLE IF NOT EXISTS turns (
    id                      TEXT PRIMARY KEY,
    session_id              TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    trigger_kind            TEXT NOT NULL,
    status                  TEXT NOT NULL,
    model_ref               TEXT,
    started_at              TEXT NOT NULL,
    ended_at                TEXT,
    usage_prompt_tokens     INTEGER,
    usage_completion_tokens INTEGER
);
CREATE INDEX IF NOT EXISTS idx_turns_session_id ON turns (session_id);
CREATE INDEX IF NOT EXISTS idx_turns_started_at ON turns (started_at);

-- Transcript items
CREATE TABLE IF NOT EXISTS transcript_items (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id     TEXT,
    seq         INTEGER NOT NULL,
    kind        TEXT NOT NULL,
    payload     TEXT NOT NULL,
    created_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_transcript_items_session_seq ON transcript_items (session_id, seq);

-- Jobs
CREATE TABLE IF NOT EXISTS jobs (
    id          TEXT PRIMARY KEY,
    job_type    TEXT NOT NULL,
    schedule    TEXT,
    due_at      TEXT,
    enabled     INTEGER NOT NULL DEFAULT 1,
    last_run_at TEXT,
    next_run_at TEXT,
    payload     TEXT NOT NULL DEFAULT '{}',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_jobs_enabled ON jobs (enabled);

-- Approvals
CREATE TABLE IF NOT EXISTS approvals (
    id                TEXT PRIMARY KEY,
    subject_type      TEXT NOT NULL,
    subject_id        TEXT NOT NULL,
    reason            TEXT NOT NULL,
    decision          TEXT,
    decided_by        TEXT,
    decided_at        TEXT,
    presented_payload TEXT NOT NULL,
    created_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_approvals_subject ON approvals (subject_type, subject_id);

-- Tool executions
CREATE TABLE IF NOT EXISTS tool_executions (
    id              TEXT PRIMARY KEY,
    tool_call_id    TEXT NOT NULL,
    session_id      TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id         TEXT NOT NULL,
    tool_name       TEXT NOT NULL,
    arguments       TEXT NOT NULL,
    status          TEXT NOT NULL,
    result_summary  TEXT,
    error_summary   TEXT,
    started_at      TEXT NOT NULL,
    ended_at        TEXT
);
CREATE INDEX IF NOT EXISTS idx_tool_executions_session_id ON tool_executions (session_id);
CREATE INDEX IF NOT EXISTS idx_tool_executions_turn_id ON tool_executions (turn_id);

-- Channel deliveries
CREATE TABLE IF NOT EXISTS channel_deliveries (
    id                  TEXT PRIMARY KEY,
    channel             TEXT NOT NULL,
    destination         TEXT NOT NULL,
    source_session_id   TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    message_kind        TEXT NOT NULL,
    provider_message_id TEXT,
    attempt_count       INTEGER NOT NULL DEFAULT 0,
    status              TEXT NOT NULL,
    sent_at             TEXT,
    created_at          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_channel_deliveries_status ON channel_deliveries (status);

-- Job runs
CREATE TABLE IF NOT EXISTS job_runs (
    id          TEXT PRIMARY KEY,
    job_id      TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    started_at  TEXT NOT NULL,
    finished_at TEXT,
    status      TEXT NOT NULL,
    output      TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_job_runs_job_started_at ON job_runs (job_id, started_at DESC);

-- Paired devices
CREATE TABLE IF NOT EXISTS paired_devices (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    public_key        TEXT NOT NULL UNIQUE,
    role              TEXT NOT NULL DEFAULT 'operator',
    scopes            TEXT NOT NULL DEFAULT '[]',
    token_hash        TEXT NOT NULL UNIQUE,
    token_expires_at  TEXT NOT NULL,
    paired_at         TEXT NOT NULL,
    last_seen_at      TEXT,
    created_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_paired_devices_token_hash ON paired_devices (token_hash);
CREATE INDEX IF NOT EXISTS idx_paired_devices_public_key ON paired_devices (public_key);

-- Pairing requests
CREATE TABLE IF NOT EXISTS pairing_requests (
    id            TEXT PRIMARY KEY,
    device_name   TEXT NOT NULL,
    public_key    TEXT NOT NULL,
    challenge     TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    expires_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_pairing_requests_expires ON pairing_requests (expires_at);

-- Memory embeddings
CREATE TABLE IF NOT EXISTS memory_embeddings (
    id          TEXT PRIMARY KEY,
    file_path   TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    chunk_text  TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE (file_path, chunk_index)
);
CREATE INDEX IF NOT EXISTS idx_memory_embeddings_file_path ON memory_embeddings (file_path);

-- FTS5 virtual table for keyword search
CREATE VIRTUAL TABLE IF NOT EXISTS memory_embeddings_fts
USING fts5(chunk_text, content='memory_embeddings', content_rowid='rowid',
           tokenize='porter unicode61');

-- Triggers to keep FTS index in sync
CREATE TRIGGER IF NOT EXISTS memory_embeddings_ai AFTER INSERT ON memory_embeddings BEGIN
    INSERT INTO memory_embeddings_fts(rowid, chunk_text) VALUES (new.rowid, new.chunk_text);
END;

CREATE TRIGGER IF NOT EXISTS memory_embeddings_ad AFTER DELETE ON memory_embeddings BEGIN
    INSERT INTO memory_embeddings_fts(memory_embeddings_fts, rowid, chunk_text) VALUES('delete', old.rowid, old.chunk_text);
END;

CREATE TRIGGER IF NOT EXISTS memory_embeddings_au AFTER UPDATE ON memory_embeddings BEGIN
    INSERT INTO memory_embeddings_fts(memory_embeddings_fts, rowid, chunk_text) VALUES('delete', old.rowid, old.chunk_text);
    INSERT INTO memory_embeddings_fts(rowid, chunk_text) VALUES (new.rowid, new.chunk_text);
END;
"#,
    },
    Migration {
        version: 2,
        name: "add_process_handles",
        sql: r#"
-- Process handles: durable background process metadata.
CREATE TABLE IF NOT EXISTS process_handles (
    process_id   TEXT PRIMARY KEY,
    tool_call_id TEXT NOT NULL,
    session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    command      TEXT NOT NULL,
    cwd          TEXT NOT NULL,
    status       TEXT NOT NULL,
    exit_code    INTEGER,
    started_at   TEXT NOT NULL,
    ended_at     TEXT
);
CREATE INDEX IF NOT EXISTS idx_process_handles_session_id ON process_handles (session_id);
CREATE INDEX IF NOT EXISTS idx_process_handles_status ON process_handles (status);

-- Add latest_turn_id to sessions for quick access to the most recent turn.
ALTER TABLE sessions ADD COLUMN latest_turn_id TEXT;

-- Add approval linkage and execution mode to tool_executions.
ALTER TABLE tool_executions ADD COLUMN approval_id TEXT;
ALTER TABLE tool_executions ADD COLUMN execution_mode TEXT;
"#,
    },
    Migration {
        version: 3,
        name: "add_scheduler_semantics",
        sql: r#"
ALTER TABLE jobs ADD COLUMN payload_kind TEXT NOT NULL DEFAULT 'system_event';
ALTER TABLE jobs ADD COLUMN delivery_mode TEXT NOT NULL DEFAULT 'none';
ALTER TABLE job_runs ADD COLUMN trigger_kind TEXT NOT NULL DEFAULT 'due';

UPDATE jobs
SET payload_kind = CASE
    WHEN job_type = 'reminder' THEN 'reminder'
    WHEN json_extract(payload, '$.payload.kind') IS NOT NULL THEN json_extract(payload, '$.payload.kind')
    WHEN json_extract(payload, '$.kind') IS NOT NULL THEN json_extract(payload, '$.kind')
    ELSE payload_kind
END;

UPDATE jobs
SET delivery_mode = CASE
    WHEN job_type = 'reminder' THEN 'announce'
    ELSE delivery_mode
END;
"#,
    },
    Migration {
        version: 4,
        name: "add_durable_claims",
        sql: r#"
-- Durable claim/lease column: set when a supervisor claims a due job for
-- execution.  NULL means unclaimed.  A stale claimed_at (older than the
-- lease duration) is treated as expired so another supervisor may reclaim.
ALTER TABLE jobs ADD COLUMN claimed_at TEXT;
CREATE INDEX IF NOT EXISTS idx_jobs_due_unclaimed
    ON jobs (enabled, next_run_at)
    WHERE claimed_at IS NULL;
"#,
    },
    Migration {
        version: 5,
        name: "approval_durability",
        sql: r#"
-- Add handle_ref and host_ref columns to approvals for cross-restart durability.
ALTER TABLE approvals ADD COLUMN handle_ref TEXT;
ALTER TABLE approvals ADD COLUMN host_ref TEXT;
"#,
    },
    Migration {
        version: 6,
        name: "process_handle_audit_linkage",
        sql: r#"
-- Add execution-mode and tool-execution linkage to process handles.
ALTER TABLE process_handles ADD COLUMN execution_mode TEXT;
ALTER TABLE process_handles ADD COLUMN tool_execution_id TEXT;
"#,
    },
    Migration {
        version: 7,
        name: "session_profile_fields",
        sql: r#"
-- Add runtime_profile and policy_profile to sessions per PROTOCOLS.md 4.1.
ALTER TABLE sessions ADD COLUMN runtime_profile TEXT;
ALTER TABLE sessions ADD COLUMN policy_profile TEXT;
"#,
    },
];

/// Run all pending migrations on the given connection.
pub fn run_migrations(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _rune_migrations (
            version  INTEGER PRIMARY KEY,
            name     TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .map_err(|e| StoreError::Migration(format!("failed to create migrations table: {e}")))?;

    for m in MIGRATIONS {
        let applied: bool = conn
            .prepare("SELECT 1 FROM _rune_migrations WHERE version = ?1")
            .and_then(|mut stmt| stmt.exists([m.version]))
            .map_err(|e| {
                StoreError::Migration(format!("failed to check migration {}: {e}", m.version))
            })?;

        if !applied {
            conn.execute_batch(m.sql).map_err(|e| {
                StoreError::Migration(format!("migration {} ({}) failed: {e}", m.version, m.name))
            })?;

            conn.execute(
                "INSERT INTO _rune_migrations (version, name) VALUES (?1, ?2)",
                rusqlite::params![m.version, m.name],
            )
            .map_err(|e| {
                StoreError::Migration(format!("failed to record migration {}: {e}", m.version))
            })?;
        }
    }

    Ok(())
}

/// Apply connection pragmas for optimal performance.
pub fn apply_pragmas(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )
    .map_err(|e| StoreError::Database(format!("failed to set SQLite pragmas: {e}")))
}
