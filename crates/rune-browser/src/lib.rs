#![doc = "Semantic browser snapshot engine for Rune: captures web pages as compact accessibility-tree representations suitable for LLM consumption."]

pub mod error;
pub mod snapshot;

pub use error::BrowserError;
pub use snapshot::{BrowserSnapshot, SnapshotElement, SnapshotEngine, SnapshotOptions};
