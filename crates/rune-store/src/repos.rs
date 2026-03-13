//! Repository trait definitions and PostgreSQL Diesel implementations.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::{Object, Pool};
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::*;
use crate::schema::{jobs, sessions, tool_executions, transcript_items, turns};

/// Persistence contract for session records.
#[async_trait]
pub trait SessionRepo: Send + Sync {
    async fn create(&self, session: NewSession) -> Result<SessionRow, StoreError>;
    async fn find_by_id(&self, id: Uuid) -> Result<SessionRow, StoreError>;
    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<SessionRow>, StoreError>;
    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError>;
}

/// Persistence contract for turn records.
#[async_trait]
pub trait TurnRepo: Send + Sync {
    async fn create(&self, turn: NewTurn) -> Result<TurnRow, StoreError>;
    async fn find_by_id(&self, id: Uuid) -> Result<TurnRow, StoreError>;
    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<TurnRow>, StoreError>;
    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<TurnRow, StoreError>;
}

/// Persistence contract for transcript items.
#[async_trait]
pub trait TranscriptRepo: Send + Sync {
    async fn append(&self, item: NewTranscriptItem) -> Result<TranscriptItemRow, StoreError>;
    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<TranscriptItemRow>, StoreError>;
}

/// Persistence contract for scheduled jobs.
#[async_trait]
pub trait JobRepo: Send + Sync {
    async fn create(&self, job: NewJob) -> Result<JobRow, StoreError>;
    async fn find_by_id(&self, id: Uuid) -> Result<JobRow, StoreError>;
    async fn list_enabled(&self) -> Result<Vec<JobRow>, StoreError>;
    async fn record_run(
        &self,
        id: Uuid,
        last_run_at: DateTime<Utc>,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<JobRow, StoreError>;
}

/// Persistence contract for approval gates.
#[async_trait]
pub trait ApprovalRepo: Send + Sync {
    async fn create(&self, approval: NewApproval) -> Result<ApprovalRow, StoreError>;
    async fn find_by_id(&self, id: Uuid) -> Result<ApprovalRow, StoreError>;
    async fn decide(
        &self,
        id: Uuid,
        decision: &str,
        decided_by: &str,
        decided_at: DateTime<Utc>,
    ) -> Result<ApprovalRow, StoreError>;
}

/// Persistence contract for tool execution audit records.
#[async_trait]
pub trait ToolExecutionRepo: Send + Sync {
    async fn create(&self, execution: NewToolExecution) -> Result<ToolExecutionRow, StoreError>;
    async fn find_by_id(&self, id: Uuid) -> Result<ToolExecutionRow, StoreError>;
    async fn complete(
        &self,
        id: Uuid,
        status: &str,
        result_summary: Option<&str>,
        error_summary: Option<&str>,
        ended_at: DateTime<Utc>,
    ) -> Result<ToolExecutionRow, StoreError>;
}

pub type PgPool = Pool<diesel_async::AsyncPgConnection>;
pub type PgConnection = Object<diesel_async::AsyncPgConnection>;

/// Shared PostgreSQL-backed repository implementation.
#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn connection(&self) -> Result<PgConnection, StoreError> {
        self.pool.get().await.map_err(StoreError::from)
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

fn not_found(entity: &'static str, id: Uuid) -> StoreError {
    StoreError::NotFound {
        entity,
        id: id.to_string(),
    }
}

#[async_trait]
impl SessionRepo for PgStore {
    async fn create(&self, session: NewSession) -> Result<SessionRow, StoreError> {
        let mut conn = self.connection().await?;
        diesel::insert_into(sessions::table)
            .values(&session)
            .returning(SessionRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<SessionRow, StoreError> {
        let mut conn = self.connection().await?;
        sessions::table
            .find(id)
            .select(SessionRow::as_select())
            .get_result(&mut conn)
            .await
            .map_err(|error| match error {
                diesel::result::Error::NotFound => not_found("session", id),
                other => StoreError::from(other),
            })
    }

    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<SessionRow>, StoreError> {
        let mut conn = self.connection().await?;
        sessions::table
            .order(sessions::updated_at.desc())
            .limit(limit)
            .offset(offset)
            .select(SessionRow::as_select())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let mut conn = self.connection().await?;
        diesel::update(sessions::table.find(id))
            .set((
                sessions::status.eq(status),
                sessions::updated_at.eq(updated_at),
                sessions::last_activity_at.eq(updated_at),
            ))
            .returning(SessionRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|error| match error {
                diesel::result::Error::NotFound => not_found("session", id),
                other => StoreError::from(other),
            })
    }
}

#[async_trait]
impl TurnRepo for PgStore {
    async fn create(&self, turn: NewTurn) -> Result<TurnRow, StoreError> {
        let mut conn = self.connection().await?;
        diesel::insert_into(turns::table)
            .values(&turn)
            .returning(TurnRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<TurnRow, StoreError> {
        let mut conn = self.connection().await?;
        turns::table
            .find(id)
            .select(TurnRow::as_select())
            .get_result(&mut conn)
            .await
            .map_err(|error| match error {
                diesel::result::Error::NotFound => not_found("turn", id),
                other => StoreError::from(other),
            })
    }

    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<TurnRow>, StoreError> {
        let mut conn = self.connection().await?;
        turns::table
            .filter(turns::session_id.eq(session_id))
            .order(turns::started_at.asc())
            .select(TurnRow::as_select())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<TurnRow, StoreError> {
        let mut conn = self.connection().await?;
        diesel::update(turns::table.find(id))
            .set((turns::status.eq(status), turns::ended_at.eq(ended_at)))
            .returning(TurnRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|error| match error {
                diesel::result::Error::NotFound => not_found("turn", id),
                other => StoreError::from(other),
            })
    }
}

#[async_trait]
impl TranscriptRepo for PgStore {
    async fn append(&self, item: NewTranscriptItem) -> Result<TranscriptItemRow, StoreError> {
        let mut conn = self.connection().await?;
        diesel::insert_into(transcript_items::table)
            .values(&item)
            .returning(TranscriptItemRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<TranscriptItemRow>, StoreError> {
        let mut conn = self.connection().await?;
        transcript_items::table
            .filter(transcript_items::session_id.eq(session_id))
            .order((transcript_items::seq.asc(), transcript_items::created_at.asc()))
            .select(TranscriptItemRow::as_select())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }
}

#[async_trait]
impl JobRepo for PgStore {
    async fn create(&self, job: NewJob) -> Result<JobRow, StoreError> {
        let mut conn = self.connection().await?;
        diesel::insert_into(jobs::table)
            .values(&job)
            .returning(JobRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<JobRow, StoreError> {
        let mut conn = self.connection().await?;
        jobs::table
            .find(id)
            .select(JobRow::as_select())
            .get_result(&mut conn)
            .await
            .map_err(|error| match error {
                diesel::result::Error::NotFound => not_found("job", id),
                other => StoreError::from(other),
            })
    }

    async fn list_enabled(&self) -> Result<Vec<JobRow>, StoreError> {
        let mut conn = self.connection().await?;
        jobs::table
            .filter(jobs::enabled.eq(true))
            .order(jobs::created_at.asc())
            .select(JobRow::as_select())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn record_run(
        &self,
        id: Uuid,
        last_run_at: DateTime<Utc>,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<JobRow, StoreError> {
        let mut conn = self.connection().await?;
        diesel::update(jobs::table.find(id))
            .set((
                jobs::last_run_at.eq(last_run_at),
                jobs::next_run_at.eq(next_run_at),
                jobs::updated_at.eq(last_run_at),
            ))
            .returning(JobRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|error| match error {
                diesel::result::Error::NotFound => not_found("job", id),
                other => StoreError::from(other),
            })
    }
}

/// Build a PostgreSQL pool from a database URL.
pub fn build_pool(database_url: &str, max_connections: u32) -> Result<PgPool, StoreError> {
    let manager = AsyncDieselConnectionManager::<diesel_async::AsyncPgConnection>::new(database_url);
    Pool::builder(manager)
        .max_size(max_connections as usize)
        .build()
        .map_err(StoreError::from)
}

pub type DynSessionRepo = Arc<dyn SessionRepo>;
pub type DynTurnRepo = Arc<dyn TurnRepo>;
pub type DynTranscriptRepo = Arc<dyn TranscriptRepo>;
pub type DynJobRepo = Arc<dyn JobRepo>;
