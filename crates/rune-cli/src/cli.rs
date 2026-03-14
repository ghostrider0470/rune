//! Clap-based CLI definition with all subcommands.

use clap::{Parser, Subcommand, ValueEnum};

/// Rune — the operator CLI for managing the Rune AI runtime.
#[derive(Debug, Parser)]
#[command(name = "rune", version, about = "Rune AI runtime operator CLI")]
pub struct Cli {
    /// Output as JSON instead of human-readable text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Use the local development profile defaults.
    #[arg(long, global = true)]
    pub dev: bool,

    /// Named config/profile selector used for local config resolution.
    #[arg(long, global = true, env = "RUNE_PROFILE")]
    pub profile: Option<String>,

    /// Override the CLI log level for this invocation.
    #[arg(long, global = true, env = "RUNE_LOG_LEVEL")]
    pub log_level: Option<String>,

    /// Disable ANSI color/styling in CLI output.
    #[arg(long, global = true)]
    pub no_color: bool,

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
    /// Manage the daemon lifecycle using OpenClaw-style naming.
    Daemon {
        #[command(subcommand)]
        action: GatewayAction,
    },
    /// Query gateway status.
    Status,
    /// Run a health check against the gateway.
    Health,
    /// Run diagnostic checks (config, connectivity, etc.).
    Doctor,
    /// Show a compact operator dashboard summary.
    Dashboard,
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
    /// Emit system events and inspect heartbeat presence.
    System {
        #[command(subcommand)]
        action: SystemAction,
    },
    /// Manage tool approval policies (allow-always / deny).
    Approvals {
        #[command(subcommand)]
        action: ApprovalsAction,
    },
    /// Manage reminders (one-shot scheduled messages).
    Reminders {
        #[command(subcommand)]
        action: RemindersAction,
    },
    /// Generate shell completion scripts.
    Completion {
        #[command(subcommand)]
        action: CompletionAction,
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
    /// Probe RPC/API reachability and auth separately from process status.
    Probe,
    /// Discover operator-facing runtime URLs and config binding details.
    Discover,
    /// Perform a raw gateway HTTP call.
    Call {
        /// HTTP method to use.
        #[arg(long, default_value = "GET")]
        method: String,
        /// Absolute path to call (for example `/status`).
        path: String,
        /// Optional JSON body string for POST/PUT/PATCH requests.
        #[arg(long)]
        body: Option<String>,
        /// Optional bearer token override.
        #[arg(long)]
        token: Option<String>,
    },
    /// Show token-usage aggregates from persisted session turns.
    UsageCost,
    /// Start the gateway daemon.
    Start,
    /// Stop the gateway daemon.
    Stop,
    /// Restart the gateway daemon.
    Restart,
    /// Run the gateway in the foreground using the local rune-gateway binary.
    Run,
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
    /// List sessions with optional activity/channel filters.
    List {
        /// Only include sessions active within the last N minutes.
        #[arg(long = "active")]
        active_minutes: Option<u64>,
        /// Only include sessions for the given channel reference.
        #[arg(long)]
        channel: Option<String>,
        /// Maximum number of sessions to return.
        #[arg(long, default_value_t = 100)]
        limit: u64,
    },
    /// Show details for a specific session.
    Show {
        /// Session ID to inspect.
        id: String,
    },
    /// Show the first-class status card for a specific session.
    Status {
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
    /// Resolve a channel name/alias to the concrete configured adapter.
    Resolve {
        /// Channel name or alias to resolve.
        target: String,
    },
    /// Show recent local log files for channel-related runtime activity.
    Logs {
        /// Channel name filter (defaults to all known channels).
        #[arg(long)]
        channel: Option<String>,
        /// Maximum number of log files to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
pub enum ModelsAction {
    /// List configured model providers and aliases.
    List,
    /// Show resolved default-model and credential readiness status.
    Status,
    /// Show configured alias-to-provider/model mappings.
    Aliases,
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
pub enum SystemAction {
    /// Queue a system event/wake for the runtime.
    Event {
        /// Text to inject.
        #[arg(long)]
        text: String,
        /// Delivery timing mode (`next-heartbeat` or `now`).
        #[arg(long, default_value = "next-heartbeat")]
        mode: String,
        /// Optional number of recent context messages to attach.
        #[arg(long = "context-messages")]
        context_messages: Option<u64>,
    },
    /// Show whether HEARTBEAT.md exists and when it last changed.
    Heartbeat {
        #[command(subcommand)]
        action: SystemHeartbeatAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum SystemHeartbeatAction {
    /// Show HEARTBEAT.md presence and metadata.
    Presence,
    /// Show HEARTBEAT.md last-modified metadata.
    Last,
    /// Enable the heartbeat runner.
    Enable,
    /// Disable the heartbeat runner.
    Disable,
    /// Show heartbeat runner status (enabled, interval, counters).
    Status,
}

#[derive(Debug, Subcommand)]
pub enum RemindersAction {
    /// Add a one-shot reminder.
    Add {
        /// Reminder message text.
        message: String,
        /// Duration from now (e.g. "30m", "2h", "1d").
        #[arg(long = "in")]
        duration: String,
        /// Target session or channel (defaults to "main").
        #[arg(long, default_value = "main")]
        target: String,
    },
    /// List pending reminders.
    List {
        /// Include delivered reminders.
        #[arg(long)]
        include_delivered: bool,
    },
    /// Cancel a reminder by ID.
    Cancel {
        /// Reminder ID to cancel.
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum CompletionAction {
    /// Print a shell completion script to stdout.
    Generate {
        /// Shell to generate completion for.
        #[arg(value_enum)]
        shell: CompletionShell,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}

#[derive(Debug, Subcommand)]
pub enum ApprovalsAction {
    /// List pending durable approval requests.
    List,
    /// Submit a decision for a pending approval request.
    Decide {
        /// Approval request ID.
        id: String,
        /// Decision: allow-once, allow-always, or deny.
        decision: String,
        /// Actor identity recorded as the approver.
        #[arg(long)]
        by: Option<String>,
    },
    /// List all tool approval policies.
    Policies,
    /// Get the approval policy for a specific tool.
    Get {
        /// Tool name to query.
        tool: String,
    },
    /// Set the approval policy for a tool.
    Set {
        /// Tool name.
        tool: String,
        /// Decision: allow-always or deny.
        decision: String,
    },
    /// Clear (remove) the approval policy for a tool.
    Clear {
        /// Tool name.
        tool: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Dump the resolved configuration.
    Show,
    /// Show the config file path that will be used for local mutations.
    File,
    /// Read a specific config key from the local TOML file.
    Get {
        /// Dot-separated config key path.
        key: String,
    },
    /// Set a specific config key in the local TOML file.
    Set {
        /// Dot-separated config key path.
        key: String,
        /// TOML value to write (for example `true`, `8787`, `"text"`, `["a"]`).
        value: String,
    },
    /// Remove a specific config key from the local TOML file.
    Unset {
        /// Dot-separated config key path.
        key: String,
    },
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

    use super::CompletionShell;

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
    fn parse_dashboard() {
        let cli = Cli::try_parse_from(["rune", "dashboard"]).unwrap();
        assert!(matches!(cli.command, Command::Dashboard));
    }

    #[test]
    fn parse_json_flag() {
        let cli = Cli::try_parse_from(["rune", "--json", "status"]).unwrap();
        assert!(cli.json);
    }

    #[test]
    fn parse_global_operator_flags() {
        let cli = Cli::try_parse_from([
            "rune",
            "--dev",
            "--profile",
            "azure",
            "--log-level",
            "debug",
            "--no-color",
            "status",
        ])
        .unwrap();
        assert!(cli.dev);
        assert_eq!(cli.profile.as_deref(), Some("azure"));
        assert_eq!(cli.log_level.as_deref(), Some("debug"));
        assert!(cli.no_color);
        assert!(matches!(cli.command, Command::Status));
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
    fn parse_gateway_probe() {
        let cli = Cli::try_parse_from(["rune", "gateway", "probe"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Probe
            }
        ));
    }

    #[test]
    fn parse_gateway_discover() {
        let cli = Cli::try_parse_from(["rune", "gateway", "discover"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Discover
            }
        ));
    }

    #[test]
    fn parse_gateway_call() {
        let cli = Cli::try_parse_from([
            "rune",
            "gateway",
            "call",
            "--method",
            "POST",
            "--body",
            "{\"ping\":true}",
            "--token",
            "secret",
            "/cron/wake",
        ])
        .unwrap();
        match cli.command {
            Command::Gateway {
                action:
                    GatewayAction::Call {
                        method,
                        path,
                        body,
                        token,
                    },
            } => {
                assert_eq!(method, "POST");
                assert_eq!(path, "/cron/wake");
                assert_eq!(body.as_deref(), Some("{\"ping\":true}"));
                assert_eq!(token.as_deref(), Some("secret"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_gateway_usage_cost() {
        let cli = Cli::try_parse_from(["rune", "gateway", "usage-cost"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::UsageCost
            }
        ));
    }

    #[test]
    fn parse_daemon_status() {
        let cli = Cli::try_parse_from(["rune", "daemon", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Daemon {
                action: GatewayAction::Status
            }
        ));
    }

    #[test]
    fn parse_daemon_restart() {
        let cli = Cli::try_parse_from(["rune", "daemon", "restart"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Daemon {
                action: GatewayAction::Restart
            }
        ));
    }

    #[test]
    fn parse_daemon_run() {
        let cli = Cli::try_parse_from(["rune", "daemon", "run"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Daemon {
                action: GatewayAction::Run
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
    fn parse_channels_resolve() {
        let cli = Cli::try_parse_from(["rune", "channels", "resolve", "telegram"]).unwrap();
        match cli.command {
            Command::Channels {
                action: ChannelsAction::Resolve { target },
            } => assert_eq!(target, "telegram"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_channels_logs() {
        let cli = Cli::try_parse_from([
            "rune",
            "channels",
            "logs",
            "--channel",
            "telegram",
            "--limit",
            "5",
        ])
        .unwrap();
        match cli.command {
            Command::Channels {
                action: ChannelsAction::Logs { channel, limit },
            } => {
                assert_eq!(channel.as_deref(), Some("telegram"));
                assert_eq!(limit, 5);
            }
            other => panic!("unexpected command: {other:?}"),
        }
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
    fn parse_models_aliases() {
        let cli = Cli::try_parse_from(["rune", "models", "aliases"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Models {
                action: ModelsAction::Aliases
            }
        ));
    }

    #[test]
    fn parse_models_set() {
        let cli = Cli::try_parse_from(["rune", "models", "set", "hamza-eastus2/gpt-5.4"]).unwrap();
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
    fn parse_system_event() {
        let cli = Cli::try_parse_from([
            "rune",
            "system",
            "event",
            "--text",
            "Reminder: check Rune",
            "--mode",
            "now",
            "--context-messages",
            "2",
        ])
        .unwrap();
        match cli.command {
            Command::System {
                action:
                    SystemAction::Event {
                        text,
                        mode,
                        context_messages,
                    },
            } => {
                assert_eq!(text, "Reminder: check Rune");
                assert_eq!(mode, "now");
                assert_eq!(context_messages, Some(2));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_system_heartbeat_presence() {
        let cli = Cli::try_parse_from(["rune", "system", "heartbeat", "presence"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::System {
                action: SystemAction::Heartbeat {
                    action: SystemHeartbeatAction::Presence
                }
            }
        ));
    }

    #[test]
    fn parse_system_heartbeat_last() {
        let cli = Cli::try_parse_from(["rune", "system", "heartbeat", "last"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::System {
                action: SystemAction::Heartbeat {
                    action: SystemHeartbeatAction::Last
                }
            }
        ));
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
    fn parse_gateway_run() {
        let cli = Cli::try_parse_from(["rune", "gateway", "run"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Run
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
                action: SessionsAction::List {
                    active_minutes: None,
                    channel: None,
                    limit: 100
                }
            }
        ));
    }

    #[test]
    fn parse_sessions_list_with_filters() {
        let cli = Cli::try_parse_from([
            "rune",
            "sessions",
            "list",
            "--active",
            "30",
            "--channel",
            "telegram",
            "--limit",
            "25",
        ])
        .unwrap();
        match cli.command {
            Command::Sessions {
                action:
                    SessionsAction::List {
                        active_minutes,
                        channel,
                        limit,
                    },
            } => {
                assert_eq!(active_minutes, Some(30));
                assert_eq!(channel.as_deref(), Some("telegram"));
                assert_eq!(limit, 25);
            }
            other => panic!("unexpected command: {other:?}"),
        }
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
    fn parse_sessions_status() {
        let cli = Cli::try_parse_from(["rune", "sessions", "status", "abc-123"]).unwrap();
        match &cli.command {
            Command::Sessions {
                action: SessionsAction::Status { id },
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
    fn parse_config_file() {
        let cli = Cli::try_parse_from(["rune", "config", "file"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Config {
                action: ConfigAction::File
            }
        ));
    }

    #[test]
    fn parse_config_get() {
        let cli = Cli::try_parse_from(["rune", "config", "get", "gateway.port"]).unwrap();
        match &cli.command {
            Command::Config {
                action: ConfigAction::Get { key },
            } => assert_eq!(key, "gateway.port"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_config_set() {
        let cli = Cli::try_parse_from(["rune", "config", "set", "gateway.port", "9090"]).unwrap();
        match &cli.command {
            Command::Config {
                action: ConfigAction::Set { key, value },
            } => {
                assert_eq!(key, "gateway.port");
                assert_eq!(value, "9090");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_config_unset() {
        let cli = Cli::try_parse_from(["rune", "config", "unset", "gateway.auth_token"]).unwrap();
        match &cli.command {
            Command::Config {
                action: ConfigAction::Unset { key },
            } => assert_eq!(key, "gateway.auth_token"),
            other => panic!("unexpected command: {other:?}"),
        }
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
    fn parse_completion_generate() {
        let cli = Cli::try_parse_from(["rune", "completion", "generate", "bash"]).unwrap();
        match cli.command {
            Command::Completion {
                action: CompletionAction::Generate { shell },
            } => {
                assert_eq!(shell, CompletionShell::Bash);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_approvals_list() {
        let cli = Cli::try_parse_from(["rune", "approvals", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Approvals {
                action: ApprovalsAction::List
            }
        ));
    }

    #[test]
    fn parse_approvals_decide() {
        let cli = Cli::try_parse_from([
            "rune",
            "approvals",
            "decide",
            "123e4567-e89b-12d3-a456-426614174000",
            "allow-once",
            "--by",
            "hamza",
        ])
        .unwrap();
        match cli.command {
            Command::Approvals {
                action: ApprovalsAction::Decide { id, decision, by },
            } => {
                assert_eq!(id, "123e4567-e89b-12d3-a456-426614174000");
                assert_eq!(decision, "allow-once");
                assert_eq!(by.as_deref(), Some("hamza"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_approvals_policies() {
        let cli = Cli::try_parse_from(["rune", "approvals", "policies"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Approvals {
                action: ApprovalsAction::Policies
            }
        ));
    }

    #[test]
    fn parse_approvals_get() {
        let cli = Cli::try_parse_from(["rune", "approvals", "get", "exec"]).unwrap();
        match cli.command {
            Command::Approvals {
                action: ApprovalsAction::Get { tool },
            } => assert_eq!(tool, "exec"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_approvals_set() {
        let cli =
            Cli::try_parse_from(["rune", "approvals", "set", "exec", "allow-always"]).unwrap();
        match cli.command {
            Command::Approvals {
                action: ApprovalsAction::Set { tool, decision },
            } => {
                assert_eq!(tool, "exec");
                assert_eq!(decision, "allow-always");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_approvals_clear() {
        let cli = Cli::try_parse_from(["rune", "approvals", "clear", "exec"]).unwrap();
        match cli.command {
            Command::Approvals {
                action: ApprovalsAction::Clear { tool },
            } => assert_eq!(tool, "exec"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_system_heartbeat_enable() {
        let cli = Cli::try_parse_from(["rune", "system", "heartbeat", "enable"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::System {
                action: SystemAction::Heartbeat {
                    action: SystemHeartbeatAction::Enable
                }
            }
        ));
    }

    #[test]
    fn parse_system_heartbeat_disable() {
        let cli = Cli::try_parse_from(["rune", "system", "heartbeat", "disable"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::System {
                action: SystemAction::Heartbeat {
                    action: SystemHeartbeatAction::Disable
                }
            }
        ));
    }

    #[test]
    fn parse_system_heartbeat_status() {
        let cli = Cli::try_parse_from(["rune", "system", "heartbeat", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::System {
                action: SystemAction::Heartbeat {
                    action: SystemHeartbeatAction::Status
                }
            }
        ));
    }

    #[test]
    fn parse_reminders_add() {
        let cli =
            Cli::try_parse_from(["rune", "reminders", "add", "Buy milk", "--in", "30m"]).unwrap();
        match cli.command {
            Command::Reminders {
                action:
                    RemindersAction::Add {
                        message,
                        duration,
                        target,
                    },
            } => {
                assert_eq!(message, "Buy milk");
                assert_eq!(duration, "30m");
                assert_eq!(target, "main");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_reminders_add_with_target() {
        let cli = Cli::try_parse_from([
            "rune",
            "reminders",
            "add",
            "Check PR",
            "--in",
            "2h",
            "--target",
            "discord",
        ])
        .unwrap();
        match cli.command {
            Command::Reminders {
                action:
                    RemindersAction::Add {
                        message,
                        duration,
                        target,
                    },
            } => {
                assert_eq!(message, "Check PR");
                assert_eq!(duration, "2h");
                assert_eq!(target, "discord");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_reminders_list() {
        let cli = Cli::try_parse_from(["rune", "reminders", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Reminders {
                action: RemindersAction::List {
                    include_delivered: false
                }
            }
        ));
    }

    #[test]
    fn parse_reminders_list_include_delivered() {
        let cli =
            Cli::try_parse_from(["rune", "reminders", "list", "--include-delivered"]).unwrap();
        match cli.command {
            Command::Reminders {
                action: RemindersAction::List { include_delivered },
            } => assert!(include_delivered),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_reminders_cancel() {
        let cli = Cli::try_parse_from(["rune", "reminders", "cancel", "abc-123"]).unwrap();
        match cli.command {
            Command::Reminders {
                action: RemindersAction::Cancel { id },
            } => assert_eq!(id, "abc-123"),
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
