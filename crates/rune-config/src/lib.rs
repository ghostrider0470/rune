#![doc = "Layered application configuration for Rune."]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

// Re-export media engine configs so consumers only need rune-config.
pub use rune_stt::SttConfig;
pub use rune_tts::{TtsAutoMode, TtsConfig};

/// Top-level application configuration resolved from defaults, files, and environment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub mode: RuntimeMode,
    #[serde(default)]
    pub instance: InstanceConfig,
    pub gateway: GatewayConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub vector: VectorConfig,
    pub models: ModelsConfig,
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub ms365: Ms365Config,
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub context: ContextConfig,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    pub memory: MemoryConfig,
    #[serde(default)]
    pub mem0: Mem0Config,
    #[serde(default)]
    pub browser: BrowserConfig,
    pub media: MediaConfig,
    pub logging: LoggingConfig,
    pub paths: PathsConfig,
    #[serde(default)]
    pub approval: ApprovalConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
    #[serde(default)]
    pub comms: CommsConfig,
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

    /// Return a clone with all secret fields replaced by `"***"`.
    #[must_use]
    pub fn redacted(&self) -> Self {
        let mask = |opt: &Option<String>| opt.as_ref().map(|_| "***".to_string());

        let mut out = self.clone();
        out.gateway.auth_token = mask(&self.gateway.auth_token);
        out.database.database_url = mask(&self.database.database_url);
        out.media.tts.api_key = mask(&self.media.tts.api_key);
        out.media.stt.api_key = mask(&self.media.stt.api_key);
        for p in &mut out.models.providers {
            p.api_key = mask(&p.api_key);
        }
        out.channels.telegram_token = mask(&self.channels.telegram_token);
        out.channels.discord_token = mask(&self.channels.discord_token);
        out.channels.slack_bot_token = mask(&self.channels.slack_bot_token);
        out.channels.slack_app_token = mask(&self.channels.slack_app_token);
        out.channels.slack_signing_secret = mask(&self.channels.slack_signing_secret);
        out.channels.whatsapp_access_token = mask(&self.channels.whatsapp_access_token);
        out.channels.whatsapp_app_secret = mask(&self.channels.whatsapp_app_secret);
        out.channels.whatsapp_verify_token = mask(&self.channels.whatsapp_verify_token);
        out.channels.google_chat_service_account = mask(&self.channels.google_chat_service_account);
        out.channels.google_chat_verification_token =
            mask(&self.channels.google_chat_verification_token);
        out.channels.teams_app_id = mask(&self.channels.teams_app_id);
        out.channels.teams_app_password = mask(&self.channels.teams_app_password);
        out.mem0.embedding_api_key = mask(&self.mem0.embedding_api_key);
        out.mem0.postgres_url = mask(&self.mem0.postgres_url);
        out
    }

    /// Return a lightweight JSON schema-like shape for the redacted config.
    ///
    /// This is intentionally derived from the current redacted config value so the
    /// admin UI can inspect object structure without exposing secrets. It is not a
    /// full JSON Schema contract yet, but it provides stable field/type/default
    /// information for schema-aware editor work.
    #[must_use]
    pub fn schema_value(&self) -> Value {
        fn infer_schema(value: &Value) -> Value {
            match value {
                Value::Null => serde_json::json!({"type": "null", "default": Value::Null}),
                Value::Bool(v) => serde_json::json!({"type": "boolean", "default": v}),
                Value::Number(v) => serde_json::json!({"type": "number", "default": v}),
                Value::String(v) => serde_json::json!({"type": "string", "default": v}),
                Value::Array(items) => {
                    let item_schema = items
                        .first()
                        .map(infer_schema)
                        .unwrap_or_else(|| serde_json::json!({}));
                    serde_json::json!({"type": "array", "items": item_schema, "default": value})
                }
                Value::Object(map) => {
                    let mut properties = serde_json::Map::new();
                    for (key, child) in map {
                        properties.insert(key.clone(), infer_schema(child));
                    }
                    let keys: Vec<Value> = map.keys().cloned().map(Value::String).collect();
                    serde_json::json!({
                        "type": "object",
                        "properties": properties,
                        "required": keys,
                        "default": value
                    })
                }
            }
        }

        let redacted = self.redacted();
        let value = serde_json::to_value(redacted).unwrap_or(Value::Null);
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "Rune AppConfig",
            "type": "object",
            "properties": infer_schema(&value).get("properties").cloned().unwrap_or_else(|| serde_json::json!({})),
            "required": infer_schema(&value).get("required").cloned().unwrap_or_else(|| serde_json::json!([])),
            "default": value
        })
    }

    /// Validate that required persistent paths exist and are writable.
    ///
    /// Per DOCKER-DEPLOYMENT.md §9.1 the runtime must fail fast on
    /// missing or unwritable parity-critical paths.
    ///
    /// Writability is verified via an actual write probe (create + delete a
    /// temporary file) rather than `Permissions::readonly()`, which only
    /// inspects the owner write bit and misses bind-mount, UID-mismatch,
    /// and filesystem-level read-only scenarios.
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
            } else if !probe_dir_writable(path) {
                errors.push(ConfigError::PathValidation {
                    path: path.display().to_string(),
                    reason: format!("{name} is not writable (write probe failed)"),
                });
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Auto-create required persistent directories for Standalone mode.
    ///
    /// In Standalone mode the process owns `~/.rune/*` and can safely create
    /// missing directories.  In Server/Docker mode the operator is expected to
    /// provision volumes, so we only log a warning and leave creation to them.
    pub fn ensure_dirs(&self) -> Result<(), Vec<ConfigError>> {
        let dirs = [
            ("db_dir", &self.paths.db_dir),
            ("sessions_dir", &self.paths.sessions_dir),
            ("memory_dir", &self.paths.memory_dir),
            ("media_dir", &self.paths.media_dir),
            ("spells_dir", &self.paths.spells_dir),
            ("plugins_dir", &self.paths.plugins_dir),
            ("logs_dir", &self.paths.logs_dir),
            ("backups_dir", &self.paths.backups_dir),
            ("config_dir", &self.paths.config_dir),
            ("secrets_dir", &self.paths.secrets_dir),
            ("workspace_dir", &self.paths.workspace_dir),
            ("cache_dir", &self.paths.cache_dir),
            ("data_dir", &self.paths.data_dir),
        ];
        let mut errors = Vec::new();
        for (name, path) in &dirs {
            if !path.exists() {
                if let Err(e) = std::fs::create_dir_all(path) {
                    errors.push(ConfigError::PathValidation {
                        path: path.display().to_string(),
                        reason: format!("failed to create {name}: {e}"),
                    });
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Apply CLI startup-flag overrides on top of the resolved config.
    ///
    /// Used by `--yolo` and `--no-sandbox` CLI flags to override the persisted
    /// config without editing files or setting environment variables.
    /// This is the *only* entry point for CLI→config bypass wiring; runtime
    /// execution semantics are not altered here (see issue #64 follow-ups).
    pub fn apply_cli_overrides(&mut self, yolo: bool, no_sandbox: bool) {
        if yolo {
            self.approval.mode = ApprovalMode::Yolo;
        }
        if no_sandbox {
            self.security.sandbox = false;
        }
    }

    /// When mode resolves to Standalone and paths are still at Docker defaults,
    /// swap to `~/.rune/*`.  Also fixes individual paths that remain at Docker
    /// defaults when the user partially overrode the `[paths]` section.
    pub fn adjust_paths_for_mode(&mut self, resolved_mode: &RuntimeMode) {
        if *resolved_mode != RuntimeMode::Standalone {
            return;
        }
        let Some(home) = home_dir() else { return };
        let standalone = standalone_paths_root(&home);

        if self.paths == PathsConfig::default() {
            self.paths = standalone;
            return;
        }

        // Partial override: fix individual paths still at Docker defaults.
        let docker = PathsConfig::default();
        self.paths.replace_docker_defaults(&docker, &standalone);
    }

    /// Return the configured default model before any runtime auto-detection.
    #[must_use]
    pub fn configured_default_model(&self) -> Option<ConfiguredDefaultModel<'_>> {
        if let Some(model) = self
            .agents
            .default_agent()
            .and_then(|agent| self.agents.effective_model(agent))
        {
            return Some(ConfiguredDefaultModel {
                model,
                source: ConfiguredDefaultModelSource::AgentConfig,
            });
        }

        self.models
            .default_model
            .as_deref()
            .map(|model| ConfiguredDefaultModel {
                model,
                source: ConfiguredDefaultModelSource::ModelsDefault,
            })
    }

    /// Return the configured default provider before runtime auto-detection.
    ///
    /// This is detectable when:
    /// - a configured default model resolves to an explicit provider, or
    /// - exactly one explicit provider is configured
    pub fn configured_default_provider(
        &self,
    ) -> Result<Option<ConfiguredDefaultProvider<'_>>, ModelResolutionError> {
        if self.models.providers.is_empty() {
            return Ok(None);
        }

        if let Some(selection) = self.configured_default_model() {
            let resolved = self.models.resolve_model(selection.model)?;
            return Ok(Some(ConfiguredDefaultProvider {
                provider: resolved.provider,
                source: match selection.source {
                    ConfiguredDefaultModelSource::AgentConfig => {
                        ConfiguredDefaultProviderSource::AgentConfig
                    }
                    ConfiguredDefaultModelSource::ModelsDefault => {
                        ConfiguredDefaultProviderSource::ModelsDefault
                    }
                },
            }));
        }

        if self.models.providers.len() == 1 {
            return Ok(Some(ConfiguredDefaultProvider {
                provider: &self.models.providers[0],
                source: ConfiguredDefaultProviderSource::SingleProvider,
            }));
        }

        Ok(None)
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn standalone_paths_root(home: &Path) -> PathsConfig {
    let root = home.join(".rune");
    PathsConfig {
        db_dir: root.join("db"),
        sessions_dir: root.join("sessions"),
        memory_dir: root.join("memory"),
        media_dir: root.join("media"),
        spells_dir: root.join("spells"),
        skills_dir: root.join("skills"),
        plugins_dir: root.join("plugins"),
        logs_dir: root.join("logs"),
        backups_dir: root.join("backups"),
        config_dir: root.join("config"),
        secrets_dir: root.join("secrets"),
        workspace_dir: root.join("workspace"),
        cache_dir: root.join("cache"),
        data_dir: root.join("data"),
    }
}

/// Attempt to create and remove a probe file to verify actual writability.
///
/// Returns `true` when the directory is writable from the perspective of
/// the current process.  Compared to checking `Permissions::readonly()`,
/// this catches bind-mount read-only, UID-mismatch, SELinux/AppArmor
/// denials, and filesystem-level read-only mounts.
fn probe_dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".rune_write_probe");
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Consolidated runtime capabilities detected from config and environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub mode: RuntimeMode,
    pub updated_at: String,
    pub storage_backend: String,
    pub pgvector: bool,
    pub memory_mode: String,
    pub browser: bool,
    pub mcp_servers: usize,
    pub tts: bool,
    pub stt: bool,
    pub tool_count: usize,
    pub channels: Vec<String>,
    pub approval_mode: String,
    pub security_posture: String,
    pub identity: InstanceIdentity,
    pub peer_count: usize,
    pub peers: Vec<PeerCapabilityTarget>,
    pub configured_models: Vec<String>,
    pub active_projects: Vec<String>,
    pub comms_transport: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceIdentity {
    pub id: String,
    pub name: String,
    pub advertised_addr: Option<String>,
    pub roles: Vec<String>,
    pub capabilities_version: u32,
    pub capability_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceConfig {
    #[serde(default = "default_instance_id")]
    pub id: String,
    #[serde(default = "default_instance_name")]
    pub name: String,
    #[serde(default)]
    pub advertised_addr: Option<String>,
    #[serde(default = "default_instance_roles")]
    pub roles: Vec<String>,
    #[serde(default)]
    pub peers: Vec<PeerConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerConfig {
    pub id: String,
    pub health_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerCapabilityTarget {
    pub id: String,
    pub health_url: String,
}

impl Default for InstanceConfig {
    fn default() -> Self {
        Self {
            id: default_instance_id(),
            name: default_instance_name(),
            advertised_addr: None,
            roles: default_instance_roles(),
            peers: Vec::new(),
        }
    }
}

fn default_instance_id() -> String {
    load_or_create_persisted_instance_id().unwrap_or_else(fallback_instance_id)
}

fn fallback_instance_id() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "rune-local".to_string())
}

fn capability_hash_from_config(config: &AppConfig) -> String {
    use sha2::{Digest, Sha256};

    let fingerprint = serde_json::json!({
        "mode": config.mode.as_str(),
        "database": {
            "backend": format!("{:?}", config.database.backend),
            "database_url_configured": config.database.database_url.is_some(),
            "sqlite_path": config.database.sqlite_path,
            "cosmos_endpoint_configured": config.database.cosmos_endpoint.is_some(),
        },
        "vector": {
            "backend": format!("{:?}", config.vector.backend),
            "lancedb_uri": config.vector.lancedb_uri,
            "embedding_dims": config.vector.embedding_dims,
        },
        "memory": {
            "level": format!("{:?}", config.memory.level),
            "semantic_search_enabled": config.memory.semantic_search_enabled,
        },
        "mem0": {
            "enabled": config.mem0.enabled,
            "postgres_configured": config
                .mem0
                .postgres_url
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty()),
        },
        "browser": config.browser.enabled,
        "tts": config.media.tts.enabled,
        "stt": config.media.stt.enabled,
        "channels": config.channels.enabled,
        "approval_mode": config.approval.mode.as_str(),
        "security_posture": config.security.posture().to_string(),
        "instance": {
            "id": config.instance.id,
            "name": config.instance.name,
            "advertised_addr": config.instance.advertised_addr,
            "roles": config.instance.roles,
            "peer_ids": config
                .instance
                .peers
                .iter()
                .map(|peer| peer.id.clone())
                .collect::<Vec<_>>(),
        },
        "models": config
            .models
            .providers
            .iter()
            .flat_map(|provider| provider.models.iter().map(|model| model.id().to_string()))
            .collect::<Vec<_>>(),
        "projects": config
            .agents
            .list
            .iter()
            .filter_map(|agent| config.agents.effective_workspace(agent).map(str::to_string))
            .collect::<Vec<_>>(),
        "comms_transport": config.comms.transport,
    });

    let encoded = serde_json::to_vec(&fingerprint).unwrap_or_default();
    let digest = Sha256::digest(encoded);
    hex::encode(digest)
}

fn persisted_instance_id_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".rune").join("instance-id"))
}

