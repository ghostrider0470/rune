#![doc = "Operator CLI for Rune: subcommands, output formatting, and gateway client."]

pub mod cli;
pub mod client;
pub mod doctor;
pub mod memory;
pub mod output;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::CommandFactory;
use clap_complete::{Shell, generate};
use std::process::Command as StdCommand;
use std::time::SystemTime;

#[cfg(test)]
pub(crate) fn test_env_lock() -> &'static std::sync::Mutex<()> {
    use std::sync::{Mutex, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub use cli::Cli;
use cli::{
    ApprovalsAction, ChannelsAction, Command, CompletionAction, CompletionShell, ConfigAction,
    CronAction, GatewayAction, MemoryAction, ModelsAction, RemindersAction, SessionsAction,
    SystemAction, SystemHeartbeatAction,
};
use client::{
    GatewayClient, config_file, config_get, config_set, config_unset, show_config, validate_config,
};
use output::{
    ChannelCapabilitiesResponse, ChannelDetail, ChannelListResponse, ChannelLogFile,
    ChannelLogsResponse, ChannelResolveResponse, ChannelStatusResponse, DashboardChannelsSummary,
    DashboardModelsSummary, DashboardResponse, DashboardSessionsSummary,
    HeartbeatPresenceResponse, ModelAliasDetail, ModelAliasesResponse, ModelListResponse,
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

fn run_gateway_foreground() -> Result<()> {
    let mut args = Vec::new();
    if let Some(config_path) = std::env::var_os("RUNE_CONFIG") {
        args.push("--config".to_string());
        args.push(config_path.to_string_lossy().into_owned());
    }

    let status = StdCommand::new("rune-gateway")
        .args(&args)
        .status()
        .context("failed to start `rune-gateway`; ensure the binary is installed and on PATH")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("`rune-gateway` exited with status {status}");
    }
}

fn completion_shell(shell: CompletionShell) -> Shell {
    match shell {
        CompletionShell::Bash => Shell::Bash,
        CompletionShell::Elvish => Shell::Elvish,
        CompletionShell::Fish => Shell::Fish,
        CompletionShell::PowerShell => Shell::PowerShell,
        CompletionShell::Zsh => Shell::Zsh,
    }
}

fn print_completion(shell: CompletionShell) -> Result<()> {
    let mut command = Cli::command();
    generate(completion_shell(shell), &mut command, "rune", &mut std::io::stdout());
    Ok(())
}

fn parse_reminder_duration(input: &str) -> Result<DateTime<Utc>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("reminder duration cannot be empty");
    }

    let split_at = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| anyhow::anyhow!("invalid reminder duration `{trimmed}`; expected forms like 30m, 2h, 1d"))?;
    let (amount_raw, unit_raw) = trimmed.split_at(split_at);
    let amount: i64 = amount_raw
        .parse()
        .with_context(|| format!("invalid reminder duration amount `{amount_raw}`"))?;
    if amount <= 0 {
        anyhow::bail!("reminder duration must be positive");
    }

    let delta = match unit_raw.trim().to_ascii_lowercase().as_str() {
        "m" | "min" | "mins" | "minute" | "minutes" => chrono::Duration::minutes(amount),
        "h" | "hr" | "hrs" | "hour" | "hours" => chrono::Duration::hours(amount),
        "d" | "day" | "days" => chrono::Duration::days(amount),
        other => {
            anyhow::bail!(
                "invalid reminder duration unit `{other}`; expected minutes (m), hours (h), or days (d)"
            )
        }
    };

    Ok(Utc::now() + delta)
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
        .find(|channel| {
            aliases
                .iter()
                .any(|alias| channel.name.eq_ignore_ascii_case(alias))
        })
        .cloned();

    ChannelResolveResponse {
        target: target.to_string(),
        matched: channel.is_some(),
        channel,
        note: if channels.is_empty() {
            Some(
                "No channels are currently described by the local config/runtime inventory."
                    .to_string(),
            )
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

fn heartbeat_presence() -> HeartbeatPresenceResponse {
    let config = load_config();
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let path = workspace_root.join("HEARTBEAT.md");
    match std::fs::metadata(&path) {
        Ok(metadata) => {
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|ts| ts.duration_since(SystemTime::UNIX_EPOCH).ok())
                .and_then(|duration| {
                    chrono::DateTime::<Utc>::from_timestamp(duration.as_secs() as i64, 0)
                })
                .map(|ts| ts.to_rfc3339());
            HeartbeatPresenceResponse {
                workspace_root: workspace_root.display().to_string(),
                path: path.display().to_string(),
                present: true,
                modified_at,
                size_bytes: Some(metadata.len()),
                note: Some(format!(
                    "Scheduled sessions load HEARTBEAT.md; runtime memory dir is {}.",
                    config.paths.memory_dir.display()
                )),
            }
        }
        Err(_) => HeartbeatPresenceResponse {
            workspace_root: workspace_root.display().to_string(),
            path: path.display().to_string(),
            present: false,
            modified_at: None,
            size_bytes: None,
            note: Some("No HEARTBEAT.md present in the current workspace root.".to_string()),
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
                .and_then(|duration| {
                    chrono::DateTime::<Utc>::from_timestamp(duration.as_secs() as i64, 0)
                })
                .map(|ts| ts.to_rfc3339());
            files.push(ChannelLogFile {
                path: path.display().to_string(),
                modified_at,
                size_bytes: metadata.len(),
            });
        }
    }

    files.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| left.path.cmp(&right.path))
    });
    files.truncate(limit);

    let note = if !logs_dir.exists() {
        Some(
            "Configured logs_dir does not exist yet; no local channel logs are available."
                .to_string(),
        )
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

fn provider_credential_source(provider: &rune_config::ModelProviderConfig) -> String {
    if provider
        .api_key
        .as_deref()
        .is_some_and(|key| !key.trim().is_empty())
    {
        "api_key".to_string()
    } else if let Some(env_var) = provider.api_key_env.as_deref() {
        format!("env:{env_var}")
    } else {
        "env:OPENAI_API_KEY".to_string()
    }
}

fn provider_credentials_ready(provider: &rune_config::ModelProviderConfig) -> bool {
    provider
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
                .is_some_and(|value| !value.trim().is_empty()))
}

