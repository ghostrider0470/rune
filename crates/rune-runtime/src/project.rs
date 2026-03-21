//! Cross-project isolation: one orchestrator per project.
//!
//! The project registry persists as a JSON file under the Main Agent's
//! workspace:
//!
//! ```text
//! {workspace}/.project-registry.json      — registry of all known projects
//! {workspace}/agents/{project_name}/       — per-orchestrator state directory
//! ```
//!
//! Each orchestrator subagent session gets `workspace_root` set to the
//! project's external repo path (e.g. `~/Development/rune`), while its
//! state/comms files live under the Main Agent's workspace tree.
//!
//! Cross-project isolation is enforced by workspace boundaries: each
//! orchestrator can only see its own project's repo.  The Main Agent is
//! the only entity with cross-project visibility.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// File name for the persisted project registry.
const REGISTRY_FILE: &str = ".project-registry.json";

/// Subdirectory under the workspace where per-orchestrator dirs are created.
const AGENTS_DIR: &str = "agents";

// ---------------------------------------------------------------------------
// Project configuration
// ---------------------------------------------------------------------------

/// Build/test/lint configuration for a project.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProjectConfig {
    /// Short identifier used as the registry key and directory name.
    pub name: String,
    /// Remote URL (e.g. `https://github.com/org/repo.git`).
    pub repo_url: String,
    /// Absolute path to the project's repo on disk (external to the workspace).
    pub repo_path: PathBuf,
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
    pub repo_path: PathBuf,
    pub agent_dir: PathBuf,
    pub orchestrator_active: bool,
}

// ---------------------------------------------------------------------------
// Project registry — persisted to {workspace}/.project-registry.json
// ---------------------------------------------------------------------------

/// Manages multiple projects.  Persisted as JSON under the Main Agent's
/// workspace so it survives restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRegistry {
    projects: HashMap<String, ProjectConfig>,
}

impl Default for ProjectRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectRegistry {
    pub fn new() -> Self {
        Self {
            projects: HashMap::new(),
        }
    }

    // -------------------------------------------------------------------
    // Persistence
    // -------------------------------------------------------------------

    /// Path to the registry file within a workspace.
    pub fn registry_path(workspace: &Path) -> PathBuf {
        workspace.join(REGISTRY_FILE)
    }

    /// Path to a project's orchestrator agent directory within a workspace.
    pub fn agent_dir(workspace: &Path, project_name: &str) -> PathBuf {
        workspace.join(AGENTS_DIR).join(project_name)
    }

