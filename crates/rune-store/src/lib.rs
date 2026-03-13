#![doc = "Persistence layer for Rune: Diesel repos, migrations, and embedded PostgreSQL fallback."]

pub mod error;
pub mod models;
pub mod repos;
pub mod schema;

pub use error::StoreError;
