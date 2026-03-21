//! Cross-project isolation: one orchestrator per project.
//!
//! Each project has its own configuration, worktree root, and build commands.
//! Orchestrators are scoped to their project and cannot access other projects'
//! repos or state. The Main Agent is the only entity with cross-project visibility.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Project configuration
// ---------------------------------------------------------------------------

/// Build/test/lint configuration for a project.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    pub repo_url: String,
    pub worktree_root: PathBuf,
    pub default_branch: String,
    pub build_command: String,
    pub test_command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lint_command: Option<String>,
}

/// Summary of a project's current status (for the Main Agent's aggregated view).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectStatusSummary {
    pub name: String,
    pub worktree_root: PathBuf,
    pub orchestrator_active: bool,
}

// ---------------------------------------------------------------------------
// Project registry
// ---------------------------------------------------------------------------

/// Manages multiple projects, each with its own configuration.
///
/// The registry enforces isolation: callers must look up a project by name and
/// operate on its config. Cross-project access is only available through the
/// aggregated status view used by the Main Agent.
#[derive(Debug, Default)]
pub struct ProjectRegistry {
    projects: HashMap<String, ProjectConfig>,
}

impl ProjectRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a project. Replaces any existing entry with the same name.
    pub fn register(&mut self, config: ProjectConfig) {
        self.projects.insert(config.name.clone(), config);
    }

    /// Look up a project by name.
    pub fn get(&self, name: &str) -> Option<&ProjectConfig> {
        self.projects.get(name)
    }

    /// List all registered projects.
    pub fn list(&self) -> Vec<&ProjectConfig> {
        self.projects.values().collect()
    }

    /// Remove a project from the registry.
    pub fn remove(&mut self, name: &str) -> Option<ProjectConfig> {
        self.projects.remove(name)
    }

    /// Number of registered projects.
    pub fn len(&self) -> usize {
        self.projects.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    /// Onboard a new project: create a config entry with sensible defaults.
    ///
    /// The caller is responsible for the actual clone/worktree creation; this
    /// method only records the project metadata. Discovery of the build system
    /// and README parsing are left to the orchestrator's first turn.
    pub fn onboard(
        &mut self,
        name: &str,
        repo_url: &str,
        worktree_root: PathBuf,
    ) -> ProjectConfig {
        let config = ProjectConfig {
            name: name.to_string(),
            repo_url: repo_url.to_string(),
            worktree_root,
            default_branch: "main".to_string(),
            build_command: "cargo build".to_string(),
            test_command: "cargo test".to_string(),
            lint_command: None,
        };
        self.register(config.clone());
        config
    }

    /// Aggregated status for the Main Agent — shows every project with a flag
    /// indicating whether its orchestrator is active.
    ///
    /// `active_projects` is a set of project names that currently have a
    /// running/idle orchestrator (sourced from the `OrchestratorRegistry`).
    pub fn all_status(&self, active_projects: &[&str]) -> Vec<ProjectStatusSummary> {
        self.projects
            .values()
            .map(|cfg| ProjectStatusSummary {
                name: cfg.name.clone(),
                worktree_root: cfg.worktree_root.clone(),
                orchestrator_active: active_projects.contains(&cfg.name.as_str()),
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_config(name: &str) -> ProjectConfig {
        ProjectConfig {
            name: name.to_string(),
            repo_url: format!("https://github.com/org/{name}.git"),
            worktree_root: PathBuf::from(format!("/work/{name}")),
            default_branch: "main".to_string(),
            build_command: "cargo build".to_string(),
            test_command: "cargo test".to_string(),
            lint_command: Some("cargo clippy".to_string()),
        }
    }

    #[test]
    fn register_and_get() {
        let mut reg = ProjectRegistry::new();
        assert!(reg.is_empty());

        reg.register(sample_config("alpha"));
        assert_eq!(reg.len(), 1);

        let cfg = reg.get("alpha").unwrap();
        assert_eq!(cfg.repo_url, "https://github.com/org/alpha.git");
    }

    #[test]
    fn register_replaces_existing() {
        let mut reg = ProjectRegistry::new();
        reg.register(sample_config("alpha"));

        let mut replacement = sample_config("alpha");
        replacement.build_command = "make".to_string();
        reg.register(replacement);

        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("alpha").unwrap().build_command, "make");
    }

    #[test]
    fn list_returns_all() {
        let mut reg = ProjectRegistry::new();
        reg.register(sample_config("a"));
        reg.register(sample_config("b"));

        let all = reg.list();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn remove_project() {
        let mut reg = ProjectRegistry::new();
        reg.register(sample_config("x"));
        assert!(reg.remove("x").is_some());
        assert!(reg.is_empty());
        assert!(reg.remove("x").is_none());
    }

    #[test]
    fn onboard_creates_defaults() {
        let mut reg = ProjectRegistry::new();
        let cfg = reg.onboard("beta", "https://github.com/org/beta.git", PathBuf::from("/w/beta"));

        assert_eq!(cfg.name, "beta");
        assert_eq!(cfg.default_branch, "main");
        assert_eq!(cfg.build_command, "cargo build");
        assert!(reg.get("beta").is_some());
    }

    #[test]
    fn all_status_reports_active_flag() {
        let mut reg = ProjectRegistry::new();
        reg.register(sample_config("a"));
        reg.register(sample_config("b"));
        reg.register(sample_config("c"));

        let summaries = reg.all_status(&["a", "c"]);
        assert_eq!(summaries.len(), 3);

        let active_names: Vec<_> = summaries
            .iter()
            .filter(|s| s.orchestrator_active)
            .map(|s| s.name.as_str())
            .collect();

        assert!(active_names.contains(&"a"));
        assert!(active_names.contains(&"c"));
        assert!(!active_names.contains(&"b"));
    }
}
