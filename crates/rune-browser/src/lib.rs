#![doc = "Semantic browser snapshot engine for Rune: captures web pages as compact accessibility-tree representations suitable for LLM consumption."]

pub mod browser;
pub mod error;
pub mod snapshot;
pub mod tool;

pub use browser::{BrowserPool, BrowserPoolConfig};
pub use error::BrowserError;
pub use snapshot::{BrowserSnapshot, SnapshotElement, SnapshotEngine, SnapshotOptions};
pub use tool::{BrowseBackend, BrowseParams, BrowseTool, browse_tool_definition};
