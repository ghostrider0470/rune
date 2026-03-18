//! SQLite-backed repository implementations.
//!
//! Uses `tokio-rusqlite` to run synchronous rusqlite operations on a
//! dedicated thread, keeping the async runtime non-blocking.

pub mod migrations;

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::*;
use crate::repos::*;

/// Open (or create) a SQLite database, apply pragmas, and run migrations.
pub async fn open_connection(path: &str) -> Result<Arc<tokio_rusqlite::Connection>, StoreError> {
    let conn = tokio_rusqlite::Connection::open(path)
        .await
        .map_err(|e| StoreError::Database(format!("failed to open SQLite at {path}: {e}")))?;

    conn.call(|conn| {
        migrations::apply_pragmas(conn)?;
        migrations::run_migrations(conn)?;
        Ok::<_, StoreError>(())
    })
    .await
    .map_err(|e| match e {
        tokio_rusqlite::Error::Error(se) => se,
        other => StoreError::Database(other.to_string()),
    })?;

    Ok(Arc::new(conn))
}

/// Open an in-memory SQLite database (for tests).
pub async fn open_memory() -> Result<Arc<tokio_rusqlite::Connection>, StoreError> {
    let conn = tokio_rusqlite::Connection::open(":memory:")
        .await
        .map_err(|e| StoreError::Database(format!("failed to open in-memory SQLite: {e}")))?;

    conn.call(|conn| {
        migrations::apply_pragmas(conn)?;
        migrations::run_migrations(conn)?;
        Ok::<_, StoreError>(())
    })
    .await
    .map_err(|e| match e {
        tokio_rusqlite::Error::Error(se) => se,
        other => StoreError::Database(other.to_string()),
    })?;

    Ok(Arc::new(conn))
}

// ── Error helpers ─────────────────────────────────────────────────────

/// Map tokio_rusqlite::Error to StoreError with entity-specific NotFound.
fn map_err(
    e: tokio_rusqlite::Error<rusqlite::Error>,
    entity: &'static str,
    id: &str,
) -> StoreError {
    match &e {
        tokio_rusqlite::Error::Error(rusqlite::Error::QueryReturnedNoRows) => {
            StoreError::NotFound {
                entity,
                id: id.to_string(),
            }
        }
        _ => StoreError::from(e),
    }
}

// ── DateTime helpers ──────────────────────────────────────────────────

fn to_rfc3339(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn parse_dt(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| s.parse::<DateTime<Utc>>().unwrap_or_default())
}

fn parse_dt_opt(s: Option<String>) -> Option<DateTime<Utc>> {
    s.map(|s| parse_dt(&s))
}

fn parse_uuid(s: &str) -> Uuid {
    s.parse::<Uuid>().unwrap_or_default()
}

fn parse_uuid_opt(s: Option<String>) -> Option<Uuid> {
    s.map(|s| parse_uuid(&s))
}

fn parse_json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_default()
}

// ── Row mapping helpers ──────────────────────────────────────────────

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        id: parse_uuid(&row.get::<_, String>(0)?),
        kind: row.get(1)?,
        status: row.get(2)?,
        workspace_root: row.get(3)?,
        channel_ref: row.get(4)?,
        requester_session_id: parse_uuid_opt(row.get(5)?),
        latest_turn_id: parse_uuid_opt(row.get(6)?),
        metadata: parse_json(&row.get::<_, String>(7)?),
        created_at: parse_dt(&row.get::<_, String>(8)?),
        updated_at: parse_dt(&row.get::<_, String>(9)?),
        last_activity_at: parse_dt(&row.get::<_, String>(10)?),
    })
}

fn row_to_turn(row: &rusqlite::Row<'_>) -> rusqlite::Result<TurnRow> {
    Ok(TurnRow {
        id: parse_uuid(&row.get::<_, String>(0)?),
        session_id: parse_uuid(&row.get::<_, String>(1)?),
        trigger_kind: row.get(2)?,
        status: row.get(3)?,
        model_ref: row.get(4)?,
        started_at: parse_dt(&row.get::<_, String>(5)?),
        ended_at: parse_dt_opt(row.get(6)?),
        usage_prompt_tokens: row.get(7)?,
        usage_completion_tokens: row.get(8)?,
    })
}

fn row_to_transcript_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranscriptItemRow> {
    Ok(TranscriptItemRow {
        id: parse_uuid(&row.get::<_, String>(0)?),
        session_id: parse_uuid(&row.get::<_, String>(1)?),
        turn_id: parse_uuid_opt(row.get(2)?),
        seq: row.get(3)?,
        kind: row.get(4)?,
        payload: parse_json(&row.get::<_, String>(5)?),
        created_at: parse_dt(&row.get::<_, String>(6)?),
    })
}

fn row_to_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRow> {
    Ok(JobRow {
        id: parse_uuid(&row.get::<_, String>(0)?),
        job_type: row.get(1)?,
        schedule: row.get(2)?,
        due_at: parse_dt_opt(row.get(3)?),
        enabled: row.get::<_, i32>(4)? != 0,
        last_run_at: parse_dt_opt(row.get(5)?),
        next_run_at: parse_dt_opt(row.get(6)?),
        payload_kind: row.get(7)?,
        delivery_mode: row.get(8)?,
        payload: parse_json(&row.get::<_, String>(9)?),
        created_at: parse_dt(&row.get::<_, String>(10)?),
        updated_at: parse_dt(&row.get::<_, String>(11)?),
    })
}

fn row_to_job_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRunRow> {
    Ok(JobRunRow {
        id: parse_uuid(&row.get::<_, String>(0)?),
        job_id: parse_uuid(&row.get::<_, String>(1)?),
        started_at: parse_dt(&row.get::<_, String>(2)?),
        finished_at: parse_dt_opt(row.get(3)?),
        trigger_kind: row.get(4)?,
        status: row.get(5)?,
        output: row.get(6)?,
        created_at: parse_dt(&row.get::<_, String>(7)?),
    })
}

