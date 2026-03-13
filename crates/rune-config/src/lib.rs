#![doc = "Layered application configuration for Rune."]

use std::path::PathBuf;

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Top-level application configuration resolved from defaults, files, and environment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    pub gateway: GatewayConfig,
    pub database: DatabaseConfig,
    pub models: ModelsConfig,
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub agents: AgentsConfig,
    pub memory: MemoryConfig,
    pub media: MediaConfig,
    pub logging: LoggingConfig,
    pub paths: PathsConfig,
}

impl AppConfig {
    /// Load configuration from defaults, optional TOML file, and environment variables.
    pub fn load(config_file: Option<impl AsRef<std::path::Path>>) -> Result<Self, ConfigError> {
        let mut figment = Figment::from(Serialized::defaults(Self::default()));

        if let Some(path) = config_file {
            figment = figment.merge(Toml::file(path));
        }

        figment = figment.merge(Env::prefixed("RUNE_").split("__"));

        figment
            .extract()
            .map_err(|e| ConfigError::Load(Box::new(e)))
    }

    /// Apply a fully-populated override on top of the current config.
    #[must_use]
    pub fn with_override(self, override_config: AppConfig) -> Self {
        override_config
    }

    /// Validate that required persistent paths exist and are writable.
    ///
    /// Per DOCKER-DEPLOYMENT.md §9.1 the runtime must fail fast on
    /// missing or unwritable parity-critical paths.
    pub fn validate_paths(&self) -> Result<(), Vec<ConfigError>> {
        let required = [
            ("db_dir", &self.paths.db_dir),
            ("sessions_dir", &self.paths.sessions_dir),
            ("memory_dir", &self.paths.memory_dir),
            ("media_dir", &self.paths.media_dir),
            ("logs_dir", &self.paths.logs_dir),
        ];
        let mut errors = Vec::new();
        for (name, path) in &required {
            if !path.exists() {
                errors.push(ConfigError::PathValidation {
                    path: path.display().to_string(),
                    reason: format!("{name} does not exist"),
                });
            } else if path
                .metadata()
                .map(|m| m.permissions().readonly())
                .unwrap_or(true)
            {
                errors.push(ConfigError::PathValidation {
                    path: path.display().to_string(),
                    reason: format!("{name} is not writable"),
                });
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Gateway listener and authentication settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub host: String,
    pub port: u16,
    pub auth_token: Option<String>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8787,
            auth_token: None,
        }
    }
}

/// Database connectivity and migration settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub database_url: Option<String>,
    pub max_connections: u32,
    pub run_migrations: bool,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            database_url: None,
            max_connections: 10,
            run_migrations: true,
        }
    }
}

/// Provider inventory and routing aliases.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelsConfig {
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub providers: Vec<ModelProviderConfig>,
}

impl ModelsConfig {
    /// Return every configured model in canonical `provider/model` form.
    #[must_use]
    pub fn inventory(&self) -> Vec<ModelInventoryEntry<'_>> {
        let mut entries = Vec::new();

        for provider in &self.providers {
            for model in &provider.models {
                entries.push(ModelInventoryEntry {
                    provider_name: &provider.name,
                    provider_kind: provider.kind.as_str(),
                    raw_model: model.id(),
                });
            }
        }

        entries
    }

    /// Return all canonical model ids in sorted order.
    #[must_use]
    pub fn model_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.inventory().into_iter().map(|model| model.model_id()).collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Resolve a configured model reference to its provider and raw model id.
    pub fn resolve_model<'a>(
        &'a self,
        model_ref: &'a str,
    ) -> Result<ResolvedModel<'a>, ModelResolutionError> {
        if self.providers.is_empty() {
            return Err(ModelResolutionError::NoProvidersConfigured);
        }

        if let Some((provider_name, raw_model)) = model_ref.split_once('/') {
            let provider = self
                .providers
                .iter()
                .find(|provider| provider.name == provider_name)
                .ok_or_else(|| ModelResolutionError::UnknownProvider {
                    provider: provider_name.to_string(),
                })?;

            if !provider.models.is_empty() && !provider.supports_model(raw_model) {
                return Err(ModelResolutionError::UnknownModel {
                    model: model_ref.to_string(),
                });
            }

            return Ok(ResolvedModel {
                model_ref,
                provider,
                raw_model,
            });
        }

        let matches = self
            .providers
            .iter()
            .filter(|provider| provider.supports_model(model_ref))
            .collect::<Vec<_>>();

        if matches.len() == 1 {
            let provider = matches[0];
            return Ok(ResolvedModel {
                model_ref,
                provider,
                raw_model: model_ref,
            });
        }

        if matches.len() > 1 {
            return Err(ModelResolutionError::AmbiguousModel {
                model: model_ref.to_string(),
                providers: matches
                    .into_iter()
                    .map(|provider| provider.name.clone())
                    .collect(),
            });
        }

        if self.providers.len() == 1 {
            return Ok(ResolvedModel {
                model_ref,
                provider: &self.providers[0],
                raw_model: model_ref,
            });
        }

        Err(ModelResolutionError::UnknownModel {
            model: model_ref.to_string(),
        })
    }
}

