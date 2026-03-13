//! `rune doctor` — diagnostic checks for the Rune installation.
//!
//! Implements check families aligned with OpenClaw parity:
//! - Config: parse/validation state and layered-resolution viability
//! - Database: PostgreSQL configuration posture and embedded fallback viability
//! - Gateway: HTTP health endpoint reachability
//! - Models: provider configuration completeness and credential references
//! - Channels: channel credential completeness where configured
//! - Workspace: required workspace files exist
//! - Paths/Docker: persistent mount/path existence and writability sanity
//! - Memory: long-term + daily-memory/search path availability
//! - Scheduler: cron/runtime storage-path sanity

use std::path::Path;
use std::time::Duration;

use rune_config::AppConfig;
use serde::Serialize;

/// Result of a single diagnostic check.
#[derive(Clone, Debug, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub category: String,
    pub status: CheckStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Status of a check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skip,
}

/// Run all diagnostic checks and return results.
pub async fn run_all_checks(
    config_path: Option<&Path>,
    gateway_url: Option<&str>,
    workspace_root: Option<&Path>,
) -> Vec<CheckResult> {
    let mut results = Vec::new();
    let loaded_config = load_config_state(config_path);

    results.push(config_load_result(&loaded_config, config_path));

    if let Ok(config) = loaded_config.as_ref() {
        results.extend(check_paths(config).await);
        results.extend(check_database_config(config).await);
        results.extend(check_models_config(config).await);
        results.extend(check_channels_config(config).await);
        results.extend(check_memory_config(config).await);
        results.extend(check_scheduler_config(config).await);
    }

    if let Some(ws) = workspace_root {
        results.extend(check_workspace(ws).await);
    }

    if let Some(url) = gateway_url {
        results.push(check_gateway(url).await);
    }

    results
}

fn load_config_state(config_path: Option<&Path>) -> Result<AppConfig, String> {
    AppConfig::load(config_path).map_err(|e| e.to_string())
}

fn config_load_result(config: &Result<AppConfig, String>, path: Option<&Path>) -> CheckResult {
    let name = "config.load".to_string();
    let category = "config".to_string();

    match config {
        Ok(_) => CheckResult {
            name,
            category,
            status: CheckStatus::Pass,
            message: match path {
                Some(p) => format!("Configuration loaded from {}", p.display()),
                None => "Configuration loaded from defaults/environment".into(),
            },
            hint: None,
        },
        Err(error) => CheckResult {
            name,
            category,
            status: CheckStatus::Fail,
            message: format!("Failed to load configuration: {error}"),
            hint: Some("Fix config syntax/values before rerunning doctor".into()),
        },
    }
}

async fn check_paths(config: &AppConfig) -> Vec<CheckResult> {
    let mut results = Vec::new();

    for (name, path, required_persistent) in [
        ("paths.db_dir", &config.paths.db_dir, true),
        ("paths.sessions_dir", &config.paths.sessions_dir, true),
        ("paths.memory_dir", &config.paths.memory_dir, true),
        ("paths.media_dir", &config.paths.media_dir, true),
        ("paths.skills_dir", &config.paths.skills_dir, true),
        ("paths.logs_dir", &config.paths.logs_dir, true),
        ("paths.backups_dir", &config.paths.backups_dir, true),
        ("paths.config_dir", &config.paths.config_dir, true),
        ("paths.secrets_dir", &config.paths.secrets_dir, true),
    ] {
        results.push(check_single_path(name, path, required_persistent));
    }

    results.push(check_docker_path_layout(config));
    results
}