fn provider_notes(provider: &rune_config::ModelProviderConfig) -> Option<String> {
    match provider.kind.as_str() {
        "azure-openai" | "azure_openai" | "azure"
            if provider.deployment_name.is_none() || provider.api_version.is_none() =>
        {
            Some("Azure OpenAI requires deployment_name and api_version for parity.".to_string())
        }
        "azure-foundry" if !provider.base_url.contains("services.ai.azure.com") => {
            Some("Azure Foundry is expected to use an Azure AI Foundry base URL.".to_string())
        }
        _ => None,
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
        .map(|provider| ModelProviderDetail {
            name: provider.name.clone(),
            kind: provider.kind.clone(),
            base_url: provider.base_url.clone(),
            default_model: default_model.clone(),
            model_alias: provider.model_alias.clone(),
            deployment_name: provider.deployment_name.clone(),
            api_version: provider.api_version.clone(),
            credential_source: provider_credential_source(provider),
            credentials_ready: provider_credentials_ready(provider),
            notes: provider_notes(provider),
        })
        .collect();

    ModelListResponse {
        default_model,
        providers,
    }
}

fn model_alias_details() -> ModelAliasesResponse {
    let config = load_config();
    let aliases = config
        .models
        .providers
        .iter()
        .filter_map(|provider| {
            provider.model_alias.as_ref().map(|alias| ModelAliasDetail {
                alias: alias.clone(),
                provider: provider.name.clone(),
                target_model: provider.models.first().map(|model| model.id().to_string()),
                provider_kind: provider.kind.clone(),
                base_url: provider.base_url.clone(),
                deployment_name: provider.deployment_name.clone(),
                api_version: provider.api_version.clone(),
                credentials_ready: provider_credentials_ready(provider),
                note: provider_notes(provider),
            })
        })
        .collect();

    ModelAliasesResponse { aliases }
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
            GatewayAction::Probe => {
                let result = client.gateway_probe().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Discover => {
                let result = client.gateway_discover().await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::Call {
                method,
                path,
                body,
                token,
            } => {
                let result = client
                    .gateway_call(&method, &path, body.as_deref(), token.as_deref())
                    .await?;
                println!("{}", render(&result, format));
            }
            GatewayAction::UsageCost => {
                let result = client.gateway_usage_cost().await?;
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
            GatewayAction::Run => {
                run_gateway_foreground()?;
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
        Command::Dashboard => {
            let gateway = client.status().await?;
            let health = client.health().await?;
            let cron = client.cron_status().await?;
            let sessions = client.sessions_list(None, None, 5).await?;
            let channels = channel_details();
            let models = model_provider_details();
            let memory = memory::status().await?;

            let dashboard = DashboardResponse {
                gateway,
                health,
                cron,
                sessions: DashboardSessionsSummary {
                    total: sessions.sessions.len(),
                    sample: sessions.sessions,
                },
                models: DashboardModelsSummary {
                    total: models.providers.len(),
                    credentials_ready: models
                        .providers
                        .iter()
                        .filter(|provider| provider.credentials_ready)
                        .count(),
                    default_model: models.default_model,
                },
                channels: DashboardChannelsSummary {
                    total: channels.len(),
                    enabled: channels.iter().filter(|channel| channel.enabled).count(),
                    configured: channels.iter().filter(|channel| channel.configured).count(),
                    ready: channels
                        .iter()
                        .filter(|channel| channel.status == "ready")
                        .count(),
                },
                memory,
            };
            println!("{}", render(&dashboard, format));
        }
        Command::Init { path } => {
            let target = std::path::Path::new(&path);
            init_workspace(target).await?;
        }
        Command::Completion { action } => match action {
            CompletionAction::Generate { shell } => {
                print_completion(shell)?;
            }
        },
        Command::Approvals { action } => match action {
            ApprovalsAction::List => {
                let result = client.approvals_list().await?;
                println!("{}", render(&result, format));
            }
            ApprovalsAction::Get { tool } => {
                let result = client.approvals_get(&tool).await?;
                println!("{}", render(&result, format));
            }
            ApprovalsAction::Set { tool, decision } => {
                let result = client.approvals_set(&tool, &decision).await?;
                println!("{}", render(&result, format));
            }
            ApprovalsAction::Clear { tool } => {
                let result = client.approvals_clear(&tool).await?;
                println!("{}", render(&result, format));
            }
        },
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
            SessionsAction::List {
                active_minutes,
                channel,
                limit,
            } => {
                let result = client
                    .sessions_list(active_minutes, channel.as_deref(), limit)
                    .await?;
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
                ModelsAction::Aliases => {
                    let result = model_alias_details();
                    println!("{}", render(&result, format));
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
        Command::System { action } => match action {
            SystemAction::Event {
                text,
                mode,
                context_messages,
            } => {
                let result = client.cron_wake(&text, &mode, context_messages).await?;
                println!("{}", render(&result, format));
            }
            SystemAction::Heartbeat { action } => match action {
                SystemHeartbeatAction::Presence | SystemHeartbeatAction::Last => {
                    let result = heartbeat_presence();
                    println!("{}", render(&result, format));
                }
                SystemHeartbeatAction::Enable => {
                    let result = client.heartbeat_enable().await?;
                    println!("{}", render(&result, format));
                }
                SystemHeartbeatAction::Disable => {
                    let result = client.heartbeat_disable().await?;
                    println!("{}", render(&result, format));
                }
                SystemHeartbeatAction::Status => {
                    let result = client.heartbeat_status().await?;
                    println!("{}", render(&result, format));
                }
            },
        },
        Command::Reminders { action } => match action {
            RemindersAction::Add {
                message,
                duration,
                target,
            } => {
                let fire_at = parse_reminder_duration(&duration)?;
                let result = client.reminders_add(&message, fire_at, &target).await?;
                println!("{}", render(&result, format));
            }
            RemindersAction::List { include_delivered } => {
                let result = client.reminders_list(include_delivered).await?;
                println!("{}", render(&result, format));
            }
            RemindersAction::Cancel { id } => {
                let result = client.reminders_cancel(&id).await?;
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
            ConfigAction::File => {
                let result = config_file();
                println!("{}", render(&result, format));
            }
            ConfigAction::Get { key } => {
                let result = config_get(&key)?;
                println!("{}", render(&result, format));
            }
            ConfigAction::Set { key, value } => {
                let result = config_set(&key, &value)?;
                println!("{}", render(&result, format));
            }
            ConfigAction::Unset { key } => {
                let result = config_unset(&key)?;
                println!("{}", render(&result, format));
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
    use tempfile::TempDir;

    #[test]
    fn set_default_model_updates_existing_models_section() {
        let _guard = crate::test_env_lock().lock().unwrap_or_else(|e| e.into_inner());
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
        assert_eq!(
            response.previous_model.as_deref(),
            Some("oc-01-openai/gpt-5.4")
        );
        assert_eq!(response.default_model, "oc-01-openai/gpt-5.4-pro");

        let updated = std::fs::read_to_string(&config_path).unwrap();
        assert!(updated.contains("default_model = \"oc-01-openai/gpt-5.4-pro\""));

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[test]
    fn set_default_model_accepts_unambiguous_short_name() {
        let _guard = crate::test_env_lock().lock().unwrap_or_else(|e| e.into_inner());
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
        assert_eq!(
            response.default_model,
            "hamza-eastus2/grok-4-fast-reasoning"
        );

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[test]
    fn parse_reminder_duration_minutes() {
        let fire_at = parse_reminder_duration("30m").unwrap();
        let delta = fire_at.signed_duration_since(Utc::now());
        assert!(delta.num_minutes() >= 29 && delta.num_minutes() <= 30);
    }

    #[test]
    fn parse_reminder_duration_hours() {
        let fire_at = parse_reminder_duration("2h").unwrap();
        let delta = fire_at.signed_duration_since(Utc::now());
        assert!(delta.num_minutes() >= 119 && delta.num_minutes() <= 120);
    }

    #[test]
    fn parse_reminder_duration_rejects_bad_unit() {
        let err = parse_reminder_duration("5w").unwrap_err();
        assert!(err.to_string().contains("invalid reminder duration unit"));
    }

    #[test]
    fn set_default_model_rejects_unknown_inventory_entry() {
        let _guard = crate::test_env_lock().lock().unwrap_or_else(|e| e.into_inner());
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
            err.to_string()
                .contains("not present in configured inventory")
                || err.to_string().contains("not resolvable")
        );

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }
}
