//! Plugin lifecycle coordinator.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::agent_registry::AgentRegistry;
use crate::command_registry::CommandRegistry;
use crate::hooks::{HookRegistrationRecord, HookRegistry};
use crate::plugin_scanner::{
    PluginDecisionKind, PluginDiscoveryDecision, PluginScanner, UnifiedScanSummary,
};
use crate::skill::SkillRegistry;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginStatus {
    pub name: String,
    pub enabled: bool,
    pub source: String,
    pub kind: String,
    pub skills: usize,
    pub agents: usize,
    pub hooks: usize,
    pub commands: usize,
    pub mcp_servers: usize,
    pub hook_registrations: Vec<HookRegistrationRecord>,
    pub last_decision: String,
    pub last_detail: String,
}

#[derive(Clone)]
pub struct PluginManager {
    scanner: Arc<PluginScanner>,
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
    kind: String,
    enabled: bool,
    skills: usize,
    agents: usize,
    hooks: usize,
    commands: usize,
    mcp_servers: usize,
    hook_registrations: Vec<HookRegistrationRecord>,
    last_decision: String,
    last_detail: String,
}

impl PluginManager {
    pub fn new(
        scanner: Arc<PluginScanner>,
        skill_registry: Arc<SkillRegistry>,
        agent_registry: Arc<AgentRegistry>,
        command_registry: Arc<CommandRegistry>,
        hook_registry: Arc<HookRegistry>,
    ) -> Self {
        Self {
            scanner,
            skill_registry,
            agent_registry,
            command_registry,
            hook_registry,
            plugin_meta: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    pub async fn reload(&self) -> UnifiedScanSummary {
        self.skill_registry.clear().await;
        self.agent_registry.clear().await;
        self.command_registry.clear().await;
        self.hook_registry.clear().await;
        let summary = self.scanner.scan().await;
        self.rebuild_meta().await;
        summary
    }

    pub async fn status(&self) -> Vec<PluginStatus> {
        let meta = self.plugin_meta.read().await;
        meta.values().map(plugin_status_from_meta).collect()
    }

    pub async fn get_plugin(&self, name: &str) -> Option<PluginStatus> {
        let meta = self.plugin_meta.read().await;
        meta.get(name).map(plugin_status_from_meta)
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
            let agents = self.agent_registry.list().await;
            for agent in agents {
                if agent.name.starts_with(&prefix) {
                    self.agent_registry.remove(&agent.name).await;
                }
            }
            let commands = self.command_registry.list().await;
            for command in commands {
                if command.plugin_name == name || command.name.starts_with(&prefix) {
                    self.command_registry.clear().await;
                    break;
                }
            }
            let _ = self.hook_registry.total_handlers().await;
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
        let hook_registrations = self.hook_registry.registrations().await;
        let mcp_servers = self.scanner.discovered_mcp_servers().await;
        let discovery_decisions = self.scanner.discovery_decisions().await;

        let mut counts: HashMap<String, PluginMeta> = HashMap::new();

        for skill in &skills {
            let plugin_name = skill
                .name
                .split_once(':')
                .map(|(p, _)| p)
                .unwrap_or(&skill.name);
            let entry = counts
                .entry(plugin_name.to_string())
                .or_insert_with(|| PluginMeta {
                    name: plugin_name.to_string(),
                    source_dir: skill.source_dir.display().to_string(),
                    kind: "claude".to_string(),
                    enabled: true,
                    skills: 0,
                    agents: 0,
                    hooks: 0,
                    commands: 0,
                    mcp_servers: 0,
                    hook_registrations: Vec::new(),
                    last_decision: PluginDecisionKind::Loaded.as_str().to_string(),
                    last_detail: "registered plugin components are active".to_string(),
                });
            entry.skills += 1;
        }

        for agent in &agents {
            let plugin_name = agent
                .name
                .split_once(':')
                .map(|(p, _)| p)
                .unwrap_or(&agent.name);
            let entry = counts
                .entry(plugin_name.to_string())
                .or_insert_with(|| PluginMeta {
                    name: plugin_name.to_string(),
                    source_dir: String::new(),
                    kind: "claude".to_string(),
                    enabled: true,
                    skills: 0,
                    agents: 0,
                    hooks: 0,
                    commands: 0,
                    mcp_servers: 0,
                    hook_registrations: Vec::new(),
                    last_decision: PluginDecisionKind::Loaded.as_str().to_string(),
                    last_detail: "registered plugin components are active".to_string(),
                });
            entry.agents += 1;
        }

        for cmd in &commands {
            let entry = counts
                .entry(cmd.plugin_name.clone())
                .or_insert_with(|| PluginMeta {
                    name: cmd.plugin_name.clone(),
                    source_dir: String::new(),
                    kind: "claude".to_string(),
                    enabled: true,
                    skills: 0,
                    agents: 0,
                    hooks: 0,
                    commands: 0,
                    mcp_servers: 0,
                    hook_registrations: Vec::new(),
                    last_decision: PluginDecisionKind::Loaded.as_str().to_string(),
                    last_detail: "registered plugin components are active".to_string(),
                });
            entry.commands += 1;
        }

        for registration in &hook_registrations {
            let entry = counts
                .entry(registration.plugin.clone())
                .or_insert_with(|| PluginMeta {
                    name: registration.plugin.clone(),
                    source_dir: String::new(),
                    kind: "claude".to_string(),
                    enabled: true,
                    skills: 0,
                    agents: 0,
                    hooks: 0,
                    commands: 0,
                    mcp_servers: 0,
                    hook_registrations: Vec::new(),
                    last_decision: PluginDecisionKind::Loaded.as_str().to_string(),
                    last_detail: "registered plugin components are active".to_string(),
                });
            entry.hooks += 1;
            entry.hook_registrations.push(registration.clone());
        }

        for server in &mcp_servers {
            let entry = counts
                .entry(server.plugin_name.clone())
                .or_insert_with(|| PluginMeta {
                    name: server.plugin_name.clone(),
                    source_dir: String::new(),
                    kind: "claude".to_string(),
                    enabled: true,
                    skills: 0,
                    agents: 0,
                    hooks: 0,
                    commands: 0,
                    mcp_servers: 0,
                    hook_registrations: Vec::new(),
                    last_decision: PluginDecisionKind::Loaded.as_str().to_string(),
                    last_detail: "registered plugin components are active".to_string(),
                });
            entry.mcp_servers += 1;
        }

        for decision in discovery_decisions {
            let entry = counts
                .entry(decision.plugin_name.clone())
                .or_insert_with(|| plugin_meta_from_decision(&decision));
            entry.source_dir = decision.source.clone();
            entry.kind = decision.kind.as_str().to_string();
            entry.last_decision = decision.decision.as_str().to_string();
            entry.last_detail = decision.detail.clone();
            entry.enabled = matches!(decision.decision, PluginDecisionKind::Loaded);
        }

        let mut ordered: Vec<_> = counts.into_iter().collect();
        ordered.sort_by(|a, b| a.0.cmp(&b.0));
        *meta = ordered.into_iter().collect();
    }
}

fn plugin_status_from_meta(m: &PluginMeta) -> PluginStatus {
    PluginStatus {
        name: m.name.clone(),
        enabled: m.enabled,
        source: m.source_dir.clone(),
        kind: m.kind.clone(),
        skills: m.skills,
        agents: m.agents,
        hooks: m.hooks,
        commands: m.commands,
        mcp_servers: m.mcp_servers,
        hook_registrations: m.hook_registrations.clone(),
        last_decision: m.last_decision.clone(),
        last_detail: m.last_detail.clone(),
    }
}

fn plugin_meta_from_decision(decision: &PluginDiscoveryDecision) -> PluginMeta {
    PluginMeta {
        name: decision.plugin_name.clone(),
        source_dir: decision.source.clone(),
        kind: decision.kind.as_str().to_string(),
        enabled: matches!(decision.decision, PluginDecisionKind::Loaded),
        skills: 0,
        agents: 0,
        hooks: 0,
        commands: 0,
        mcp_servers: 0,
        hook_registrations: Vec::new(),
        last_decision: decision.decision.as_str().to_string(),
        last_detail: decision.detail.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::{HookEvent, HookHandler};
    use crate::plugin::PluginRegistry;

    #[tokio::test]
    async fn plugin_status_empty() {
        let scanner = Arc::new(PluginScanner::new(
            &rune_config::PluginsConfig {
                scan_dirs: vec![],
                ..Default::default()
            },
            Arc::new(PluginRegistry::new()),
            Arc::new(SkillRegistry::new()),
            Arc::new(AgentRegistry::new()),
            Arc::new(CommandRegistry::new()),
            Arc::new(HookRegistry::new()),
        ));
        let mgr = PluginManager::new(
            scanner,
            Arc::new(SkillRegistry::new()),
            Arc::new(AgentRegistry::new()),
            Arc::new(CommandRegistry::new()),
            Arc::new(HookRegistry::new()),
        );
        let status = mgr.status().await;
        assert!(status.is_empty());
    }

    struct NamedHandler(&'static str);

    #[async_trait::async_trait]
    impl HookHandler for NamedHandler {
        async fn handle(
            &self,
            _event: &HookEvent,
            _context: &mut serde_json::Value,
        ) -> Result<(), String> {
            Ok(())
        }

        fn plugin_name(&self) -> &str {
            self.0
        }
    }

    #[tokio::test]
    async fn plugin_status_includes_hook_registration_metadata() {
        let skill_registry = Arc::new(SkillRegistry::new());
        let agent_registry = Arc::new(AgentRegistry::new());
        let command_registry = Arc::new(CommandRegistry::new());
        let hook_registry = Arc::new(HookRegistry::new());
        hook_registry
            .register(HookEvent::PreToolCall, Box::new(NamedHandler("alpha")))
            .await;
        hook_registry
            .register(HookEvent::PostToolCall, Box::new(NamedHandler("alpha")))
            .await;
        hook_registry
            .register(HookEvent::PreToolCall, Box::new(NamedHandler("beta")))
            .await;

        let scanner = Arc::new(PluginScanner::new(
            &rune_config::PluginsConfig {
                scan_dirs: vec![],
                ..Default::default()
            },
            Arc::new(PluginRegistry::new()),
            skill_registry.clone(),
            agent_registry.clone(),
            command_registry.clone(),
            hook_registry.clone(),
        ));
        let mgr = PluginManager::new(
            scanner,
            skill_registry,
            agent_registry,
            command_registry,
            hook_registry,
        );

        mgr.rebuild_meta().await;
        let mut status = mgr.status().await;
        status.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(status.len(), 2);
        assert_eq!(status[0].name, "alpha");
        assert_eq!(status[0].hooks, 2);
        assert_eq!(status[0].hook_registrations.len(), 2);
        assert_eq!(status[0].hook_registrations[0].event, "pre_tool_call");
        assert_eq!(status[0].hook_registrations[0].order, 0);
        assert_eq!(status[0].hook_registrations[1].event, "post_tool_call");
        assert_eq!(status[0].hook_registrations[1].order, 0);

        assert_eq!(status[1].name, "beta");
        assert_eq!(status[1].hooks, 1);
        assert_eq!(status[1].hook_registrations.len(), 1);
        assert_eq!(status[1].hook_registrations[0].event, "pre_tool_call");
        assert_eq!(status[1].hook_registrations[0].order, 1);
    }
}