    /// Load the registry from `{workspace}/.project-registry.json`.
    /// Returns an empty registry if the file does not exist.
    pub fn load(workspace: &Path) -> Result<Self, io::Error> {
        let path = Self::registry_path(workspace);
        if !path.exists() {
            return Ok(Self::new());
        }
        let data = std::fs::read_to_string(&path)?;
        serde_json::from_str(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Write the registry to `{workspace}/.project-registry.json`.
    pub fn save(&self, workspace: &Path) -> Result<(), io::Error> {
        let path = Self::registry_path(workspace);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, data)
    }

    // -------------------------------------------------------------------
    // CRUD
    // -------------------------------------------------------------------

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

    pub fn len(&self) -> usize {
        self.projects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    // -------------------------------------------------------------------
    // Onboarding
    // -------------------------------------------------------------------

    /// Onboard a new project: register it with sensible defaults and create
    /// the orchestrator agent directory at `{workspace}/agents/{name}/`.
    ///
    /// The caller is responsible for cloning the repo to `repo_path`.
    /// Build-system discovery and README parsing are left to the orchestrator's
    /// first turn.
    pub fn onboard(
        &mut self,
        name: &str,
        repo_url: &str,
        repo_path: PathBuf,
        workspace: &Path,
    ) -> Result<ProjectConfig, io::Error> {
        let config = ProjectConfig {
            name: name.to_string(),
            repo_url: repo_url.to_string(),
            repo_path,
            default_branch: "main".to_string(),
            build_command: "cargo build".to_string(),
            test_command: "cargo test".to_string(),
            lint_command: None,
        };

        // Create orchestrator agent directory.
        let agent_dir = Self::agent_dir(workspace, name);
        std::fs::create_dir_all(&agent_dir)?;

        self.register(config.clone());
        self.save(workspace)?;

        Ok(config)
    }

    // -------------------------------------------------------------------
    // Reporting
    // -------------------------------------------------------------------

    /// Aggregated status for the Main Agent — shows every project with a flag
    /// indicating whether its orchestrator is active.
    ///
    /// `active_projects` comes from `OrchestratorRegistry`.
    pub fn all_status(
        &self,
        workspace: &Path,
        active_projects: &[&str],
    ) -> Vec<ProjectStatusSummary> {
        self.projects
            .values()
            .map(|cfg| ProjectStatusSummary {
                name: cfg.name.clone(),
                repo_path: cfg.repo_path.clone(),
                agent_dir: Self::agent_dir(workspace, &cfg.name),
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

    fn sample_config(name: &str) -> ProjectConfig {
        ProjectConfig {
            name: name.to_string(),
            repo_url: format!("https://github.com/org/{name}.git"),
            repo_path: PathBuf::from(format!("/home/user/dev/{name}")),
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
        assert_eq!(cfg.repo_path, PathBuf::from("/home/user/dev/alpha"));
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
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();

        let mut reg = ProjectRegistry::new();
        reg.register(sample_config("alpha"));
        reg.register(sample_config("beta"));
        reg.save(ws).unwrap();

        let loaded = ProjectRegistry::load(ws).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get("alpha"), reg.get("alpha"));
        assert_eq!(loaded.get("beta"), reg.get("beta"));
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = ProjectRegistry::load(dir.path()).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn onboard_creates_agent_dir_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();

        let mut reg = ProjectRegistry::new();
        let cfg = reg
            .onboard(
                "rune",
                "https://github.com/org/rune.git",
                PathBuf::from("/home/user/dev/rune"),
                ws,
            )
            .unwrap();

        assert_eq!(cfg.name, "rune");
        assert_eq!(cfg.default_branch, "main");

        // Agent directory created.
        let agent_dir = ProjectRegistry::agent_dir(ws, "rune");
        assert!(agent_dir.exists());
        assert!(agent_dir.is_dir());

        // Registry persisted.
        let loaded = ProjectRegistry::load(ws).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.get("rune").unwrap().name, "rune");
    }

    #[test]
    fn agent_dir_path() {
        let ws = Path::new("/home/hamza/.rune/workspace");
        assert_eq!(
            ProjectRegistry::agent_dir(ws, "rune"),
            PathBuf::from("/home/hamza/.rune/workspace/agents/rune"),
        );
    }

    #[test]
    fn registry_path() {
        let ws = Path::new("/home/hamza/.rune/workspace");
        assert_eq!(
            ProjectRegistry::registry_path(ws),
            PathBuf::from("/home/hamza/.rune/workspace/.project-registry.json"),
        );
    }

    #[test]
    fn all_status_reports_active_flag() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();

        let mut reg = ProjectRegistry::new();
        reg.register(sample_config("a"));
        reg.register(sample_config("b"));
        reg.register(sample_config("c"));

        let summaries = reg.all_status(ws, &["a", "c"]);
        assert_eq!(summaries.len(), 3);

        let active_names: Vec<_> = summaries
            .iter()
            .filter(|s| s.orchestrator_active)
            .map(|s| s.name.as_str())
            .collect();

        assert!(active_names.contains(&"a"));
        assert!(active_names.contains(&"c"));
        assert!(!active_names.contains(&"b"));

        // agent_dir paths are correct.
        let a_summary = summaries.iter().find(|s| s.name == "a").unwrap();
        assert_eq!(a_summary.agent_dir, ws.join("agents/a"));
    }
}
