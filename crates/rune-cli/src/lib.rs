#![doc = "Operator CLI for Rune: subcommands, output formatting, and gateway client."]

pub mod cli;
pub mod client;
pub mod doctor;
pub mod memory;
pub mod output;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::time::SystemTime;

pub use cli::Cli;
use cli::{
    ChannelsAction, Command, ConfigAction, CronAction, GatewayAction, MemoryAction, ModelsAction,
    SessionsAction,
};
use client::{GatewayClient, show_config, validate_config};
use output::{
    ChannelCapabilitiesResponse, ChannelDetail, ChannelListResponse, ChannelLogFile,
    ChannelLogsResponse, ChannelResolveResponse, ChannelStatusResponse, ModelListResponse,
    ModelProviderDetail, ModelSetResponse, ModelStatusResponse, OutputFormat, render,
};

/// Initialize a workspace directory with default files.
fn load_config() -> rune_config::AppConfig {
    rune_config::AppConfig::load(None::<&std::path::Path>).unwrap_or_default()
}

fn discover_local_config_path() -> std::path::PathBuf {
    std::env::var_os("RUNE_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("config.toml"))
}

fn set_default_model(model_ref: &str) -> Result<ModelSetResponse> {
    let config_path = discover_local_config_path();
    let config = rune_config::AppConfig::load(Some(&config_path)).with_context(|| {
        format!(
            "failed to load config from {} before updating default model",
            config_path.display()
        )
    })?;

    let resolved = config.models.resolve_model(model_ref).with_context(|| {
        format!("model `{model_ref}` is not resolvable from configured inventory")
    })?;
    let canonical = resolved.canonical_model_id();
    let inventory = config.models.model_ids();
    if !inventory.is_empty() && !inventory.iter().any(|entry| entry == &canonical) {
        anyhow::bail!("model `{model_ref}` is not present in configured inventory");
    }
    let previous = config.models.default_model.clone();

    let original = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;

    let mut lines = original
        .lines()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>();

    let mut in_models = false;
    let mut replaced = false;
    let mut insert_at = None;

    for (idx, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = trimmed.trim_matches(&['[', ']'][..]);
            if section == "models" {
                in_models = true;
                insert_at = Some(idx + 1);
                continue;
            }
            if in_models {
                insert_at = Some(idx);
                break;
            }
        }

        if in_models && trimmed.starts_with("default_model") {
            *line = format!("default_model = \"{canonical}\"");
            replaced = true;
            break;
        }
    }

    if !replaced {
        if let Some(idx) = insert_at {
            lines.insert(idx, format!("default_model = \"{canonical}\""));
        } else {
            if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
                lines.push(String::new());
            }
            lines.push("[models]".to_string());
            lines.push(format!("default_model = \"{canonical}\""));
        }
    }

    let updated = format!("{}\n", lines.join("\n"));
    std::fs::write(&config_path, updated)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    Ok(ModelSetResponse {
        changed: previous.as_deref() != Some(canonical.as_str()),
        config_path: config_path.display().to_string(),
        previous_model: previous,
        default_model: canonical,
        note: "Local config updated; restart gateway to apply new default sessions.".to_string(),
    })
}

fn channel_details() -> Vec<ChannelDetail> {
    let config = load_config();
    let telegram_configured = config
        .channels
        .telegram_token
        .as_deref()
        .is_some_and(|token| !token.trim().is_empty());
    let telegram_enabled = config
        .channels
        .enabled
        .iter()
        .any(|name| name == "telegram");

    vec![ChannelDetail {
        name: "telegram".to_string(),
        enabled: telegram_enabled,
        configured: telegram_configured,
        status: if telegram_enabled && telegram_configured {
            "ready".to_string()
        } else if telegram_configured {
            "configured".to_string()
        } else {
            "disabled".to_string()
        },
        capabilities: vec![
            "receive.message".to_string(),
            "receive.edit".to_string(),
            "send.message".to_string(),
            "send.reply".to_string(),
            "edit.message".to_string(),
            "delete.message".to_string(),
        ],
        notes: if telegram_configured {
            None
        } else {
            Some("Set channels.telegram_token and enable telegram in channels.enabled".to_string())
        },
    }]
}

