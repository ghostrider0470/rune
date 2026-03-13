#![doc = "Tool system skeleton for Rune: definitions, registry, executor trait, and built-in stubs."]

mod definition;
mod error;
pub mod exec_tool;
mod executor;
pub mod file_tools;
pub mod memory_tool;
pub mod process_tool;
mod registry;
pub mod session_tool;
mod stubs;

pub use definition::{ToolCall, ToolDefinition, ToolResult};
pub use error::ToolError;
pub use executor::{AlwaysAllow, ApprovalCheck, ToolExecutor};
pub use registry::ToolRegistry;
pub use stubs::{register_builtin_stubs, validate_arguments, StubExecutor};

#[cfg(test)]
mod tests;
