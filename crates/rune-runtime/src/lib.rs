#![doc = "Session engine, turn loop, context assembly, and tool orchestration for Rune."]

mod compaction;
mod context;
mod engine;
mod error;
mod executor;
pub mod heartbeat;
pub mod hooks;
pub mod lane_queue;
pub mod memory;
pub mod plugin;
pub mod scheduler;
pub mod session_loop;
mod session_metadata;
pub mod skill;
pub mod skill_loader;
mod usage;
pub mod workspace;

pub use compaction::{CompactionStrategy, NoOpCompaction};
pub use context::ContextAssembler;
pub use engine::SessionEngine;
pub use error::RuntimeError;
pub use executor::TurnExecutor;
pub use hooks::{HookEvent, HookHandler, HookRegistry};
pub use lane_queue::{Lane, LanePermit, LaneQueue, LaneStats};
pub use plugin::{PluginLoader, PluginManifest, PluginRegistry, PluginScanSummary};
pub use skill::{Skill, SkillRegistry};
pub use skill_loader::{SkillLoader, SkillScanSummary};
pub use usage::UsageAccumulator;

#[cfg(test)]
mod tests;