fn resolve_channel(target: &str, channels: &[ChannelDetail]) -> ChannelResolveResponse {
    let normalized = target.trim().to_ascii_lowercase();
    let aliases = match normalized.as_str() {
        "tg" | "telegram-bot" | "telegram_bot" => vec!["telegram"],
        other => vec![other],
    };

    let channel = channels
        .iter()
        .find(|channel| aliases.iter().any(|alias| channel.name.eq_ignore_ascii_case(alias)))
        .cloned();

    ChannelResolveResponse {
        target: target.to_string(),
        matched: channel.is_some(),
        channel,
        note: if channels.is_empty() {
            Some("No channels are currently described by the local config/runtime inventory.".to_string())
        } else if normalized != "telegram" && aliases == vec![normalized.as_str()] {
            Some(format!(
                "Known channels: {}",
                channels
                    .iter()
                    .map(|channel| channel.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        } else {
            None
        },
    }
}

fn channel_logs(filter: Option<&str>, limit: usize) -> ChannelLogsResponse {
    let config = load_config();
    let logs_dir = config.paths.logs_dir;
    let normalized_filter = filter.map(|value| value.trim().to_ascii_lowercase());

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&logs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let file_name = match path.file_name().and_then(|name| name.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };
            if let Some(filter_value) = &normalized_filter {
                if !file_name.to_ascii_lowercase().contains(filter_value) {
                    continue;
                }
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|ts| ts.duration_since(SystemTime::UNIX_EPOCH).ok())
                .and_then(|duration| chrono::DateTime::<Utc>::from_timestamp(duration.as_secs() as i64, 0))
                .map(|ts| ts.to_rfc3339());
            files.push(ChannelLogFile {
                path: path.display().to_string(),
                modified_at,
                size_bytes: metadata.len(),
            });
        }
    }

    files.sort_by(|left, right| right.modified_at.cmp(&left.modified_at).then_with(|| left.path.cmp(&right.path)));
    files.truncate(limit);

    let note = if !logs_dir.exists() {
        Some("Configured logs_dir does not exist yet; no local channel logs are available.".to_string())
    } else if files.is_empty() {
        Some("No matching log files found in the configured logs_dir.".to_string())
    } else {
        Some("This is a local filesystem view of channel-related logs, not a remote provider log API.".to_string())
    };

    ChannelLogsResponse {
        logs_dir: logs_dir.display().to_string(),
        filter: filter.map(str::to_string),
        files,
        note,
    }
}

fn model_provider_details() -> ModelListResponse {
    let config = load_config();
    let default_model = config.models.default_model.clone().or_else(|| {
        config
            .agents
            .default_agent()
            .and_then(|agent| config.agents.effective_model(agent))
            .map(ToOwned::to_owned)
    });

    let providers = config
        .models
        .providers
        .iter()
        .map(|provider| {
            let credential_source = if provider
                .api_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty())
            {
                "api_key".to_string()
            } else if let Some(env_var) = provider.api_key_env.as_deref() {
                format!("env:{env_var}")
            } else {
                "env:OPENAI_API_KEY".to_string()
            };

            let credentials_ready = provider
                .api_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty())
                || provider
                    .api_key_env
                    .as_deref()
                    .and_then(|env_var| std::env::var(env_var).ok())
                    .is_some_and(|value| !value.trim().is_empty())
                || (provider.api_key_env.is_none()
                    && std::env::var("OPENAI_API_KEY")
                        .ok()
                        .is_some_and(|value| !value.trim().is_empty()));

            let notes = match provider.kind.as_str() {
                "azure-openai" | "azure_openai" | "azure"
                    if provider.deployment_name.is_none() || provider.api_version.is_none() =>
                {
                    Some(
                        "Azure OpenAI requires deployment_name and api_version for parity."
                            .to_string(),
                    )
                }
                "azure-foundry" if !provider.base_url.contains("services.ai.azure.com") => Some(
                    "Azure Foundry is expected to use an Azure AI Foundry base URL.".to_string(),
                ),
                _ => None,
            };

            ModelProviderDetail {
                name: provider.name.clone(),
                kind: provider.kind.clone(),
                base_url: provider.base_url.clone(),
                default_model: default_model.clone(),
                model_alias: provider.model_alias.clone(),
                deployment_name: provider.deployment_name.clone(),
                api_version: provider.api_version.clone(),
                credential_source,
                credentials_ready,
                notes,
            }
        })
        .collect();

    ModelListResponse {
        default_model,
        providers,
    }
}

