#![doc = "Tool system skeleton for Rune: definitions, registry, executor trait, and built-in stubs."]

mod definition;
mod error;
mod executor;
mod registry;
mod stubs;

pub use definition::{ToolCall, ToolDefinition, ToolResult};
pub use error::ToolError;
pub use executor::{AlwaysAllow, ApprovalCheck, ToolExecutor};
pub use registry::ToolRegistry;
pub use stubs::{register_builtin_stubs, validate_arguments, StubExecutor};

#[cfg(test)]
mod tests;