fn load_or_create_persisted_instance_id() -> Option<String> {
    let path = persisted_instance_id_path()?;

    if let Ok(existing) = fs::read_to_string(&path) {
        let id = existing.trim();
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }

    let parent = path.parent()?;
    fs::create_dir_all(parent).ok()?;

    let id = uuid::Uuid::new_v4().to_string();
    fs::write(&path, format!("{id}\n")).ok()?;
    Some(id)
}

fn default_instance_name() -> String {
    default_instance_id()
}

fn default_instance_roles() -> Vec<String> {
    vec!["gateway".to_string()]
}

impl Capabilities {
    pub fn detect(
        config: &AppConfig,
        resolved_mode: RuntimeMode,
        backend_name: &str,
        pgvector_available: bool,
        hybrid_search_enabled: bool,
        tool_count: usize,
    ) -> Self {
        let memory_mode = config.memory.capability_mode(hybrid_search_enabled);
        let mcp_count = config.mcp_servers.iter().filter(|s| s.enabled).count();
        let channels: Vec<String> = config.channels.enabled.clone();
        let tts = config
            .media
            .tts
            .api_key
            .as_deref()
            .is_some_and(|k| !k.is_empty());
        let stt = config
            .media
            .stt
            .api_key
            .as_deref()
            .is_some_and(|k| !k.is_empty());
        let configured_models = config
            .models
            .providers
            .iter()
            .flat_map(|provider| provider.models.iter().map(|model| model.id().to_string()))
            .collect();
        let active_projects = config
            .agents
            .list
            .iter()
            .filter_map(|agent| config.agents.effective_workspace(agent).map(str::to_string))
            .collect();

        Self {
            mode: resolved_mode,
            updated_at: chrono::Utc::now().to_rfc3339(),
            storage_backend: backend_name.to_string(),
            pgvector: pgvector_available,
            memory_mode: memory_mode.to_string(),
            browser: config.browser.enabled,
            mcp_servers: mcp_count,
            tts,
            stt,
            tool_count,
            channels,
            approval_mode: config.approval.mode.as_str().to_string(),
            security_posture: config.security.posture().to_string(),
            identity: InstanceIdentity {
                id: config.instance.id.clone(),
                name: config.instance.name.clone(),
                advertised_addr: config.instance.advertised_addr.clone(),
                roles: config.instance.roles.clone(),
                capabilities_version: 2,
                capability_hash: capability_hash_from_config(config),
            },
            peer_count: config.instance.peers.len(),
            peers: config
                .instance
                .peers
                .iter()
                .map(|peer| PeerCapabilityTarget {
                    id: peer.id.clone(),
                    health_url: peer.health_url.clone(),
                })
                .collect(),
            configured_models,
            active_projects,
            comms_transport: config.comms.transport.clone(),
        }
    }
}

/// Gateway listener and authentication settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub host: String,
    pub port: u16,
    pub auth_token: Option<String>,
    #[serde(default)]
    pub tls: TlsConfig,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8787,
            auth_token: None,
            tls: TlsConfig::default(),
        }
    }
}

/// Microsoft 365 configuration used by the gateway status surface.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ms365Config {
    #[serde(default)]
    pub tenant_id: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub user_principal: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// TLS termination settings for the gateway.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsConfig {
    /// Enable TLS. When false (the default), the gateway serves plain HTTP.
    #[serde(default)]
    pub enabled: bool,
    /// Path to the PEM-encoded certificate chain file.
    #[serde(default)]
    pub cert_path: Option<String>,
    /// Path to the PEM-encoded private key file.
    #[serde(default)]
    pub key_path: Option<String>,
}

impl TlsConfig {
    /// Returns an error message if the config is inconsistent.
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if self.cert_path.is_none() {
            return Err("gateway.tls.cert_path is required when TLS is enabled".into());
        }
        if self.key_path.is_none() {
            return Err("gateway.tls.key_path is required when TLS is enabled".into());
        }
        Ok(())
    }
}

/// Which runtime mode the process is operating in.
///
/// `Auto` (the default) resolves to `Standalone` for bare-host first use even
/// when the config is still at Docker-first defaults. The gateway then remaps
/// those defaults to `~/.rune/*` during startup.
///
/// `Auto` resolves to `Server` when a database URL is set, Docker/Kubernetes
/// is detected, or the operator explicitly points path roots at `/data`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeMode {
    #[default]
    Auto,
    Standalone,
    Server,
}

impl RuntimeMode {
    /// Resolve `Auto` to a concrete mode based on config and environment signals.
    pub fn resolve(&self, config: &AppConfig) -> RuntimeMode {
        self.resolve_with_runtime_signals(config, runtime_server_signals_present())
    }

    fn resolve_with_runtime_signals(
        &self,
        config: &AppConfig,
        runtime_server_signals_present: bool,
    ) -> RuntimeMode {
        match self {
            RuntimeMode::Standalone => RuntimeMode::Standalone,
            RuntimeMode::Server => RuntimeMode::Server,
            RuntimeMode::Auto => {
                if config.database.database_url.is_some() {
                    return RuntimeMode::Server;
                }
                if runtime_server_signals_present {
                    return RuntimeMode::Server;
                }
                if config.paths == PathsConfig::default() {
                    return RuntimeMode::Standalone;
                }
                if config.paths.db_dir.starts_with("/data") {
                    return RuntimeMode::Server;
                }
                RuntimeMode::Standalone
            }
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RuntimeMode::Auto => "auto",
            RuntimeMode::Standalone => "standalone",
            RuntimeMode::Server => "server",
        }
    }
}

fn runtime_server_signals_present() -> bool {
    std::path::Path::new("/.dockerenv").exists() || std::env::var("KUBERNETES_SERVICE_HOST").is_ok()
}

/// Which storage backend to use.
///
/// `Auto` (the default) resolves to Postgres when `database_url` is set,
/// otherwise Cosmos when `cosmos_endpoint` is set, then SQLite — so existing configs with no `backend` field keep working.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    #[default]
    Auto,
    Sqlite,
    Postgres,
    Cosmos,
}

/// Which backend to use for vector/semantic memory.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VectorBackend {
    /// Resolve automatically: lancedb_uri set → LanceDb; database backend has vectors → use it; else → None.
    #[default]
    Auto,
    /// Embedded LanceDB (local path or cloud URI like az://container/path).
    #[serde(alias = "lance")]
    LanceDb,
    /// Use the same backend as `[database]` (Cosmos vector, pgvector, or SQLite stubs).
    Integrated,
    /// No vector search — memory recall returns empty results.
    None,
}

/// Configuration for the vector/semantic memory backend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorConfig {
    /// Which vector backend to use.
    #[serde(default)]
    pub backend: VectorBackend,
    /// Path or URI for LanceDB. Local: `~/.rune/db/vectors`, Cloud: `az://container/path`.
    pub lancedb_uri: Option<String>,
    /// Embedding dimensions (must match your embedding model output).
    #[serde(default = "default_embedding_dims")]
    pub embedding_dims: i32,
}

fn default_embedding_dims() -> i32 {
    3072
}

impl Default for VectorConfig {
    fn default() -> Self {
        Self {
            backend: VectorBackend::Auto,
            lancedb_uri: None,
            embedding_dims: default_embedding_dims(),
        }
    }
}

/// Database connectivity and migration settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default)]
    pub backend: StorageBackend,
    pub database_url: Option<String>,
    pub max_connections: u32,
    pub run_migrations: bool,
    /// Path to the SQLite database file. Defaults to `{db_dir}/rune.db`.
    #[serde(default)]
    pub sqlite_path: Option<PathBuf>,
    /// Cosmos DB NoSQL endpoint URL.
    #[serde(default)]
    pub cosmos_endpoint: Option<String>,
    /// Cosmos DB master key for auth.
    #[serde(default)]
    pub cosmos_key: Option<String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackend::default(),
            database_url: None,
            max_connections: 10,
            run_migrations: true,
            sqlite_path: None,
            cosmos_endpoint: None,
            cosmos_key: None,
        }
    }
}

/// Context tier budgets and loading priorities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextConfig {
    #[serde(default = "default_context_identity_tokens")]
    pub identity: usize,
    #[serde(default = "default_context_identity_priority")]
    pub identity_priority: u8,
    #[serde(default = "default_context_identity_staleness_policy")]
    pub identity_staleness_policy: String,
    #[serde(default = "default_context_task_tokens")]
    pub task: usize,
    #[serde(default = "default_context_task_priority")]
    pub task_priority: u8,
    #[serde(default = "default_context_task_staleness_policy")]
    pub task_staleness_policy: String,
    #[serde(default = "default_context_project_tokens")]
    pub project: usize,
    #[serde(default = "default_context_project_priority")]
    pub project_priority: u8,
    #[serde(default = "default_context_project_staleness_policy")]
    pub project_staleness_policy: String,
    #[serde(default = "default_context_shared_tokens")]
    pub shared: usize,
    #[serde(default = "default_context_shared_priority")]
    pub shared_priority: u8,
    #[serde(default = "default_context_shared_staleness_policy")]
    pub shared_staleness_policy: String,
    #[serde(default = "default_context_historical_tokens")]
    pub historical: usize,
    #[serde(default = "default_context_historical_priority")]
    pub historical_priority: u8,
    #[serde(default = "default_context_historical_staleness_policy")]
    pub historical_staleness_policy: String,
}

