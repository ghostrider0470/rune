#![doc = "Layered application configuration for Rune."]

use std::collections::HashMap;
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
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
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

/// Runtime execution controls.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub lanes: LaneQueueConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            lanes: LaneQueueConfig::default(),
        }
    }
}

/// MCP server configuration entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransportKind,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// MCP transport kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransportKind {
    Stdio,
    Http,
}

/// Per-lane concurrency caps for turn execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneQueueConfig {
    pub main_capacity: usize,
    pub subagent_capacity: usize,
    pub cron_capacity: usize,
}

impl Default for LaneQueueConfig {
    fn default() -> Self {
        Self {
            main_capacity: 4,
            subagent_capacity: 8,
            cron_capacity: 1024,
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
        let mut ids: Vec<String> = self
            .inventory()
            .into_iter()
            .map(|model| model.model_id())
            .collect();
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
    /// Discord bot token.
    #[serde(default)]
    pub discord_token: Option<String>,
    /// Discord guild (server) ID to watch.
    #[serde(default)]
    pub discord_guild_id: Option<String>,
    /// Discord text channel IDs to poll for inbound messages.
    #[serde(default)]
    pub discord_channel_ids: Vec<String>,
    /// Slack bot OAuth token (`xoxb-...`).
    #[serde(default)]
    pub slack_bot_token: Option<String>,
    /// Slack app-level token (`xapp-...`) for Socket Mode.
    #[serde(default)]
    pub slack_app_token: Option<String>,
    /// Local address for the Slack Events API listener (for example `0.0.0.0:3100`).
    #[serde(default)]
    pub slack_listen_addr: Option<String>,
    /// WhatsApp Cloud API permanent access token.
    #[serde(default)]
    pub whatsapp_access_token: Option<String>,
    /// WhatsApp phone number ID from the Business dashboard.
    #[serde(default)]
    pub whatsapp_phone_number_id: Option<String>,
    /// Token used by Meta to verify the WhatsApp webhook endpoint.
    #[serde(default)]
    pub whatsapp_verify_token: Option<String>,
    /// App secret used to validate `X-Hub-Signature-256` on inbound webhook POSTs.
    #[serde(default)]
    pub whatsapp_app_secret: Option<String>,
    /// Local address for the WhatsApp webhook listener (for example `0.0.0.0:3200`).
    #[serde(default)]
    pub whatsapp_listen_addr: Option<String>,
    /// Signal phone number (e.g. `"+15551234567"`).
    #[serde(default)]
    pub signal_number: Option<String>,
    /// Base URL of the signal-cli REST API daemon.
    #[serde(default)]
    pub signal_api_url: Option<String>,
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
            .or_else(|| {
                self.defaults
                    .model
                    .as_ref()
                    .map(AgentModelSelection::primary)
            })
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
        assert!(config.mcp_servers.is_empty());
    }

    #[test]
    fn file_values_override_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("RUNE_GATEWAY__PORT");
            std::env::remove_var("RUNE_RUNTIME__LANES__MAIN_CAPACITY");
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

[runtime.lanes]
main_capacity = 6
subagent_capacity = 9
cron_capacity = 128
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(config.gateway.host, "127.0.0.1");
        assert_eq!(config.gateway.port, 9999);
        assert_eq!(config.database.max_connections, 42);
        assert!(!config.database.run_migrations);
        assert_eq!(config.runtime.lanes.main_capacity, 6);
        assert_eq!(config.runtime.lanes.subagent_capacity, 9);
        assert_eq!(config.runtime.lanes.cron_capacity, 128);

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

[runtime.lanes]
main_capacity = 4
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_GATEWAY__PORT", "9090");
            std::env::set_var("RUNE_RUNTIME__LANES__MAIN_CAPACITY", "12");
        }
        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(config.gateway.port, 9090);
        assert_eq!(config.runtime.lanes.main_capacity, 12);
        unsafe {
            std::env::remove_var("RUNE_GATEWAY__PORT");
            std::env::remove_var("RUNE_RUNTIME__LANES__MAIN_CAPACITY");
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

    #[test]
    fn channels_config_loads_new_provider_fields_from_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = temp_config_path("channels-provider-fields");
        fs::write(
            &path,
            r#"
[channels]
enabled = ["telegram", "discord", "slack", "whatsapp", "signal"]
telegram_token = "telegram-token"
discord_token = "discord-token"
discord_guild_id = "guild-123"
slack_bot_token = "xoxb-token"
slack_app_token = "xapp-token"
whatsapp_access_token = "wa-token"
whatsapp_phone_number_id = "phone-123"
whatsapp_verify_token = "verify-token"
whatsapp_app_secret = "app-secret"
signal_number = "+15551234567"
signal_api_url = "http://signal.local:8080"
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(
            config.channels.enabled,
            vec!["telegram", "discord", "slack", "whatsapp", "signal"]
        );
        assert_eq!(
            config.channels.telegram_token.as_deref(),
            Some("telegram-token")
        );
        assert_eq!(
            config.channels.discord_token.as_deref(),
            Some("discord-token")
        );
        assert_eq!(
            config.channels.discord_guild_id.as_deref(),
            Some("guild-123")
        );
        assert_eq!(
            config.channels.slack_bot_token.as_deref(),
            Some("xoxb-token")
        );
        assert_eq!(
            config.channels.slack_app_token.as_deref(),
            Some("xapp-token")
        );
        assert_eq!(
            config.channels.whatsapp_access_token.as_deref(),
            Some("wa-token")
        );
        assert_eq!(
            config.channels.whatsapp_phone_number_id.as_deref(),
            Some("phone-123")
        );
        assert_eq!(
            config.channels.whatsapp_verify_token.as_deref(),
            Some("verify-token")
        );
        assert_eq!(
            config.channels.whatsapp_app_secret.as_deref(),
            Some("app-secret")
        );
        assert_eq!(
            config.channels.signal_number.as_deref(),
            Some("+15551234567")
        );
        assert_eq!(
            config.channels.signal_api_url.as_deref(),
            Some("http://signal.local:8080")
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn mcp_servers_load_from_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = temp_config_path("mcp-servers");
        fs::write(
            &path,
            r#"
[[mcp_servers]]
name = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
cwd = "/workspace"
enabled = true

[[mcp_servers]]
name = "remote"
transport = "http"
url = "http://127.0.0.1:8788/mcp"
enabled = false
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(config.mcp_servers.len(), 2);
        assert_eq!(config.mcp_servers[0].name, "filesystem");
        assert_eq!(config.mcp_servers[0].command.as_deref(), Some("npx"));
        assert_eq!(config.mcp_servers[0].args.len(), 3);
        assert_eq!(config.mcp_servers[0].cwd.as_deref(), Some("/workspace"));
        assert!(config.mcp_servers[0].enabled);
        assert_eq!(config.mcp_servers[1].transport, McpTransportKind::Http);
        assert_eq!(
            config.mcp_servers[1].url.as_deref(),
            Some("http://127.0.0.1:8788/mcp")
        );
        assert!(!config.mcp_servers[1].enabled);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn channels_config_env_overrides_file_values() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = temp_config_path("channels-env-override");
        fs::write(
            &path,
            r#"
[channels]
discord_token = "discord-from-file"
whatsapp_verify_token = "verify-from-file"
whatsapp_app_secret = "secret-from-file"
signal_api_url = "http://file-signal:8080"
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_CHANNELS__DISCORD_TOKEN", "discord-from-env");
            std::env::set_var("RUNE_CHANNELS__WHATSAPP_VERIFY_TOKEN", "verify-from-env");
            std::env::set_var("RUNE_CHANNELS__WHATSAPP_APP_SECRET", "secret-from-env");
            std::env::set_var("RUNE_CHANNELS__SIGNAL_API_URL", "http://env-signal:8080");
        }

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(
            config.channels.discord_token.as_deref(),
            Some("discord-from-env")
        );
        assert_eq!(
            config.channels.whatsapp_verify_token.as_deref(),
            Some("verify-from-env")
        );
        assert_eq!(
            config.channels.whatsapp_app_secret.as_deref(),
            Some("secret-from-env")
        );
        assert_eq!(
            config.channels.signal_api_url.as_deref(),
            Some("http://env-signal:8080")
        );

        unsafe {
            std::env::remove_var("RUNE_CHANNELS__DISCORD_TOKEN");
            std::env::remove_var("RUNE_CHANNELS__WHATSAPP_VERIFY_TOKEN");
            std::env::remove_var("RUNE_CHANNELS__WHATSAPP_APP_SECRET");
            std::env::remove_var("RUNE_CHANNELS__SIGNAL_API_URL");
        }
        let _ = fs::remove_file(path);
    }

    #[test]
    fn channels_config_defaults_optional_provider_fields_to_none() {
        let config = AppConfig::default();

        assert!(config.channels.enabled.is_empty());
        assert_eq!(config.channels.telegram_token, None);
        assert_eq!(config.channels.discord_token, None);
        assert_eq!(config.channels.discord_guild_id, None);
        assert_eq!(config.channels.slack_bot_token, None);
        assert_eq!(config.channels.slack_app_token, None);
        assert_eq!(config.channels.whatsapp_access_token, None);
        assert_eq!(config.channels.whatsapp_phone_number_id, None);
        assert_eq!(config.channels.whatsapp_verify_token, None);
        assert_eq!(config.channels.whatsapp_app_secret, None);
        assert_eq!(config.channels.signal_number, None);
        assert_eq!(config.channels.signal_api_url, None);
    }

    #[test]
    fn channels_config_supports_enabled_list_from_environment() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var(
                "RUNE_CHANNELS__ENABLED",
                "[\"telegram\",\"discord\",\"signal\"]",
            );
        }

        let config = AppConfig::load(None::<&std::path::Path>).unwrap();
        assert_eq!(
            config.channels.enabled,
            vec!["telegram", "discord", "signal"]
        );

        unsafe {
            std::env::remove_var("RUNE_CHANNELS__ENABLED");
        }
    }

    #[test]
    fn models_config_loads_provider_kinds_and_inventory_from_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = temp_config_path("models-provider-kinds");
        fs::write(
            &path,
            r#"
[models]
default_model = "google/gemini-2.0-flash"

[[models.providers]]
name = "google"
kind = "gemini"
base_url = ""
api_key_env = "GOOGLE_API_KEY"
models = ["gemini-2.0-flash"]

[[models.providers]]
name = "bedrock"
kind = "aws-bedrock"
base_url = "https://bedrock-runtime.us-east-1.amazonaws.com"
deployment_name = "us-east-1"
api_key_env = "BEDROCK_COMBINED"
models = ["anthropic.claude-3-5-sonnet-20241022-v2:0"]
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(
            config.models.default_model.as_deref(),
            Some("google/gemini-2.0-flash")
        );
        assert_eq!(config.models.providers.len(), 2);
        assert_eq!(config.models.providers[0].kind, "gemini");
        assert_eq!(config.models.providers[1].kind, "aws-bedrock");
        assert_eq!(
            config.models.model_ids(),
            vec![
                "bedrock/anthropic.claude-3-5-sonnet-20241022-v2:0".to_string(),
                "google/gemini-2.0-flash".to_string(),
            ]
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn resolve_model_accepts_provider_alias_kinds_without_affecting_inventory_matching() {
        let config = ModelsConfig {
            default_model: Some("google/gemini-2.0-flash".into()),
            providers: vec![
                ModelProviderConfig {
                    name: "google".into(),
                    kind: "gemini".into(),
                    base_url: String::new(),
                    api_key: None,
                    deployment_name: None,
                    api_version: None,
                    api_key_env: Some("GOOGLE_API_KEY".into()),
                    model_alias: None,
                    models: vec![ConfiguredModel::Id("gemini-2.0-flash".into())],
                },
                ModelProviderConfig {
                    name: "bedrock".into(),
                    kind: "aws-bedrock".into(),
                    base_url: String::new(),
                    api_key: None,
                    deployment_name: Some("us-east-1".into()),
                    api_version: None,
                    api_key_env: Some("BEDROCK_COMBINED".into()),
                    model_alias: None,
                    models: vec![ConfiguredModel::Id(
                        "anthropic.claude-3-5-sonnet-20241022-v2:0".into(),
                    )],
                },
            ],
        };

        let gemini = config.resolve_model("google/gemini-2.0-flash").unwrap();
        assert_eq!(gemini.provider.kind, "gemini");
        assert_eq!(gemini.raw_model, "gemini-2.0-flash");

        let bedrock = config
            .resolve_model("bedrock/anthropic.claude-3-5-sonnet-20241022-v2:0")
            .unwrap();
        assert_eq!(bedrock.provider.kind, "aws-bedrock");
        assert_eq!(
            bedrock.canonical_model_id(),
            "bedrock/anthropic.claude-3-5-sonnet-20241022-v2:0"
        );
    }

    #[test]
    fn channels_config_supports_new_provider_fields_from_environment() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("RUNE_CHANNELS__TELEGRAM_TOKEN", "telegram-env");
            std::env::set_var("RUNE_CHANNELS__DISCORD_TOKEN", "discord-env");
            std::env::set_var("RUNE_CHANNELS__DISCORD_GUILD_ID", "guild-env");
            std::env::set_var("RUNE_CHANNELS__SLACK_BOT_TOKEN", "xoxb-env");
            std::env::set_var("RUNE_CHANNELS__SLACK_APP_TOKEN", "xapp-env");
            std::env::set_var("RUNE_CHANNELS__WHATSAPP_ACCESS_TOKEN", "wa-env");
            std::env::set_var("RUNE_CHANNELS__WHATSAPP_PHONE_NUMBER_ID", "phone-env");
            std::env::set_var("RUNE_CHANNELS__WHATSAPP_VERIFY_TOKEN", "verify-env");
            std::env::set_var("RUNE_CHANNELS__WHATSAPP_APP_SECRET", "secret-env");
            std::env::set_var("RUNE_CHANNELS__SIGNAL_API_URL", "http://signal-env:8080");
        }

        let config = AppConfig::load(None::<&std::path::Path>).unwrap();
        assert_eq!(
            config.channels.telegram_token.as_deref(),
            Some("telegram-env")
        );
        assert_eq!(
            config.channels.discord_token.as_deref(),
            Some("discord-env")
        );
        assert_eq!(
            config.channels.discord_guild_id.as_deref(),
            Some("guild-env")
        );
        assert_eq!(config.channels.slack_bot_token.as_deref(), Some("xoxb-env"));
        assert_eq!(config.channels.slack_app_token.as_deref(), Some("xapp-env"));
        assert_eq!(
            config.channels.whatsapp_access_token.as_deref(),
            Some("wa-env")
        );
        assert_eq!(
            config.channels.whatsapp_phone_number_id.as_deref(),
            Some("phone-env")
        );
        assert_eq!(
            config.channels.whatsapp_verify_token.as_deref(),
            Some("verify-env")
        );
        assert_eq!(
            config.channels.whatsapp_app_secret.as_deref(),
            Some("secret-env")
        );
        assert_eq!(
            config.channels.signal_api_url.as_deref(),
            Some("http://signal-env:8080")
        );

        unsafe {
            std::env::remove_var("RUNE_CHANNELS__TELEGRAM_TOKEN");
            std::env::remove_var("RUNE_CHANNELS__DISCORD_TOKEN");
            std::env::remove_var("RUNE_CHANNELS__DISCORD_GUILD_ID");
            std::env::remove_var("RUNE_CHANNELS__SLACK_BOT_TOKEN");
            std::env::remove_var("RUNE_CHANNELS__SLACK_APP_TOKEN");
            std::env::remove_var("RUNE_CHANNELS__WHATSAPP_ACCESS_TOKEN");
            std::env::remove_var("RUNE_CHANNELS__WHATSAPP_PHONE_NUMBER_ID");
            std::env::remove_var("RUNE_CHANNELS__WHATSAPP_VERIFY_TOKEN");
            std::env::remove_var("RUNE_CHANNELS__WHATSAPP_APP_SECRET");
            std::env::remove_var("RUNE_CHANNELS__SIGNAL_API_URL");
        }
    }
}