fn row_to_approval(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApprovalRow> {
    Ok(ApprovalRow {
        id: parse_uuid(&row.get::<_, String>(0)?),
        subject_type: row.get(1)?,
        subject_id: parse_uuid(&row.get::<_, String>(2)?),
        reason: row.get(3)?,
        decision: row.get(4)?,
        decided_by: row.get(5)?,
        decided_at: parse_dt_opt(row.get(6)?),
        presented_payload: parse_json(&row.get::<_, String>(7)?),
        created_at: parse_dt(&row.get::<_, String>(8)?),
    })
}

fn row_to_tool_execution(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolExecutionRow> {
    Ok(ToolExecutionRow {
        id: parse_uuid(&row.get::<_, String>(0)?),
        tool_call_id: parse_uuid(&row.get::<_, String>(1)?),
        session_id: parse_uuid(&row.get::<_, String>(2)?),
        turn_id: parse_uuid(&row.get::<_, String>(3)?),
        tool_name: row.get(4)?,
        arguments: parse_json(&row.get::<_, String>(5)?),
        status: row.get(6)?,
        result_summary: row.get(7)?,
        error_summary: row.get(8)?,
        started_at: parse_dt(&row.get::<_, String>(9)?),
        ended_at: parse_dt_opt(row.get(10)?),
        approval_id: parse_uuid_opt(row.get(11)?),
        execution_mode: row.get(12)?,
    })
}

fn row_to_process_handle(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProcessHandleRow> {
    Ok(ProcessHandleRow {
        process_id: parse_uuid(&row.get::<_, String>(0)?),
        tool_call_id: parse_uuid(&row.get::<_, String>(1)?),
        session_id: parse_uuid(&row.get::<_, String>(2)?),
        command: row.get(3)?,
        cwd: row.get(4)?,
        status: row.get(5)?,
        exit_code: row.get(6)?,
        started_at: parse_dt(&row.get::<_, String>(7)?),
        ended_at: parse_dt_opt(row.get(8)?),
    })
}

fn row_to_paired_device(row: &rusqlite::Row<'_>) -> rusqlite::Result<PairedDeviceRow> {
    Ok(PairedDeviceRow {
        id: parse_uuid(&row.get::<_, String>(0)?),
        name: row.get(1)?,
        public_key: row.get(2)?,
        role: row.get(3)?,
        scopes: parse_json(&row.get::<_, String>(4)?),
        token_hash: row.get(5)?,
        token_expires_at: parse_dt(&row.get::<_, String>(6)?),
        paired_at: parse_dt(&row.get::<_, String>(7)?),
        last_seen_at: parse_dt_opt(row.get(8)?),
        created_at: parse_dt(&row.get::<_, String>(9)?),
    })
}

fn row_to_pairing_request(row: &rusqlite::Row<'_>) -> rusqlite::Result<PairingRequestRow> {
    Ok(PairingRequestRow {
        id: parse_uuid(&row.get::<_, String>(0)?),
        device_name: row.get(1)?,
        public_key: row.get(2)?,
        challenge: row.get(3)?,
        created_at: parse_dt(&row.get::<_, String>(4)?),
        expires_at: parse_dt(&row.get::<_, String>(5)?),
    })
}

// ══════════════════════════════════════════════════════════════════════
// Column lists
// ══════════════════════════════════════════════════════════════════════

const SESSION_COLS: &str = "id, kind, status, workspace_root, channel_ref, requester_session_id, latest_turn_id, metadata, created_at, updated_at, last_activity_at";
const TURN_COLS: &str = "id, session_id, trigger_kind, status, model_ref, started_at, ended_at, usage_prompt_tokens, usage_completion_tokens";
const TRANSCRIPT_COLS: &str = "id, session_id, turn_id, seq, kind, payload, created_at";
const JOB_COLS: &str = "id, job_type, schedule, due_at, enabled, last_run_at, next_run_at, payload_kind, delivery_mode, payload, created_at, updated_at";
const JOB_RUN_COLS: &str =
    "id, job_id, started_at, finished_at, trigger_kind, status, output, created_at";
const APPROVAL_COLS: &str = "id, subject_type, subject_id, reason, decision, decided_by, decided_at, presented_payload, created_at";
const TOOL_EXEC_COLS: &str = "id, tool_call_id, session_id, turn_id, tool_name, arguments, status, result_summary, error_summary, started_at, ended_at, approval_id, execution_mode";
const PROCESS_HANDLE_COLS: &str =
    "process_id, tool_call_id, session_id, command, cwd, status, exit_code, started_at, ended_at";
const PAIRED_DEVICE_COLS: &str = "id, name, public_key, role, scopes, token_hash, token_expires_at, paired_at, last_seen_at, created_at";
const PAIRING_REQUEST_COLS: &str = "id, device_name, public_key, challenge, created_at, expires_at";

/// Return `Err(QueryReturnedNoRows)` when `affected == 0` inside a closure.
fn require_affected(affected: usize) -> rusqlite::Result<()> {
    if affected == 0 {
        Err(rusqlite::Error::QueryReturnedNoRows)
    } else {
        Ok(())
    }
}

// ══════════════════════════════════════════════════════════════════════
// SqliteSessionRepo
// ══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct SqliteSessionRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteSessionRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl SessionRepo for SqliteSessionRepo {
    async fn create(&self, s: NewSession) -> Result<SessionRow, StoreError> {
        self.conn.call(move |conn| {
            conn.execute(
                &format!("INSERT INTO sessions ({SESSION_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"),
                rusqlite::params![
                    s.id.to_string(), s.kind, s.status, s.workspace_root, s.channel_ref,
                    s.requester_session_id.map(|u| u.to_string()),
                    s.latest_turn_id.map(|u| u.to_string()),
                    serde_json::to_string(&s.metadata).unwrap_or_default(),
                    to_rfc3339(&s.created_at), to_rfc3339(&s.updated_at), to_rfc3339(&s.last_activity_at),
                ],
            )?;
            conn.prepare(&format!("SELECT {SESSION_COLS} FROM sessions WHERE id = ?1"))?
                .query_row([s.id.to_string()], row_to_session)
        }).await.map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<SessionRow, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                conn.prepare(&format!(
                    "SELECT {SESSION_COLS} FROM sessions WHERE id = ?1"
                ))?
                .query_row([&id_s], row_to_session)
            })
            .await
            .map_err(|e| map_err(e, "session", &id.to_string()))
    }

    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<SessionRow>, StoreError> {
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(&format!(
                "SELECT {SESSION_COLS} FROM sessions ORDER BY created_at DESC LIMIT ?1 OFFSET ?2"
            ))?;
                stmt.query_map(rusqlite::params![limit, offset], row_to_session)?
                    .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_channel_ref(
        &self,
        channel_ref: &str,
    ) -> Result<Option<SessionRow>, StoreError> {
        let cr = channel_ref.to_string();
        self.conn.call(move |conn| {
            match conn.prepare(&format!(
                "SELECT {SESSION_COLS} FROM sessions WHERE channel_ref = ?1 AND status NOT IN ('completed','failed','cancelled') ORDER BY created_at DESC LIMIT 1"
            ))?.query_row([&cr], row_to_session) {
                Ok(row) => Ok(Some(row)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e),
            }
        }).await.map_err(StoreError::from)
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        // Parse and validate target status before entering the DB closure.
        let target: rune_core::SessionStatus = status
            .parse()
            .map_err(|e: rune_core::CoreError| StoreError::InvalidTransition(e.to_string()))?;
        let id_s = id.to_string();
        let status = status.to_string();
        self.conn.call(move |conn| {
            // Read current status and validate the FSM transition.
            let current_str: String = conn
                .prepare("SELECT status FROM sessions WHERE id = ?1")?
                .query_row([&id_s], |row| row.get(0))?;
            if let Ok(current) = current_str.parse::<rune_core::SessionStatus>() {
                if let Err(e) = current.transition(target) {
                    return Err(rusqlite::Error::InvalidParameterName(e.to_string()));
                }
            }

            require_affected(conn.execute(
                "UPDATE sessions SET status = ?1, updated_at = ?2, last_activity_at = ?2 WHERE id = ?3",
                rusqlite::params![status, to_rfc3339(&updated_at), &id_s],
            )?)?;
            conn.prepare(&format!("SELECT {SESSION_COLS} FROM sessions WHERE id = ?1"))?
                .query_row([&id_s], row_to_session)
        }).await.map_err(|e| {
            // Surface FSM violations as InvalidTransition, not generic DB errors.
            match &e {
                tokio_rusqlite::Error::Error(rusqlite::Error::InvalidParameterName(msg)) => {
                    StoreError::InvalidTransition(msg.clone())
                }
                _ => map_err(e, "session", &id.to_string()),
            }
        })
    }

    async fn update_metadata(
        &self,
        id: Uuid,
        metadata: serde_json::Value,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let id_s = id.to_string();
        self.conn.call(move |conn| {
            require_affected(conn.execute(
                "UPDATE sessions SET metadata = ?1, updated_at = ?2, last_activity_at = ?2 WHERE id = ?3",
                rusqlite::params![
                    serde_json::to_string(&metadata).unwrap_or_default(),
                    to_rfc3339(&updated_at), &id_s,
                ],
            )?)?;
            conn.prepare(&format!("SELECT {SESSION_COLS} FROM sessions WHERE id = ?1"))?
                .query_row([&id_s], row_to_session)
        }).await.map_err(|e| map_err(e, "session", &id.to_string()))
    }

    async fn update_latest_turn(
        &self,
        id: Uuid,
        turn_id: Uuid,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let id_s = id.to_string();
        let turn_s = turn_id.to_string();
        self.conn.call(move |conn| {
            require_affected(conn.execute(
                "UPDATE sessions SET latest_turn_id = ?1, updated_at = ?2, last_activity_at = ?2 WHERE id = ?3",
                rusqlite::params![turn_s, to_rfc3339(&updated_at), &id_s],
            )?)?;
            conn.prepare(&format!("SELECT {SESSION_COLS} FROM sessions WHERE id = ?1"))?
                .query_row([&id_s], row_to_session)
        }).await.map_err(|e| map_err(e, "session", &id.to_string()))
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| Ok(conn.execute("DELETE FROM sessions WHERE id = ?1", [&id_s])? > 0))
            .await
            .map_err(StoreError::from)
    }
}