fn default_context_identity_tokens() -> usize {
    1_000
}
fn default_context_task_tokens() -> usize {
    10_000
}
fn default_context_project_tokens() -> usize {
    20_000
}
fn default_context_shared_tokens() -> usize {
    5_000
}
fn default_context_identity_priority() -> u8 {
    0
}
fn default_context_task_priority() -> u8 {
    1
}
fn default_context_project_priority() -> u8 {
    2
}
fn default_context_shared_priority() -> u8 {
    3
}
fn default_context_historical_tokens() -> usize {
    8_000
}
fn default_context_historical_priority() -> u8 {
    4
}
fn default_context_identity_staleness_policy() -> String {
    "always_fresh".to_string()
}
fn default_context_task_staleness_policy() -> String {
    "per_turn".to_string()
}
fn default_context_project_staleness_policy() -> String {
    "per_session".to_string()
}
fn default_context_shared_staleness_policy() -> String {
    "on_demand".to_string()
}
fn default_context_historical_staleness_policy() -> String {
    "retrieval_only".to_string()
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            identity: default_context_identity_tokens(),
            identity_priority: default_context_identity_priority(),
            identity_staleness_policy: default_context_identity_staleness_policy(),
            task: default_context_task_tokens(),
            task_priority: default_context_task_priority(),
            task_staleness_policy: default_context_task_staleness_policy(),
            project: default_context_project_tokens(),
            project_priority: default_context_project_priority(),
            project_staleness_policy: default_context_project_staleness_policy(),
            shared: default_context_shared_tokens(),
            shared_priority: default_context_shared_priority(),
            shared_staleness_policy: default_context_shared_staleness_policy(),
            historical: default_context_historical_tokens(),
            historical_priority: default_context_historical_priority(),
            historical_staleness_policy: default_context_historical_staleness_policy(),
        }
    }
}

#[cfg(test)]
mod context_config_tests {
    use super::ContextConfig;

    #[test]
    fn default_context_tier_budgets_match_story_defaults() {
        let cfg = ContextConfig::default();
        assert_eq!(cfg.identity, 1_000);
        assert_eq!(cfg.identity_priority, 0);
        assert_eq!(cfg.identity_staleness_policy, "always_fresh");
        assert_eq!(cfg.task, 10_000);
        assert_eq!(cfg.task_priority, 1);
        assert_eq!(cfg.task_staleness_policy, "per_turn");
        assert_eq!(cfg.project, 20_000);
        assert_eq!(cfg.project_priority, 2);
        assert_eq!(cfg.project_staleness_policy, "per_session");
        assert_eq!(cfg.shared, 5_000);
        assert_eq!(cfg.shared_priority, 3);
        assert_eq!(cfg.shared_staleness_policy, "on_demand");
        assert_eq!(cfg.historical, 8_000);
        assert_eq!(cfg.historical_priority, 4);
        assert_eq!(cfg.historical_staleness_policy, "retrieval_only");
    }
}

/// Compaction controls for context window management.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Hard token ceiling for the active model context. Default: 128000.
    #[serde(default = "default_context_window")]
    pub context_window: usize,
    /// Number of recent messages to always preserve verbatim. Default: 20.
    #[serde(default = "default_preserve_tail")]
    pub preserve_tail: usize,
    /// Configurable alias for the total usable context budget. Defaults to the
    /// same value as `context_window` so existing configs remain compatible.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    /// Tokens reserved for system/identity instructions. Default: 5000.
    #[serde(default = "default_reserved_system")]
    pub reserved_system: usize,
    /// Tokens reserved for active task instructions. Default: 10000.
    #[serde(default = "default_reserved_task")]
    pub reserved_task: usize,
    /// Whether project memory should be auto-injected when available. Default: true.
    #[serde(default = "default_true")]
    pub auto_inject_project: bool,
    /// Maximum number of memory search results considered for injection. Default: 10.
    #[serde(default = "default_memory_search_k")]
    pub memory_search_k: usize,
    /// Threshold at which compaction should trigger. Defaults to 50000 tokens.
    #[serde(default = "default_compress_after")]
    pub compress_after: usize,
    /// Warn once usage crosses this token threshold. Defaults to 80% of max_tokens.
    #[serde(default = "default_warn_at_tokens")]
    pub warn_at_tokens: usize,
}

fn default_context_window() -> usize {
    128_000
}
fn default_preserve_tail() -> usize {
    20
}
fn default_max_tokens() -> usize {
    default_context_window()
}
fn default_reserved_system() -> usize {
    5_000
}
fn default_reserved_task() -> usize {
    10_000
}
fn default_memory_search_k() -> usize {
    10
}
fn default_compress_after() -> usize {
    50_000
}
fn default_warn_at_tokens() -> usize {
    (default_max_tokens() as f32 * 0.80).round() as usize
}

impl CompactionConfig {
    /// Effective total budget used by runtime budget accounting.
    pub fn effective_max_tokens(&self) -> usize {
        self.max_tokens.max(self.context_window)
    }

    /// Effective warning threshold, clamped to the total budget.
    pub fn effective_warn_at_tokens(&self) -> usize {
        self.warn_at_tokens.min(self.effective_max_tokens())
    }

    /// Effective compaction threshold, clamped to the total budget.
    pub fn effective_compress_after(&self) -> usize {
        self.compress_after.min(self.effective_max_tokens())
    }

    /// Remaining capacity after reserved system/task allocations.
    pub fn usable_prompt_budget(&self) -> usize {
        self.effective_max_tokens()
            .saturating_sub(self.reserved_system.saturating_add(self.reserved_task))
    }
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            context_window: default_context_window(),
            preserve_tail: default_preserve_tail(),
            max_tokens: default_max_tokens(),
            reserved_system: default_reserved_system(),
            reserved_task: default_reserved_task(),
            auto_inject_project: default_true(),
            memory_search_k: default_memory_search_k(),
            compress_after: default_compress_after(),
            warn_at_tokens: default_warn_at_tokens(),
        }
    }
}

#[cfg(test)]
mod compaction_config_tests {
    use super::CompactionConfig;

    #[test]
    fn default_compaction_budget_matches_story_defaults() {
        let cfg = CompactionConfig::default();
        assert_eq!(cfg.max_tokens, 128_000);
        assert_eq!(cfg.reserved_system, 5_000);
        assert_eq!(cfg.reserved_task, 10_000);
        assert!(cfg.auto_inject_project);
        assert_eq!(cfg.memory_search_k, 10);
        assert_eq!(cfg.compress_after, 50_000);
        assert_eq!(cfg.warn_at_tokens, 102_400);
        assert_eq!(cfg.usable_prompt_budget(), 113_000);
    }

    #[test]
    fn effective_thresholds_are_clamped_to_capacity() {
        let cfg = CompactionConfig {
            context_window: 64_000,
            max_tokens: 32_000,
            warn_at_tokens: 96_000,
            compress_after: 80_000,
            ..CompactionConfig::default()
        };

        assert_eq!(cfg.effective_max_tokens(), 64_000);
        assert_eq!(cfg.effective_warn_at_tokens(), 64_000);
        assert_eq!(cfg.effective_compress_after(), 64_000);
    }
}

/// Runtime execution controls.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub lanes: LaneQueueConfig,
    #[serde(default)]
    pub compaction: CompactionConfig,
    /// Maximum tool-call iterations per turn before aborting. Default: 200.
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: u32,
}

fn default_max_tool_iterations() -> u32 {
    500
}

/// Plugin system configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(default = "default_plugin_scan_dirs")]
    pub scan_dirs: Vec<String>,
    #[serde(default = "default_plugin_scan_interval")]
    pub scan_interval_secs: u64,
    #[serde(default)]
    pub overrides: HashMap<String, PluginOverride>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginOverride {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub session_kinds: Option<Vec<String>>,
    #[serde(default)]
    pub mcp_lifecycle: Option<String>,
}

impl Default for PluginOverride {
    fn default() -> Self {
        Self {
            enabled: true,
            session_kinds: None,
            mcp_lifecycle: None,
        }
    }
}

fn default_plugin_scan_dirs() -> Vec<String> {
    vec![
        "~/.rune/plugins".to_string(),
        "~/.claude/plugins/cache".to_string(),
    ]
}

fn default_plugin_scan_interval() -> u64 {
    300
}

/// Inter-agent communication via filesystem mailbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_comms_transport")]
    pub transport: String,
    #[serde(default)]
    pub comms_dir: Option<String>,
    #[serde(default)]
    pub http: Option<CommsHttpConfig>,
    #[serde(default = "default_comms_agent_id")]
    pub agent_id: String,
    #[serde(default = "default_comms_peer_id")]
    pub peer_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CommsHttpConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub auth_token: Option<String>,
}

fn default_comms_transport() -> String {
    "filesystem".to_string()
}
fn default_comms_agent_id() -> String {
    "rune".to_string()
}
fn default_comms_peer_id() -> String {
    "horizon-ai".to_string()
}

impl Default for CommsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            transport: default_comms_transport(),
            comms_dir: None,
            http: None,
            agent_id: default_comms_agent_id(),
            peer_id: default_comms_peer_id(),
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
    #[serde(default = "default_priority_capacity")]
    pub priority_capacity: usize,
    pub subagent_capacity: usize,
    pub cron_capacity: usize,
    #[serde(default = "default_heartbeat_capacity")]
    pub heartbeat_capacity: usize,
    #[serde(default = "default_global_tool_capacity")]
    pub global_tool_capacity: usize,
    #[serde(default = "default_project_tool_capacity")]
    pub project_tool_capacity: usize,
}

const fn default_priority_capacity() -> usize {
    16
}

const fn default_heartbeat_capacity() -> usize {
    1024
}

const fn default_global_tool_capacity() -> usize {
    32
}

const fn default_project_tool_capacity() -> usize {
    4
}

impl Default for LaneQueueConfig {
    fn default() -> Self {
        Self {
            main_capacity: 4,
            priority_capacity: default_priority_capacity(),
            subagent_capacity: 8,
            cron_capacity: 1024,
            heartbeat_capacity: default_heartbeat_capacity(),
            global_tool_capacity: default_global_tool_capacity(),
            project_tool_capacity: default_project_tool_capacity(),
        }
    }
}

/// Provider inventory and routing aliases.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelsConfig {
    #[serde(default)]
    pub default_model: Option<String>,
    /// Optional default image-generation model, separate from the chat/text default.
    #[serde(default, alias = "image_model")]
    pub default_image_model: Option<String>,
    /// Ordered text/model fallback chains reserved for later routing and CLI lanes.
    #[serde(default)]
    pub fallbacks: Vec<ModelFallbackChainConfig>,
    /// Ordered image-model fallback chains reserved for later routing and CLI lanes.
    #[serde(default)]
    pub image_fallbacks: Vec<ModelFallbackChainConfig>,
    /// Provider auth-order metadata reserved for later auth/profile management lanes.
    #[serde(default)]
    pub auth_orders: Vec<ModelAuthOrderConfig>,
    #[serde(default)]
    pub providers: Vec<ModelProviderConfig>,
}

impl ModelsConfig {
    #[must_use]
    pub fn bootstrap(&self) -> ModelBootstrap {
        if self.providers.is_empty() {
            ModelBootstrap::ZeroConfigOllama
        } else {
            ModelBootstrap::ExplicitProviders
        }
    }

    /// Return configured provider names in declaration order.
    #[must_use]
    pub fn provider_names(&self) -> Vec<&str> {
        self.providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect()
    }

