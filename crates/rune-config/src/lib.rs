#![doc = "Layered application configuration for Rune."]

use std::path::PathBuf;

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Top-level application configuration resolved from defaults, files, and environment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    pub gateway: GatewayConfig,
    pub database: DatabaseConfig,
    pub models: ModelsConfig,
    pub channels: ChannelsConfig,
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

        figment.extract().map_err(|e| ConfigError::Load(Box::new(e)))
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
            } else if path.metadata().map(|m| m.permissions().readonly()).unwrap_or(true) {
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
    pub providers: Vec<ModelProviderConfig>,
}

/// A single configured model-provider target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProviderConfig {
    pub provider_name: String,
    pub endpoint: String,
    pub deployment_name: Option<String>,
    pub api_version: Option<String>,
    pub api_key_env: Option<String>,
    pub model_alias: Option<String>,
}

/// Channel adapter inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    pub enabled: Vec<String>,
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
}
