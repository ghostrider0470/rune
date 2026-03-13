#![doc = "Tool system for Rune: definitions, registry, executor trait, and built-in implementations."]

mod builtins;
mod definition;
mod error;
mod executor;
mod registry;

pub use builtins::{register_builtin_stubs, register_builtin_tools, validate_arguments, BuiltinToolExecutor};
pub use definition::{ToolCall, ToolDefinition, ToolResult};
pub use error::ToolError;
pub use executor::{AlwaysAllow, ApprovalCheck, ToolExecutor};
pub use registry::ToolRegistry;

#[cfg(test)]
mod tests;
