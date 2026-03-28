#![doc = "Session engine, turn loop, context assembly, and tool orchestration for Rune."]

pub mod comms;
pub use comms::{
    CommsClient, CommsMessageSummary, CommsTransport, CommsTransportKind, FsCommsTransport,
    build_comms_transport,
};

pub mod agent_registry;
pub mod cancellation;
pub mod claude_plugin;
pub mod command_registry;
mod compaction;
mod context;
pub mod context_budget;
pub mod dispatcher;
mod engine;
mod error;
mod executor;
pub mod heartbeat;
pub mod hooks;
pub mod lane_queue;
pub mod mem0;
pub mod memory;
pub mod merge_queue;
pub mod orchestrator;
pub mod plugin;
pub mod plugin_manager;
pub mod plugin_scanner;
pub mod project;
pub mod restart_continuity;
pub mod scheduler;
pub mod session_loop;
mod session_metadata;
pub mod skill;
pub mod skill_loader;
pub mod spell;
pub mod spell_loader;
mod usage;
pub mod workspace;

pub use agent_registry::AgentRegistry;
pub use command_registry::CommandRegistry;
pub use compaction::{CompactionStrategy, NoOpCompaction, TokenBudgetCompaction};
pub use context::ContextAssembler;
pub use context_budget::{
    BudgetItem, BudgetReport, Checkpoint, CheckpointStore, GcResult, Partition, PartitionBudget,
    PartitionReport, TokenBudget, heartbeat_gc, persist_checkpoint, recover_checkpoint,
};
pub use dispatcher::{DispatchDecision, MessageDispatcher, OrchestratorRegistry};
pub use engine::SessionEngine;
pub use error::RuntimeError;
pub use executor::TurnExecutor;
pub use hooks::{HookEvent, HookHandler, HookRegistry};
pub use cancellation::{TurnCancellationHandle, TurnCancellationRegistry};
pub use lane_queue::{Lane, LanePermit, LaneQueue, LaneStats, ToolPermit};
pub use mem0::Mem0Engine;
pub use plugin::{PluginLoader, PluginManifest, PluginRegistry, PluginScanSummary};
pub use plugin_manager::PluginManager;
pub use plugin_scanner::{PluginScanner, UnifiedScanSummary};
pub use project::{ProjectConfig, ProjectRegistry};
pub use session_loop::TelegramFileDownloader;
pub use spell::{Spell, SpellKind, SpellRegistry};
pub use spell_loader::{SpellLoader, SpellScanSummary};
// Backward-compat re-exports (issue #299)
pub use skill::{Skill, SkillRegistry};
pub use skill_loader::{SkillLoader, SkillScanSummary};
pub use usage::UsageAccumulator;

#[cfg(test)]
mod tests;