// ══════════════════════════════════════════════════════════════════════
// SqliteTurnRepo
// ══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct SqliteTurnRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteTurnRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl TurnRepo for SqliteTurnRepo {
    async fn create(&self, t: NewTurn) -> Result<TurnRow, StoreError> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    &format!("INSERT INTO turns ({TURN_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"),
                    rusqlite::params![
                        t.id.to_string(),
                        t.session_id.to_string(),
                        t.trigger_kind,
                        t.status,
                        t.model_ref,
                        to_rfc3339(&t.started_at),
                        t.ended_at.as_ref().map(to_rfc3339),
                        t.usage_prompt_tokens,
                        t.usage_completion_tokens,
                    ],
                )?;
                conn.prepare(&format!("SELECT {TURN_COLS} FROM turns WHERE id = ?1"))?
                    .query_row([t.id.to_string()], row_to_turn)
            })
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<TurnRow, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                conn.prepare(&format!("SELECT {TURN_COLS} FROM turns WHERE id = ?1"))?
                    .query_row([&id_s], row_to_turn)
            })
            .await
            .map_err(|e| map_err(e, "turn", &id.to_string()))
    }

    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<TurnRow>, StoreError> {
        let sid = session_id.to_string();
        self.conn
            .call(move |conn| {
                conn.prepare(&format!(
                    "SELECT {TURN_COLS} FROM turns WHERE session_id = ?1 ORDER BY started_at ASC"
                ))?
                .query_map([&sid], row_to_turn)?
                .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::from)
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<TurnRow, StoreError> {
        let id_s = id.to_string();
        let status = status.to_string();
        self.conn
            .call(move |conn| {
                require_affected(conn.execute(
                    "UPDATE turns SET status = ?1, ended_at = ?2 WHERE id = ?3",
                    rusqlite::params![status, ended_at.as_ref().map(to_rfc3339), &id_s],
                )?)?;
                conn.prepare(&format!("SELECT {TURN_COLS} FROM turns WHERE id = ?1"))?
                    .query_row([&id_s], row_to_turn)
            })
            .await
            .map_err(|e| map_err(e, "turn", &id.to_string()))
    }

    async fn update_usage(
        &self,
        id: Uuid,
        prompt_tokens: i32,
        completion_tokens: i32,
    ) -> Result<TurnRow, StoreError> {
        let id_s = id.to_string();
        self.conn.call(move |conn| {
            require_affected(conn.execute(
                "UPDATE turns SET usage_prompt_tokens = ?1, usage_completion_tokens = ?2 WHERE id = ?3",
                rusqlite::params![prompt_tokens, completion_tokens, &id_s],
            )?)?;
            conn.prepare(&format!("SELECT {TURN_COLS} FROM turns WHERE id = ?1"))?
                .query_row([&id_s], row_to_turn)
        }).await.map_err(|e| map_err(e, "turn", &id.to_string()))
    }
}

// ══════════════════════════════════════════════════════════════════════
// SqliteTranscriptRepo
// ══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct SqliteTranscriptRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteTranscriptRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl TranscriptRepo for SqliteTranscriptRepo {
    async fn append(&self, item: NewTranscriptItem) -> Result<TranscriptItemRow, StoreError> {
        self.conn.call(move |conn| {
            conn.execute(
                &format!("INSERT INTO transcript_items ({TRANSCRIPT_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7)"),
                rusqlite::params![
                    item.id.to_string(), item.session_id.to_string(),
                    item.turn_id.map(|u| u.to_string()),
                    item.seq, item.kind,
                    serde_json::to_string(&item.payload).unwrap_or_default(),
                    to_rfc3339(&item.created_at),
                ],
            )?;
            conn.prepare(&format!("SELECT {TRANSCRIPT_COLS} FROM transcript_items WHERE id = ?1"))?
                .query_row([item.id.to_string()], row_to_transcript_item)
        }).await.map_err(StoreError::from)
    }

    async fn list_by_session(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<TranscriptItemRow>, StoreError> {
        let sid = session_id.to_string();
        self.conn.call(move |conn| {
            conn.prepare(&format!("SELECT {TRANSCRIPT_COLS} FROM transcript_items WHERE session_id = ?1 ORDER BY seq ASC"))?
                .query_map([&sid], row_to_transcript_item)?
                .collect::<Result<Vec<_>, _>>()
        }).await.map_err(StoreError::from)
    }

    async fn delete_by_session(&self, session_id: Uuid) -> Result<usize, StoreError> {
        let sid = session_id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute("DELETE FROM transcript_items WHERE session_id = ?1", [&sid])
            })
            .await
            .map_err(StoreError::from)
    }
}

