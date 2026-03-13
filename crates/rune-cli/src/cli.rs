//! Clap-based CLI definition with all subcommands.

use clap::{Parser, Subcommand};

/// Rune — the operator CLI for managing the Rune AI runtime.
#[derive(Debug, Parser)]
#[command(name = "rune", version, about = "Rune AI runtime operator CLI")]
pub struct Cli {
    /// Output as JSON instead of human-readable text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Gateway base URL (default: http://127.0.0.1:8787).
    #[arg(
        long,
        global = true,
        env = "RUNE_GATEWAY_URL",
        default_value = "http://127.0.0.1:8787"
    )]
    pub gateway_url: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage the gateway daemon.
    Gateway {
        #[command(subcommand)]
        action: GatewayAction,
    },
    /// Query gateway status.
    Status,
    /// Run a health check against the gateway.
    Health,
    /// Run diagnostic checks (config, connectivity, etc.).
    Doctor,
    /// Manage cron jobs.
    Cron {
        #[command(subcommand)]
        action: CronAction,
    },
    /// Manage sessions.
    Sessions {
        #[command(subcommand)]
        action: SessionsAction,
    },
    /// Inspect configured channel adapters.
    Channels {
        #[command(subcommand)]
        action: ChannelsAction,
    },
    /// Inspect configured model providers and routing.
    Models {
        #[command(subcommand)]
        action: ModelsAction,
    },
    /// Inspect workspace memory files and retrieval state.
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Initialize a new workspace with default files.
    Init {
        /// Directory to initialize (defaults to current directory).
        #[arg(default_value = ".")]
        path: String,
    },
    /// Manage configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum GatewayAction {
    /// Query gateway status.
    Status,
    /// Run a health check against the gateway.
    Health,
    /// Start the gateway daemon.
    Start,
    /// Stop the gateway daemon.
    Stop,
    /// Restart the gateway daemon.
    Restart,
}

#[derive(Debug, Subcommand)]
pub enum CronAction {
    /// Show scheduler status.
    Status,
    /// List cron jobs.
    List {
        /// Include disabled jobs in the listing.
        #[arg(long)]
        include_disabled: bool,
    },
    /// Add a one-shot reminder/system event job.
    Add {
        /// Human-readable name for the job.
        #[arg(long)]
        name: Option<String>,
        /// Text for the system event payload.
        #[arg(long)]
        text: String,
        /// Fire time as RFC3339/ISO-8601.
        #[arg(long)]
        at: String,
        /// Session target (`main` or `isolated`). Defaults to `main`.
        #[arg(long, default_value = "main")]
        session_target: String,
    },
    /// Update a job's display name.
    Edit {
        /// Job ID.
        id: String,
        /// New name.
        #[arg(long)]
        name: String,
    },
    /// Enable a job.
    Enable {
        /// Job ID.
        id: String,
    },
    /// Disable a job.
    Disable {
        /// Job ID.
        id: String,
    },
    /// Remove a job.
    Rm {
        /// Job ID.
        id: String,
    },
    /// Trigger a job immediately.
    Run {
        /// Job ID.
        id: String,
    },
    /// Show run history for a job.
    Runs {
        /// Job ID.
        id: String,
    },
    /// Queue a wake event for the runtime heartbeat/session layer.
    Wake {
        /// Wake/reminder text to inject.
        #[arg(long)]
        text: String,
        /// Delivery timing mode (`next-heartbeat` or `now`).
        #[arg(long, default_value = "next-heartbeat")]
        mode: String,
        /// Optional number of recent context messages to attach.
        #[arg(long = "context-messages")]
        context_messages: Option<u64>,
    },
}