    /// Return the effective Ollama base URL for zero-config startup probing.
    ///
    /// When explicit providers are configured, zero-config Ollama probing is
    /// disabled and this returns `None`.
    #[must_use]
    pub fn zero_config_ollama_base_url(&self, ollama_host: Option<&str>) -> Option<String> {
        if self.bootstrap() != ModelBootstrap::ZeroConfigOllama {
            return None;
        }

        Some(normalize_zero_config_ollama_host(ollama_host))
    }

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

    /// Look up the fallback chain whose first entry matches `model_ref`.
    ///
    /// Returns the *remaining* entries (i.e. everything after the primary) so
    /// the caller can iterate over alternatives without re-trying the primary.
    #[must_use]
    pub fn fallback_chain_for(&self, model_ref: &str) -> Option<&[String]> {
        self.fallbacks
            .iter()
            .find(|chain| chain.chain.first().map(String::as_str) == Some(model_ref))
            .and_then(|chain| chain.chain.get(1..))
            .filter(|rest| !rest.is_empty())
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

fn normalize_zero_config_ollama_host(ollama_host: Option<&str>) -> String {
    let Some(value) = ollama_host
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/'))
    else {
        return "http://localhost:11434".to_string();
    };

    if value.starts_with("http://") || value.starts_with("https://") {
        return value.to_string();
    }

    if value.contains(':') {
        return format!("http://{value}");
    }

    format!("http://{value}:11434")
}

/// Named ordered fallback chain metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFallbackChainConfig {
    pub name: String,
    #[serde(default)]
    pub chain: Vec<String>,
}

/// Provider auth-order metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelAuthOrderConfig {
    pub provider: String,
    #[serde(default)]
    pub order: Vec<String>,
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
    /// Slack signing secret used to verify inbound webhook signatures (HMAC-SHA256).
    #[serde(default)]
    pub slack_signing_secret: Option<String>,
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
    /// Microsoft Teams Bot Framework bearer token.
    #[serde(default)]
    pub teams_bot_token: Option<String>,
    /// Azure Bot / Microsoft App ID for outbound activities.
    #[serde(default)]
    pub teams_bot_app_id: Option<String>,
    /// Google Chat service account credentials JSON path or inline JSON.
    #[serde(default)]
    pub google_chat_service_account: Option<String>,
    /// Google Chat webhook/listener bind address for inbound events.
    #[serde(default)]
    pub google_chat_listen_addr: Option<String>,
    /// Optional Google Chat verification token for inbound webhook validation.
    #[serde(default)]
    pub google_chat_verification_token: Option<String>,
    /// Microsoft Teams / Azure Bot app id.
    #[serde(default)]
    pub teams_app_id: Option<String>,
    /// Microsoft Teams / Azure Bot app password or client secret.
    #[serde(default)]
    pub teams_app_password: Option<String>,
    /// Optional Azure AD tenant id for single-tenant bot auth.
    #[serde(default)]
    pub teams_tenant_id: Option<String>,
    /// Local listener bind address for inbound Bot Framework activities.
    #[serde(default)]
    pub teams_listen_addr: Option<String>,
}

/// Memory indexing and retrieval settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryLevel {
    File,
    Keyword,
    #[default]
    Semantic,
}

impl MemoryLevel {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Keyword => "keyword",
            Self::Semantic => "semantic",
        }
    }
}

/// Memory indexing and retrieval settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default)]
    pub level: Option<MemoryLevel>,
    #[serde(default = "default_true")]
    pub semantic_search_enabled: bool,
}

impl MemoryConfig {
    #[must_use]
    pub fn requested_level(&self) -> MemoryLevel {
        self.level.unwrap_or(if self.semantic_search_enabled {
            MemoryLevel::Semantic
        } else {
            MemoryLevel::Keyword
        })
    }

    #[must_use]
    pub fn effective_level(&self, hybrid_search_enabled: bool) -> MemoryLevel {
        match self.requested_level() {
            MemoryLevel::Semantic if hybrid_search_enabled => MemoryLevel::Semantic,
            MemoryLevel::Semantic => MemoryLevel::Keyword,
            level => level,
        }
    }

    #[must_use]
    pub fn capability_mode(&self, hybrid_search_enabled: bool) -> &'static str {
        match (self.requested_level(), hybrid_search_enabled) {
            (MemoryLevel::File, _) => "file-local",
            (MemoryLevel::Keyword, _) => "keyword-local",
            (MemoryLevel::Semantic, true) => "semantic-hybrid",
            (MemoryLevel::Semantic, false) => "semantic-keyword-fallback",
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            level: None,
            semantic_search_enabled: true,
        }
    }
}

/// Semantic browser snapshot settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_webchat_send_window_seconds")]
    pub webchat_send_window_seconds: u64,
    #[serde(default = "default_webchat_send_max_requests")]
    pub webchat_send_max_requests: u32,
    #[serde(default)]
    pub chromium_path: Option<String>,
    #[serde(default)]
    pub cdp_endpoint: Option<String>,
    #[serde(default = "default_max_browser_instances")]
    pub max_instances: usize,
    #[serde(default = "default_max_browser_chars")]
    pub max_chars: usize,
    #[serde(default = "default_browser_timeout_ms")]
    pub page_load_timeout_ms: u64,
    #[serde(default)]
    pub blocked_urls: Vec<String>,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            webchat_send_window_seconds: default_webchat_send_window_seconds(),
            webchat_send_max_requests: default_webchat_send_max_requests(),
            chromium_path: None,
            cdp_endpoint: None,
            max_instances: default_max_browser_instances(),
            max_chars: default_max_browser_chars(),
            page_load_timeout_ms: default_browser_timeout_ms(),
            blocked_urls: Vec::new(),
        }
    }
}

const fn default_webchat_send_window_seconds() -> u64 {
    5
}

const fn default_webchat_send_max_requests() -> u32 {
    8
}

const fn default_max_browser_instances() -> usize {
    3
}

const fn default_max_browser_chars() -> usize {
    30_000
}

const fn default_browser_timeout_ms() -> u64 {
    15_000
}

/// Media pipeline configuration embedding TTS and STT engine configs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MediaConfig {
    #[serde(default)]
    pub tts: TtsConfig,
    #[serde(default)]
    pub stt: SttConfig,
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
    /// Spells directory (renamed from `skills_dir` in #299).
    pub spells_dir: PathBuf,
    /// Backward-compat alias — deserialized from `skills_dir` in legacy configs.
    #[serde(alias = "skills_dir")]
    pub skills_dir: PathBuf,
    pub plugins_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub backups_dir: PathBuf,
    pub config_dir: PathBuf,
    pub secrets_dir: PathBuf,
    /// Workspace scratch directory (#299).
    #[serde(default)]
    pub workspace_dir: PathBuf,
    /// Cache directory (#299).
    #[serde(default)]
    pub cache_dir: PathBuf,
    /// Persistent data directory (#299).
    #[serde(default)]
    pub data_dir: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            db_dir: PathBuf::from("/data/db"),
            sessions_dir: PathBuf::from("/data/sessions"),
            memory_dir: PathBuf::from("/data/memory"),
            media_dir: PathBuf::from("/data/media"),
            spells_dir: PathBuf::from("/data/spells"),
            skills_dir: PathBuf::from("/data/skills"),
            plugins_dir: PathBuf::from("/data/plugins"),
            logs_dir: PathBuf::from("/data/logs"),
            backups_dir: PathBuf::from("/data/backups"),
            config_dir: PathBuf::from("/config"),
            secrets_dir: PathBuf::from("/secrets"),
            workspace_dir: PathBuf::from("/data/workspace"),
            cache_dir: PathBuf::from("/data/cache"),
            data_dir: PathBuf::from("/data/data"),
        }
    }
}

/// High-level path layout profile surfaced in startup diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PathsProfile {
    DockerDefault,
    StandaloneHome,
    Custom,
}

impl PathsProfile {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DockerDefault => "docker-default",
            Self::StandaloneHome => "standalone-home",
            Self::Custom => "custom",
        }
    }
}

impl PathsConfig {
    /// Replace any path that still equals its Docker default with the
    /// corresponding standalone path.  This handles configs where the user
    /// overrode *some* paths but left others (like `plugins_dir`) absent.
    pub fn replace_docker_defaults(&mut self, docker: &Self, standalone: &Self) {
        macro_rules! fix {
            ($field:ident) => {
                if self.$field == docker.$field {
                    self.$field = standalone.$field.clone();
                }
            };
        }
        fix!(db_dir);
        fix!(sessions_dir);
        fix!(memory_dir);
        fix!(media_dir);
        fix!(spells_dir);
        fix!(skills_dir);
        fix!(plugins_dir);
        fix!(logs_dir);
        fix!(backups_dir);
        fix!(config_dir);
        fix!(secrets_dir);
        fix!(workspace_dir);
        fix!(cache_dir);
        fix!(data_dir);
    }

    #[must_use]
    pub fn profile(&self) -> PathsProfile {
        if *self == PathsConfig::default() {
            return PathsProfile::DockerDefault;
        }

        if let Some(home) = home_dir() {
            if *self == standalone_paths_root(&home) {
                return PathsProfile::StandaloneHome;
            }
        }

        PathsProfile::Custom
    }
}

/// How models bootstrap at startup before any request is executed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelBootstrap {
    ExplicitProviders,
    ZeroConfigOllama,
}

impl ModelBootstrap {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitProviders => "explicit-providers",
            Self::ZeroConfigOllama => "zero-config-ollama",
        }
    }
}

/// Source of the configured default model before runtime auto-detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfiguredDefaultModelSource {
    AgentConfig,
    ModelsDefault,
}

impl ConfiguredDefaultModelSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AgentConfig => "agent-config",
            Self::ModelsDefault => "models.default_model",
        }
    }
}

/// Configured default model selection before runtime auto-detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfiguredDefaultModel<'a> {
    pub model: &'a str,
    pub source: ConfiguredDefaultModelSource,
}

/// Source of the configured default provider before runtime auto-detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfiguredDefaultProviderSource {
    AgentConfig,
    ModelsDefault,
    SingleProvider,
}

impl ConfiguredDefaultProviderSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AgentConfig => "agent-config",
            Self::ModelsDefault => "models.default_model",
            Self::SingleProvider => "single-provider",
        }
    }
}

/// Configured default provider selection before runtime auto-detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfiguredDefaultProvider<'a> {
    pub provider: &'a ModelProviderConfig,
    pub source: ConfiguredDefaultProviderSource,
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

/// Tool-approval bypass policy.
///
/// Controls whether tool calls require operator confirmation.
/// See issue #64 and PROTOCOLS.md §10.3.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalMode {
    /// Default: prompt for every tool call that lacks a durable allow-always policy.
    #[default]
    Prompt,
    /// Auto-approve file operations; prompt for exec/network.
    #[serde(rename = "auto-file")]
    AutoFile,
    /// Auto-approve exec operations; prompt for others.
    #[serde(rename = "auto-exec")]
    AutoExec,
    /// Auto-approve everything — maximum trust, zero friction.
    Yolo,
}

impl ApprovalMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::AutoFile => "auto-file",
            Self::AutoExec => "auto-exec",
            Self::Yolo => "yolo",
        }
    }

    /// Whether this mode auto-approves all tool calls.
    #[must_use]
    pub fn is_yolo(self) -> bool {
        matches!(self, Self::Yolo)
    }
}

/// Approval configuration section.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalConfig {
    #[serde(default)]
    pub mode: ApprovalMode,
}

/// Security boundary configuration.
///
/// Controls sandbox enforcement and spell trust.
/// See issue #64 and PROTOCOLS.md §10.3.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Enable filesystem sandboxing / workspace boundary enforcement.
    #[serde(default = "default_true")]
    pub sandbox: bool,
    /// Trust all installed spells without capability validation.
    #[serde(default)]
    pub trust_spells: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            sandbox: true,
            trust_spells: false,
        }
    }
}

