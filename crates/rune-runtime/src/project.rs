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
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const REGISTRY_FILE: &str = ".project-registry.json";
const AGENTS_DIR: &str = "agents";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProjectConfig {
    pub name: String,
    pub repo_url: String,
    pub repo_path: PathBuf,
    pub default_branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default = "registered_at_now")]
    pub registered_at: u64,
    pub build_command: String,
    pub test_command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lint_command: Option<String>,
}

fn registered_at_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectStatusSummary {
    pub name: String,
    pub repo_path: PathBuf,
    pub agent_dir: PathBuf,
    pub orchestrator_active: bool,
    pub is_active: bool,
    pub default_branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    pub registered_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRegistry {
    projects: HashMap<String, ProjectConfig>,
    active_project: Option<String>,
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
            active_project: None,
        }
    }

    pub fn registry_path(workspace: &Path) -> PathBuf {
        workspace.join(REGISTRY_FILE)
    }

    pub fn agent_dir(workspace: &Path, project_name: &str) -> PathBuf {
        workspace.join(AGENTS_DIR).join(project_name)
    }

    pub fn load(workspace: &Path) -> Result<Self, io::Error> {
        let path = Self::registry_path(workspace);
        if !path.exists() {
            return Ok(Self::new());
        }
        let data = std::fs::read_to_string(&path)?;
        serde_json::from_str(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn save(&self, workspace: &Path) -> Result<(), io::Error> {
        let path = Self::registry_path(workspace);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, data)
    }

    pub fn register(&mut self, config: ProjectConfig) {
        self.projects.insert(config.name.clone(), config);
    }

    pub fn get(&self, name: &str) -> Option<&ProjectConfig> {
        self.projects.get(name)
    }

    pub fn list(&self) -> Vec<&ProjectConfig> {
        let mut projects: Vec<_> = self.projects.values().collect();
        projects.sort_by(|a, b| a.name.cmp(&b.name));
        projects
    }

    pub fn active_project(&self) -> Option<&str> {
        self.active_project.as_deref()
    }

    pub fn switch_active(&mut self, name: &str) -> Result<(), io::Error> {
        if !self.projects.contains_key(name) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("project '{name}' is not registered"),
            ));
        }
        self.active_project = Some(name.to_string());
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Option<ProjectConfig> {
        let removed = self.projects.remove(name);
        if self.active_project.as_deref() == Some(name) {
            self.active_project = None;
        }
        removed
    }

    pub fn len(&self) -> usize {
        self.projects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    pub fn onboard(
        &mut self,
        name: &str,
        repo_url: &str,
        repo_path: PathBuf,
        workspace: &Path,
        default_branch: Option<&str>,
        default_model: Option<String>,
    ) -> Result<ProjectConfig, io::Error> {
        let config = ProjectConfig {
            name: name.to_string(),
            repo_url: repo_url.to_string(),
            repo_path,
            default_branch: default_branch.unwrap_or("main").to_string(),
            default_model,
            registered_at: registered_at_now(),
            build_command: "cargo build".to_string(),
            test_command: "cargo test".to_string(),
            lint_command: None,
        };

        let agent_dir = Self::agent_dir(workspace, name);
        std::fs::create_dir_all(&agent_dir)?;

        self.register(config.clone());
        if self.active_project.is_none() {
            self.active_project = Some(name.to_string());
        }
        self.save(workspace)?;

        Ok(config)
    }

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
                is_active: self.active_project.as_deref() == Some(cfg.name.as_str()),
                default_branch: cfg.default_branch.clone(),
                default_model: cfg.default_model.clone(),
                registered_at: cfg.registered_at,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config(name: &str) -> ProjectConfig {
        ProjectConfig {
            name: name.to_string(),
            repo_url: format!("https://github.com/org/{name}.git"),
            repo_path: PathBuf::from(format!("/home/user/dev/{name}")),
            default_branch: "main".to_string(),
            default_model: Some("gpt-5.4".to_string()),
            registered_at: 1_700_000_000,
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
        reg.register(sample_config("b"));
        reg.register(sample_config("a"));

        let all = reg.list();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "a");
        assert_eq!(all[1].name, "b");
    }

    #[test]
    fn remove_project() {
        let mut reg = ProjectRegistry::new();
        reg.register(sample_config("x"));
        reg.switch_active("x").unwrap();
        assert!(reg.remove("x").is_some());
        assert!(reg.is_empty());
        assert_eq!(reg.active_project(), None);
        assert!(reg.remove("x").is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();

        let mut reg = ProjectRegistry::new();
        reg.register(sample_config("alpha"));
        reg.register(sample_config("beta"));
        reg.switch_active("beta").unwrap();
        reg.save(ws).unwrap();

        let loaded = ProjectRegistry::load(ws).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get("alpha"), reg.get("alpha"));
        assert_eq!(loaded.get("beta"), reg.get("beta"));
        assert_eq!(loaded.active_project(), Some("beta"));
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
                None,
                None,
            )
            .unwrap();

        assert_eq!(cfg.name, "rune");
        assert_eq!(cfg.default_branch, "main");
        assert_eq!(reg.active_project(), Some("rune"));

        let agent_dir = ProjectRegistry::agent_dir(ws, "rune");
        assert!(agent_dir.exists());
        assert!(agent_dir.is_dir());

        let loaded = ProjectRegistry::load(ws).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.get("rune").unwrap().name, "rune");
        assert_eq!(loaded.active_project(), Some("rune"));
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
        reg.switch_active("b").unwrap();

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

        let selected = summaries.iter().find(|s| s.is_active).unwrap();
        assert_eq!(selected.name, "b");

        let a_summary = summaries.iter().find(|s| s.name == "a").unwrap();
        assert_eq!(a_summary.agent_dir, ws.join("agents/a"));
    }

    #[test]
    fn switch_active_project_requires_registered_name() {
        let mut reg = ProjectRegistry::new();
        reg.register(sample_config("a"));
        reg.switch_active("a").unwrap();
        assert_eq!(reg.active_project(), Some("a"));
        assert!(reg.switch_active("missing").is_err());
    }
}