#[derive(Debug, Subcommand)]
pub enum SessionsAction {
    /// List all sessions.
    List,
    /// Show details for a specific session.
    Show {
        /// Session ID to inspect.
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ChannelsAction {
    /// List configured channel adapters and whether they are enabled.
    List,
    /// Show detailed channel status from resolved config.
    Status,
    /// Show channel capability inventory.
    Capabilities,
}

#[derive(Debug, Subcommand)]
pub enum ModelsAction {
    /// List configured model providers and aliases.
    List,
    /// Show resolved default-model and credential readiness status.
    Status,
    /// Set the default model in local config.toml after validating against configured inventory.
    Set {
        /// Model id to set. Accepts canonical `provider/model` ids and unambiguous short names.
        model: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum MemoryAction {
    /// Show workspace memory status.
    Status,
    /// Search MEMORY.md and memory/*.md for a query.
    Search {
        /// Query text to search for.
        query: String,
        /// Maximum number of hits to return.
        #[arg(long, default_value_t = 10)]
        max_results: usize,
    },
    /// Read a bounded snippet from MEMORY.md or memory/*.md.
    Get {
        /// Path relative to the workspace root: MEMORY.md or memory/*.md.
        path: String,
        /// Starting line number (1-indexed).
        #[arg(long, default_value_t = 1)]
        from: usize,
        /// Maximum lines to read.
        #[arg(long)]
        lines: Option<usize>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Dump the resolved configuration.
    Show,
    /// Validate the configuration file.
    Validate {
        /// Path to config file (default: rune.toml).
        #[arg(short, long)]
        file: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_status() {
        let cli = Cli::try_parse_from(["rune", "status"]).unwrap();
        assert!(matches!(cli.command, Command::Status));
        assert!(!cli.json);
    }

    #[test]
    fn parse_health() {
        let cli = Cli::try_parse_from(["rune", "health"]).unwrap();
        assert!(matches!(cli.command, Command::Health));
    }

    #[test]
    fn parse_doctor() {
        let cli = Cli::try_parse_from(["rune", "doctor"]).unwrap();
        assert!(matches!(cli.command, Command::Doctor));
    }

    #[test]
    fn parse_json_flag() {
        let cli = Cli::try_parse_from(["rune", "--json", "status"]).unwrap();
        assert!(cli.json);
    }

    #[test]
    fn parse_gateway_status() {
        let cli = Cli::try_parse_from(["rune", "gateway", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Status
            }
        ));
    }

    #[test]
    fn parse_gateway_health() {
        let cli = Cli::try_parse_from(["rune", "gateway", "health"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Health
            }
        ));
    }

    #[test]
    fn parse_channels_list() {
        let cli = Cli::try_parse_from(["rune", "channels", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Channels {
                action: ChannelsAction::List
            }
        ));
    }

    #[test]
    fn parse_channels_status() {
        let cli = Cli::try_parse_from(["rune", "channels", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Channels {
                action: ChannelsAction::Status
            }
        ));
    }

    #[test]
    fn parse_channels_capabilities() {
        let cli = Cli::try_parse_from(["rune", "channels", "capabilities"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Channels {
                action: ChannelsAction::Capabilities
            }
        ));
    }

    #[test]
    fn parse_models_list() {
        let cli = Cli::try_parse_from(["rune", "models", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Models {
                action: ModelsAction::List
            }
        ));
    }

    #[test]
    fn parse_models_status() {
        let cli = Cli::try_parse_from(["rune", "models", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Models {
                action: ModelsAction::Status
            }
        ));
    }

    #[test]
    fn parse_models_set() {
        let cli = Cli::try_parse_from([
            "rune",
            "models",
            "set",
            "hamza-eastus2/gpt-5.4",
        ])
        .unwrap();
        match cli.command {
            Command::Models {
                action: ModelsAction::Set { model },
            } => assert_eq!(model, "hamza-eastus2/gpt-5.4"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_memory_status() {
        let cli = Cli::try_parse_from(["rune", "memory", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Memory {
                action: MemoryAction::Status
            }
        ));
    }

    #[test]
    fn parse_memory_search() {
        let cli = Cli::try_parse_from([
            "rune",
            "memory",
            "search",
            "dark mode",
            "--max-results",
            "3",
        ])
        .unwrap();
        match cli.command {
            Command::Memory {
                action: MemoryAction::Search { query, max_results },
            } => {
                assert_eq!(query, "dark mode");
                assert_eq!(max_results, 3);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_memory_get() {
        let cli = Cli::try_parse_from([
            "rune",
            "memory",
            "get",
            "memory/2026-03-13.md",
            "--from",
            "10",
            "--lines",
            "5",
        ])
        .unwrap();
        match cli.command {
            Command::Memory {
                action: MemoryAction::Get { path, from, lines },
            } => {
                assert_eq!(path, "memory/2026-03-13.md");
                assert_eq!(from, 10);
                assert_eq!(lines, Some(5));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_gateway_start() {
        let cli = Cli::try_parse_from(["rune", "gateway", "start"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Start
            }
        ));
    }

    #[test]
    fn parse_gateway_stop() {
        let cli = Cli::try_parse_from(["rune", "gateway", "stop"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Stop
            }
        ));
    }

    #[test]
    fn parse_gateway_restart() {
        let cli = Cli::try_parse_from(["rune", "gateway", "restart"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Restart
            }
        ));
    }

    #[test]
    fn parse_cron_status() {
        let cli = Cli::try_parse_from(["rune", "cron", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Cron {
                action: CronAction::Status
            }
        ));
    }

    #[test]
    fn parse_cron_list_with_include_disabled() {
        let cli = Cli::try_parse_from(["rune", "cron", "list", "--include-disabled"]).unwrap();
        match cli.command {
            Command::Cron {
                action: CronAction::List { include_disabled },
            } => assert!(include_disabled),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_cron_add() {
        let cli = Cli::try_parse_from([
            "rune",
            "cron",
            "add",
            "--text",
            "hello",
            "--at",
            "2026-03-13T13:00:00Z",
        ])
        .unwrap();
        match cli.command {
            Command::Cron {
                action:
                    CronAction::Add {
                        text,
                        at,
                        session_target,
                        ..
                    },
            } => {
                assert_eq!(text, "hello");
                assert_eq!(at, "2026-03-13T13:00:00Z");
                assert_eq!(session_target, "main");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_cron_edit() {
        let cli =
            Cli::try_parse_from(["rune", "cron", "edit", "job-1", "--name", "renamed"]).unwrap();
        match cli.command {
            Command::Cron {
                action: CronAction::Edit { id, name },
            } => {
                assert_eq!(id, "job-1");
                assert_eq!(name, "renamed");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_cron_enable_disable_rm_run_runs() {
        for (subcommand, matcher) in [
            ("enable", "enable"),
            ("disable", "disable"),
            ("rm", "rm"),
            ("run", "run"),
            ("runs", "runs"),
        ] {
            let cli = Cli::try_parse_from(["rune", "cron", subcommand, "job-1"]).unwrap();
            match (matcher, cli.command) {
                (
                    "enable",
                    Command::Cron {
                        action: CronAction::Enable { id },
                    },
                )
                | (
                    "disable",
                    Command::Cron {
                        action: CronAction::Disable { id },
                    },
                )
                | (
                    "rm",
                    Command::Cron {
                        action: CronAction::Rm { id },
                    },
                )
                | (
                    "run",
                    Command::Cron {
                        action: CronAction::Run { id },
                    },
                )
                | (
                    "runs",
                    Command::Cron {
                        action: CronAction::Runs { id },
                    },
                ) => assert_eq!(id, "job-1"),
                other => panic!("unexpected parse result: {other:?}"),
            }
        }
    }

    #[test]
    fn parse_cron_wake() {
        let cli = Cli::try_parse_from([
            "rune",
            "cron",
            "wake",
            "--text",
            "Reminder: check Rune",
            "--mode",
            "now",
            "--context-messages",
            "3",
        ])
        .unwrap();
        match cli.command {
            Command::Cron {
                action:
                    CronAction::Wake {
                        text,
                        mode,
                        context_messages,
                    },
            } => {
                assert_eq!(text, "Reminder: check Rune");
                assert_eq!(mode, "now");
                assert_eq!(context_messages, Some(3));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_sessions_list() {
        let cli = Cli::try_parse_from(["rune", "sessions", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Sessions {
                action: SessionsAction::List
            }
        ));
    }

    #[test]
    fn parse_sessions_show() {
        let cli = Cli::try_parse_from(["rune", "sessions", "show", "abc-123"]).unwrap();
        match &cli.command {
            Command::Sessions {
                action: SessionsAction::Show { id },
            } => assert_eq!(id, "abc-123"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_config_show() {
        let cli = Cli::try_parse_from(["rune", "config", "show"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Config {
                action: ConfigAction::Show
            }
        ));
    }

    #[test]
    fn parse_config_validate() {
        let cli = Cli::try_parse_from(["rune", "config", "validate"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Config {
                action: ConfigAction::Validate { file: None }
            }
        ));
    }

    #[test]
    fn parse_config_validate_with_file() {
        let cli = Cli::try_parse_from(["rune", "config", "validate", "-f", "custom.toml"]).unwrap();
        match &cli.command {
            Command::Config {
                action: ConfigAction::Validate { file },
            } => assert_eq!(file.as_deref(), Some("custom.toml")),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_custom_gateway_url() {
        let cli = Cli::try_parse_from(["rune", "--gateway-url", "http://localhost:9999", "health"])
            .unwrap();
        assert_eq!(cli.gateway_url, "http://localhost:9999");
    }

    #[test]
    fn missing_subcommand_is_error() {
        assert!(Cli::try_parse_from(["rune"]).is_err());
    }
}