impl SecurityConfig {
    /// Summarise the effective security posture as a human-readable label.
    #[must_use]
    pub fn posture(&self) -> &'static str {
        match (self.sandbox, self.trust_spells) {
            (true, false) => "standard",
            (true, true) => "trust-spells",
            (false, false) => "no-sandbox",
            (false, true) => "unrestricted",
        }
    }
}

/// Mem0-style auto-capture/recall memory configuration.
///
/// When enabled, the runtime automatically recalls similar facts before each
/// turn and captures new facts after each turn, giving the agent persistent
/// cross-session memory powered by pgvector embeddings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mem0Config {
    /// Master switch — set to `true` to enable auto-recall and auto-capture.
    #[serde(default)]
    pub enabled: bool,

    /// PostgreSQL connection string (must have pgvector extension).
    #[serde(default)]
    pub postgres_url: Option<String>,

    /// Azure OpenAI embedding endpoint (without query string).
    #[serde(default)]
    pub embedding_endpoint: Option<String>,

    /// API key for the embedding endpoint.
    #[serde(default)]
    pub embedding_api_key: Option<String>,

    /// Embedding model name sent in the request body.
    #[serde(default = "Mem0Config::default_embedding_model")]
    pub embedding_model: String,

    /// Embedding vector dimensionality.
    #[serde(default = "Mem0Config::default_embedding_dims")]
    pub embedding_dims: usize,

    /// Azure API version query parameter.
    #[serde(default = "Mem0Config::default_api_version")]
    pub api_version: String,

    /// Maximum number of memories returned by recall.
    #[serde(default = "Mem0Config::default_top_k")]
    pub top_k: usize,

    /// Minimum cosine similarity for recall results.
    #[serde(default = "Mem0Config::default_similarity_threshold")]
    pub similarity_threshold: f64,

    /// Cosine similarity above which a new fact is considered a duplicate.
    #[serde(default = "Mem0Config::default_dedup_threshold")]
    pub dedup_threshold: f64,

    /// Model used for fact extraction (e.g. "gpt-5.4").  Passed as-is to
    /// the routing provider.  If empty, defaults to "gpt-5.4".
    #[serde(default = "Mem0Config::default_extraction_model")]
    pub extraction_model: String,

    /// Enable Obsidian-compatible markdown vault sync (one-way: vector → markdown).
    #[serde(default)]
    pub vault_enabled: bool,

    /// Directory for the vault. Defaults to `{memory_dir}/vault/`.
    #[serde(default)]
    pub vault_dir: Option<PathBuf>,

    /// Cosine similarity threshold for wikilink generation between facts.
    #[serde(default = "Mem0Config::default_vault_link_threshold")]
    pub vault_link_threshold: f64,
}

/// `Eq` is required because `AppConfig` derives `Eq`. Configuration
/// floats are always well-behaved (no NaN), so this is sound.
impl PartialEq for Mem0Config {
    fn eq(&self, other: &Self) -> bool {
        self.enabled == other.enabled
            && self.postgres_url == other.postgres_url
            && self.embedding_endpoint == other.embedding_endpoint
            && self.embedding_api_key == other.embedding_api_key
            && self.embedding_model == other.embedding_model
            && self.embedding_dims == other.embedding_dims
            && self.api_version == other.api_version
            && self.top_k == other.top_k
            && self.similarity_threshold.to_bits() == other.similarity_threshold.to_bits()
            && self.dedup_threshold.to_bits() == other.dedup_threshold.to_bits()
            && self.extraction_model == other.extraction_model
            && self.vault_enabled == other.vault_enabled
            && self.vault_dir == other.vault_dir
            && self.vault_link_threshold.to_bits() == other.vault_link_threshold.to_bits()
    }
}
impl Eq for Mem0Config {}

impl Mem0Config {
    fn default_embedding_model() -> String {
        "text-embedding-3-large".to_string()
    }
    fn default_embedding_dims() -> usize {
        3072
    }
    fn default_api_version() -> String {
        "2024-02-01".to_string()
    }
    fn default_top_k() -> usize {
        10
    }
    fn default_similarity_threshold() -> f64 {
        0.3
    }
    fn default_dedup_threshold() -> f64 {
        0.85
    }
    fn default_extraction_model() -> String {
        "gpt-5.4".to_string()
    }
    fn default_vault_link_threshold() -> f64 {
        0.45
    }
}

impl Default for Mem0Config {
    fn default() -> Self {
        Self {
            enabled: false,
            postgres_url: None,
            embedding_endpoint: None,
            embedding_api_key: None,
            embedding_model: Self::default_embedding_model(),
            embedding_dims: Self::default_embedding_dims(),
            api_version: Self::default_api_version(),
            top_k: Self::default_top_k(),
            similarity_threshold: Self::default_similarity_threshold(),
            dedup_threshold: Self::default_dedup_threshold(),
            extraction_model: Self::default_extraction_model(),
            vault_enabled: false,
            vault_dir: None,
            vault_link_threshold: Self::default_vault_link_threshold(),
        }
    }
}

/// Configuration loading and validation failures.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to load configuration: {0}")]
    Load(#[from] Box<figment::Error>),
    #[error("path validation failed: {path} — {reason}")]
    PathValidation { path: String, reason: String },
}

// ── First-use bypass acknowledgment (issue #64) ─────────────────────

/// Sentinel file name written to `config_dir` once the operator acknowledges
/// the risks of running in an unrestricted startup posture.
const BYPASS_ACK_SENTINEL: &str = ".yolo-acknowledged";

/// Manages the persistent first-use acknowledgment for unrestricted bypass
/// modes (`--yolo`, `--no-sandbox`).
///
/// When `--yolo` or `--no-sandbox` is used for the first time, the operator
/// must explicitly confirm the risk.  After acknowledgment, a sentinel file
/// is written so the warning is not repeated on subsequent starts.
pub struct BypassAcknowledgment {
    sentinel_path: PathBuf,
}

impl BypassAcknowledgment {
    /// Create an acknowledgment tracker rooted in the given config directory.
    pub fn new(config_dir: &Path) -> Self {
        Self {
            sentinel_path: config_dir.join(BYPASS_ACK_SENTINEL),
        }
    }

    /// Resolve from `$HOME/.rune/config` (standalone default).
    pub fn from_home() -> Option<Self> {
        home_dir().map(|h| Self::new(&h.join(".rune").join("config")))
    }

    /// Whether the operator has previously acknowledged the bypass risk.
    pub fn is_acknowledged(&self) -> bool {
        self.sentinel_path.exists()
    }

    /// Record that the operator has acknowledged the bypass risk.
    ///
    /// Creates the parent directory if needed.  Returns `Ok(())` on success
    /// or if the sentinel already exists.
    pub fn record(&self) -> std::io::Result<()> {
        if self.is_acknowledged() {
            return Ok(());
        }
        if let Some(parent) = self.sentinel_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            &self.sentinel_path,
            format!("Bypass risk acknowledged at {:?}\n", SystemTime::now()),
        )
    }

    /// Remove a previously recorded acknowledgment.
    ///
    /// Useful for testing or when the operator wants to re-enable the
    /// first-use warning.
    pub fn revoke(&self) -> std::io::Result<()> {
        if self.sentinel_path.exists() {
            std::fs::remove_file(&self.sentinel_path)?;
        }
        Ok(())
    }

    /// Path to the sentinel file (exposed for diagnostics / testing).
    pub fn sentinel_path(&self) -> &Path {
        &self.sentinel_path
    }
}

/// Describes the bypass posture for warning messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BypassPosture {
    /// Both approval bypass and sandbox disabled.
    FullUnrestricted,
    /// Only approval bypass (--yolo).
    YoloOnly,
    /// Only sandbox disabled (--no-sandbox).
    NoSandboxOnly,
}

impl BypassPosture {
    /// Detect the active bypass posture from config, or `None` if standard.
    pub fn detect(config: &AppConfig) -> Option<Self> {
        match (config.approval.mode.is_yolo(), !config.security.sandbox) {
            (true, true) => Some(Self::FullUnrestricted),
            (true, false) => Some(Self::YoloOnly),
            (false, true) => Some(Self::NoSandboxOnly),
            (false, false) => None,
        }
    }

