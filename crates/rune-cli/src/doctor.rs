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

use crate::output::{
    DoctorBackendMatrixEntry, DoctorCheck as OutputDoctorCheck, DoctorContextTierCounter,
    DoctorMemoryHierarchySummary, DoctorPathSummary, DoctorReport, DoctorTopologySummary,
    ReplacementReadinessBlocker, ReplacementReadinessReport,
};
use rune_config::{AppConfig, RuntimeMode};
use serde::Serialize;
use std::collections::BTreeMap;

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
        results.extend(check_approval_security(config));
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
    let resolved_mode = config.mode.resolve(config);
    let mut results = Vec::new();

    for (name, path, required_persistent) in [
        ("paths.db_dir", &config.paths.db_dir, true),
        ("paths.sessions_dir", &config.paths.sessions_dir, true),
        ("paths.memory_dir", &config.paths.memory_dir, true),
        ("paths.media_dir", &config.paths.media_dir, true),
        ("paths.spells_dir", &config.paths.spells_dir, true),
        ("paths.logs_dir", &config.paths.logs_dir, true),
        ("paths.backups_dir", &config.paths.backups_dir, true),
        ("paths.config_dir", &config.paths.config_dir, true),
        ("paths.secrets_dir", &config.paths.secrets_dir, true),
        ("paths.workspace_dir", &config.paths.workspace_dir, true),
        ("paths.cache_dir", &config.paths.cache_dir, true),
        ("paths.data_dir", &config.paths.data_dir, true),
    ] {
        results.push(check_single_path(
            name,
            path,
            required_persistent,
            &resolved_mode,
        ));
    }

    results.push(check_docker_path_layout(config));
    results
}

fn check_single_path(
    name: &str,
    path: &Path,
    required_persistent: bool,
    mode: &RuntimeMode,
) -> CheckResult {
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
            hint: Some(match mode {
                RuntimeMode::Standalone => {
                    format!("Run: mkdir -p {}", path.display())
                }
                _ => {
                    format!(
                        "Mount a writable volume at {} (e.g. -v /host/path:{})",
                        path.display(),
                        path.display()
                    )
                }
            }),
        };
    }

    if !path.is_dir() {
        return CheckResult {
            name: name.into(),
            category,
            status: CheckStatus::Fail,
            message: format!("{} exists but is not a directory", path.display()),
            hint: Some(format!(
                "Remove the non-directory entry and recreate: rm {} && mkdir -p {}",
                path.display(),
                path.display()
            )),
        };
    }

    if !probe_writable(path) {
        return CheckResult {
            name: name.into(),
            category,
            status: CheckStatus::Fail,
            message: format!("{} is not writable (write probe failed)", path.display()),
            hint: Some(match mode {
                RuntimeMode::Standalone => {
                    format!("Fix permissions: chmod u+w {}", path.display())
                }
                _ => {
                    format!(
                        "Ensure the volume at {} is writable; check mount flags and container user UID",
                        path.display()
                    )
                }
            }),
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

/// Attempt to write and remove a probe file to verify actual writability.
///
/// `std::fs::Permissions::readonly()` only checks the owner write bit on Unix,
/// which is unreliable for bind-mounts, different-user ownership, and
/// filesystem-level read-only mounts.  A write probe catches all of these.
fn probe_writable(dir: &Path) -> bool {
    let probe = dir.join(".rune_doctor_probe");
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
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

    let using_external_postgres = config.database.database_url.is_some();
    let azure_sql_configured = config.database.azure_sql_server.is_some()
        || config.database.azure_sql_database.is_some()
        || config.database.azure_sql_user.is_some()
        || config.database.azure_sql_password.is_some()
        || config.database.azure_sql_access_token.is_some();
    let resolved_backend = match config.database.backend {
        rune_config::StorageBackend::Postgres => "postgres",
        rune_config::StorageBackend::Sqlite => "sqlite",
        rune_config::StorageBackend::Cosmos => "cosmos",
        rune_config::StorageBackend::AzureSql => "azure_sql",
        rune_config::StorageBackend::Auto => {
            if using_external_postgres {
                "postgres"
            } else if config.database.cosmos_endpoint.is_some() {
                "cosmos"
            } else if azure_sql_configured {
                "azure_sql"
            } else {
                "sqlite"
            }
        }
    };

    results.push(CheckResult {
        name: "database.backend".into(),
        category: "database".into(),
        status: CheckStatus::Pass,
        message: format!(
            "Configured backend = {:?}; resolved runtime backend = {resolved_backend}",
            config.database.backend
        ),
        hint: if matches!(config.database.backend, rune_config::StorageBackend::Auto) {
            Some("Auto resolves to PostgreSQL when database_url is set, otherwise Cosmos when cosmos_endpoint is set, otherwise flags unsupported Azure SQL config, otherwise SQLite".into())
        } else {
            None
        },
    });

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
                Some("Expected postgres:// or postgresql:// URL when using external PostgreSQL".into())
            },
        }
    } else {
        CheckResult {
            name: "database.url".into(),
            category: "database".into(),
            status: if resolved_backend == "sqlite" {
                CheckStatus::Pass
            } else {
                CheckStatus::Warn
            },
            message: if resolved_backend == "sqlite" {
                "database_url not set; runtime will use the SQLite-backed repo path".into()
            } else {
                "database_url not set; runtime will rely on embedded PostgreSQL".into()
            },
            hint: if resolved_backend == "sqlite" {
                Some("Fine for standalone/local mode; set database.backend=postgres or database_url to use PostgreSQL".into())
            } else {
                Some("Fine for zero-config local dev; production should usually point at durable PostgreSQL".into())
            },
        }
    });

    if resolved_backend == "postgres" && !using_external_postgres {
        results.extend(check_embedded_postgres_layout(config));
    }

    if azure_sql_configured
        || matches!(
            config.database.backend,
            rune_config::StorageBackend::AzureSql
        )
    {
        let missing_identity = config.database.azure_sql_server.is_none()
            || config.database.azure_sql_database.is_none();
        let auth_configured = config.database.azure_sql_access_token.is_some()
            || (config.database.azure_sql_user.is_some()
                && config.database.azure_sql_password.is_some());
        results.push(CheckResult {
            name: "database.azure_sql".into(),
            category: "database".into(),
            status: CheckStatus::Warn,
            message: "Azure SQL Database config detected; Rune will route it through the shared SQL-family backend path".into(),
            hint: Some("Issue #782 shipped config/runtime wiring, but the current implementation still reuses Rune's PostgreSQL-feature-gated SQL backend path rather than a SQL Server-native store; validate protocol/driver compatibility in your target environment".into()),
        });
        results.push(CheckResult {
            name: "database.azure_sql.identity".into(),
            category: "database".into(),
            status: if missing_identity { CheckStatus::Fail } else { CheckStatus::Warn },
            message: if missing_identity {
                "Azure SQL config is missing azure_sql_server or azure_sql_database".into()
            } else {
                "Azure SQL server/database fields are present".into()
            },
            hint: Some("Azure SQL routing requires both azure_sql_server and azure_sql_database when this backend is selected".into()),
        });
        results.push(CheckResult {
            name: "database.azure_sql.auth".into(),
            category: "database".into(),
            status: if auth_configured { CheckStatus::Warn } else { CheckStatus::Fail },
            message: if auth_configured {
                "Azure SQL auth material is present for the shared SQL-family backend path".into()
            } else {
                "Azure SQL auth material is incomplete (set access token or username/password pair)".into()
            },
            hint: Some("Configure either azure_sql_access_token or azure_sql_user + azure_sql_password; verify what your deployed SQL driver path accepts before production rollout".into()),
        });
    }

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

