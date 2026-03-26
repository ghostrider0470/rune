//! Plugin lifecycle coordinator.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::agent_registry::AgentRegistry;
use crate::command_registry::CommandRegistry;
use crate::hooks::HookRegistry;
use crate::plugin::PluginRegistry;
use crate::plugin_scanner::{PluginScanner, UnifiedScanSummary};
use crate::skill::SkillRegistry;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginStatus {
    pub name: String,
    pub enabled: bool,
    pub source: String,
    pub skills: usize,
    pub agents: usize,
    pub hooks: usize,
    pub commands: usize,
    pub mcp_servers: usize,
}

#[derive(Clone)]
pub struct PluginManager {
    scanner: Arc<PluginScanner>,
    plugin_registry: Arc<PluginRegistry>,
    skill_registry: Arc<SkillRegistry>,
    agent_registry: Arc<AgentRegistry>,
    command_registry: Arc<CommandRegistry>,
    hook_registry: Arc<HookRegistry>,
    plugin_meta: Arc<tokio::sync::RwLock<HashMap<String, PluginMeta>>>,
}

#[derive(Clone, Debug)]
struct PluginMeta {
    name: String,
    source_dir: String,
    enabled: bool,
    skills: usize,
    agents: usize,
    hooks: usize,
    commands: usize,
    mcp_servers: usize,
}

impl PluginManager {
    pub fn new(
        scanner: Arc<PluginScanner>,
        plugin_registry: Arc<PluginRegistry>,
        skill_registry: Arc<SkillRegistry>,
        agent_registry: Arc<AgentRegistry>,
        command_registry: Arc<CommandRegistry>,
        hook_registry: Arc<HookRegistry>,
    ) -> Self {
        Self {
            scanner, plugin_registry, skill_registry, agent_registry,
            command_registry, hook_registry,
            plugin_meta: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    pub async fn reload(&self) -> UnifiedScanSummary {
        let summary = self.scanner.scan().await;
        self.rebuild_meta().await;
        summary
    }

    pub async fn status(&self) -> Vec<PluginStatus> {
        let meta = self.plugin_meta.read().await;
        meta.values().map(|m| PluginStatus {
            name: m.name.clone(), enabled: m.enabled, source: m.source_dir.clone(),
            skills: m.skills, agents: m.agents, hooks: m.hooks,
            commands: m.commands, mcp_servers: m.mcp_servers,
        }).collect()
    }

    pub async fn get_plugin(&self, name: &str) -> Option<PluginStatus> {
        let meta = self.plugin_meta.read().await;
        meta.get(name).map(|m| PluginStatus {
            name: m.name.clone(), enabled: m.enabled, source: m.source_dir.clone(),
            skills: m.skills, agents: m.agents, hooks: m.hooks,
            commands: m.commands, mcp_servers: m.mcp_servers,
        })
    }

    pub async fn disable(&self, name: &str) -> bool {
        let mut meta = self.plugin_meta.write().await;
        if let Some(m) = meta.get_mut(name) {
            m.enabled = false;
            let prefix = format!("{name}:");
            let skills = self.skill_registry.list().await;
            for skill in skills {
                if skill.name.starts_with(&prefix) {
                    self.skill_registry.remove(&skill.name).await;
                }
            }
            info!(plugin = name, "plugin disabled");
            true
        } else {
            false
        }
    }

    pub async fn enable(&self, name: &str) -> bool {
        let mut meta = self.plugin_meta.write().await;
        if let Some(m) = meta.get_mut(name) {
            m.enabled = true;
            drop(meta);
            self.scanner.scan().await;
            self.rebuild_meta().await;
            info!(plugin = name, "plugin enabled");
            true
        } else {
            false
        }
    }

    async fn rebuild_meta(&self) {
        let mut meta = self.plugin_meta.write().await;
        meta.clear();

        let skills = self.skill_registry.list().await;
        let agents = self.agent_registry.list().await;
        let commands = self.command_registry.list().await;

        let mut counts: HashMap<String, PluginMeta> = HashMap::new();

        for skill in &skills {
            let plugin_name = skill.name.split_once(':').map(|(p, _)| p).unwrap_or(&skill.name);
            let entry = counts.entry(plugin_name.to_string()).or_insert_with(|| PluginMeta {
                name: plugin_name.to_string(), source_dir: skill.source_dir.display().to_string(),
                enabled: true, skills: 0, agents: 0, hooks: 0, commands: 0, mcp_servers: 0,
            });
            entry.skills += 1;
        }

        for agent in &agents {
            let plugin_name = agent.name.split_once(':').map(|(p, _)| p).unwrap_or(&agent.name);
            let entry = counts.entry(plugin_name.to_string()).or_insert_with(|| PluginMeta {
                name: plugin_name.to_string(), source_dir: String::new(),
                enabled: true, skills: 0, agents: 0, hooks: 0, commands: 0, mcp_servers: 0,
            });
            entry.agents += 1;
        }

        for cmd in &commands {
            let entry = counts.entry(cmd.plugin_name.clone()).or_insert_with(|| PluginMeta {
                name: cmd.plugin_name.clone(), source_dir: String::new(),
                enabled: true, skills: 0, agents: 0, hooks: 0, commands: 0, mcp_servers: 0,
            });
            entry.commands += 1;
        }

        *meta = counts;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plugin_status_empty() {
        let scanner = Arc::new(PluginScanner::new(
            vec![], Arc::new(PluginRegistry::new()), Arc::new(SkillRegistry::new()),
            Arc::new(AgentRegistry::new()), Arc::new(CommandRegistry::new()), Arc::new(HookRegistry::new()),
        ));
        let mgr = PluginManager::new(
            scanner, Arc::new(PluginRegistry::new()), Arc::new(SkillRegistry::new()),
            Arc::new(AgentRegistry::new()), Arc::new(CommandRegistry::new()), Arc::new(HookRegistry::new()),
        );
        let status = mgr.status().await;
        assert!(status.is_empty());
    }
}