/// A single configured model-provider target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProviderConfig {
    /// Display/routing name for this provider.
    #[serde(alias = "provider_name")]
    pub name: String,
    /// Provider kind: "anthropic", "openai", "azure-openai".
    #[serde(default)]
    pub kind: String,
    /// API endpoint / base URL.
    #[serde(alias = "endpoint")]
    pub base_url: String,
    /// Direct API key (takes precedence over api_key_env).
    #[serde(default)]
    pub api_key: Option<String>,
    pub deployment_name: Option<String>,
    pub api_version: Option<String>,
    /// Environment variable holding the API key.
    pub api_key_env: Option<String>,
    pub model_alias: Option<String>,
    /// Inventory of models available through this provider.
    #[serde(default)]
    pub models: Vec<ConfiguredModel>,
}

impl ModelProviderConfig {
    #[must_use]
    pub fn supports_model(&self, raw_model: &str) -> bool {
        self.models.is_empty() || self.models.iter().any(|model| model.id() == raw_model)
    }
}

/// Configured model entry for a provider inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfiguredModel {
    Id(String),
    Detailed(ConfiguredModelDetail),
}

impl ConfiguredModel {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Id(id) => id,
            Self::Detailed(detail) => &detail.id,
        }
    }
}

/// Rich configured model entry. Extra metadata is preserved for future use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfiguredModelDetail {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

/// Canonical inventory entry for a configured model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInventoryEntry<'a> {
    pub provider_name: &'a str,
    pub provider_kind: &'a str,
    pub raw_model: &'a str,
}

impl ModelInventoryEntry<'_> {
    #[must_use]
    pub fn model_id(&self) -> String {
        format!("{}/{}", self.provider_name, self.raw_model)
    }
}

/// A resolved model reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedModel<'a> {
    pub model_ref: &'a str,
    pub provider: &'a ModelProviderConfig,
    pub raw_model: &'a str,
}

impl ResolvedModel<'_> {
    #[must_use]
    pub fn canonical_model_id(&self) -> String {
        format!("{}/{}", self.provider.name, self.raw_model)
    }
}

/// Model inventory and routing errors.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ModelResolutionError {
    #[error("no model providers configured")]
    NoProvidersConfigured,
    #[error("unknown model provider '{provider}'")]
    UnknownProvider { provider: String },
    #[error("unknown model '{model}'")]
    UnknownModel { model: String },
    #[error("ambiguous model '{model}' across providers: {providers:?}")]
    AmbiguousModel {
        model: String,
        providers: Vec<String>,
    },
}

/// Channel adapter inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    pub enabled: Vec<String>,
    /// Telegram bot token for the Bot API.
    #[serde(default)]
    pub telegram_token: Option<String>,
}

/// Memory indexing and retrieval settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub semantic_search_enabled: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            semantic_search_enabled: true,
        }
    }
}

/// Media pipeline feature flags and limits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaConfig {
    pub transcription_enabled: bool,
    pub tts_enabled: bool,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            transcription_enabled: true,
            tts_enabled: true,
        }
    }
}

/// Logging behavior.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub json: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            json: true,
        }
    }
}

/// Canonical persistent path layout for Docker-first and host installs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathsConfig {
    pub db_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub memory_dir: PathBuf,
    pub media_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub backups_dir: PathBuf,
    pub config_dir: PathBuf,
    pub secrets_dir: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            db_dir: PathBuf::from("/data/db"),
            sessions_dir: PathBuf::from("/data/sessions"),
            memory_dir: PathBuf::from("/data/memory"),
            media_dir: PathBuf::from("/data/media"),
            skills_dir: PathBuf::from("/data/skills"),
            logs_dir: PathBuf::from("/data/logs"),
            backups_dir: PathBuf::from("/data/backups"),
            config_dir: PathBuf::from("/config"),
            secrets_dir: PathBuf::from("/secrets"),
        }
    }
}

/// Multi-agent configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentsConfig {
    /// Defaults applied to every agent unless overridden.
    #[serde(default)]
    pub defaults: AgentDefaults,
    /// Named agent definitions.
    #[serde(default)]
    pub list: Vec<AgentConfig>,
}

