#![doc = "Session engine, turn loop, context assembly, and tool orchestration for Rune."]

mod compaction;
mod context;
mod engine;
mod error;
mod executor;
mod usage;

pub use compaction::{CompactionStrategy, NoOpCompaction};
pub use context::ContextAssembler;
pub use engine::SessionEngine;
pub use error::RuntimeError;
pub use executor::TurnExecutor;
pub use usage::UsageAccumulator;

#[cfg(test)]
mod tests;
