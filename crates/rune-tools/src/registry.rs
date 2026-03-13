use std::collections::HashMap;

use crate::definition::ToolDefinition;
use crate::error::ToolError;

/// Central registry of available tools and their schemas.
#[derive(Debug, Default)]
pub struct ToolRegistry {
    tools: HashMap<String, ToolDefinition>,
}

impl ToolRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool definition. Overwrites if a tool with the same name already exists.
    pub fn register(&mut self, tool: ToolDefinition) {
        self.tools.insert(tool.name.clone(), tool);
    }

    /// Look up a tool by name.
    pub fn lookup(&self, name: &str) -> Result<&ToolDefinition, ToolError> {
        self.tools.get(name).ok_or_else(|| ToolError::UnknownTool {
            name: name.to_string(),
        })
    }

    /// List all registered tools (sorted by name for determinism).
    #[must_use]
    pub fn list(&self) -> Vec<&ToolDefinition> {
        let mut tools: Vec<_> = self.tools.values().collect();
        tools.sort_by_key(|t| &t.name);
        tools
    }

    /// Number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}
