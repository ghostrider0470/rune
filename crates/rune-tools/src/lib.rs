#![doc = "Tool system primitives and built-in executors for Rune: shared definitions, registry, approval/policy helpers, process/file/session/memory tool implementations, and compatibility test scaffolding."]

pub mod approval;
pub mod cron_tool;
mod definition;
mod error;
pub mod exec_tool;
mod executor;
pub mod file_tools;
pub mod gateway_tool;
pub mod git_tool;
pub mod memory_index;
pub mod memory_tool;
pub mod message_tool;
pub mod process_audit;
pub mod process_tool;
mod registry;
pub mod session_tool;
pub mod spawn_tool;
mod stubs;
pub mod subagent_tool;
pub mod web_fetch_tool;

pub use definition::{ToolCall, ToolDefinition, ToolResult};
pub use error::ToolError;
pub use executor::{AlwaysAllow, ApprovalCheck, ToolExecutor};
pub use registry::ToolRegistry;
pub use stubs::{StubExecutor, register_builtin_stubs, validate_arguments};

#[cfg(test)]
mod tests;
