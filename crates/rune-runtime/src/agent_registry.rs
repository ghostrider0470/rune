//! Registry for subagent templates loaded from plugins.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::debug;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub system_prompt: String,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Clone)]
pub struct AgentRegistry {
    inner: Arc<RwLock<HashMap<String, AgentTemplate>>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub async fn register(&self, template: AgentTemplate) {
        let name = template.name.clone();
        self.inner.write().await.insert(name.clone(), template);
        debug!(agent = %name, "agent template registered");
    }

    pub async fn remove(&self, name: &str) -> Option<AgentTemplate> {
        self.inner.write().await.remove(name)
    }

    pub async fn get(&self, name: &str) -> Option<AgentTemplate> {
        self.inner.read().await.get(name).cloned()
    }

    pub async fn list(&self) -> Vec<AgentTemplate> {
        self.inner.read().await.values().cloned().collect()
    }

    pub async fn clear(&self) {
        self.inner.write().await.clear();
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn agent_registry_crud() {
        let reg = AgentRegistry::new();
        reg.register(AgentTemplate {
            name: "test:reviewer".into(),
            description: "Reviews code".into(),
            when_to_use: "When reviewing code".into(),
            system_prompt: "Review carefully".into(),
            model: None,
            allowed_tools: None,
        }).await;
        assert_eq!(reg.len().await, 1);
        let t = reg.get("test:reviewer").await.unwrap();
        assert_eq!(t.description, "Reviews code");
        reg.remove("test:reviewer").await;
        assert!(reg.is_empty().await);
    }
}
