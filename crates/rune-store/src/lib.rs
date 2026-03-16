#![doc = "Persistence layer for Rune: Diesel repos, migrations, and embedded PostgreSQL fallback."]

pub mod embedded;
pub mod error;
pub mod models;
pub mod pg;
pub mod pool;
pub mod repos;
pub mod schema;

pub use embedded::EmbeddedPg;
pub use error::StoreError;
pub use pool::PgVectorStatus;
pub use pg::{
    PgApprovalRepo, PgDeviceRepo, PgJobRepo, PgJobRunRepo, PgMemoryEmbeddingRepo,
    PgToolApprovalPolicyRepo, PgToolExecutionRepo,
};
pub use repos::{
    ApprovalRepo, DeviceRepo, JobRepo, JobRunRepo, MemoryEmbeddingRepo, ToolApprovalPolicy,
    ToolExecutionRepo,
};