fn check_embedded_postgres_layout(config: &AppConfig) -> Vec<CheckResult> {
    let db_dir = &config.paths.db_dir;
    let install_dir = db_dir.join("pg_install");
    let cluster_dir = db_dir.join("pg_data");
    let password_file = db_dir.join(".pg_password");

    let mut results = vec![CheckResult {
        name: "database.embedded.mode".into(),
        category: "database".into(),
        status: CheckStatus::Pass,
        message: format!(
            "Embedded PostgreSQL fallback selected; durable state root is {}",
            db_dir.display()
        ),
        hint: Some(
            "For zero-config local dev this is expected; production can switch to external PostgreSQL via database_url or use SQLite-backed standalone mode where appropriate".into(),
        ),
    }];

    results.push(CheckResult {
        name: "database.embedded.db_dir".into(),
        category: "database".into(),
        status: if db_dir.exists() && db_dir.is_dir() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        message: if db_dir.exists() && db_dir.is_dir() {
            format!("Embedded PostgreSQL root exists at {}", db_dir.display())
        } else {
            format!("Embedded PostgreSQL root missing at {}", db_dir.display())
        },
        hint: if db_dir.exists() && db_dir.is_dir() {
            None
        } else {
            Some("Create/mount db_dir so embedded PostgreSQL state survives restarts".into())
        },
    });

    results.push(CheckResult {
        name: "database.embedded.install_dir".into(),
        category: "database".into(),
        status: if install_dir.exists() && install_dir.is_dir() {
            CheckStatus::Pass
        } else {
            CheckStatus::Skip
        },
        message: if install_dir.exists() && install_dir.is_dir() {
            format!(
                "Embedded PostgreSQL installation cache present at {}",
                install_dir.display()
            )
        } else {
            format!(
                "Embedded PostgreSQL installation cache not initialized yet ({})",
                install_dir.display()
            )
        },
        hint: if install_dir.exists() && install_dir.is_dir() {
            None
        } else {
            Some("This is expected before first embedded-Postgres startup".into())
        },
    });

    results.push(CheckResult {
        name: "database.embedded.cluster_dir".into(),
        category: "database".into(),
        status: if cluster_dir.exists() && cluster_dir.is_dir() {
            CheckStatus::Pass
        } else {
            CheckStatus::Skip
        },
        message: if cluster_dir.exists() && cluster_dir.is_dir() {
            format!(
                "Embedded PostgreSQL cluster data present at {}",
                cluster_dir.display()
            )
        } else {
            format!(
                "Embedded PostgreSQL cluster data not initialized yet ({})",
                cluster_dir.display()
            )
        },
        hint: if cluster_dir.exists() && cluster_dir.is_dir() {
            None
        } else {
            Some("This is expected before first embedded-Postgres startup".into())
        },
    });

    results.push(CheckResult {
        name: "database.embedded.password_file".into(),
        category: "database".into(),
        status: if password_file.exists() {
            CheckStatus::Pass
        } else {
            CheckStatus::Skip
        },
        message: if password_file.exists() {
            format!(
                "Embedded PostgreSQL password state present at {}",
                password_file.display()
            )
        } else {
            format!(
                "Embedded PostgreSQL password state not initialized yet ({})",
                password_file.display()
            )
        },
        hint: if password_file.exists() {
            None
        } else {
            Some("Will be created automatically on first embedded-Postgres startup".into())
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
            message: config
                .models
                .zero_config_ollama_base_url(std::env::var("OLLAMA_HOST").ok().as_deref())
                .map(|base| format!("No explicit model providers configured; zero-config Ollama auto-detect is available at {base}"))
                .unwrap_or_else(|| "No model providers configured; runtime will be limited to fallback/demo behavior".into()),
            hint: Some(
                config
                    .models
                    .zero_config_ollama_base_url(std::env::var("OLLAMA_HOST").ok().as_deref())
                    .map(|_| "Start Ollama locally or configure Azure/OpenAI-compatible providers for full parity".into())
                    .unwrap_or_else(|| "Configure Azure OpenAI/OpenAI-compatible providers for parity work".into()),
            ),
        });
        return results;
    }

    results.push(CheckResult {
        name: "models.providers".into(),
        category: "models".into(),
        status: CheckStatus::Pass,
        message: format!(
            "{} model provider(s) configured",
            config.models.providers.len()
        ),
        hint: None,
    });

    for provider in &config.models.providers {
        let prefix = format!("models.provider.{}", provider.name);
        let api_key_state = provider
            .api_key_env
            .as_deref()
            .and_then(|env_name| std::env::var(env_name).ok())
            .filter(|value| !value.trim().is_empty());

        results.push(CheckResult {
            name: format!("{prefix}.base_url"),
            category: "models".into(),
            status: if provider.base_url.trim().is_empty() {
                CheckStatus::Fail
            } else {
                CheckStatus::Pass
            },
            message: if provider.base_url.trim().is_empty() {
                "Provider endpoint is missing".into()
            } else {
                format!("Endpoint configured: {}", provider.base_url)
            },
            hint: if provider.base_url.trim().is_empty() {
                Some("Set a reachable provider endpoint".into())
            } else {
                None
            },
        });

        let looks_azure = provider.base_url.contains("openai.azure.com")
            || provider.name.to_ascii_lowercase().contains("azure");
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
                Some(
                    "Configure an API-key env var reference and ensure it is populated at runtime"
                        .into(),
                )
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
    let requested_level = config.memory.requested_level();

    results.push(CheckResult {
        name: "memory.level".into(),
        category: "memory".into(),
        status: CheckStatus::Pass,
        message: match requested_level {
            rune_config::MemoryLevel::File => {
                "Configured memory level: file (local file scan only)".into()
            }
            rune_config::MemoryLevel::Keyword => {
                "Configured memory level: keyword (local keyword retrieval)".into()
            }
            rune_config::MemoryLevel::Semantic => {
                "Configured memory level: semantic (hybrid when available, keyword fallback otherwise)".into()
            }
        },
        hint: Some(format!(
            "Resolved from memory.level or legacy semantic_search_enabled; active preference is `{}`",
            requested_level.as_str()
        )),
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

fn check_approval_security(config: &AppConfig) -> Vec<CheckResult> {
    let approval_mode = config.approval.mode;
    let security = &config.security;
    let posture = security.posture();

    let mut results = Vec::new();

    // ── Approval mode ────────────────────────────────────────────────
    let is_yolo = approval_mode.is_yolo();
    results.push(CheckResult {
        name: "approval.mode".into(),
        category: "security".into(),
        status: if is_yolo {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        },
        message: format!("Approval mode: {}", approval_mode.as_str()),
        hint: if is_yolo {
            Some(
                "Yolo mode auto-approves all tool calls — appropriate for trusted local dev only"
                    .into(),
            )
        } else {
            None
        },
    });

    // ── Security posture ─────────────────────────────────────────────
    let sandbox_off = !security.sandbox;
    results.push(CheckResult {
        name: "security.posture".into(),
        category: "security".into(),
        status: if sandbox_off {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        },
        message: format!(
            "Security posture: {} (sandbox={}, trust_spells={})",
            posture, security.sandbox, security.trust_spells
        ),
        hint: if sandbox_off {
            Some("Sandbox is disabled — workspace boundary enforcement is off".into())
        } else {
            None
        },
    });

    // ── Combined yolo+no-sandbox warning ─────────────────────────────
    if is_yolo && sandbox_off {
        results.push(CheckResult {
            name: "security.unrestricted".into(),
            category: "security".into(),
            status: CheckStatus::Warn,
            message: "Running in fully unrestricted mode (yolo + no sandbox)".into(),
            hint: Some(
                "All permissions auto-approved and sandbox disabled — use only in trusted environments".into(),
            ),
        });
    }

    results
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

#[derive(Clone, Debug)]
struct BackendMatrixRawEntry {
    subsystem: &'static str,
    backend: String,
    status: String,
    capability: String,
    fix_hint: Option<String>,
}

pub fn build_backend_matrix(config: &AppConfig) -> Vec<DoctorBackendMatrixEntry> {
    build_backend_matrix_raw(config)
        .into_iter()
        .map(|entry| DoctorBackendMatrixEntry {
            subsystem: entry.subsystem.to_string(),
            backend: entry.backend,
            status: entry.status,
            capability: entry.capability,
            fix_hint: entry.fix_hint,
        })
        .collect()
}

fn build_backend_matrix_raw(config: &AppConfig) -> Vec<BackendMatrixRawEntry> {
    let resolved_storage = resolved_storage_backend(config);
    let storage_connected = matches!(resolved_storage, "sqlite" | "postgres" | "cosmos");
    let storage_repos = configured_repo_count(config);
    let storage_capability = format!(
        "{storage_repos} repo surfaces configured; mode={}",
        config.mode.resolve(config).as_str()
    );

    let vector_backend = resolved_vector_backend(config);
    let memory_level = config.memory.requested_level();
    let vector_status = if matches!(memory_level, rune_config::MemoryLevel::Semantic) {
        if vector_backend == "none" {
            "degraded"
        } else {
            "connected"
        }
    } else {
        "unavailable"
    };
    let vector_capability = match vector_backend.as_str() {
        "lancedb" => format!(
            "{}-dim semantic search via LanceDB",
            config.vector.embedding_dims
        ),
        "pgvector" => format!(
            "{}-dim semantic search via pgvector",
            config.vector.embedding_dims
        ),
        "integrated" => "integrated vector backend via primary database".to_string(),
        _ => "semantic retrieval disabled; keyword/file memory only".to_string(),
    };

    let comms_enabled = config.comms.enabled && config.comms.comms_dir.is_some();
    let comms_status = if comms_enabled {
        "connected"
    } else {
        "unavailable"
    };
    let comms_backend = if comms_enabled {
        "filesystem"
    } else {
        "disabled"
    };
    let comms_capability = if comms_enabled {
        format!(
            "peer={} dir={}",
            config.comms.peer_id.as_str(),
            config.comms.comms_dir.as_deref().unwrap_or("<unset>")
        )
    } else {
        "inter-agent comms not configured".to_string()
    };

    let enabled_channels = configured_channels(config);
    let channels_status = if enabled_channels.is_empty() {
        "unavailable"
    } else {
        "connected"
    };
    let channels_backend = if enabled_channels.is_empty() {
        "none".to_string()
    } else {
        enabled_channels.join(", ")
    };
    let channels_capability = if enabled_channels.is_empty() {
        "no channels enabled".to_string()
    } else {
        format!("{} enabled channel(s)", enabled_channels.len())
    };

    let provider_descriptions = model_provider_descriptions(config);
    let models_status = if provider_descriptions.is_empty() {
        "degraded"
    } else {
        "connected"
    };
    let models_backend = if provider_descriptions.is_empty() {
        config
            .models
            .zero_config_ollama_base_url(std::env::var("OLLAMA_HOST").ok().as_deref())
            .map(|_| "ollama-zero-config".to_string())
            .unwrap_or_else(|| "none".to_string())
    } else {
        provider_descriptions
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let models_capability = if provider_descriptions.is_empty() {
        config
            .models
            .zero_config_ollama_base_url(std::env::var("OLLAMA_HOST").ok().as_deref())
            .map(|base| format!("zero-config Ollama available at {base}"))
            .unwrap_or_else(|| "no providers configured".to_string())
    } else {
        provider_descriptions
            .into_iter()
            .map(|(name, count)| {
                format!(
                    "{name} ({count} model{})",
                    if count == 1 { "" } else { "s" }
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    let memory_backend = match memory_level {
        rune_config::MemoryLevel::File => "file".to_string(),
        rune_config::MemoryLevel::Keyword => "keyword".to_string(),
        rune_config::MemoryLevel::Semantic => match vector_backend.as_str() {
            "none" => "keyword".to_string(),
            backend => backend.to_string(),
        },
    };
    let memory_status = match memory_level {
        rune_config::MemoryLevel::Semantic if vector_backend == "none" => "degraded",
        _ => "connected",
    };
    let memory_capability = match memory_level {
        rune_config::MemoryLevel::File => "file scan only".to_string(),
        rune_config::MemoryLevel::Keyword => "keyword retrieval".to_string(),
        rune_config::MemoryLevel::Semantic => match vector_backend.as_str() {
            "none" => "semantic requested but vector backend unavailable; keyword fallback only"
                .to_string(),
            backend => format!("semantic ({backend}) + keyword retrieval"),
        },
    };

    vec![
        BackendMatrixRawEntry {
            subsystem: "storage",
            backend: resolved_storage.to_string(),
            status: if storage_connected {
                "connected"
            } else {
                "degraded"
            }
            .to_string(),
            capability: storage_capability,
            fix_hint: if storage_connected {
                None
            } else {
                Some("Set database.backend to sqlite/postgres/cosmos and provide matching connection settings".to_string())
            },
        },
        BackendMatrixRawEntry {
            subsystem: "vector",
            backend: vector_backend.clone(),
            status: vector_status.to_string(),
            capability: vector_capability,
            fix_hint: if vector_backend == "none"
                && matches!(memory_level, rune_config::MemoryLevel::Semantic)
            {
                Some(
                    "Enable vector.backend=lancedb or postgres+pgvector for semantic memory"
                        .to_string(),
                )
            } else {
                None
            },
        },
        BackendMatrixRawEntry {
            subsystem: "comms",
            backend: comms_backend.to_string(),
            status: comms_status.to_string(),
            capability: comms_capability,
            fix_hint: if comms_enabled {
                None
            } else {
                Some(
                    "Set comms.enabled=true and comms.comms_dir to enable peer messaging"
                        .to_string(),
                )
            },
        },
        BackendMatrixRawEntry {
            subsystem: "channels",
            backend: channels_backend,
            status: channels_status.to_string(),
            capability: channels_capability,
            fix_hint: if enabled_channels.is_empty() {
                Some(
                    "Configure at least one channel credential and add it to channels.enabled"
                        .to_string(),
                )
            } else {
                None
            },
        },
        BackendMatrixRawEntry {
            subsystem: "models",
            backend: models_backend,
            status: models_status.to_string(),
            capability: models_capability,
            fix_hint: if config.models.providers.is_empty() {
                Some(
                    "Add one or more models.providers entries for production inference".to_string(),
                )
            } else {
                None
            },
        },
        BackendMatrixRawEntry {
            subsystem: "memory",
            backend: memory_backend,
            status: memory_status.to_string(),
            capability: memory_capability,
            fix_hint: if memory_status == "degraded" {
                Some(
                    "Configure a semantic vector backend or lower memory.level to keyword/file"
                        .to_string(),
                )
            } else {
                None
            },
        },
    ]
}

fn resolved_storage_backend(config: &AppConfig) -> &'static str {
    let azure_sql_configured = config.database.azure_sql_server.is_some()
        || config.database.azure_sql_database.is_some()
        || config.database.azure_sql_user.is_some()
        || config.database.azure_sql_password.is_some()
        || config.database.azure_sql_access_token.is_some();

    match config.database.backend {
        rune_config::StorageBackend::Postgres => "postgres",
        rune_config::StorageBackend::Sqlite => "sqlite",
        rune_config::StorageBackend::Cosmos => "cosmos",
        rune_config::StorageBackend::AzureSql => "azure_sql",
        rune_config::StorageBackend::Auto => {
            if config.database.database_url.is_some() {
                "postgres"
            } else if config.database.cosmos_endpoint.is_some() {
                "cosmos"
            } else if azure_sql_configured {
                "azure_sql"
            } else {
                "sqlite"
            }
        }
    }
}

fn resolved_vector_backend(config: &AppConfig) -> String {
    match config.vector.backend {
        rune_config::VectorBackend::LanceDb => "lancedb".to_string(),
        rune_config::VectorBackend::Integrated => {
            if resolved_storage_backend(config) == "postgres" {
                "pgvector".to_string()
            } else {
                "integrated".to_string()
            }
        }
        rune_config::VectorBackend::None => "none".to_string(),
        rune_config::VectorBackend::Auto => {
            if config.vector.lancedb_uri.is_some() {
                "lancedb".to_string()
            } else if resolved_storage_backend(config) == "postgres" {
                "pgvector".to_string()
            } else {
                "none".to_string()
            }
        }
    }
}

const READINESS_INTERACTIVE_RESPONSE_SLO_MS: u64 = 2_000;
const READINESS_QUEUE_DELAY_SLO_MS: u64 = 500;
const READINESS_STUCK_TURN_RATE_SLO_PERCENT: f64 = 1.0;
const READINESS_RECOVERY_TIME_SLO_SECONDS: u64 = 60;

fn readiness_summary_message() -> String {
    format!(
        "targets: interactive_response<= {}ms, queue_delay<= {}ms, stuck_turn_rate<= {:.1}%, recovery_time<= {}s; readiness is blocked until the gateway publishes live queue-delay, stuck-turn-rate, and recovery-time evidence",
        READINESS_INTERACTIVE_RESPONSE_SLO_MS,
        READINESS_QUEUE_DELAY_SLO_MS,
        READINESS_STUCK_TURN_RATE_SLO_PERCENT,
        READINESS_RECOVERY_TIME_SLO_SECONDS
    )
}

fn replacement_readiness_report() -> ReplacementReadinessReport {
    let blockers = vec![
        ReplacementReadinessBlocker {
            category: "operational".to_string(),
            status: "blocked".to_string(),
            detail: "readiness evidence is still reported as pending until the gateway publishes live queue-delay, stuck-turn-rate, and recovery-time signals directly in status/doctor surfaces".to_string(),
            issue: None,
        },
        ReplacementReadinessBlocker {
            category: "documentation".to_string(),
            status: "blocked".to_string(),
            detail: "parity and operator docs still need reconciliation with shipped replacement evidence".to_string(),
            issue: Some("#896".to_string()),
        },
    ];
    ReplacementReadinessReport {
        verdict: "not_ready".to_string(),
        summary: format!(
            "Rune is not yet an honest OpenClaw replacement; {} blocker categories remain open",
            blockers.len()
        ),
        blockers,
    }
}

fn configured_repo_count(config: &AppConfig) -> usize {
    let mut count = 4;
    if !config.channels.enabled.is_empty() {
        count += 1;
    }
    if config.comms.enabled {
        count += 1;
    }
    if !config.mcp_servers.is_empty() {
        count += 1;
    }
    count
}

fn configured_channels(config: &AppConfig) -> Vec<String> {
    config
        .channels
        .enabled
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect()
}

fn model_provider_descriptions(config: &AppConfig) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for provider in &config.models.providers {
        counts.insert(provider.name.clone(), provider.models.len());
    }
    counts.into_iter().collect()
}

pub fn build_doctor_report(results: &[CheckResult], config: &AppConfig) -> DoctorReport {
    let effective_memory_mode = config
        .memory
        .capability_mode(config.memory.semantic_search_enabled);
    let l2_backend = if config.mem0.enabled
        && config
            .mem0
            .postgres_url
            .as_ref()
            .is_some_and(|v| !v.trim().is_empty())
    {
        "Mem0 + pgvector"
    } else {
        match config.memory.requested_level().as_str() {
            "semantic" => "semantic memory fallback",
            "keyword" => "keyword/local memory",
            "file" => "file memory",
            _ => "memory",
        }
    };
    let promotion = if config.mem0.enabled {
        "L2 hits become L1 candidates when reused through stable prompt prefixes on later turns/sessions; Mem0 access_count tracks hot recall frequency".to_string()
    } else {
        "L2 promotion to L1 candidates depends on stable prompt-prefix reuse on later turns/sessions".to_string()
    };
    let l3_ready = !config.paths.sessions_dir.as_os_str().is_empty();
    let demotion_target = if config.mem0.enabled {
        "warm/cold memory (Mem0 + transcript archive)"
    } else {
        "warm/cold memory (local memory + transcript archive)"
    };
    let checks = results
        .iter()
        .map(|result| OutputDoctorCheck {
            name: result.name.clone(),
            status: match result.status {
                CheckStatus::Pass => "pass",
                CheckStatus::Warn => "warn",
                CheckStatus::Fail => "fail",
                CheckStatus::Skip => "skip",
            }
            .to_string(),
            message: result.message.clone(),
        })
        .collect::<Vec<_>>();

    let overall = if results
        .iter()
        .any(|result| result.status == CheckStatus::Fail)
    {
        "unhealthy"
    } else if results
        .iter()
        .any(|result| result.status == CheckStatus::Warn)
    {
        "degraded"
    } else {
        "healthy"
    }
    .to_string();

    let resolved_mode = config.mode.resolve(config);

    DoctorReport {
        overall,
        readiness_status: Some("slo_defined_evidence_pending".to_string()),
        readiness_summary: Some(readiness_summary_message()),
        replacement_readiness: Some(replacement_readiness_report()),
        checks,
        paths: Some(DoctorPathSummary {
            profile: config.paths.profile().as_str().to_string(),
            mode: resolved_mode.as_str().to_string(),
            auto_create_missing: resolved_mode == RuntimeMode::Standalone,
        }),
        topology: Some(DoctorTopologySummary {
            deployment: resolved_mode.as_str().to_string(),
            database: resolved_storage_backend(config).to_string(),
            models: if config.models.providers.is_empty() {
                "fallback".to_string()
            } else {
                "configured".to_string()
            },
            search: config.memory.requested_level().as_str().to_string(),
        }),
        backend_matrix: build_backend_matrix(config),
        memory_hierarchy: Some(DoctorMemoryHierarchySummary {
            l0: format!(
                "current turn context window (active transcript + system/task/project context, warn_at={} tokens, compress_after={} tokens)",
                config.runtime.compaction.warn_at_tokens,
                config.runtime.compaction.compress_after
            ),
            l1: "prompt cache via provider prefixes (offline doctor cannot inspect live cache metrics)"
                .to_string(),
            l2: format!("{} ({})", l2_backend, effective_memory_mode),
            l3: if l3_ready {
                "durable session logs in transcript/session storage (ready for compaction handoff)".to_string()
            } else {
                "durable session logs in transcript/session storage".to_string()
            },
            promotion,
            demotion: format!(
                "compaction checkpoints persist stale L0 context to {} after {} tokens",
                demotion_target,
                config.runtime.compaction.compress_after
            ),
            metrics: if config.mem0.enabled {
                format!(
                    "offline doctor has no live cache metrics; Mem0 access_count persists hot-memory reuse. Context tiers ship static budgets: loaded_tiers=5, total_budget={}, estimated_tokens=0, compaction_trigger_tokens={}, over_budget=false, over_compaction_threshold=false, compaction_required=false, l3_cold_storage_enabled={}; gateway doctor exposes prompt_cache_rows/cached_tokens totals",
                    config.context.identity
                        + config.context.task
                        + config.context.project
                        + config.context.shared
                        + config.context.historical,
                    config.runtime.compaction.compress_after,
                    l3_ready
                )
            } else {
                format!(
                    "offline doctor has no live cache metrics; run doctor against the gateway for prompt_cache_rows/cached_tokens totals. Context tiers ship static budgets: loaded_tiers=5, total_budget={}, estimated_tokens=0, compaction_trigger_tokens={}, over_budget=false, over_compaction_threshold=false, compaction_required=false, l3_cold_storage_enabled={}",
                    config.context.identity
                        + config.context.task
                        + config.context.project
                        + config.context.shared
                        + config.context.historical,
                    config.runtime.compaction.compress_after,
                    l3_ready
                )
            },
            readiness_status: Some("slo_defined_evidence_pending".to_string()),
            readiness_summary: Some(readiness_summary_message()),
            last_checkpoint_at: None,
            prompt_cache_rows: 0,
            cached_tokens: 0,
            total_input_tokens: 0,
            cache_hit_ratio_percent: 0.0,
            l2_recall_hits: 0,
            l2_warm_memories: 0,
            l2_hot_memories: 0,
            l2_cold_memories: 0,
            l2_total_memories: 0,
            context_total_budget: (config.context.identity
                + config.context.task
                + config.context.project
                + config.context.shared
                + config.context.historical) as u64,
            context_total_estimated_tokens: 0,
            context_compaction_trigger_tokens: config.runtime.compaction.compress_after as u64,
            context_over_budget: false,
            context_over_compaction_threshold: false,
            context_compaction_required: false,
            l3_cold_storage_enabled: l3_ready,
            loaded_tier_count: 5,
            context_tier_counters: vec![
                DoctorContextTierCounter {
                    kind: "identity".to_string(),
                    token_budget: config.context.identity as u64,
                    estimated_tokens: 0,
                    priority: 0,
                    staleness_policy: "always_fresh".to_string(),
                    loaded: true,
                    refresh_required: true,
                    source: "system_instructions".to_string(),
                },
                DoctorContextTierCounter {
                    kind: "active_task".to_string(),
                    token_budget: config.context.task as u64,
                    estimated_tokens: 0,
                    priority: 1,
                    staleness_policy: "per_turn".to_string(),
                    loaded: true,
                    refresh_required: true,
                    source: "task_brief".to_string(),
                },
                DoctorContextTierCounter {
                    kind: "project".to_string(),
                    token_budget: config.context.project as u64,
                    estimated_tokens: 0,
                    priority: 2,
                    staleness_policy: "per_session".to_string(),
                    loaded: true,
                    refresh_required: false,
                    source: "project_memory".to_string(),
                },
                DoctorContextTierCounter {
                    kind: "shared".to_string(),
                    token_budget: config.context.shared as u64,
                    estimated_tokens: 0,
                    priority: 3,
                    staleness_policy: "on_demand".to_string(),
                    loaded: true,
                    refresh_required: false,
                    source: "shared_memory".to_string(),
                },
                DoctorContextTierCounter {
                    kind: "historical".to_string(),
                    token_budget: 0,
                    estimated_tokens: 0,
                    priority: 4,
                    staleness_policy: "retrieval_only".to_string(),
                    loaded: true,
                    refresh_required: false,
                    source: "transcript_archive".to_string(),
                },
            ],
        }),
        run_at: chrono::Utc::now().to_rfc3339(),
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

    let pass = results
        .iter()
        .filter(|r| r.status == CheckStatus::Pass)
        .count();
    let warn = results
        .iter()
        .filter(|r| r.status == CheckStatus::Warn)
        .count();
    let fail = results
        .iter()
        .filter(|r| r.status == CheckStatus::Fail)
        .count();

    out.push_str(&format!(
        "\n{pass} passed, {warn} warnings, {fail} failed\n"
    ));
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
        config.paths.spells_dir = root.join("data/spells");
        config.paths.skills_dir = root.join("data/skills");
        config.paths.logs_dir = root.join("data/logs");
        config.paths.backups_dir = root.join("data/backups");
        config.paths.config_dir = root.join("config");
        config.paths.secrets_dir = root.join("secrets");
        config.paths.workspace_dir = root.join("data/workspace");
        config.paths.cache_dir = root.join("data/cache");
        config.paths.data_dir = root.join("data/data");
        config
    }

    async fn create_path_layout(root: &Path) {
        for rel in [
            "data/db",
            "data/sessions",
            "data/memory",
            "data/media",
            "data/spells",
            "data/skills",
            "data/logs",
            "data/backups",
            "config",
            "secrets",
            "data/workspace",
            "data/cache",
            "data/data",
        ] {
            tokio::fs::create_dir_all(root.join(rel)).await.unwrap();
        }
    }

    #[tokio::test]
    async fn workspace_full_passes() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("AGENTS.md"), "# Agents")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("SOUL.md"), "# Soul")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("USER.md"), "# User")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("MEMORY.md"), "# Memory")
            .await
            .unwrap();
        tokio::fs::create_dir_all(tmp.path().join("memory"))
            .await
            .unwrap();

        let results = check_workspace(tmp.path()).await;
        let pass_count = results
            .iter()
            .filter(|r| r.status == CheckStatus::Pass)
            .count();
        assert!(
            pass_count >= 4,
            "expected at least 4 passes, got {pass_count}"
        );
    }

    #[tokio::test]
    async fn workspace_empty_warns() {
        let tmp = TempDir::new().unwrap();
        let results = check_workspace(tmp.path()).await;
        let warn_count = results
            .iter()
            .filter(|r| r.status == CheckStatus::Warn)
            .count();
        assert!(
            warn_count >= 3,
            "expected warnings for missing required files"
        );
    }

    #[tokio::test]
    async fn config_driven_checks_cover_paths_models_channels_and_memory() {
        let tmp = TempDir::new().unwrap();
        create_path_layout(tmp.path()).await;

        let mut config = test_config_with_paths(tmp.path());
        config
            .models
            .providers
            .push(rune_config::ModelProviderConfig {
                name: "azure-openai".into(),
                kind: "azure-openai".into(),
                base_url: "https://example.openai.azure.com".into(),
                api_key: None,
                deployment_name: Some("gpt-4.1".into()),
                api_version: Some("2024-10-21".into()),
                api_key_env: Some("RUNE_TEST_AZURE_KEY".into()),
                model_alias: Some("default".into()),
                models: vec![rune_config::ConfiguredModel::Id("gpt-4.1".into())],
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

        assert!(
            results
                .iter()
                .any(|r| r.name == "models.providers" && r.status == CheckStatus::Pass)
        );
        assert!(
            results
                .iter()
                .any(|r| r.name == "channels.telegram.auth" && r.status == CheckStatus::Pass)
        );
        assert!(
            results
                .iter()
                .any(|r| r.name == "memory.dir" && r.status == CheckStatus::Pass)
        );
        assert!(
            results
                .iter()
                .any(|r| r.name == "scheduler.storage" && r.status == CheckStatus::Pass)
        );

        unsafe {
            std::env::remove_var("RUNE_TEST_AZURE_KEY");
        }
    }

    #[tokio::test]
    async fn missing_provider_credentials_warn() {
        let tmp = TempDir::new().unwrap();
        create_path_layout(tmp.path()).await;
        let mut config = test_config_with_paths(tmp.path());
        config
            .models
            .providers
            .push(rune_config::ModelProviderConfig {
                name: "azure-openai".into(),
                kind: "azure-openai".into(),
                base_url: "https://example.openai.azure.com".into(),
                api_key: None,
                deployment_name: None,
                api_version: None,
                api_key_env: Some("RUNE_TEST_MISSING_KEY".into()),
                model_alias: None,
                models: vec![rune_config::ConfiguredModel::Id("gpt-4.1".into())],
            });

        unsafe {
            std::env::remove_var("RUNE_TEST_MISSING_KEY");
        }

        let results = check_models_config(&config).await;
        assert!(
            results
                .iter()
                .any(|r| r.name.ends_with(".auth") && r.status == CheckStatus::Warn)
        );
        assert!(
            results
                .iter()
                .any(|r| r.name.ends_with(".azure") && r.status == CheckStatus::Warn)
        );
    }

    #[tokio::test]
    async fn sqlite_auto_backend_checks_reflect_zero_config_layout() {
        let tmp = TempDir::new().unwrap();
        create_path_layout(tmp.path()).await;
        let mut config = test_config_with_paths(tmp.path());
        config.database.database_url = None;

        let initial = check_database_config(&config).await;
        assert!(initial.iter().any(|r| {
            r.name == "database.backend"
                && r.status == CheckStatus::Pass
                && r.message.contains("resolved runtime backend = sqlite")
        }));
        assert!(initial.iter().any(|r| {
            r.name == "database.url"
                && r.status == CheckStatus::Pass
                && r.message.contains("SQLite-backed repo path")
        }));
        assert!(!initial.iter().any(|r| r.name == "database.embedded.mode"));

        config.database.backend = rune_config::StorageBackend::Postgres;
        let postgres_fallback = check_database_config(&config).await;
        assert!(
            postgres_fallback
                .iter()
                .any(|r| { r.name == "database.embedded.mode" && r.status == CheckStatus::Pass })
        );
        assert!(postgres_fallback.iter().any(|r| {
            r.name == "database.embedded.install_dir" && r.status == CheckStatus::Skip
        }));
        assert!(postgres_fallback.iter().any(|r| {
            r.name == "database.embedded.cluster_dir" && r.status == CheckStatus::Skip
        }));

        tokio::fs::create_dir_all(config.paths.db_dir.join("pg_install"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(config.paths.db_dir.join("pg_data"))
            .await
            .unwrap();
        tokio::fs::write(config.paths.db_dir.join(".pg_password"), "runedev")
            .await
            .unwrap();

        let warmed = check_database_config(&config).await;
        assert!(warmed.iter().any(|r| {
            r.name == "database.embedded.install_dir" && r.status == CheckStatus::Pass
        }));
        assert!(warmed.iter().any(|r| {
            r.name == "database.embedded.cluster_dir" && r.status == CheckStatus::Pass
        }));
        assert!(warmed.iter().any(|r| {
            r.name == "database.embedded.password_file" && r.status == CheckStatus::Pass
        }));
    }

    #[tokio::test]
    async fn azure_sql_backend_checks_warn_on_shared_sql_path() {
        let tmp = TempDir::new().unwrap();
        create_path_layout(tmp.path()).await;
        let mut config = test_config_with_paths(tmp.path());
        config.database.backend = rune_config::StorageBackend::AzureSql;
        config.database.azure_sql_server = Some("server.database.windows.net".into());
        config.database.azure_sql_database = Some("rune".into());
        config.database.azure_sql_user = Some("hamza".into());
        config.database.azure_sql_password = Some("secret".into());

        let results = check_database_config(&config).await;
        assert!(results.iter().any(|r| {
            r.name == "database.backend"
                && r.status == CheckStatus::Pass
                && r.message.contains("resolved runtime backend = azure_sql")
        }));
        assert!(results.iter().any(|r| {
            r.name == "database.azure_sql"
                && r.status == CheckStatus::Warn
                && r.message
                    .contains("route it through the shared SQL-family backend path")
        }));
        assert!(
            results.iter().any(|r| {
                r.name == "database.azure_sql.identity" && r.status == CheckStatus::Warn
            })
        );
        assert!(
            results
                .iter()
                .any(|r| { r.name == "database.azure_sql.auth" && r.status == CheckStatus::Warn })
        );
    }

    #[tokio::test]
    async fn azure_sql_auto_detection_surfaces_missing_fields() {
        let tmp = TempDir::new().unwrap();
        create_path_layout(tmp.path()).await;
        let mut config = test_config_with_paths(tmp.path());
        config.database.backend = rune_config::StorageBackend::Auto;
        config.database.azure_sql_server = Some("server.database.windows.net".into());

        let results = check_database_config(&config).await;
        assert!(results.iter().any(|r| {
            r.name == "database.backend"
                && r.status == CheckStatus::Pass
                && r.message.contains("resolved runtime backend = azure_sql")
        }));
        assert!(
            results.iter().any(|r| {
                r.name == "database.azure_sql.identity" && r.status == CheckStatus::Fail
            })
        );
        assert!(
            results
                .iter()
                .any(|r| { r.name == "database.azure_sql.auth" && r.status == CheckStatus::Fail })
        );
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

    // ── Write-probe and mode-aware hint tests ────────────────────────────

    #[test]
    fn probe_writable_passes_for_writable_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(probe_writable(tmp.path()));
    }

    #[cfg(unix)]
    #[test]
    fn probe_writable_detects_readonly_dir() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let ro = tmp.path().join("readonly");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();

        // If the current process can still write (e.g. running as root),
        // the probe correctly returns true — skip the assertion.
        let actually_readonly = std::fs::write(ro.join(".test_guard"), b"x").is_err();
        if actually_readonly {
            assert!(!probe_writable(&ro), "probe should fail on read-only dir");
        }

        // Restore permissions for cleanup.
        let _ = std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755));
    }

    #[test]
    fn missing_path_hint_standalone_suggests_mkdir() {
        let nonexistent = PathBuf::from("/tmp/rune-test-nonexistent-path-xyz");
        let result = check_single_path(
            "paths.db_dir",
            &nonexistent,
            true,
            &rune_config::RuntimeMode::Standalone,
        );
        assert_eq!(result.status, CheckStatus::Warn);
        let hint = result.hint.unwrap();
        assert!(
            hint.contains("mkdir -p"),
            "standalone hint should suggest mkdir, got: {hint}"
        );
    }

    #[test]
    fn missing_path_hint_server_suggests_volume_mount() {
        let nonexistent = PathBuf::from("/data/db");
        let result = check_single_path(
            "paths.db_dir",
            &nonexistent,
            true,
            &rune_config::RuntimeMode::Server,
        );
        assert_eq!(result.status, CheckStatus::Warn);
        let hint = result.hint.unwrap();
        assert!(
            hint.contains("-v") && hint.contains("volume"),
            "server hint should suggest volume mount, got: {hint}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_path_returns_fail_with_mode_hint() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let ro = tmp.path().join("readonly");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();

        let actually_readonly = std::fs::write(ro.join(".test_guard"), b"x").is_err();
        if actually_readonly {
            // Standalone mode — hint should suggest chmod.
            let standalone = check_single_path(
                "paths.db_dir",
                &ro,
                true,
                &rune_config::RuntimeMode::Standalone,
            );
            assert_eq!(standalone.status, CheckStatus::Fail);
            assert!(standalone.message.contains("write probe failed"));
            let hint = standalone.hint.unwrap();
            assert!(
                hint.contains("chmod"),
                "standalone unwritable hint should suggest chmod, got: {hint}"
            );

            // Server mode — hint should reference mount flags / UID.
            let server =
                check_single_path("paths.db_dir", &ro, true, &rune_config::RuntimeMode::Server);
            assert_eq!(server.status, CheckStatus::Fail);
            let hint = server.hint.unwrap();
            assert!(
                hint.contains("mount flags") || hint.contains("UID"),
                "server unwritable hint should reference mount/UID, got: {hint}"
            );
        }

        let _ = std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755));
    }

    #[test]
    fn not_a_directory_gives_rm_and_mkdir_hint() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("not_a_dir");
        std::fs::write(&file_path, b"oops").unwrap();

        let result = check_single_path(
            "paths.db_dir",
            &file_path,
            true,
            &rune_config::RuntimeMode::Standalone,
        );
        assert_eq!(result.status, CheckStatus::Fail);
        assert!(result.message.contains("not a directory"));
        let hint = result.hint.unwrap();
        assert!(
            hint.contains("mkdir -p"),
            "not-a-dir hint should suggest mkdir, got: {hint}"
        );
    }

    // ── Approval / security doctor check tests ───────────────────────

    #[test]
    fn approval_default_passes() {
        let config = AppConfig::default();
        let results = check_approval_security(&config);
        assert!(results.iter().any(|r| {
            r.name == "approval.mode"
                && r.status == CheckStatus::Pass
                && r.message.contains("prompt")
        }));
        assert!(results.iter().any(|r| {
            r.name == "security.posture"
                && r.status == CheckStatus::Pass
                && r.message.contains("standard")
        }));
        assert!(
            !results.iter().any(|r| r.name == "security.unrestricted"),
            "no unrestricted warning for default config"
        );
    }

    #[test]
    fn approval_yolo_warns() {
        let mut config = AppConfig::default();
        config.approval.mode = rune_config::ApprovalMode::Yolo;
        let results = check_approval_security(&config);
        assert!(results.iter().any(|r| {
            r.name == "approval.mode" && r.status == CheckStatus::Warn && r.message.contains("yolo")
        }));
    }

    #[test]
    fn security_no_sandbox_warns() {
        let mut config = AppConfig::default();
        config.security.sandbox = false;
        let results = check_approval_security(&config);
        assert!(results.iter().any(|r| {
            r.name == "security.posture"
                && r.status == CheckStatus::Warn
                && r.message.contains("no-sandbox")
        }));
    }

    #[test]
    fn yolo_plus_no_sandbox_emits_unrestricted_warning() {
        let mut config = AppConfig::default();
        config.approval.mode = rune_config::ApprovalMode::Yolo;
        config.security.sandbox = false;
        let results = check_approval_security(&config);
        assert!(
            results
                .iter()
                .any(|r| r.name == "security.unrestricted" && r.status == CheckStatus::Warn),
            "expected unrestricted warning for yolo + no-sandbox"
        );
    }

    /// Simulate what happens when --yolo --no-sandbox CLI flags are applied
    /// via `apply_cli_overrides` before doctor runs.
    #[test]
    fn cli_bypass_flags_flow_through_to_doctor() {
        let mut config = AppConfig::default();
        // Default should pass
        let before = check_approval_security(&config);
        assert!(before.iter().all(|r| r.status == CheckStatus::Pass));

        // Apply CLI overrides (simulating --yolo --no-sandbox)
        config.apply_cli_overrides(true, true);

        let after = check_approval_security(&config);
        assert!(
            after
                .iter()
                .any(|r| r.name == "approval.mode" && r.status == CheckStatus::Warn),
            "yolo via CLI flag should warn in doctor"
        );
        assert!(
            after
                .iter()
                .any(|r| r.name == "security.posture" && r.status == CheckStatus::Warn),
            "no-sandbox via CLI flag should warn in doctor"
        );
        assert!(
            after
                .iter()
                .any(|r| r.name == "security.unrestricted" && r.status == CheckStatus::Warn),
            "combined CLI flags should trigger unrestricted warning"
        );
    }
}