// ══════════════════════════════════════════════════════════════════════
// SqliteJobRepo
// ══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct SqliteJobRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteJobRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl JobRepo for SqliteJobRepo {
    async fn create(&self, j: NewJob) -> Result<JobRow, StoreError> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    &format!(
                        "INSERT INTO jobs ({JOB_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"
                    ),
                    rusqlite::params![
                        j.id.to_string(),
                        j.job_type,
                        j.schedule,
                        j.due_at.as_ref().map(to_rfc3339),
                        j.enabled as i32,
                        Option::<String>::None,
                        Option::<String>::None,
                        j.payload_kind,
                        j.delivery_mode,
                        serde_json::to_string(&j.payload).unwrap_or_default(),
                        to_rfc3339(&j.created_at),
                        to_rfc3339(&j.updated_at),
                    ],
                )?;
                conn.prepare(&format!("SELECT {JOB_COLS} FROM jobs WHERE id = ?1"))?
                    .query_row([j.id.to_string()], row_to_job)
            })
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<JobRow, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                conn.prepare(&format!("SELECT {JOB_COLS} FROM jobs WHERE id = ?1"))?
                    .query_row([&id_s], row_to_job)
            })
            .await
            .map_err(|e| map_err(e, "job", &id.to_string()))
    }

    async fn list_enabled(&self) -> Result<Vec<JobRow>, StoreError> {
        self.conn
            .call(move |conn| {
                conn.prepare(&format!(
                    "SELECT {JOB_COLS} FROM jobs WHERE enabled = 1 ORDER BY created_at ASC"
                ))?
                .query_map([], row_to_job)?
                .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::from)
    }

    async fn list_by_type(
        &self,
        job_type: &str,
        include_disabled: bool,
    ) -> Result<Vec<JobRow>, StoreError> {
        let jt = job_type.to_string();
        self.conn.call(move |conn| {
            let sql = if include_disabled {
                format!("SELECT {JOB_COLS} FROM jobs WHERE job_type = ?1 ORDER BY COALESCE(due_at, '9999-12-31') ASC, created_at ASC")
            } else {
                format!("SELECT {JOB_COLS} FROM jobs WHERE job_type = ?1 AND enabled = 1 ORDER BY COALESCE(due_at, '9999-12-31') ASC, created_at ASC")
            };
            conn.prepare(&sql)?.query_map([&jt], row_to_job)?.collect::<Result<Vec<_>, _>>()
        }).await.map_err(StoreError::from)
    }

    async fn update_job(
        &self,
        id: Uuid,
        enabled: bool,
        due_at: Option<DateTime<Utc>>,
        payload_kind: &str,
        delivery_mode: &str,
        payload: serde_json::Value,
        updated_at: DateTime<Utc>,
        last_run_at: Option<DateTime<Utc>>,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<JobRow, StoreError> {
        let id_s = id.to_string();
        let payload_kind = payload_kind.to_string();
        let delivery_mode = delivery_mode.to_string();
        self.conn.call(move |conn| {
            require_affected(conn.execute(
                "UPDATE jobs SET enabled=?1, due_at=?2, payload_kind=?3, delivery_mode=?4, payload=?5, updated_at=?6, last_run_at=?7, next_run_at=?8 WHERE id=?9",
                rusqlite::params![
                    enabled as i32, due_at.as_ref().map(to_rfc3339),
                    payload_kind, delivery_mode,
                    serde_json::to_string(&payload).unwrap_or_default(),
                    to_rfc3339(&updated_at), last_run_at.as_ref().map(to_rfc3339),
                    next_run_at.as_ref().map(to_rfc3339), &id_s,
                ],
            )?)?;
            conn.prepare(&format!("SELECT {JOB_COLS} FROM jobs WHERE id = ?1"))?
                .query_row([&id_s], row_to_job)
        }).await.map_err(|e| map_err(e, "job", &id.to_string()))
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| Ok(conn.execute("DELETE FROM jobs WHERE id = ?1", [&id_s])? > 0))
            .await
            .map_err(StoreError::from)
    }

    async fn record_run(
        &self,
        id: Uuid,
        last_run_at: DateTime<Utc>,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<JobRow, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                require_affected(conn.execute(
                    "UPDATE jobs SET last_run_at=?1, next_run_at=?2, updated_at=?1 WHERE id=?3",
                    rusqlite::params![
                        to_rfc3339(&last_run_at),
                        next_run_at.as_ref().map(to_rfc3339),
                        &id_s
                    ],
                )?)?;
                conn.prepare(&format!("SELECT {JOB_COLS} FROM jobs WHERE id = ?1"))?
                    .query_row([&id_s], row_to_job)
            })
            .await
            .map_err(|e| map_err(e, "job", &id.to_string()))
    }
}

