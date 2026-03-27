#![doc = "Persistence layer for Rune: trait-based repos with Postgres and SQLite backends."]

#[cfg(feature = "postgres")]
pub mod embedded;
pub mod error;
pub mod factory;
pub mod models;
#[cfg(feature = "postgres")]
pub mod pg;
#[cfg(feature = "postgres")]
pub mod pool;
pub mod repos;
#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "cosmos")]
pub mod cosmos;
#[cfg(feature = "lancedb")]
pub mod lancedb;

#[cfg(feature = "postgres")]
pub use embedded::EmbeddedPg;
pub use error::StoreError;
pub use factory::{RepoSet, StorageInfo, build_repos};
#[cfg(feature = "postgres")]
pub use pg::{
    PgApprovalRepo, PgDeviceRepo, PgJobRepo, PgJobRunRepo, PgMemoryEmbeddingRepo,
    PgMemoryFactRepo, PgProcessHandleRepo, PgToolApprovalPolicyRepo, PgToolExecutionRepo,
};
#[cfg(feature = "postgres")]
pub use pool::PgVectorStatus;
pub use repos::{
    ApprovalRepo, DeviceRepo, JobRepo, JobRunRepo, MemoryEmbeddingRepo, MemoryFactRepo,
    ProcessHandleRepo, ToolApprovalPolicy, ToolExecutionRepo,
};

pub mod turn_status;
