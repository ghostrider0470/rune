//! Registry for slash commands loaded from plugins.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::debug;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub prompt_body: String,
    pub plugin_name: String,
}

impl Command {
    pub fn short_name(&self) -> &str {
        self.name.split_once(':').map(|(_, s)| s).unwrap_or(&self.name)
    }

    pub fn expand(&self, args: &str) -> String {
        if self.prompt_body.contains("$ARGUMENTS") {
            self.prompt_body.replace("$ARGUMENTS", args)
        } else if args.is_empty() {
            self.prompt_body.clone()
        } else {
            format!("{}\n\nARGUMENTS: {}", self.prompt_body, args)
        }
    }
}

#[derive(Clone)]
pub struct CommandRegistry {
    inner: Arc<RwLock<HashMap<String, Command>>>,
    aliases: Arc<RwLock<HashMap<String, String>>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            aliases: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, cmd: Command) {
        let name = cmd.name.clone();
        let short = cmd.short_name().to_string();
        let mut aliases = self.aliases.write().await;
        if !aliases.contains_key(&short) {
            aliases.insert(short, name.clone());
        }
        drop(aliases);
        self.inner.write().await.insert(name.clone(), cmd);
        debug!(command = %name, "command registered");
    }

    pub async fn get(&self, name: &str) -> Option<Command> {
        let inner = self.inner.read().await;
        if let Some(cmd) = inner.get(name) {
            return Some(cmd.clone());
        }
        let aliases = self.aliases.read().await;
        if let Some(full) = aliases.get(name) {
            return inner.get(full).cloned();
        }
        None
    }

    pub async fn list(&self) -> Vec<Command> {
        self.inner.read().await.values().cloned().collect()
    }

    pub async fn clear(&self) {
        self.inner.write().await.clear();
        self.aliases.write().await.clear();
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn command_registry_crud_and_alias() {
        let reg = CommandRegistry::new();
        reg.register(Command {
            name: "superpowers:commit".into(),
            description: "Create a commit".into(),
            prompt_body: "Commit changes. $ARGUMENTS".into(),
            plugin_name: "superpowers".into(),
        }).await;
        assert!(reg.get("superpowers:commit").await.is_some());
        assert!(reg.get("commit").await.is_some());
        assert_eq!(reg.len().await, 1);
    }

    #[test]
    fn command_expand_with_arguments() {
        let cmd = Command {
            name: "test:deploy".into(),
            description: "Deploy".into(),
            prompt_body: "Deploy to $ARGUMENTS environment.".into(),
            plugin_name: "test".into(),
        };
        assert_eq!(cmd.expand("production"), "Deploy to production environment.");
    }

    #[test]
    fn command_expand_without_placeholder() {
        let cmd = Command {
            name: "test:status".into(),
            description: "Status".into(),
            prompt_body: "Show system status.".into(),
            plugin_name: "test".into(),
        };
        assert_eq!(cmd.expand(""), "Show system status.");
        assert_eq!(cmd.expand("verbose"), "Show system status.\n\nARGUMENTS: verbose");
    }
}