// ══════════════════════════════════════════════════════════════════════
// SqliteJobRunRepo
// ══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct SqliteJobRunRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteJobRunRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl JobRunRepo for SqliteJobRunRepo {
    async fn create(&self, r: NewJobRun) -> Result<JobRunRow, StoreError> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    &format!(
                        "INSERT INTO job_runs ({JOB_RUN_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)"
                    ),
                    rusqlite::params![
                        r.id.to_string(),
                        r.job_id.to_string(),
                        to_rfc3339(&r.started_at),
                        r.finished_at.as_ref().map(to_rfc3339),
                        r.trigger_kind,
                        r.status,
                        r.output,
                        to_rfc3339(&r.created_at),
                    ],
                )?;
                conn.prepare(&format!(
                    "SELECT {JOB_RUN_COLS} FROM job_runs WHERE id = ?1"
                ))?
                .query_row([r.id.to_string()], row_to_job_run)
            })
            .await
            .map_err(StoreError::from)
    }

    async fn complete(
        &self,
        id: Uuid,
        status: &str,
        output: Option<&str>,
        finished_at: DateTime<Utc>,
    ) -> Result<JobRunRow, StoreError> {
        let id_s = id.to_string();
        let status = status.to_string();
        let output = output.map(String::from);
        self.conn
            .call(move |conn| {
                require_affected(conn.execute(
                    "UPDATE job_runs SET status=?1, output=?2, finished_at=?3 WHERE id=?4",
                    rusqlite::params![status, output, to_rfc3339(&finished_at), &id_s],
                )?)?;
                conn.prepare(&format!(
                    "SELECT {JOB_RUN_COLS} FROM job_runs WHERE id = ?1"
                ))?
                .query_row([&id_s], row_to_job_run)
            })
            .await
            .map_err(|e| map_err(e, "job_run", &id.to_string()))
    }

    async fn list_by_job(
        &self,
        job_id: Uuid,
        limit: Option<i64>,
    ) -> Result<Vec<JobRunRow>, StoreError> {
        let jid = job_id.to_string();
        self.conn.call(move |conn| {
            let sql = if let Some(lim) = limit {
                format!("SELECT {JOB_RUN_COLS} FROM job_runs WHERE job_id = ?1 ORDER BY started_at DESC LIMIT {lim}")
            } else {
                format!("SELECT {JOB_RUN_COLS} FROM job_runs WHERE job_id = ?1 ORDER BY started_at DESC")
            };
            conn.prepare(&sql)?.query_map([&jid], row_to_job_run)?.collect::<Result<Vec<_>, _>>()
        }).await.map_err(StoreError::from)
    }
}

// ══════════════════════════════════════════════════════════════════════
// SqliteApprovalRepo
// ══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct SqliteApprovalRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteApprovalRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl ApprovalRepo for SqliteApprovalRepo {
    async fn create(&self, a: NewApproval) -> Result<ApprovalRow, StoreError> {
        self.conn.call(move |conn| {
            conn.execute(
                &format!("INSERT INTO approvals ({APPROVAL_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"),
                rusqlite::params![
                    a.id.to_string(), a.subject_type, a.subject_id.to_string(), a.reason,
                    Option::<String>::None, Option::<String>::None, Option::<String>::None,
                    serde_json::to_string(&a.presented_payload).unwrap_or_default(),
                    to_rfc3339(&a.created_at),
                ],
            )?;
            conn.prepare(&format!("SELECT {APPROVAL_COLS} FROM approvals WHERE id = ?1"))?
                .query_row([a.id.to_string()], row_to_approval)
        }).await.map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<ApprovalRow, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                conn.prepare(&format!(
                    "SELECT {APPROVAL_COLS} FROM approvals WHERE id = ?1"
                ))?
                .query_row([&id_s], row_to_approval)
            })
            .await
            .map_err(|e| map_err(e, "approval", &id.to_string()))
    }

    async fn list(&self, pending_only: bool) -> Result<Vec<ApprovalRow>, StoreError> {
        self.conn.call(move |conn| {
            let sql = if pending_only {
                format!("SELECT {APPROVAL_COLS} FROM approvals WHERE decision IS NULL ORDER BY created_at DESC, id DESC")
            } else {
                format!("SELECT {APPROVAL_COLS} FROM approvals ORDER BY created_at DESC, id DESC")
            };
            conn.prepare(&sql)?.query_map([], row_to_approval)?.collect::<Result<Vec<_>, _>>()
        }).await.map_err(StoreError::from)
    }

    async fn decide(
        &self,
        id: Uuid,
        decision: &str,
        decided_by: &str,
        decided_at: DateTime<Utc>,
    ) -> Result<ApprovalRow, StoreError> {
        let id_s = id.to_string();
        let decision = decision.to_string();
        let decided_by = decided_by.to_string();
        self.conn
            .call(move |conn| {
                require_affected(conn.execute(
                    "UPDATE approvals SET decision=?1, decided_by=?2, decided_at=?3 WHERE id=?4",
                    rusqlite::params![decision, decided_by, to_rfc3339(&decided_at), &id_s],
                )?)?;
                conn.prepare(&format!(
                    "SELECT {APPROVAL_COLS} FROM approvals WHERE id = ?1"
                ))?
                .query_row([&id_s], row_to_approval)
            })
            .await
            .map_err(|e| map_err(e, "approval", &id.to_string()))
    }

    async fn update_presented_payload(
        &self,
        id: Uuid,
        presented_payload: serde_json::Value,
    ) -> Result<ApprovalRow, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                require_affected(conn.execute(
                    "UPDATE approvals SET presented_payload=?1 WHERE id=?2",
                    rusqlite::params![
                        serde_json::to_string(&presented_payload).unwrap_or_default(),
                        &id_s
                    ],
                )?)?;
                conn.prepare(&format!(
                    "SELECT {APPROVAL_COLS} FROM approvals WHERE id = ?1"
                ))?
                .query_row([&id_s], row_to_approval)
            })
            .await
            .map_err(|e| map_err(e, "approval", &id.to_string()))
    }
}

// ══════════════════════════════════════════════════════════════════════
// SqliteToolApprovalPolicyRepo
// ══════════════════════════════════════════════════════════════════════

const TOOL_POLICY_SUBJECT_TYPE: &str = "tool_policy";

fn tool_policy_subject_id() -> Uuid {
    Uuid::nil()
}

