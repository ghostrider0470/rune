#![doc = "Persistence layer for Rune: Diesel repos, migrations, and embedded PostgreSQL fallback."]

pub mod database;
pub mod error;
pub mod models;
pub mod repos;
pub mod schema;

pub use database::{EmbeddedPostgres, StoreRuntime, connect, run_migrations};
pub use error::StoreError;
pub use repos::{PgPool, PgStore, build_pool};
