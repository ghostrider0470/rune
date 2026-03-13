#![doc = "Session engine, turn loop, context assembly, and tool orchestration for Rune."]

mod compaction;
mod context;
mod engine;
mod error;
mod executor;
pub mod heartbeat;
pub mod memory;
pub mod scheduler;
pub mod session_loop;
mod session_metadata;
mod usage;
pub mod workspace;

pub use compaction::{CompactionStrategy, NoOpCompaction};
pub use context::ContextAssembler;
pub use engine::SessionEngine;
pub use error::RuntimeError;
pub use executor::TurnExecutor;
pub use usage::UsageAccumulator;

#[cfg(test)]
mod tests;