#[derive(Clone)]
pub struct SqliteToolApprovalPolicyRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteToolApprovalPolicyRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl ToolApprovalPolicyRepo for SqliteToolApprovalPolicyRepo {
    async fn list_policies(&self) -> Result<Vec<ToolApprovalPolicy>, StoreError> {
        self.conn.call(move |conn| {
            let rows = conn.prepare(&format!(
                "SELECT {APPROVAL_COLS} FROM approvals WHERE subject_type = ?1 ORDER BY reason ASC"
            ))?.query_map([TOOL_POLICY_SUBJECT_TYPE], row_to_approval)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows.into_iter().map(|r| ToolApprovalPolicy {
                tool_name: r.reason,
                decision: r.decision.unwrap_or_default(),
                decided_at: r.decided_at.unwrap_or(r.created_at),
            }).collect())
        }).await.map_err(StoreError::from)
    }

    async fn get_policy(&self, tool_name: &str) -> Result<Option<ToolApprovalPolicy>, StoreError> {
        let tn = tool_name.to_string();
        self.conn.call(move |conn| {
            match conn.prepare(&format!(
                "SELECT {APPROVAL_COLS} FROM approvals WHERE subject_type = ?1 AND reason = ?2 LIMIT 1"
            ))?.query_row(rusqlite::params![TOOL_POLICY_SUBJECT_TYPE, tn], row_to_approval) {
                Ok(r) => Ok(Some(ToolApprovalPolicy {
                    tool_name: r.reason,
                    decision: r.decision.unwrap_or_default(),
                    decided_at: r.decided_at.unwrap_or(r.created_at),
                })),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e),
            }
        }).await.map_err(StoreError::from)
    }

    async fn set_policy(
        &self,
        tool_name: &str,
        decision: &str,
    ) -> Result<ToolApprovalPolicy, StoreError> {
        let tn = tool_name.to_string();
        let dec = decision.to_string();
        self.conn.call(move |conn| {
            let now = Utc::now();
            conn.execute(
                "DELETE FROM approvals WHERE subject_type = ?1 AND reason = ?2",
                rusqlite::params![TOOL_POLICY_SUBJECT_TYPE, &tn],
            )?;
            let id = Uuid::now_v7();
            conn.execute(
                &format!("INSERT INTO approvals ({APPROVAL_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"),
                rusqlite::params![
                    id.to_string(), TOOL_POLICY_SUBJECT_TYPE,
                    tool_policy_subject_id().to_string(), &tn,
                    &dec, "cli", to_rfc3339(&now),
                    serde_json::to_string(&serde_json::json!({"decision": &dec})).unwrap_or_default(),
                    to_rfc3339(&now),
                ],
            )?;
            Ok(ToolApprovalPolicy { tool_name: tn, decision: dec, decided_at: now })
        }).await.map_err(StoreError::from)
    }

    async fn clear_policy(&self, tool_name: &str) -> Result<bool, StoreError> {
        let tn = tool_name.to_string();
        self.conn
            .call(move |conn| {
                Ok(conn.execute(
                    "DELETE FROM approvals WHERE subject_type = ?1 AND reason = ?2",
                    rusqlite::params![TOOL_POLICY_SUBJECT_TYPE, tn],
                )? > 0)
            })
            .await
            .map_err(StoreError::from)
    }
}

// ══════════════════════════════════════════════════════════════════════
// SqliteToolExecutionRepo
// ══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct SqliteToolExecutionRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteToolExecutionRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl ToolExecutionRepo for SqliteToolExecutionRepo {
    async fn create(&self, e: NewToolExecution) -> Result<ToolExecutionRow, StoreError> {
        self.conn.call(move |conn| {
            conn.execute(
                &format!("INSERT INTO tool_executions ({TOOL_EXEC_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)"),
                rusqlite::params![
                    e.id.to_string(), e.tool_call_id.to_string(), e.session_id.to_string(),
                    e.turn_id.to_string(), e.tool_name,
                    serde_json::to_string(&e.arguments).unwrap_or_default(),
                    e.status, Option::<String>::None, Option::<String>::None,
                    to_rfc3339(&e.started_at), Option::<String>::None,
                    e.approval_id.map(|id| id.to_string()), e.execution_mode,
                ],
            )?;
            conn.prepare(&format!("SELECT {TOOL_EXEC_COLS} FROM tool_executions WHERE id = ?1"))?
                .query_row([e.id.to_string()], row_to_tool_execution)
        }).await.map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<ToolExecutionRow, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                conn.prepare(&format!(
                    "SELECT {TOOL_EXEC_COLS} FROM tool_executions WHERE id = ?1"
                ))?
                .query_row([&id_s], row_to_tool_execution)
            })
            .await
            .map_err(|e| map_err(e, "tool_execution", &id.to_string()))
    }

    async fn list_recent(&self, limit: i64) -> Result<Vec<ToolExecutionRow>, StoreError> {
        self.conn
            .call(move |conn| {
                conn.prepare(&format!(
                    "SELECT {TOOL_EXEC_COLS} FROM tool_executions ORDER BY started_at DESC LIMIT ?1"
                ))?
                .query_map([limit], row_to_tool_execution)?
                .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::from)
    }

    async fn complete(
        &self,
        id: Uuid,
        status: &str,
        result_summary: Option<&str>,
        error_summary: Option<&str>,
        ended_at: DateTime<Utc>,
    ) -> Result<ToolExecutionRow, StoreError> {
        let id_s = id.to_string();
        let status = status.to_string();
        let result_summary = result_summary.map(String::from);
        let error_summary = error_summary.map(String::from);
        self.conn.call(move |conn| {
            require_affected(conn.execute(
                "UPDATE tool_executions SET status=?1, result_summary=?2, error_summary=?3, ended_at=?4 WHERE id=?5",
                rusqlite::params![status, result_summary, error_summary, to_rfc3339(&ended_at), &id_s],
            )?)?;
            conn.prepare(&format!("SELECT {TOOL_EXEC_COLS} FROM tool_executions WHERE id = ?1"))?
                .query_row([&id_s], row_to_tool_execution)
        }).await.map_err(|e| map_err(e, "tool_execution", &id.to_string()))
    }
}