impl AgentsConfig {
    /// Return the agent marked `default = true`, or the first agent if none is marked.
    pub fn default_agent(&self) -> Option<&AgentConfig> {
        self.list
            .iter()
            .find(|a| a.default.unwrap_or(false))
            .or_else(|| self.list.first())
    }

    /// Find an agent by id.
    pub fn find(&self, id: &str) -> Option<&AgentConfig> {
        self.list.iter().find(|a| a.id == id)
    }

    /// Resolve the effective model for an agent (agent override → defaults → None).
    pub fn effective_model<'a>(&'a self, agent: &'a AgentConfig) -> Option<&'a str> {
        agent
            .model
            .as_ref()
            .map(AgentModelSelection::primary)
            .or_else(|| self.defaults.model.as_ref().map(AgentModelSelection::primary))
    }

    /// Resolve the effective workspace for an agent.
    pub fn effective_workspace<'a>(&'a self, agent: &'a AgentConfig) -> Option<&'a str> {
        agent
            .workspace
            .as_deref()
            .or(self.defaults.workspace.as_deref())
    }

    /// Resolve the effective system prompt for an agent.
    pub fn effective_system_prompt<'a>(&'a self, agent: &'a AgentConfig) -> Option<&'a str> {
        agent
            .system_prompt
            .as_deref()
            .or(self.defaults.system_prompt.as_deref())
    }
}

/// Defaults that apply to all agents unless individually overridden.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentDefaults {
    pub model: Option<AgentModelSelection>,
    pub workspace: Option<String>,
    pub system_prompt: Option<String>,
}

/// A single agent definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Unique identifier for this agent.
    pub id: String,
    /// Whether this is the default agent for direct messages.
    #[serde(default)]
    pub default: Option<bool>,
    /// Model override (falls back to agents.defaults.model).
    pub model: Option<AgentModelSelection>,
    /// Workspace path override.
    pub workspace: Option<String>,
    /// System prompt override.
    pub system_prompt: Option<String>,
}

/// Agent model selection, compatible with both plain strings and OpenClaw-style
/// structured selections.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AgentModelSelection {
    Id(String),
    Structured(StructuredAgentModelSelection),
}

impl AgentModelSelection {
    #[must_use]
    pub fn primary(&self) -> &str {
        match self {
            Self::Id(id) => id,
            Self::Structured(model) => &model.primary,
        }
    }
}

/// Structured agent model selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredAgentModelSelection {
    pub primary: String,
}