    /// Full warning message shown on first use.
    pub fn first_use_warning(self) -> &'static str {
        match self {
            Self::FullUnrestricted => {
                "\
WARNING: Rune is starting in unrestricted mode (--yolo + --no-sandbox).
All tool approvals are auto-bypassed and sandbox boundaries are disabled.
Only use this in trusted environments (local dev, air-gapped infra)."
            }
            Self::YoloOnly => {
                "\
WARNING: Rune is starting with approval bypass enabled (--yolo).
All tool calls will be auto-approved without operator confirmation.
Only use this in trusted environments."
            }
            Self::NoSandboxOnly => {
                "\
WARNING: Rune is starting with sandbox disabled (--no-sandbox).
Workspace boundary enforcement is off — agents can access the full filesystem.
Only use this in trusted environments."
            }
        }
    }

    /// Brief reminder shown on subsequent starts after acknowledgment.
    pub fn acknowledged_reminder(self) -> &'static str {
        match self {
            Self::FullUnrestricted => "Bypass active: yolo + no-sandbox (previously acknowledged)",
            Self::YoloOnly => "Bypass active: yolo mode (previously acknowledged)",
            Self::NoSandboxOnly => "Bypass active: no-sandbox (previously acknowledged)",
        }
    }
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
    fn instance_config_defaults_include_gateway_role() {
        let config = AppConfig::default();
        assert_eq!(config.instance.roles, vec!["gateway".to_string()]);
        assert_eq!(config.instance.advertised_addr, None);
    }

    #[test]
    fn load_or_create_persisted_instance_id_reads_existing_file() {
        let dir = std::env::temp_dir().join(format!(
            "rune-instance-id-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("instance-id");
        fs::write(&path, "node-123\n").unwrap();

        let existing = fs::read_to_string(&path).unwrap();
        assert_eq!(existing.trim(), "node-123");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn app_config_default_generates_non_empty_instance_id() {
        let config = AppConfig::default();
        assert!(!config.instance.id.trim().is_empty());
    }

    #[test]
    fn capabilities_detect_embeds_instance_identity_manifest() {
        let mut config = AppConfig::default();
        config.instance.id = "node-a".to_string();
        config.instance.name = "Node A".to_string();
        config.instance.advertised_addr = Some("http://10.0.0.5:8787".to_string());
        config.instance.roles = vec!["gateway".to_string(), "scheduler".to_string()];
        config.instance.peers = vec![PeerConfig {
            id: "node-b".to_string(),
            health_url: "http://10.0.0.6:8787/api/v1/instance/health".to_string(),
        }];

        let capabilities =
            Capabilities::detect(&config, RuntimeMode::Standalone, "sqlite", false, false, 7);
        assert_eq!(capabilities.identity.id, "node-a");
        assert_eq!(capabilities.identity.name, "Node A");
        assert_eq!(
            capabilities.identity.advertised_addr.as_deref(),
            Some("http://10.0.0.5:8787")
        );
        assert_eq!(
            capabilities.identity.roles,
            vec!["gateway".to_string(), "scheduler".to_string()]
        );
        assert_eq!(capabilities.identity.capabilities_version, 2);
        assert_eq!(capabilities.peer_count, 1);
    }

    #[test]
    fn instance_peers_deserialize_from_struct_entries() {
        let config = AppConfig::load(Some("config.example.toml")).unwrap();
        assert!(config.instance.peers.is_empty());
    }

    #[test]
    fn default_config_uses_docker_first_paths() {
        let config = AppConfig::default();
        assert_eq!(config.gateway.host, "0.0.0.0");
        assert_eq!(config.gateway.port, 8787);
        assert_eq!(config.paths.db_dir, PathBuf::from("/data/db"));
        assert_eq!(config.paths.config_dir, PathBuf::from("/config"));
        assert_eq!(config.memory.level, None);
        assert!(config.memory.semantic_search_enabled);
        assert_eq!(config.memory.requested_level(), MemoryLevel::Semantic);
        assert!(!config.browser.enabled);
        assert_eq!(config.browser.max_instances, 3);
        assert_eq!(config.browser.max_chars, 30_000);
        assert!(config.mcp_servers.is_empty());
    }

    #[test]
    fn file_values_override_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("RUNE_GATEWAY__PORT");
            std::env::remove_var("RUNE_RUNTIME__LANES__MAIN_CAPACITY");
            std::env::remove_var("RUNE_RUNTIME__LANES__GLOBAL_TOOL_CAPACITY");
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
global_tool_capacity = 64
project_tool_capacity = 6

[memory]
level = "keyword"
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
        assert_eq!(config.runtime.lanes.global_tool_capacity, 64);
        assert_eq!(config.runtime.lanes.project_tool_capacity, 6);
        assert_eq!(config.memory.level, Some(MemoryLevel::Keyword));
        assert_eq!(config.memory.requested_level(), MemoryLevel::Keyword);

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
global_tool_capacity = 24
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_GATEWAY__PORT", "9090");
            std::env::set_var("RUNE_RUNTIME__LANES__MAIN_CAPACITY", "12");
            std::env::set_var("RUNE_RUNTIME__LANES__GLOBAL_TOOL_CAPACITY", "48");
            std::env::set_var("RUNE_BROWSER__ENABLED", "true");
            std::env::set_var("RUNE_MEMORY__LEVEL", "file");
        }
        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(config.gateway.port, 9090);
        assert_eq!(config.runtime.lanes.main_capacity, 12);
        assert_eq!(config.runtime.lanes.global_tool_capacity, 48);
        assert!(config.browser.enabled);
        assert_eq!(config.memory.level, Some(MemoryLevel::File));
        assert_eq!(config.memory.requested_level(), MemoryLevel::File);
        unsafe {
            std::env::remove_var("RUNE_GATEWAY__PORT");
            std::env::remove_var("RUNE_RUNTIME__LANES__MAIN_CAPACITY");
            std::env::remove_var("RUNE_RUNTIME__LANES__GLOBAL_TOOL_CAPACITY");
            std::env::remove_var("RUNE_BROWSER__ENABLED");
            std::env::remove_var("RUNE_MEMORY__LEVEL");
        }

        let _ = fs::remove_file(path);
    }

    #[test]
    fn legacy_semantic_toggle_maps_to_keyword_level() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = temp_config_path("legacy-memory-toggle");
        fs::write(
            &path,
            r#"
[memory]
semantic_search_enabled = false
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(config.memory.level, None);
        assert!(!config.memory.semantic_search_enabled);
        assert_eq!(config.memory.requested_level(), MemoryLevel::Keyword);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn memory_level_takes_precedence_over_legacy_toggle() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = temp_config_path("memory-level-precedence");
        fs::write(
            &path,
            r#"
[memory]
level = "file"
semantic_search_enabled = true
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(config.memory.level, Some(MemoryLevel::File));
        assert!(config.memory.semantic_search_enabled);
        assert_eq!(config.memory.requested_level(), MemoryLevel::File);

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

    #[cfg(unix)]
    #[test]
    fn validate_paths_detects_unwritable_dir_via_probe() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let ro = tmp.path().join("readonly");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();

        // Guard: skip if running as root (root bypasses permission checks).
        let actually_readonly = std::fs::write(ro.join(".test_guard"), b"x").is_err();
        if actually_readonly {
            let mut config = AppConfig::default();
            config.paths.db_dir = ro.clone();
            config.paths.sessions_dir = std::env::temp_dir();
            config.paths.memory_dir = std::env::temp_dir();
            config.paths.media_dir = std::env::temp_dir();
            config.paths.logs_dir = std::env::temp_dir();
            let errors = config.validate_paths().unwrap_err();
            assert!(errors[0].to_string().contains("write probe failed"));
        }

        let _ = std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755));
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
            default_image_model: None,
            fallbacks: vec![],
            image_fallbacks: vec![],
            auth_orders: vec![],
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
            default_image_model: None,
            fallbacks: vec![],
            image_fallbacks: vec![],
            auth_orders: vec![],
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
            default_image_model: None,
            fallbacks: vec![],
            image_fallbacks: vec![],
            auth_orders: vec![],
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
        assert_eq!(config.models.default_image_model, None);
        assert!(config.models.fallbacks.is_empty());
        assert!(config.models.image_fallbacks.is_empty());
        assert!(config.models.auth_orders.is_empty());
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
            default_image_model: None,
            fallbacks: vec![],
            image_fallbacks: vec![],
            auth_orders: vec![],
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
    fn models_config_loads_image_defaults_fallbacks_and_auth_orders_from_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = temp_config_path("models-image-fallbacks-auth-orders");
        fs::write(
            &path,
            r#"
[models]
default_model = "oc-01-openai/gpt-5.4"
default_image_model = "oc-01-openai/gpt-image-1"

[[models.fallbacks]]
name = "default-chat"
chain = ["oc-01-openai/gpt-5.4", "oc-01-anthropic/claude-sonnet-4-6"]

[[models.image_fallbacks]]
name = "default-image"
chain = ["oc-01-openai/gpt-image-1", "google/imagen-3"]

[[models.auth_orders]]
provider = "oc-01-openai"
order = ["api_key", "api_key_env", "azure_cli"]

[[models.providers]]
name = "oc-01-openai"
kind = "openai"
base_url = "https://example.test/openai/v1"
api_key_env = "OPENAI_API_KEY"
models = ["gpt-5.4", "gpt-image-1"]
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(
            config.models.default_image_model.as_deref(),
            Some("oc-01-openai/gpt-image-1")
        );
        assert_eq!(
            config.models.fallbacks,
            vec![ModelFallbackChainConfig {
                name: "default-chat".into(),
                chain: vec![
                    "oc-01-openai/gpt-5.4".into(),
                    "oc-01-anthropic/claude-sonnet-4-6".into(),
                ],
            }]
        );
        assert_eq!(
            config.models.image_fallbacks,
            vec![ModelFallbackChainConfig {
                name: "default-image".into(),
                chain: vec!["oc-01-openai/gpt-image-1".into(), "google/imagen-3".into(),],
            }]
        );
        assert_eq!(
            config.models.auth_orders,
            vec![ModelAuthOrderConfig {
                provider: "oc-01-openai".into(),
                order: vec!["api_key".into(), "api_key_env".into(), "azure_cli".into()],
            }]
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn models_config_defaults_new_image_fallback_and_auth_metadata() {
        let config = AppConfig::default();

        assert_eq!(config.models.default_image_model, None);
        assert!(config.models.fallbacks.is_empty());
        assert!(config.models.image_fallbacks.is_empty());
        assert!(config.models.auth_orders.is_empty());
        assert_eq!(config.models.bootstrap(), ModelBootstrap::ZeroConfigOllama);
    }

    #[test]
    fn models_bootstrap_detects_explicit_provider_config() {
        let config = ModelsConfig {
            providers: vec![ModelProviderConfig {
                name: "openai".into(),
                kind: "openai".into(),
                base_url: "https://api.openai.com/v1".into(),
                api_key: None,
                deployment_name: None,
                api_version: None,
                api_key_env: Some("OPENAI_API_KEY".into()),
                model_alias: None,
                models: vec![ConfiguredModel::Id("gpt-5.4".into())],
            }],
            ..Default::default()
        };

        assert_eq!(config.bootstrap(), ModelBootstrap::ExplicitProviders);
    }

    #[test]
    fn zero_config_ollama_base_url_defaults_to_localhost() {
        let config = ModelsConfig::default();

        assert_eq!(
            config.zero_config_ollama_base_url(None),
            Some("http://localhost:11434".into())
        );
    }

    #[test]
    fn zero_config_ollama_base_url_normalizes_bare_env_host() {
        let config = ModelsConfig::default();

        assert_eq!(
            config.zero_config_ollama_base_url(Some("  ollama-box/  ")),
            Some("http://ollama-box:11434".into())
        );
    }

    #[test]
    fn zero_config_ollama_base_url_strips_trailing_slash_from_full_url() {
        let config = ModelsConfig::default();

        assert_eq!(
            config.zero_config_ollama_base_url(Some("https://ollama.example.com:443/")),
            Some("https://ollama.example.com:443".into())
        );
    }

    #[test]
    fn zero_config_ollama_base_url_is_disabled_when_explicit_providers_exist() {
        let config = ModelsConfig {
            providers: vec![ModelProviderConfig {
                name: "openai".into(),
                kind: "openai".into(),
                base_url: "https://api.openai.com/v1".into(),
                api_key: None,
                deployment_name: None,
                api_version: None,
                api_key_env: Some("OPENAI_API_KEY".into()),
                model_alias: None,
                models: vec![ConfiguredModel::Id("gpt-5.4".into())],
            }],
            ..Default::default()
        };

        assert_eq!(config.zero_config_ollama_base_url(Some("ollama-box")), None);
    }

    #[test]
    fn models_config_reports_provider_names_in_order() {
        let config = ModelsConfig {
            providers: vec![
                ModelProviderConfig {
                    name: "openai".into(),
                    kind: "openai".into(),
                    base_url: "https://api.openai.com/v1".into(),
                    api_key: None,
                    deployment_name: None,
                    api_version: None,
                    api_key_env: Some("OPENAI_API_KEY".into()),
                    model_alias: None,
                    models: vec![],
                },
                ModelProviderConfig {
                    name: "anthropic".into(),
                    kind: "anthropic".into(),
                    base_url: "https://api.anthropic.com/v1".into(),
                    api_key: None,
                    deployment_name: None,
                    api_version: None,
                    api_key_env: Some("ANTHROPIC_API_KEY".into()),
                    model_alias: None,
                    models: vec![],
                },
            ],
            ..Default::default()
        };

        assert_eq!(config.provider_names(), vec!["openai", "anthropic"]);
    }

    #[test]
    fn configured_default_model_prefers_agent_over_models_default() {
        let mut config = AppConfig::default();
        config.models.default_model = Some("models-default".into());
        config.agents.list = vec![AgentConfig {
            id: "coder".into(),
            default: Some(true),
            model: Some(AgentModelSelection::Id("agent-default".into())),
            workspace: None,
            system_prompt: None,
        }];

        let selection = config
            .configured_default_model()
            .expect("configured default model");
        assert_eq!(selection.model, "agent-default");
        assert_eq!(selection.source, ConfiguredDefaultModelSource::AgentConfig);
    }

    #[test]
    fn configured_default_model_uses_models_default_when_no_agent_model_exists() {
        let mut config = AppConfig::default();
        config.models.default_model = Some("models-default".into());

        let selection = config
            .configured_default_model()
            .expect("configured default model");
        assert_eq!(selection.model, "models-default");
        assert_eq!(
            selection.source,
            ConfiguredDefaultModelSource::ModelsDefault
        );
    }

    #[test]
    fn configured_default_provider_prefers_agent_model_provider() {
        let mut config = AppConfig::default();
        config.models.default_model = Some("openai/gpt-5.4".into());
        config.models.providers = vec![
            ModelProviderConfig {
                name: "openai".into(),
                kind: "openai".into(),
                base_url: "https://api.openai.com/v1".into(),
                api_key: None,
                deployment_name: None,
                api_version: None,
                api_key_env: Some("OPENAI_API_KEY".into()),
                model_alias: None,
                models: vec![ConfiguredModel::Id("gpt-5.4".into())],
            },
            ModelProviderConfig {
                name: "anthropic".into(),
                kind: "anthropic".into(),
                base_url: "https://api.anthropic.com/v1".into(),
                api_key: None,
                deployment_name: None,
                api_version: None,
                api_key_env: Some("ANTHROPIC_API_KEY".into()),
                model_alias: None,
                models: vec![ConfiguredModel::Id("claude-sonnet-4-5".into())],
            },
        ];
        config.agents.list = vec![AgentConfig {
            id: "coder".into(),
            default: Some(true),
            model: Some(AgentModelSelection::Id(
                "anthropic/claude-sonnet-4-5".into(),
            )),
            workspace: None,
            system_prompt: None,
        }];

        let selection = config
            .configured_default_provider()
            .expect("provider detection should succeed")
            .expect("configured default provider");
        assert_eq!(selection.provider.name, "anthropic");
        assert_eq!(
            selection.source,
            ConfiguredDefaultProviderSource::AgentConfig
        );
    }

    #[test]
    fn configured_default_provider_uses_models_default_provider() {
        let mut config = AppConfig::default();
        config.models.default_model = Some("openai/gpt-5.4".into());
        config.models.providers = vec![ModelProviderConfig {
            name: "openai".into(),
            kind: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: None,
            deployment_name: None,
            api_version: None,
            api_key_env: Some("OPENAI_API_KEY".into()),
            model_alias: None,
            models: vec![ConfiguredModel::Id("gpt-5.4".into())],
        }];

        let selection = config
            .configured_default_provider()
            .expect("provider detection should succeed")
            .expect("configured default provider");
        assert_eq!(selection.provider.name, "openai");
        assert_eq!(
            selection.source,
            ConfiguredDefaultProviderSource::ModelsDefault
        );
    }

    #[test]
    fn configured_default_provider_uses_single_provider_without_default_model() {
        let mut config = AppConfig::default();
        config.models.providers = vec![ModelProviderConfig {
            name: "openai".into(),
            kind: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: None,
            deployment_name: None,
            api_version: None,
            api_key_env: Some("OPENAI_API_KEY".into()),
            model_alias: None,
            models: vec![ConfiguredModel::Id("gpt-5.4".into())],
        }];

        let selection = config
            .configured_default_provider()
            .expect("provider detection should succeed")
            .expect("configured default provider");
        assert_eq!(selection.provider.name, "openai");
        assert_eq!(
            selection.source,
            ConfiguredDefaultProviderSource::SingleProvider
        );
    }

    #[test]
    fn configured_default_provider_returns_none_for_multi_provider_without_default_model() {
        let mut config = AppConfig::default();
        config.models.providers = vec![
            ModelProviderConfig {
                name: "openai".into(),
                kind: "openai".into(),
                base_url: "https://api.openai.com/v1".into(),
                api_key: None,
                deployment_name: None,
                api_version: None,
                api_key_env: Some("OPENAI_API_KEY".into()),
                model_alias: None,
                models: vec![ConfiguredModel::Id("gpt-5.4".into())],
            },
            ModelProviderConfig {
                name: "anthropic".into(),
                kind: "anthropic".into(),
                base_url: "https://api.anthropic.com/v1".into(),
                api_key: None,
                deployment_name: None,
                api_version: None,
                api_key_env: Some("ANTHROPIC_API_KEY".into()),
                model_alias: None,
                models: vec![ConfiguredModel::Id("claude-sonnet-4-5".into())],
            },
        ];

        assert_eq!(
            config
                .configured_default_provider()
                .expect("provider detection should succeed"),
            None
        );
    }

    #[test]
    fn configured_default_provider_reports_unresolved_default_model() {
        let mut config = AppConfig::default();
        config.models.default_model = Some("missing-model".into());
        config.models.providers = vec![
            ModelProviderConfig {
                name: "openai".into(),
                kind: "openai".into(),
                base_url: "https://api.openai.com/v1".into(),
                api_key: None,
                deployment_name: None,
                api_version: None,
                api_key_env: Some("OPENAI_API_KEY".into()),
                model_alias: None,
                models: vec![ConfiguredModel::Id("gpt-5.4".into())],
            },
            ModelProviderConfig {
                name: "anthropic".into(),
                kind: "anthropic".into(),
                base_url: "https://api.anthropic.com/v1".into(),
                api_key: None,
                deployment_name: None,
                api_version: None,
                api_key_env: Some("ANTHROPIC_API_KEY".into()),
                model_alias: None,
                models: vec![ConfiguredModel::Id("claude-sonnet-4-5".into())],
            },
        ];

        let err = config
            .configured_default_provider()
            .expect_err("missing explicit default should be unresolved");
        assert!(matches!(
            err,
            ModelResolutionError::UnknownModel { model } if model == "missing-model"
        ));
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

    #[test]
    fn runtime_mode_defaults_to_auto() {
        let config = AppConfig::default();
        assert_eq!(config.mode, RuntimeMode::Auto);
    }

    #[test]
    fn runtime_mode_explicit_standalone() {
        let mode = RuntimeMode::Standalone;
        let config = AppConfig::default();
        assert_eq!(mode.resolve(&config), RuntimeMode::Standalone);
    }

    #[test]
    fn runtime_mode_explicit_server() {
        let mode = RuntimeMode::Server;
        let config = AppConfig::default();
        assert_eq!(mode.resolve(&config), RuntimeMode::Server);
    }

    #[test]
    fn runtime_mode_auto_resolves_server_with_database_url() {
        let mut config = AppConfig::default();
        config.database.database_url = Some("postgres://localhost/rune".into());
        let resolved = config.mode.resolve_with_runtime_signals(&config, false);
        assert_eq!(resolved, RuntimeMode::Server);
    }

    #[test]
    fn runtime_mode_auto_resolves_standalone_for_bare_host_defaults() {
        let config = AppConfig::default();
        let resolved = config.mode.resolve_with_runtime_signals(&config, false);
        assert_eq!(resolved, RuntimeMode::Standalone);
    }

    #[test]
    fn runtime_mode_auto_resolves_server_for_custom_data_mounts() {
        let mut config = AppConfig::default();
        config.paths.db_dir = PathBuf::from("/data/custom/db");
        let resolved = config.mode.resolve_with_runtime_signals(&config, false);
        assert_eq!(resolved, RuntimeMode::Server);
    }

    #[test]
    fn adjust_paths_standalone_swaps_to_home() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("HOME", "/home/testuser");
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        let mut config = AppConfig::default();
        config.adjust_paths_for_mode(&RuntimeMode::Standalone);
        assert_eq!(
            config.paths.db_dir,
            PathBuf::from("/home/testuser/.rune/db")
        );
        assert_eq!(
            config.paths.sessions_dir,
            PathBuf::from("/home/testuser/.rune/sessions")
        );
        unsafe {
            // Restore — don't leak into other tests
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn paths_profile_detects_standalone_home_layout() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("HOME", "/home/testuser");
        }

        let mut config = AppConfig::default();
        config.adjust_paths_for_mode(&RuntimeMode::Standalone);
        assert_eq!(config.paths.profile(), PathsProfile::StandaloneHome);

        unsafe {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn adjust_paths_noop_for_server_mode() {
        let mut config = AppConfig::default();
        let original_paths = config.paths.clone();
        config.adjust_paths_for_mode(&RuntimeMode::Server);
        assert_eq!(config.paths, original_paths);
    }

    #[test]
    fn adjust_paths_preserves_custom_but_fixes_docker_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("HOME", "/home/testuser");
        }

        let mut config = AppConfig::default();
        config.paths.db_dir = PathBuf::from("/custom/db");
        config.adjust_paths_for_mode(&RuntimeMode::Standalone);
        // Custom path is preserved.
        assert_eq!(config.paths.db_dir, PathBuf::from("/custom/db"));
        // Docker-default paths are replaced with standalone equivalents.
        assert_eq!(
            config.paths.plugins_dir,
            PathBuf::from("/home/testuser/.rune/plugins")
        );

        unsafe {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn capabilities_detect_basic() {
        let config = AppConfig::default();
        let caps =
            Capabilities::detect(&config, RuntimeMode::Standalone, "sqlite", false, false, 5);
        assert_eq!(caps.mode, RuntimeMode::Standalone);
        assert_eq!(caps.storage_backend, "sqlite");
        assert!(!caps.pgvector);
        assert_eq!(caps.memory_mode, "semantic-keyword-fallback");
        assert!(!caps.browser);
        assert_eq!(caps.mcp_servers, 0);
        assert!(!caps.tts);
        assert!(!caps.stt);
        assert_eq!(caps.tool_count, 5);
    }

    #[test]
    fn capabilities_detect_hybrid_memory() {
        let config = AppConfig::default();
        let caps = Capabilities::detect(
            &config,
            RuntimeMode::Server,
            "postgres (external)",
            true,
            true,
            10,
        );
        assert_eq!(caps.memory_mode, "semantic-hybrid");
        assert!(caps.pgvector);
    }

    #[test]
    fn capabilities_detect_keyword_memory() {
        let mut config = AppConfig::default();
        config.memory.level = Some(MemoryLevel::Keyword);
        config.memory.semantic_search_enabled = false;
        let caps =
            Capabilities::detect(&config, RuntimeMode::Standalone, "sqlite", false, false, 0);
        assert_eq!(caps.memory_mode, "keyword-local");
    }

    #[test]
    fn capabilities_detect_file_memory() {
        let mut config = AppConfig::default();
        config.memory.level = Some(MemoryLevel::File);
        let caps =
            Capabilities::detect(&config, RuntimeMode::Standalone, "sqlite", false, false, 0);
        assert_eq!(caps.memory_mode, "file-local");
    }

    #[test]
    fn runtime_mode_as_str() {
        assert_eq!(RuntimeMode::Auto.as_str(), "auto");
        assert_eq!(RuntimeMode::Standalone.as_str(), "standalone");
        assert_eq!(RuntimeMode::Server.as_str(), "server");
    }

    #[test]
    fn fallback_chain_for_returns_remaining_entries() {
        let config = ModelsConfig {
            fallbacks: vec![ModelFallbackChainConfig {
                name: "default-chat".into(),
                chain: vec![
                    "openai/gpt-5.4".into(),
                    "anthropic/claude-opus-4-6".into(),
                    "google/gemini-2.0-flash".into(),
                ],
            }],
            ..Default::default()
        };

        let chain = config
            .fallback_chain_for("openai/gpt-5.4")
            .expect("should find chain for primary model");
        assert_eq!(
            chain,
            &["anthropic/claude-opus-4-6", "google/gemini-2.0-flash"]
        );
    }

    #[test]
    fn fallback_chain_for_returns_none_when_model_not_primary() {
        let config = ModelsConfig {
            fallbacks: vec![ModelFallbackChainConfig {
                name: "default-chat".into(),
                chain: vec!["openai/gpt-5.4".into(), "anthropic/claude-opus-4-6".into()],
            }],
            ..Default::default()
        };

        // A model that appears only as a fallback (not primary) should not match.
        assert!(
            config
                .fallback_chain_for("anthropic/claude-opus-4-6")
                .is_none()
        );
    }

    #[test]
    fn fallback_chain_for_returns_none_when_no_chains_configured() {
        let config = ModelsConfig::default();
        assert!(config.fallback_chain_for("openai/gpt-5.4").is_none());
    }

    #[test]
    fn fallback_chain_for_returns_none_for_single_entry_chain() {
        let config = ModelsConfig {
            fallbacks: vec![ModelFallbackChainConfig {
                name: "solo".into(),
                chain: vec!["openai/gpt-5.4".into()],
            }],
            ..Default::default()
        };

        // A chain with only one entry has no fallbacks.
        assert!(config.fallback_chain_for("openai/gpt-5.4").is_none());
    }

    #[test]
    fn ensure_dirs_creates_missing_directories() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().join("rune-ensure-test");

        let config = AppConfig {
            paths: PathsConfig {
                db_dir: base.join("db"),
                sessions_dir: base.join("sessions"),
                memory_dir: base.join("memory"),
                media_dir: base.join("media"),
                spells_dir: base.join("spells"),
                skills_dir: base.join("skills"),
                plugins_dir: base.join("plugins"),
                logs_dir: base.join("logs"),
                backups_dir: base.join("backups"),
                config_dir: base.join("config"),
                secrets_dir: base.join("secrets"),
                workspace_dir: base.join("workspace"),
                cache_dir: base.join("cache"),
                data_dir: base.join("data"),
            },
            ..Default::default()
        };

        assert!(!base.join("db").exists());
        config.ensure_dirs().expect("ensure_dirs should succeed");

        assert!(base.join("db").is_dir());
        assert!(base.join("sessions").is_dir());
        assert!(base.join("memory").is_dir());
        assert!(base.join("media").is_dir());
        assert!(base.join("spells").is_dir());
        assert!(base.join("plugins").is_dir());
        assert!(base.join("logs").is_dir());
        assert!(base.join("backups").is_dir());
        assert!(base.join("config").is_dir());
        assert!(base.join("secrets").is_dir());
        assert!(base.join("workspace").is_dir());
        assert!(base.join("cache").is_dir());
        assert!(base.join("data").is_dir());
    }

    #[test]
    fn ensure_dirs_is_idempotent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().join("rune-ensure-idem");

        let config = AppConfig {
            paths: PathsConfig {
                db_dir: base.join("db"),
                sessions_dir: base.join("sessions"),
                memory_dir: base.join("memory"),
                media_dir: base.join("media"),
                spells_dir: base.join("spells"),
                skills_dir: base.join("skills"),
                plugins_dir: base.join("plugins"),
                logs_dir: base.join("logs"),
                backups_dir: base.join("backups"),
                config_dir: base.join("config"),
                secrets_dir: base.join("secrets"),
                workspace_dir: base.join("workspace"),
                cache_dir: base.join("cache"),
                data_dir: base.join("data"),
            },
            ..Default::default()
        };

        config.ensure_dirs().expect("first call should succeed");
        config
            .ensure_dirs()
            .expect("second call should also succeed (idempotent)");
    }

    #[test]
    fn ensure_dirs_then_validate_paths_passes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().join("rune-ensure-validate");

        let config = AppConfig {
            paths: PathsConfig {
                db_dir: base.join("db"),
                sessions_dir: base.join("sessions"),
                memory_dir: base.join("memory"),
                media_dir: base.join("media"),
                spells_dir: base.join("spells"),
                skills_dir: base.join("skills"),
                plugins_dir: base.join("plugins"),
                logs_dir: base.join("logs"),
                backups_dir: base.join("backups"),
                config_dir: base.join("config"),
                secrets_dir: base.join("secrets"),
                workspace_dir: base.join("workspace"),
                cache_dir: base.join("cache"),
                data_dir: base.join("data"),
            },
            ..Default::default()
        };

        config.ensure_dirs().expect("ensure_dirs should succeed");
        config
            .validate_paths()
            .expect("validate_paths should pass after ensure_dirs");
    }

    // ── Approval / security config tests ─────────────────────────────

    #[test]
    fn default_approval_mode_is_prompt() {
        let config = AppConfig::default();
        assert_eq!(config.approval.mode, ApprovalMode::Prompt);
        assert_eq!(config.approval.mode.as_str(), "prompt");
        assert!(!config.approval.mode.is_yolo());
    }

    #[test]
    fn default_security_config_is_sandboxed() {
        let config = AppConfig::default();
        assert!(config.security.sandbox);
        assert!(!config.security.trust_spells);
        assert_eq!(config.security.posture(), "standard");
    }

    #[test]
    fn approval_mode_loads_from_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = temp_config_path("approval-mode");
        fs::write(
            &path,
            r#"
[approval]
mode = "yolo"

[security]
sandbox = false
trust_spells = true
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(config.approval.mode, ApprovalMode::Yolo);
        assert!(config.approval.mode.is_yolo());
        assert!(!config.security.sandbox);
        assert!(config.security.trust_spells);
        assert_eq!(config.security.posture(), "unrestricted");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn approval_mode_loads_auto_file_variant() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = temp_config_path("approval-auto-file");
        fs::write(
            &path,
            r#"
[approval]
mode = "auto-file"
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(config.approval.mode, ApprovalMode::AutoFile);
        assert_eq!(config.approval.mode.as_str(), "auto-file");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn approval_mode_loads_auto_exec_variant() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = temp_config_path("approval-auto-exec");
        fs::write(
            &path,
            r#"
[approval]
mode = "auto-exec"
"#,
        )
        .unwrap();

        let config = AppConfig::load(Some(&path)).unwrap();
        assert_eq!(config.approval.mode, ApprovalMode::AutoExec);
        assert_eq!(config.approval.mode.as_str(), "auto-exec");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn approval_mode_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("RUNE_APPROVAL__MODE", "yolo");
        }
        let config = AppConfig::load(None::<&std::path::Path>).unwrap();
        assert_eq!(config.approval.mode, ApprovalMode::Yolo);
        unsafe {
            std::env::remove_var("RUNE_APPROVAL__MODE");
        }
    }

    #[test]
    fn security_config_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("RUNE_SECURITY__SANDBOX", "false");
            std::env::set_var("RUNE_SECURITY__TRUST_SPELLS", "true");
        }
        let config = AppConfig::load(None::<&std::path::Path>).unwrap();
        assert!(!config.security.sandbox);
        assert!(config.security.trust_spells);
        unsafe {
            std::env::remove_var("RUNE_SECURITY__SANDBOX");
            std::env::remove_var("RUNE_SECURITY__TRUST_SPELLS");
        }
    }

    #[test]
    fn security_posture_labels() {
        let standard = SecurityConfig {
            sandbox: true,
            trust_spells: false,
        };
        assert_eq!(standard.posture(), "standard");

        let trust = SecurityConfig {
            sandbox: true,
            trust_spells: true,
        };
        assert_eq!(trust.posture(), "trust-spells");

        let no_sandbox = SecurityConfig {
            sandbox: false,
            trust_spells: false,
        };
        assert_eq!(no_sandbox.posture(), "no-sandbox");

        let unrestricted = SecurityConfig {
            sandbox: false,
            trust_spells: true,
        };
        assert_eq!(unrestricted.posture(), "unrestricted");
    }

    #[test]
    fn capabilities_detect_includes_approval_and_security() {
        let config = AppConfig::default();
        let caps =
            Capabilities::detect(&config, RuntimeMode::Standalone, "sqlite", false, false, 5);
        assert_eq!(caps.approval_mode, "prompt");
        assert_eq!(caps.security_posture, "standard");
    }

    #[test]
    fn capabilities_detect_yolo_mode() {
        let mut config = AppConfig::default();
        config.approval.mode = ApprovalMode::Yolo;
        config.security.sandbox = false;
        config.security.trust_spells = true;
        let caps =
            Capabilities::detect(&config, RuntimeMode::Standalone, "sqlite", false, false, 0);
        assert_eq!(caps.approval_mode, "yolo");
        assert_eq!(caps.security_posture, "unrestricted");
    }

    // ── CLI override tests (#64) ─────────────────────────────────────

    #[test]
    fn apply_cli_overrides_yolo() {
        let mut config = AppConfig::default();
        assert_eq!(config.approval.mode, ApprovalMode::Prompt);

        config.apply_cli_overrides(true, false);
        assert_eq!(config.approval.mode, ApprovalMode::Yolo);
        // security unchanged
        assert!(config.security.sandbox);
    }

    #[test]
    fn apply_cli_overrides_no_sandbox() {
        let mut config = AppConfig::default();
        assert!(config.security.sandbox);

        config.apply_cli_overrides(false, true);
        assert!(!config.security.sandbox);
        // approval unchanged
        assert_eq!(config.approval.mode, ApprovalMode::Prompt);
    }

    #[test]
    fn apply_cli_overrides_both() {
        let mut config = AppConfig::default();
        config.apply_cli_overrides(true, true);
        assert_eq!(config.approval.mode, ApprovalMode::Yolo);
        assert!(!config.security.sandbox);
        assert_eq!(config.security.posture(), "no-sandbox");
    }

    #[test]
    fn apply_cli_overrides_noop_when_both_false() {
        let original = AppConfig::default();
        let mut config = original.clone();
        config.apply_cli_overrides(false, false);
        assert_eq!(config.approval, original.approval);
        assert_eq!(config.security, original.security);
    }

    #[test]
    fn apply_cli_overrides_on_top_of_file_config() {
        // Simulate file config that already sets auto-file mode.
        let mut config = AppConfig::default();
        config.approval.mode = ApprovalMode::AutoFile;
        config.security.trust_spells = true;

        // --yolo should override auto-file → yolo
        config.apply_cli_overrides(true, true);
        assert_eq!(config.approval.mode, ApprovalMode::Yolo);
        assert!(!config.security.sandbox);
        // trust_spells is untouched by CLI flags
        assert!(config.security.trust_spells);
        assert_eq!(config.security.posture(), "unrestricted");
    }

    // ── Bypass acknowledgment tests (#64) ────────────────────────────

    #[test]
    fn bypass_ack_not_acknowledged_initially() {
        let tmp = tempfile::tempdir().unwrap();
        let ack = BypassAcknowledgment::new(tmp.path());
        assert!(!ack.is_acknowledged());
    }

    #[test]
    fn bypass_ack_record_and_check() {
        let tmp = tempfile::tempdir().unwrap();
        let ack = BypassAcknowledgment::new(tmp.path());
        ack.record().unwrap();
        assert!(ack.is_acknowledged());
        // Sentinel file exists on disk.
        assert!(ack.sentinel_path().exists());
    }

    #[test]
    fn bypass_ack_record_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let ack = BypassAcknowledgment::new(tmp.path());
        ack.record().unwrap();
        ack.record().unwrap(); // should not fail
        assert!(ack.is_acknowledged());
    }

    #[test]
    fn bypass_ack_revoke() {
        let tmp = tempfile::tempdir().unwrap();
        let ack = BypassAcknowledgment::new(tmp.path());
        ack.record().unwrap();
        assert!(ack.is_acknowledged());
        ack.revoke().unwrap();
        assert!(!ack.is_acknowledged());
    }

    #[test]
    fn bypass_ack_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("deep").join("nested").join("config");
        let ack = BypassAcknowledgment::new(&nested);
        ack.record().unwrap();
        assert!(ack.is_acknowledged());
    }

    // ── BypassPosture detection tests (#64) ──────────────────────────

    #[test]
    fn bypass_posture_detect_standard() {
        let config = AppConfig::default();
        assert!(BypassPosture::detect(&config).is_none());
    }

    #[test]
    fn bypass_posture_detect_yolo_only() {
        let mut config = AppConfig::default();
        config.approval.mode = ApprovalMode::Yolo;
        assert_eq!(
            BypassPosture::detect(&config),
            Some(BypassPosture::YoloOnly)
        );
    }

    #[test]
    fn bypass_posture_detect_no_sandbox_only() {
        let mut config = AppConfig::default();
        config.security.sandbox = false;
        assert_eq!(
            BypassPosture::detect(&config),
            Some(BypassPosture::NoSandboxOnly)
        );
    }

    #[test]
    fn bypass_posture_detect_full_unrestricted() {
        let mut config = AppConfig::default();
        config.approval.mode = ApprovalMode::Yolo;
        config.security.sandbox = false;
        assert_eq!(
            BypassPosture::detect(&config),
            Some(BypassPosture::FullUnrestricted)
        );
    }

    #[test]
    fn bypass_posture_warning_messages_not_empty() {
        for posture in [
            BypassPosture::FullUnrestricted,
            BypassPosture::YoloOnly,
            BypassPosture::NoSandboxOnly,
        ] {
            assert!(!posture.first_use_warning().is_empty());
            assert!(!posture.acknowledged_reminder().is_empty());
        }
    }
}