// ══════════════════════════════════════════════════════════════════════
// SqliteProcessHandleRepo
// ══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct SqliteProcessHandleRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteProcessHandleRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl ProcessHandleRepo for SqliteProcessHandleRepo {
    async fn create(&self, h: NewProcessHandle) -> Result<ProcessHandleRow, StoreError> {
        self.conn.call(move |conn| {
            conn.execute(
                &format!("INSERT INTO process_handles ({PROCESS_HANDLE_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"),
                rusqlite::params![
                    h.process_id.to_string(), h.tool_call_id.to_string(),
                    h.session_id.to_string(), h.command, h.cwd,
                    h.status, Option::<i32>::None,
                    to_rfc3339(&h.started_at), Option::<String>::None,
                ],
            )?;
            conn.prepare(&format!("SELECT {PROCESS_HANDLE_COLS} FROM process_handles WHERE process_id = ?1"))?
                .query_row([h.process_id.to_string()], row_to_process_handle)
        }).await.map_err(StoreError::from)
    }

    async fn find_by_id(&self, process_id: Uuid) -> Result<ProcessHandleRow, StoreError> {
        let id_s = process_id.to_string();
        self.conn
            .call(move |conn| {
                conn.prepare(&format!(
                    "SELECT {PROCESS_HANDLE_COLS} FROM process_handles WHERE process_id = ?1"
                ))?
                .query_row([&id_s], row_to_process_handle)
            })
            .await
            .map_err(|e| map_err(e, "process_handle", &process_id.to_string()))
    }

    async fn update_status(
        &self,
        process_id: Uuid,
        status: &str,
        exit_code: Option<i32>,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<ProcessHandleRow, StoreError> {
        let id_s = process_id.to_string();
        let status = status.to_string();
        let ended_at_s = ended_at.map(|dt| to_rfc3339(&dt));
        self.conn
            .call(move |conn| {
                require_affected(conn.execute(
                    "UPDATE process_handles SET status=?1, exit_code=?2, ended_at=?3 WHERE process_id=?4",
                    rusqlite::params![status, exit_code, ended_at_s, &id_s],
                )?)?;
                conn.prepare(&format!(
                    "SELECT {PROCESS_HANDLE_COLS} FROM process_handles WHERE process_id = ?1"
                ))?
                .query_row([&id_s], row_to_process_handle)
            })
            .await
            .map_err(|e| map_err(e, "process_handle", &process_id.to_string()))
    }

    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<ProcessHandleRow>, StoreError> {
        let sid = session_id.to_string();
        self.conn
            .call(move |conn| {
                conn.prepare(&format!(
                    "SELECT {PROCESS_HANDLE_COLS} FROM process_handles WHERE session_id = ?1 ORDER BY started_at DESC"
                ))?
                .query_map([&sid], row_to_process_handle)?
                .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::from)
    }

    async fn list_active(&self) -> Result<Vec<ProcessHandleRow>, StoreError> {
        self.conn
            .call(move |conn| {
                conn.prepare(
                    "SELECT process_id, tool_call_id, session_id, command, cwd, status, exit_code, started_at, ended_at FROM process_handles WHERE status IN ('running', 'backgrounded') ORDER BY started_at DESC"
                )?
                .query_map([], row_to_process_handle)?
                .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::from)
    }
}

// ══════════════════════════════════════════════════════════════════════
// SqliteMemoryEmbeddingRepo
// ══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct SqliteMemoryEmbeddingRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteMemoryEmbeddingRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl MemoryEmbeddingRepo for SqliteMemoryEmbeddingRepo {
    async fn upsert_chunk(
        &self,
        file_path: &str,
        chunk_index: i32,
        chunk_text: &str,
        _embedding: &[f32],
    ) -> Result<(), StoreError> {
        let fp = file_path.to_string();
        let ct = chunk_text.to_string();
        self.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO memory_embeddings (id, file_path, chunk_index, chunk_text, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (file_path, chunk_index)
                 DO UPDATE SET chunk_text = excluded.chunk_text, created_at = excluded.created_at",
                rusqlite::params![Uuid::now_v7().to_string(), fp, chunk_index, ct, to_rfc3339(&Utc::now())],
            )?;
            Ok(())
        }).await.map_err(StoreError::from)
    }

    async fn delete_by_file(&self, file_path: &str) -> Result<usize, StoreError> {
        let fp = file_path.to_string();
        self.conn
            .call(move |conn| {
                conn.execute("DELETE FROM memory_embeddings WHERE file_path = ?1", [&fp])
            })
            .await
            .map_err(StoreError::from)
    }

    async fn keyword_search(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<KeywordSearchRow>, StoreError> {
        let q = query.to_string();
        self.conn
            .call(move |conn| {
                conn.prepare(
                    "SELECT me.file_path, me.chunk_text, fts.rank AS score
                 FROM memory_embeddings_fts fts
                 JOIN memory_embeddings me ON me.rowid = fts.rowid
                 WHERE memory_embeddings_fts MATCH ?1
                 ORDER BY fts.rank
                 LIMIT ?2",
                )?
                .query_map(rusqlite::params![q, limit], |row| {
                    Ok(KeywordSearchRow {
                        file_path: row.get(0)?,
                        chunk_text: row.get(1)?,
                        // FTS5 rank is negative (lower = better). Negate for consistency with PG.
                        score: {
                            let r: f64 = row.get(2)?;
                            -r
                        },
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::from)
    }

    async fn vector_search(
        &self,
        _embedding: &[f32],
        _limit: i64,
    ) -> Result<Vec<VectorSearchRow>, StoreError> {
        Ok(vec![])
    }

    async fn count(&self) -> Result<i64, StoreError> {
        self.conn
            .call(move |conn| {
                conn.query_row("SELECT COUNT(*) FROM memory_embeddings", [], |row| {
                    row.get(0)
                })
            })
            .await
            .map_err(StoreError::from)
    }

    async fn list_indexed_files(&self) -> Result<Vec<String>, StoreError> {
        self.conn
            .call(move |conn| {
                conn.prepare(
                    "SELECT DISTINCT file_path FROM memory_embeddings ORDER BY file_path ASC",
                )?
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()
            })
            .await
            .map_err(StoreError::from)
    }
}

// ══════════════════════════════════════════════════════════════════════
// SqliteDeviceRepo
// ══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct SqliteDeviceRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteDeviceRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl DeviceRepo for SqliteDeviceRepo {
    async fn create_device(&self, d: NewPairedDevice) -> Result<PairedDeviceRow, StoreError> {
        self.conn.call(move |conn| {
            conn.execute(
                &format!("INSERT INTO paired_devices ({PAIRED_DEVICE_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"),
                rusqlite::params![
                    d.id.to_string(), d.name, d.public_key, d.role,
                    serde_json::to_string(&d.scopes).unwrap_or_default(),
                    d.token_hash, to_rfc3339(&d.token_expires_at),
                    to_rfc3339(&d.paired_at), Option::<String>::None,
                    to_rfc3339(&d.created_at),
                ],
            )?;
            conn.prepare(&format!("SELECT {PAIRED_DEVICE_COLS} FROM paired_devices WHERE id = ?1"))?
                .query_row([d.id.to_string()], row_to_paired_device)
        }).await.map_err(StoreError::from)
    }

    async fn find_device_by_id(&self, id: Uuid) -> Result<PairedDeviceRow, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                conn.prepare(&format!(
                    "SELECT {PAIRED_DEVICE_COLS} FROM paired_devices WHERE id = ?1"
                ))?
                .query_row([&id_s], row_to_paired_device)
            })
            .await
            .map_err(|e| map_err(e, "paired_device", &id.to_string()))
    }

    async fn find_device_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        let th = token_hash.to_string();
        self.conn
            .call(move |conn| {
                match conn
                    .prepare(&format!(
                        "SELECT {PAIRED_DEVICE_COLS} FROM paired_devices WHERE token_hash = ?1"
                    ))?
                    .query_row([&th], row_to_paired_device)
                {
                    Ok(row) => Ok(Some(row)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(e),
                }
            })
            .await
            .map_err(StoreError::from)
    }

    async fn find_device_by_public_key(
        &self,
        public_key: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        let pk = public_key.to_string();
        self.conn
            .call(move |conn| {
                match conn
                    .prepare(&format!(
                        "SELECT {PAIRED_DEVICE_COLS} FROM paired_devices WHERE public_key = ?1"
                    ))?
                    .query_row([&pk], row_to_paired_device)
                {
                    Ok(row) => Ok(Some(row)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(e),
                }
            })
            .await
            .map_err(StoreError::from)
    }

    async fn list_devices(&self) -> Result<Vec<PairedDeviceRow>, StoreError> {
        self.conn
            .call(move |conn| {
                conn.prepare(&format!(
                    "SELECT {PAIRED_DEVICE_COLS} FROM paired_devices ORDER BY paired_at ASC"
                ))?
                .query_map([], row_to_paired_device)?
                .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::from)
    }

    async fn update_token(
        &self,
        id: Uuid,
        token_hash: &str,
        token_expires_at: DateTime<Utc>,
    ) -> Result<PairedDeviceRow, StoreError> {
        let id_s = id.to_string();
        let th = token_hash.to_string();
        self.conn
            .call(move |conn| {
                require_affected(conn.execute(
                    "UPDATE paired_devices SET token_hash=?1, token_expires_at=?2 WHERE id=?3",
                    rusqlite::params![th, to_rfc3339(&token_expires_at), &id_s],
                )?)?;
                conn.prepare(&format!(
                    "SELECT {PAIRED_DEVICE_COLS} FROM paired_devices WHERE id = ?1"
                ))?
                .query_row([&id_s], row_to_paired_device)
            })
            .await
            .map_err(|e| map_err(e, "paired_device", &id.to_string()))
    }

    async fn update_role(
        &self,
        id: Uuid,
        role: &str,
        scopes: serde_json::Value,
    ) -> Result<PairedDeviceRow, StoreError> {
        let id_s = id.to_string();
        let role = role.to_string();
        self.conn
            .call(move |conn| {
                require_affected(conn.execute(
                    "UPDATE paired_devices SET role=?1, scopes=?2 WHERE id=?3",
                    rusqlite::params![
                        role,
                        serde_json::to_string(&scopes).unwrap_or_default(),
                        &id_s
                    ],
                )?)?;
                conn.prepare(&format!(
                    "SELECT {PAIRED_DEVICE_COLS} FROM paired_devices WHERE id = ?1"
                ))?
                .query_row([&id_s], row_to_paired_device)
            })
            .await
            .map_err(|e| map_err(e, "paired_device", &id.to_string()))
    }

    async fn touch_last_seen(
        &self,
        id: Uuid,
        last_seen_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                require_affected(conn.execute(
                    "UPDATE paired_devices SET last_seen_at=?1 WHERE id=?2",
                    rusqlite::params![to_rfc3339(&last_seen_at), &id_s],
                )?)?;
                Ok(())
            })
            .await
            .map_err(|e| map_err(e, "paired_device", &id.to_string()))
    }

    async fn delete_device(&self, id: Uuid) -> Result<bool, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                Ok(conn.execute("DELETE FROM paired_devices WHERE id = ?1", [&id_s])? > 0)
            })
            .await
            .map_err(StoreError::from)
    }

    async fn create_pairing_request(
        &self,
        r: NewPairingRequest,
    ) -> Result<PairingRequestRow, StoreError> {
        self.conn.call(move |conn| {
            conn.execute(
                &format!("INSERT INTO pairing_requests ({PAIRING_REQUEST_COLS}) VALUES (?1,?2,?3,?4,?5,?6)"),
                rusqlite::params![
                    r.id.to_string(), r.device_name, r.public_key, r.challenge,
                    to_rfc3339(&r.created_at), to_rfc3339(&r.expires_at),
                ],
            )?;
            conn.prepare(&format!("SELECT {PAIRING_REQUEST_COLS} FROM pairing_requests WHERE id = ?1"))?
                .query_row([r.id.to_string()], row_to_pairing_request)
        }).await.map_err(StoreError::from)
    }

    async fn take_pairing_request(
        &self,
        id: Uuid,
    ) -> Result<Option<PairingRequestRow>, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                match conn
                    .prepare(&format!(
                        "SELECT {PAIRING_REQUEST_COLS} FROM pairing_requests WHERE id = ?1"
                    ))?
                    .query_row([&id_s], row_to_pairing_request)
                {
                    Ok(row) => {
                        conn.execute("DELETE FROM pairing_requests WHERE id = ?1", [&id_s])?;
                        Ok(Some(row))
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(e),
                }
            })
            .await
            .map_err(StoreError::from)
    }

    async fn delete_pairing_request(&self, id: Uuid) -> Result<bool, StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                Ok(conn.execute("DELETE FROM pairing_requests WHERE id = ?1", [&id_s])? > 0)
            })
            .await
            .map_err(StoreError::from)
    }

    async fn list_pending_requests(&self) -> Result<Vec<PairingRequestRow>, StoreError> {
        self.conn.call(move |conn| {
            let now = to_rfc3339(&Utc::now());
            conn.prepare(&format!(
                "SELECT {PAIRING_REQUEST_COLS} FROM pairing_requests WHERE expires_at > ?1 ORDER BY created_at ASC"
            ))?.query_map([&now], row_to_pairing_request)?.collect::<Result<Vec<_>, _>>()
        }).await.map_err(StoreError::from)
    }

    async fn prune_expired_requests(&self) -> Result<usize, StoreError> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM pairing_requests WHERE expires_at <= ?1",
                    [&to_rfc3339(&Utc::now())],
                )
            })
            .await
            .map_err(StoreError::from)
    }
}