async fn init_workspace(path: &std::path::Path) -> Result<()> {
    tokio::fs::create_dir_all(path)
        .await
        .with_context(|| format!("cannot create directory: {}", path.display()))?;
    tokio::fs::create_dir_all(path.join("memory")).await?;

    let files: &[(&str, &str)] = &[
        (
            "AGENTS.md",
            "# AGENTS.md - Your Workspace\n\nAdd your agent configuration here.\n",
        ),
        (
            "SOUL.md",
            "# SOUL.md - Who You Are\n\nDefine your assistant's personality and style.\n",
        ),
        (
            "USER.md",
            "# USER.md - About Your Human\n\n- **Name:**\n- **Timezone:**\n- **Notes:**\n",
        ),
        (
            "TOOLS.md",
            "# TOOLS.md - Local Notes\n\nAdd environment-specific tool notes here.\n",
        ),
        (
            "MEMORY.md",
            "# MEMORY.md\n\nLong-term memory — curated and updated over time.\n",
        ),
    ];

    let mut created = 0;
    for (name, content) in files {
        let file_path = path.join(name);
        if !file_path.exists() {
            tokio::fs::write(&file_path, content).await?;
            created += 1;
            println!("  ✓ Created {name}");
        } else {
            println!("  ○ {name} already exists, skipping");
        }
    }

    println!(
        "\nWorkspace initialized at {} ({created} files created)",
        path.display()
    );
    Ok(())
}