/// Configuration loading and validation failures.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to load configuration: {0}")]
    Load(#[from] Box<figment::Error>),
    #[error("path validation failed: {path} — {reason}")]
    PathValidation { path: String, reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{LazyLock, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn temp_config_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("rune-config-{name}-{nanos}.toml"))
    }

    #[test]
    fn default_config_uses_docker_first_paths() {
        let config = AppConfig::default();
        assert_eq!(config.gateway.host, "0.0.0.0");
        assert_eq!(config.gateway.port, 8787);
        assert_eq!(config.paths.db_dir, PathBuf::from("/data/db"));
        assert_eq!(config.paths.config_dir, PathBuf::from("/config"));
        assert!(config.memory.semantic_search_enabled);
    }

    #[test]
    fn file_values_override_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("RUNE_GATEWAY__PORT");
        }

        let path = temp_config_path("file-override");
        fs::write(
            &path,
            r#"
[gateway]
host = "127.0.0.1"
port = 9999

[database]
max_connections = 42
run_migrations = false
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(config.gateway.host, "127.0.0.1");
        assert_eq!(config.gateway.port, 9999);
        assert_eq!(config.database.max_connections, 42);
        assert!(!config.database.run_migrations);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn environment_values_override_file_values() {
        let _guard = ENV_LOCK.lock().unwrap();

        let path = temp_config_path("env-override");
        fs::write(
            &path,
            r#"
[gateway]
port = 8787
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_GATEWAY__PORT", "9090");
        }
        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(config.gateway.port, 9090);
        unsafe {
            std::env::remove_var("RUNE_GATEWAY__PORT");
        }

        let _ = fs::remove_file(path);
    }

    #[test]
    fn validate_paths_reports_missing_dirs() {
        let mut config = AppConfig::default();
        config.paths.db_dir = PathBuf::from("/nonexistent/rune-test-path");
        let errors = config.validate_paths().unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors[0].to_string().contains("does not exist"));
    }

    #[test]
    fn validate_paths_passes_for_existing_writable_dirs() {
        let tmp = std::env::temp_dir();
        let mut config = AppConfig::default();
        config.paths.db_dir = tmp.clone();
        config.paths.sessions_dir = tmp.clone();
        config.paths.memory_dir = tmp.clone();
        config.paths.media_dir = tmp.clone();
        config.paths.logs_dir = tmp;
        assert!(config.validate_paths().is_ok());
    }

    #[test]
    fn invalid_config_returns_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("RUNE_GATEWAY__PORT");
        }

        let path = temp_config_path("invalid");
        fs::write(
            &path,
            r#"
[gateway]
port = "not-a-number"
"#,
        )
        .unwrap();

        let err = AppConfig::load(Some(&path)).unwrap_err();
        assert!(err.to_string().contains("failed to load configuration"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn model_inventory_lists_provider_model_ids() {
        let config = ModelsConfig {
            default_model: Some("oc-01-openai/gpt-5.4".into()),
            providers: vec![
                ModelProviderConfig {
                    name: "oc-01-openai".into(),
                    kind: "openai".into(),
                    base_url: "https://example.test/openai/v1".into(),
                    api_key: None,
                    deployment_name: None,
                    api_version: None,
                    api_key_env: Some("OPENAI_API_KEY".into()),
                    model_alias: None,
                    models: vec![
                        ConfiguredModel::Id("gpt-5.4".into()),
                        ConfiguredModel::Id("gpt-5.4-pro".into()),
                    ],
                },
                ModelProviderConfig {
                    name: "oc-01-anthropic".into(),
                    kind: "anthropic".into(),
                    base_url: "https://example.test/anthropic".into(),
                    api_key: None,
                    deployment_name: None,
                    api_version: None,
                    api_key_env: Some("ANTHROPIC_API_KEY".into()),
                    model_alias: None,
                    models: vec![ConfiguredModel::Id("claude-opus-4-6".into())],
                },
            ],
        };

        assert_eq!(
            config.model_ids(),
            vec![
                "oc-01-anthropic/claude-opus-4-6".to_string(),
                "oc-01-openai/gpt-5.4".to_string(),
                "oc-01-openai/gpt-5.4-pro".to_string(),
            ]
        );
    }

    #[test]
    fn model_resolution_supports_provider_model_ids() {
        let config = ModelsConfig {
            default_model: None,
            providers: vec![ModelProviderConfig {
                name: "hamza-eastus2".into(),
                kind: "openai".into(),
                base_url: "https://example.test/openai/v1".into(),
                api_key: None,
                deployment_name: None,
                api_version: None,
                api_key_env: Some("OPENAI_API_KEY".into()),
                model_alias: None,
                models: vec![
                    ConfiguredModel::Id("gpt-5.4".into()),
                    ConfiguredModel::Id("grok-4-fast-reasoning".into()),
                ],
            }],
        };

        let resolved = config
            .resolve_model("hamza-eastus2/grok-4-fast-reasoning")
            .unwrap();
        assert_eq!(resolved.provider.name, "hamza-eastus2");
        assert_eq!(resolved.raw_model, "grok-4-fast-reasoning");
        assert_eq!(
            resolved.canonical_model_id(),
            "hamza-eastus2/grok-4-fast-reasoning"
        );
    }

    #[test]
    fn model_resolution_rejects_ambiguous_short_names() {
        let config = ModelsConfig {
            default_model: None,
            providers: vec![
                ModelProviderConfig {
                    name: "oc-01-openai".into(),
                    kind: "openai".into(),
                    base_url: "https://example.test/openai-a".into(),
                    api_key: None,
                    deployment_name: None,
                    api_version: None,
                    api_key_env: Some("OPENAI_API_KEY".into()),
                    model_alias: None,
                    models: vec![ConfiguredModel::Id("gpt-5.4".into())],
                },
                ModelProviderConfig {
                    name: "hamza-eastus2".into(),
                    kind: "openai".into(),
                    base_url: "https://example.test/openai-b".into(),
                    api_key: None,
                    deployment_name: None,
                    api_version: None,
                    api_key_env: Some("OPENAI_API_KEY".into()),
                    model_alias: None,
                    models: vec![ConfiguredModel::Id("gpt-5.4".into())],
                },
            ],
        };

        let err = config.resolve_model("gpt-5.4").unwrap_err();
        assert!(matches!(
            err,
            ModelResolutionError::AmbiguousModel { model, .. } if model == "gpt-5.4"
        ));
    }

    #[test]
    fn agent_model_selection_accepts_structured_primary_shape() {
        let path = temp_config_path("agent-model-structured");
        fs::write(
            &path,
            r#"
[agents.defaults.model]
primary = "openai-codex/gpt-5.3-codex"

[[agents.list]]
id = "coder"
default = true

[agents.list.model]
primary = "oc-01-openai/gpt-5.4"
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        let agent = config.agents.default_agent().unwrap();
        assert_eq!(
            config.agents.effective_model(agent),
            Some("oc-01-openai/gpt-5.4")
        );

        let _ = fs::remove_file(path);
    }
}
