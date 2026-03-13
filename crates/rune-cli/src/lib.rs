#![doc = "Operator CLI for Rune: subcommands, output formatting, and gateway client."]

pub mod cli;
pub mod client;
pub mod doctor;
pub mod memory;
pub mod output;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

pub use cli::Cli;
use cli::{
    ChannelsAction, Command, ConfigAction, CronAction, GatewayAction, MemoryAction, ModelsAction,
    SessionsAction,
};
use client::{GatewayClient, show_config, validate_config};
use output::{
    ChannelCapabilitiesResponse, ChannelDetail, ChannelListResponse, ChannelStatusResponse,
    ModelListResponse, ModelProviderDetail, ModelStatusResponse, OutputFormat, render,
};

/// Initialize a workspace directory with default files.
fn load_config() -> rune_config::AppConfig {
    rune_config::AppConfig::load(None::<&std::path::Path>).unwrap_or_default()
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
        Command::Gateway { action } => match action {
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