/// Execute the parsed CLI command against the configured gateway and print output.
pub async fn run(cli: Cli) -> Result<()> {
    let format = OutputFormat::from_json_flag(cli.json);
    let client = GatewayClient::new(&cli.gateway_url);

    match cli.command {
        Command::Gateway { action } | Command::Daemon { action } => match action {
            GatewayAction::Status => {
                let result = client.gateway_status().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Health => {
                let result = client.gateway_health().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Start => {
                let result = client.gateway_start().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Stop => {
                let result = client.gateway_stop().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Restart => {
                let result = client.gateway_restart().await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Status => {
            let result = client.status().await?;
            println!("{}", render(&result, format));
        }
        Command::Health => {
            let result = client.health().await?;
            println!("{}", render(&result, format));
        }
        Command::Doctor => {
            let ws_root = dirs::home_dir()
                .map(|h| h.join(".rune/workspace"))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let results =
                doctor::run_all_checks(None, Some(&cli.gateway_url), Some(&ws_root)).await;
            let output = doctor::format_results(&results);
            if matches!(format, OutputFormat::Json) {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&results).unwrap_or_default()
                );
            } else {
                print!("{output}");
            }
        }
        Command::Init { path } => {
            let target = std::path::Path::new(&path);
            init_workspace(target).await?;
        }
        Command::Cron { action } => match action {
            CronAction::Status => {
                let result = client.cron_status().await?;
                println!("{}", render(&result, format));
            }
            CronAction::List { include_disabled } => {
                let result = client.cron_list(include_disabled).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Add {
                name,
                text,
                at,
                session_target,
            } => {
                let at = DateTime::parse_from_rfc3339(&at)
                    .with_context(|| format!("invalid --at timestamp: {at}"))?
                    .with_timezone(&Utc);
                let result = client
                    .cron_add_system_event(name.as_deref(), &text, at, &session_target)
                    .await?;
                println!("{}", render(&result, format));
            }
            CronAction::Edit { id, name } => {
                let result = client.cron_edit_name(&id, &name).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Enable { id } => {
                let result = client.cron_enable(&id).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Disable { id } => {
                let result = client.cron_disable(&id).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Rm { id } => {
                let result = client.cron_remove(&id).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Run { id } => {
                let result = client.cron_run(&id).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Runs { id } => {
                let result = client.cron_runs(&id).await?;
                println!("{}", render(&result, format));
            }
            CronAction::Wake {
                text,
                mode,
                context_messages,
            } => {
                let result = client.cron_wake(&text, &mode, context_messages).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Sessions { action } => match action {
            SessionsAction::List => {
                let result = client.sessions_list().await?;
                println!("{}", render(&result, format));
            }
            SessionsAction::Show { id } => {
                let result = client.sessions_show(&id).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Channels { action } => {
            let channels = channel_details();
            match action {
                ChannelsAction::List => {
                    let result = ChannelListResponse { channels };
                    println!("{}", render(&result, format));
                }
                ChannelsAction::Status => {
                    let ready = channels
                        .iter()
                        .filter(|channel| channel.status == "ready")
                        .count();
                    let result = ChannelStatusResponse {
                        total: channels.len(),
                        enabled: channels.iter().filter(|channel| channel.enabled).count(),
                        configured: channels.iter().filter(|channel| channel.configured).count(),
                        ready,
                        channels,
                    };
                    println!("{}", render(&result, format));
                }
                ChannelsAction::Capabilities => {
                    let result = ChannelCapabilitiesResponse { channels };
                    println!("{}", render(&result, format));
                }
                ChannelsAction::Resolve { target } => {
                    let result = resolve_channel(&target, &channels);
                    println!("{}", render(&result, format));
                }
                ChannelsAction::Logs { channel, limit } => {
                    let result = channel_logs(channel.as_deref(), limit);
                    println!("{}", render(&result, format));
                }
            }
        }
        Command::Models { action } => {
            let result = model_provider_details();
            match action {
                ModelsAction::List => {
                    println!("{}", render(&result, format));
                }
                ModelsAction::Status => {
                    let ready = result
                        .providers
                        .iter()
                        .filter(|provider| provider.credentials_ready)
                        .count();
                    let status = ModelStatusResponse {
                        default_model: result.default_model,
                        total: result.providers.len(),
                        credentials_ready: ready,
                        providers: result.providers,
                    };
                    println!("{}", render(&status, format));
                }
                ModelsAction::Set { model } => {
                    let result = set_default_model(&model)?;
                    println!("{}", render(&result, format));
                }
            }
        }
        Command::Memory { action } => match action {
            MemoryAction::Status => {
                let result = memory::status().await?;
                println!("{}", render(&result, format));
            }
            MemoryAction::Search { query, max_results } => {
                let result = memory::search(&query, max_results).await?;
                println!("{}", render(&result, format));
            }
            MemoryAction::Get { path, from, lines } => {
                let result = memory::get(&path, from, lines).await?;
                println!("{}", render(&result, format));
            }
        },
        Command::Config { action } => match action {
            ConfigAction::Show => {
                let result = show_config()?;
                if matches!(format, OutputFormat::Json) {
                    println!("{result}");
                } else {
                    println!("Resolved configuration:\n{result}");
                }
            }
            ConfigAction::Validate { file } => {
                let result = validate_config(file.as_deref());
                println!("{}", render(&result, format));
            }
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};
    use tempfile::TempDir;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn set_default_model_updates_existing_models_section() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[models]
default_model = "oc-01-openai/gpt-5.4"

[[models.providers]]
name = "oc-01-openai"
kind = "openai"
base_url = "https://example.test/openai/v1"
api_key = "test-key"
models = ["gpt-5.4", "gpt-5.4-pro"]
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_CONFIG", &config_path);
        }

        let response = set_default_model("oc-01-openai/gpt-5.4-pro").unwrap();
        assert!(response.changed);
        assert_eq!(response.previous_model.as_deref(), Some("oc-01-openai/gpt-5.4"));
        assert_eq!(response.default_model, "oc-01-openai/gpt-5.4-pro");

        let updated = std::fs::read_to_string(&config_path).unwrap();
        assert!(updated.contains("default_model = \"oc-01-openai/gpt-5.4-pro\""));

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[test]
    fn set_default_model_accepts_unambiguous_short_name() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[[models.providers]]
name = "hamza-eastus2"
kind = "openai"
base_url = "https://example.test/openai/v1"
api_key = "test-key"
models = ["grok-4-fast-reasoning"]
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_CONFIG", &config_path);
        }

        let response = set_default_model("grok-4-fast-reasoning").unwrap();
        assert_eq!(response.default_model, "hamza-eastus2/grok-4-fast-reasoning");

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[test]
    fn set_default_model_rejects_unknown_inventory_entry() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[[models.providers]]
name = "oc-01-openai"
kind = "openai"
base_url = "https://example.test/openai/v1"
api_key = "test-key"
models = ["gpt-5.4"]
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("RUNE_CONFIG", &config_path);
        }

        let err = set_default_model("not-a-real-model").unwrap_err();
        assert!(
            err.to_string().contains("not present in configured inventory")
                || err.to_string().contains("not resolvable")
        );

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }
}