fn check_single_path(name: &str, path: &Path, required_persistent: bool) -> CheckResult {
    let category = "paths".to_string();

    if !path.exists() {
        return CheckResult {
            name: name.into(),
            category,
            status: if required_persistent {
                CheckStatus::Warn
            } else {
                CheckStatus::Skip
            },
            message: format!("{} is missing", path.display()),
            hint: Some("Create/mount this path before production use".into()),
        };
    }

    if !path.is_dir() {
        return CheckResult {
            name: name.into(),
            category,
            status: CheckStatus::Fail,
            message: format!("{} exists but is not a directory", path.display()),
            hint: Some("Replace it with a writable directory".into()),
        };
    }

    let readonly = path
        .metadata()
        .map(|m| m.permissions().readonly())
        .unwrap_or(true);

    if readonly {
        return CheckResult {
            name: name.into(),
            category,
            status: CheckStatus::Warn,
            message: format!("{} is present but appears read-only", path.display()),
            hint: Some("Doctor expects writable persistent mounts for parity-critical paths".into()),
        };
    }

    CheckResult {
        name: name.into(),
        category,
        status: CheckStatus::Pass,
        message: format!("{} is present and writable", path.display()),
        hint: None,
    }
}

fn check_docker_path_layout(config: &AppConfig) -> CheckResult {
    let dockerish = [
        config.paths.db_dir.starts_with("/data"),
        config.paths.sessions_dir.starts_with("/data"),
        config.paths.memory_dir.starts_with("/data"),
        config.paths.media_dir.starts_with("/data"),
        config.paths.logs_dir.starts_with("/data"),
        config.paths.backups_dir.starts_with("/data"),
        config.paths.config_dir.starts_with("/config"),
        config.paths.secrets_dir.starts_with("/secrets"),
    ]
    .into_iter()
    .all(|v| v);

    CheckResult {
        name: "docker.layout".into(),
        category: "docker".into(),
        status: if dockerish {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        message: if dockerish {
            "Persistent/config paths follow Docker-first mount conventions".into()
        } else {
            "One or more paths diverge from the Docker-first /data,/config,/secrets layout".into()
        },
        hint: if dockerish {
            None
        } else {
            Some("Release-target deployments should keep durable state under /data/* and config/secrets under /config/*, /secrets/*".into())
        },
    }
}

async fn check_database_config(config: &AppConfig) -> Vec<CheckResult> {
    let mut results = Vec::new();

    results.push(if let Some(url) = &config.database.database_url {
        let scheme = url.split(':').next().unwrap_or_default();
        CheckResult {
            name: "database.url".into(),
            category: "database".into(),
            status: if scheme.starts_with("postgres") {
                CheckStatus::Pass
            } else {
                CheckStatus::Warn
            },
            message: format!("database_url configured with scheme `{scheme}`"),
            hint: if scheme.starts_with("postgres") {
                None
            } else {
                Some("Expected postgres:// or postgresql:// URL for parity target".into())
            },
        }
    } else {
        CheckResult {
            name: "database.url".into(),
            category: "database".into(),
            status: CheckStatus::Warn,
            message: "database_url not set; runtime will rely on embedded PostgreSQL fallback".into(),
            hint: Some("Fine for zero-config local dev; production should usually point at durable PostgreSQL".into()),
        }
    });

    results.push(CheckResult {
        name: "database.migrations".into(),
        category: "database".into(),
        status: if config.database.run_migrations {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        message: if config.database.run_migrations {
            "Automatic migrations are enabled".into()
        } else {
            "Automatic migrations are disabled".into()
        },
        hint: if config.database.run_migrations {
            None
        } else {
            Some("Ensure schema/version compatibility is handled out-of-band before startup".into())
        },
    });

    results.push(CheckResult {
        name: "database.pool".into(),
        category: "database".into(),
        status: if config.database.max_connections > 0 {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        message: format!("max_connections = {}", config.database.max_connections),
        hint: if config.database.max_connections > 0 {
            None
        } else {
            Some("Set database.max_connections to a positive value".into())
        },
    });

    results
}

async fn check_models_config(config: &AppConfig) -> Vec<CheckResult> {
    let mut results = Vec::new();

    if config.models.providers.is_empty() {
        results.push(CheckResult {
            name: "models.providers".into(),
            category: "models".into(),
            status: CheckStatus::Warn,
            message: "No model providers configured; runtime will be limited to fallback/demo behavior".into(),
            hint: Some("Configure Azure OpenAI/OpenAI-compatible providers for parity work".into()),
        });
        return results;
    }

    results.push(CheckResult {
        name: "models.providers".into(),
        category: "models".into(),
        status: CheckStatus::Pass,
        message: format!("{} model provider(s) configured", config.models.providers.len()),
        hint: None,
    });

    for provider in &config.models.providers {
        let prefix = format!("models.provider.{}", provider.provider_name);
        let api_key_state = provider
            .api_key_env
            .as_deref()
            .and_then(|env_name| std::env::var(env_name).ok())
            .filter(|value| !value.trim().is_empty());

        results.push(CheckResult {
            name: format!("{prefix}.endpoint"),
            category: "models".into(),
            status: if provider.endpoint.trim().is_empty() {
                CheckStatus::Fail
            } else {
                CheckStatus::Pass
            },
            message: if provider.endpoint.trim().is_empty() {
                "Provider endpoint is missing".into()
            } else {
                format!("Endpoint configured: {}", provider.endpoint)
            },
            hint: if provider.endpoint.trim().is_empty() {
                Some("Set a reachable provider endpoint".into())
            } else {
                None
            },
        });

        let looks_azure = provider.endpoint.contains("openai.azure.com")
            || provider.provider_name.to_ascii_lowercase().contains("azure");
        results.push(CheckResult {
            name: format!("{prefix}.auth"),
            category: "models".into(),
            status: if api_key_state.is_some() {
                CheckStatus::Pass
            } else {
                CheckStatus::Warn
            },
            message: match provider.api_key_env.as_deref() {
                Some(env_name) if api_key_state.is_some() => {
                    format!("Credential env `{env_name}` is set")
                }
                Some(env_name) => format!("Credential env `{env_name}` is not set"),
                None => "No api_key_env configured".into(),
            },
            hint: if api_key_state.is_some() {
                None
            } else {
                Some("Configure an API-key env var reference and ensure it is populated at runtime".into())
            },
        });

        if looks_azure {
            results.push(CheckResult {
                name: format!("{prefix}.azure"),
                category: "models".into(),
                status: if provider.deployment_name.is_some() && provider.api_version.is_some() {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Warn
                },
                message: if provider.deployment_name.is_some() && provider.api_version.is_some() {
                    "Azure deployment name and api_version are configured".into()
                } else {
                    "Azure-style provider is missing deployment_name and/or api_version".into()
                },
                hint: if provider.deployment_name.is_some() && provider.api_version.is_some() {
                    None
                } else {
                    Some("Azure OpenAI parity expects deployment_name and api_version".into())
                },
            });
        }
    }

    results
}

async fn check_channels_config(config: &AppConfig) -> Vec<CheckResult> {
    let mut results = Vec::new();

    if config.channels.enabled.is_empty() {
        results.push(CheckResult {
            name: "channels.enabled".into(),
            category: "channels".into(),
            status: CheckStatus::Skip,
            message: "No channels enabled".into(),
            hint: None,
        });
        return results;
    }

    results.push(CheckResult {
        name: "channels.enabled".into(),
        category: "channels".into(),
        status: CheckStatus::Pass,
        message: format!("Enabled channels: {}", config.channels.enabled.join(", ")),
        hint: None,
    });

    let telegram_enabled = config
        .channels
        .enabled
        .iter()
        .any(|name| name.eq_ignore_ascii_case("telegram"));

    if telegram_enabled {
        let token_present = config
            .channels
            .telegram_token
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        results.push(CheckResult {
            name: "channels.telegram.auth".into(),
            category: "channels".into(),
            status: if token_present {
                CheckStatus::Pass
            } else {
                CheckStatus::Warn
            },
            message: if token_present {
                "Telegram token is configured".into()
            } else {
                "Telegram channel enabled but telegram_token is missing".into()
            },
            hint: if token_present {
                None
            } else {
                Some("Set channels.telegram_token before enabling Telegram delivery".into())
            },
        });
    }

    results
}

async fn check_memory_config(config: &AppConfig) -> Vec<CheckResult> {
    let mut results = Vec::new();

    results.push(CheckResult {
        name: "memory.search".into(),
        category: "memory".into(),
        status: if config.memory.semantic_search_enabled {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        message: if config.memory.semantic_search_enabled {
            "Semantic memory search is enabled".into()
        } else {
            "Semantic memory search is disabled".into()
        },
        hint: if config.memory.semantic_search_enabled {
            None
        } else {
            Some("Disable only intentionally; parity expects searchable memory surfaces".into())
        },
    });

    let memory_dir = &config.paths.memory_dir;
    results.push(CheckResult {
        name: "memory.dir".into(),
        category: "memory".into(),
        status: if memory_dir.exists() && memory_dir.is_dir() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        message: if memory_dir.exists() && memory_dir.is_dir() {
            format!("Memory directory available at {}", memory_dir.display())
        } else {
            format!("Memory directory missing at {}", memory_dir.display())
        },
        hint: if memory_dir.exists() && memory_dir.is_dir() {
            None
        } else {
            Some("Create/mount the configured memory_dir for daily-note retrieval".into())
        },
    });

    results
}

async fn check_scheduler_config(config: &AppConfig) -> Vec<CheckResult> {
    vec![CheckResult {
        name: "scheduler.storage".into(),
        category: "scheduler".into(),
        status: if config.paths.sessions_dir.exists() && config.paths.logs_dir.exists() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        message: if config.paths.sessions_dir.exists() && config.paths.logs_dir.exists() {
            "Session/log storage paths exist for scheduler/runtime bookkeeping".into()
        } else {
            "Session/log storage paths are incomplete for reliable scheduler bookkeeping".into()
        },
        hint: if config.paths.sessions_dir.exists() && config.paths.logs_dir.exists() {
            None
        } else {
            Some("Ensure durable sessions_dir and logs_dir mounts exist before relying on cron/restart durability".into())
        },
    }]
}

async fn check_workspace(root: &Path) -> Vec<CheckResult> {
    let category = "workspace";
    let required_files = [
        ("AGENTS.md", "Agent configuration"),
        ("SOUL.md", "Agent personality"),
        ("USER.md", "User context"),
    ];

    let optional_files = [
        ("MEMORY.md", "Long-term memory"),
        ("TOOLS.md", "Tool notes"),
        ("HEARTBEAT.md", "Heartbeat config"),
    ];

    let mut results = Vec::new();

    for (file, desc) in &required_files {
        let path = root.join(file);
        if path.exists() {
            results.push(CheckResult {
                name: format!("workspace.{file}"),
                category: category.into(),
                status: CheckStatus::Pass,
                message: format!("{desc} file exists"),
                hint: None,
            });
        } else {
            results.push(CheckResult {
                name: format!("workspace.{file}"),
                category: category.into(),
                status: CheckStatus::Warn,
                message: format!("{desc} file missing"),
                hint: Some(format!("Create {file} in your workspace")),
            });
        }
    }

    for (file, desc) in &optional_files {
        let path = root.join(file);
        results.push(CheckResult {
            name: format!("workspace.{file}"),
            category: category.into(),
            status: if path.exists() {
                CheckStatus::Pass
            } else {
                CheckStatus::Skip
            },
            message: if path.exists() {
                format!("{desc} file exists")
            } else {
                format!("{desc} not found (optional)")
            },
            hint: None,
        });
    }

    let memory_dir = root.join("memory");
    results.push(CheckResult {
        name: "workspace.memory_dir".into(),
        category: category.into(),
        status: if memory_dir.is_dir() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        message: if memory_dir.is_dir() {
            "Memory directory exists".into()
        } else {
            "Memory directory missing".into()
        },
        hint: if memory_dir.is_dir() {
            None
        } else {
            Some("Create memory/ directory for daily notes".into())
        },
    });

    results
}

async fn check_gateway(url: &str) -> CheckResult {
    let name = "gateway.health".to_string();
    let category = "gateway".to_string();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(e) => {
            return CheckResult {
                name,
                category,
                status: CheckStatus::Fail,
                message: format!("Cannot create HTTP client: {e}"),
                hint: None,
            };
        }
    };

    let health_url = format!("{}/health", url.trim_end_matches('/'));

    match client.get(&health_url).send().await {
        Ok(resp) if resp.status().is_success() => CheckResult {
            name,
            category,
            status: CheckStatus::Pass,
            message: format!("Gateway healthy at {url}"),
            hint: None,
        },
        Ok(resp) => CheckResult {
            name,
            category,
            status: CheckStatus::Warn,
            message: format!("Gateway responded with {}", resp.status()),
            hint: Some("Check gateway logs".into()),
        },
        Err(e) => CheckResult {
            name,
            category,
            status: CheckStatus::Fail,
            message: format!("Cannot reach gateway: {e}"),
            hint: Some(format!("Ensure gateway is running at {url}")),
        },
    }
}

/// Format check results for terminal output.
pub fn format_results(results: &[CheckResult]) -> String {
    let mut out = String::new();
    let mut current_category = String::new();

    for r in results {
        if r.category != current_category {
            if !current_category.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("─── {} ───\n", r.category));
            current_category.clone_from(&r.category);
        }

        let icon = match r.status {
            CheckStatus::Pass => "✓",
            CheckStatus::Warn => "⚠",
            CheckStatus::Fail => "✗",
            CheckStatus::Skip => "○",
        };

        out.push_str(&format!("  {icon} {} — {}\n", r.name, r.message));
        if let Some(hint) = &r.hint {
            out.push_str(&format!("    ↳ {hint}\n"));
        }
    }

    let pass = results.iter().filter(|r| r.status == CheckStatus::Pass).count();
    let warn = results.iter().filter(|r| r.status == CheckStatus::Warn).count();
    let fail = results.iter().filter(|r| r.status == CheckStatus::Fail).count();

    out.push_str(&format!("\n{pass} passed, {warn} warnings, {fail} failed\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn test_config_with_paths(root: &Path) -> AppConfig {
        let mut config = AppConfig::default();
        config.paths.db_dir = root.join("data/db");
        config.paths.sessions_dir = root.join("data/sessions");
        config.paths.memory_dir = root.join("data/memory");
        config.paths.media_dir = root.join("data/media");
        config.paths.skills_dir = root.join("data/skills");
        config.paths.logs_dir = root.join("data/logs");
        config.paths.backups_dir = root.join("data/backups");
        config.paths.config_dir = root.join("config");
        config.paths.secrets_dir = root.join("secrets");
        config
    }

    async fn create_path_layout(root: &Path) {
        for rel in [
            "data/db",
            "data/sessions",
            "data/memory",
            "data/media",
            "data/skills",
            "data/logs",
            "data/backups",
            "config",
            "secrets",
        ] {
            tokio::fs::create_dir_all(root.join(rel)).await.unwrap();
        }
    }

    #[tokio::test]
    async fn workspace_full_passes() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("AGENTS.md"), "# Agents").await.unwrap();
        tokio::fs::write(tmp.path().join("SOUL.md"), "# Soul").await.unwrap();
        tokio::fs::write(tmp.path().join("USER.md"), "# User").await.unwrap();
        tokio::fs::write(tmp.path().join("MEMORY.md"), "# Memory").await.unwrap();
        tokio::fs::create_dir_all(tmp.path().join("memory")).await.unwrap();

        let results = check_workspace(tmp.path()).await;
        let pass_count = results.iter().filter(|r| r.status == CheckStatus::Pass).count();
        assert!(pass_count >= 4, "expected at least 4 passes, got {pass_count}");
    }

    #[tokio::test]
    async fn workspace_empty_warns() {
        let tmp = TempDir::new().unwrap();
        let results = check_workspace(tmp.path()).await;
        let warn_count = results.iter().filter(|r| r.status == CheckStatus::Warn).count();
        assert!(warn_count >= 3, "expected warnings for missing required files");
    }

    #[tokio::test]
    async fn config_driven_checks_cover_paths_models_channels_and_memory() {
        let tmp = TempDir::new().unwrap();
        create_path_layout(tmp.path()).await;

        let mut config = test_config_with_paths(tmp.path());
        config.models.providers.push(rune_config::ModelProviderConfig {
            provider_name: "azure-openai".into(),
            endpoint: "https://example.openai.azure.com".into(),
            deployment_name: Some("gpt-4.1".into()),
            api_version: Some("2024-10-21".into()),
            api_key_env: Some("RUNE_TEST_AZURE_KEY".into()),
            model_alias: Some("default".into()),
        });
        config.channels.enabled.push("telegram".into());
        config.channels.telegram_token = Some("123:abc".into());

        unsafe {
            std::env::set_var("RUNE_TEST_AZURE_KEY", "secret");
        }

        let results = [
            check_paths(&config).await,
            check_database_config(&config).await,
            check_models_config(&config).await,
            check_channels_config(&config).await,
            check_memory_config(&config).await,
            check_scheduler_config(&config).await,
        ]
        .concat();

        assert!(results.iter().any(|r| r.name == "models.providers" && r.status == CheckStatus::Pass));
        assert!(results.iter().any(|r| r.name == "channels.telegram.auth" && r.status == CheckStatus::Pass));
        assert!(results.iter().any(|r| r.name == "memory.dir" && r.status == CheckStatus::Pass));
        assert!(results.iter().any(|r| r.name == "scheduler.storage" && r.status == CheckStatus::Pass));

        unsafe {
            std::env::remove_var("RUNE_TEST_AZURE_KEY");
        }
    }

    #[tokio::test]
    async fn missing_provider_credentials_warn() {
        let tmp = TempDir::new().unwrap();
        create_path_layout(tmp.path()).await;
        let mut config = test_config_with_paths(tmp.path());
        config.models.providers.push(rune_config::ModelProviderConfig {
            provider_name: "azure-openai".into(),
            endpoint: "https://example.openai.azure.com".into(),
            deployment_name: None,
            api_version: None,
            api_key_env: Some("RUNE_TEST_MISSING_KEY".into()),
            model_alias: None,
        });

        unsafe {
            std::env::remove_var("RUNE_TEST_MISSING_KEY");
        }

        let results = check_models_config(&config).await;
        assert!(results.iter().any(|r| r.name.ends_with(".auth") && r.status == CheckStatus::Warn));
        assert!(results.iter().any(|r| r.name.ends_with(".azure") && r.status == CheckStatus::Warn));
    }

    #[test]
    fn docker_layout_warns_on_nonstandard_mounts() {
        let mut config = AppConfig::default();
        config.paths.db_dir = PathBuf::from("./db");
        let result = check_docker_path_layout(&config);
        assert_eq!(result.status, CheckStatus::Warn);
    }

    #[test]
    fn format_output_includes_summary() {
        let results = vec![
            CheckResult {
                name: "test.pass".into(),
                category: "test".into(),
                status: CheckStatus::Pass,
                message: "passed".into(),
                hint: None,
            },
            CheckResult {
                name: "test.fail".into(),
                category: "test".into(),
                status: CheckStatus::Fail,
                message: "failed".into(),
                hint: Some("fix it".into()),
            },
        ];

        let out = format_results(&results);
        assert!(out.contains("1 passed"));
        assert!(out.contains("1 failed"));
        assert!(out.contains("fix it"));
    }
}
