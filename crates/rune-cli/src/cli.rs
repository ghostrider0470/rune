//! Clap-based CLI definition with all subcommands.

use clap::{ArgGroup, Parser, Subcommand, ValueEnum};

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

    /// Auto-approve all tool calls (sets approval.mode=yolo for this invocation).
    ///
    /// Intended for trusted local dev environments where interactive approval
    /// prompts are unwanted.  Equivalent to `RUNE_APPROVAL__MODE=yolo`.
    #[arg(long, global = true)]
    pub yolo: bool,

    /// Disable filesystem sandbox / workspace boundary enforcement.
    ///
    /// Intended for trusted environments where the agent needs unrestricted
    /// filesystem access.  Equivalent to `RUNE_SECURITY__SANDBOX=false`.
    #[arg(long, global = true)]
    pub no_sandbox: bool,

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
    /// Send and manage messages across channel adapters.
    Message {
        #[command(subcommand)]
        action: MessageAction,
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
        /// Delivery mode for the scheduled work (`none`, `announce`, or `webhook`).
        #[arg(long, value_enum, default_value_t = CronDeliveryMode::None)]
        delivery_mode: CronDeliveryMode,
        /// Webhook URL for `webhook` delivery mode.
        #[arg(long)]
        webhook_url: Option<String>,
    },
    /// Inspect a job.
    Show {
        /// Job ID.
        id: String,
    },
    /// Update a job's display name, delivery mode, or webhook URL.
    #[command(group(
        ArgGroup::new("cron_edit_changes")
            .required(true)
            .multiple(true)
            .args(["name", "delivery_mode", "webhook_url"])
    ))]
    Edit {
        /// Job ID.
        id: String,
        /// New name.
        #[arg(long)]
        name: Option<String>,
        /// New delivery mode.
        #[arg(long, value_enum)]
        delivery_mode: Option<CronDeliveryMode>,
        /// New webhook URL for `webhook` delivery mode.
        #[arg(long)]
        webhook_url: Option<String>,
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
        #[arg(long, value_enum, default_value_t = WakeMode::NextHeartbeat)]
        mode: WakeMode,
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
    /// Show provider auth/API-key configuration status and management hints.
    Auth,
    /// Set the default model in local config.toml after validating against configured inventory.
    Set {
        /// Model id to set. Accepts canonical `provider/model` ids and unambiguous short names.
        model: String,
    },
    /// Set the default image model in local config.toml after validating against configured inventory.
    SetImage {
        /// Image model id to set. Accepts canonical `provider/model` ids and unambiguous short names.
        model: String,
    },
    /// List configured text fallback chains.
    Fallbacks,
    /// List configured image fallback chains.
    ImageFallbacks,
    /// Scan locally reachable providers (for example Ollama) for available models.
    Scan,
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
    /// Inject, schedule, and list system events.
    Event {
        #[command(subcommand)]
        action: SystemEventAction,
    },
    /// Show whether HEARTBEAT.md exists and when it last changed.
    Heartbeat {
        #[command(subcommand)]
        action: SystemHeartbeatAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum SystemEventAction {
    /// Inject an immediate system event/wake into the runtime.
    Inject {
        /// Text to inject.
        #[arg(long)]
        text: String,
        /// Delivery timing mode (`next-heartbeat` or `now`).
        #[arg(long, value_enum, default_value_t = WakeMode::NextHeartbeat)]
        mode: WakeMode,
        /// Optional number of recent context messages to attach.
        #[arg(long = "context-messages")]
        context_messages: Option<u64>,
    },
    /// Schedule a future system event as a cron job.
    Schedule {
        /// Text for the system event payload.
        #[arg(long)]
        text: String,
        /// Fire time as RFC3339/ISO-8601.
        #[arg(long)]
        at: String,
        /// Human-readable name for the job.
        #[arg(long)]
        name: Option<String>,
        /// Session target (`main` or `isolated`). Defaults to `main`.
        #[arg(long, default_value = "main")]
        session_target: String,
        /// Delivery mode for the scheduled work (`none`, `announce`, or `webhook`).
        #[arg(long, value_enum, default_value_t = CronDeliveryMode::None)]
        delivery_mode: CronDeliveryMode,
        /// Webhook URL for `webhook` delivery mode.
        #[arg(long)]
        webhook_url: Option<String>,
    },
    /// List system-event cron jobs.
    List {
        /// Include disabled jobs in the listing.
        #[arg(long)]
        include_disabled: bool,
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
pub enum MessageAction {
    /// Send a message through a channel adapter.
    Send {
        /// Target channel adapter (e.g. "telegram", "discord", "slack").
        #[arg(long)]
        channel: String,
        /// Message body text.
        #[arg(long)]
        text: String,
        /// Optional session ID to associate the message with.
        #[arg(long)]
        session: Option<String>,
        /// Optional thread/reply-to ID for threaded messaging.
        #[arg(long)]
        thread: Option<String>,
    },
    /// Search message history across channels.
    Search {
        /// Query text to search for in message content.
        query: String,
        /// Restrict results to a specific channel adapter.
        #[arg(long)]
        channel: Option<String>,
        /// Restrict results to a specific session ID.
        #[arg(long)]
        session: Option<String>,
        /// Maximum number of results to return.
        #[arg(long, default_value_t = 25)]
        limit: u64,
    },
    /// Broadcast a message to multiple channel adapters simultaneously.
    Broadcast {
        /// Message body text.
        #[arg(long)]
        text: String,
        /// Comma-separated list of target channels (default: all enabled channels).
        #[arg(long, value_delimiter = ',')]
        channels: Vec<String>,
        /// Optional session ID to associate the broadcast with.
        #[arg(long)]
        session: Option<String>,
    },
    /// Read/fetch a single message by ID from a channel adapter.
    Read {
        /// ID of the message to read.
        #[arg(long)]
        message_id: String,
        /// Channel adapter the message belongs to.
        #[arg(long)]
        channel: String,
        /// Session ID the message belongs to.
        #[arg(long)]
        session: Option<String>,
    },
    /// Delete a message by ID from a channel adapter.
    Delete {
        /// ID of the message to delete.
        #[arg(long)]
        message_id: String,
        /// Channel adapter the message belongs to.
        #[arg(long)]
        channel: String,
        /// Session ID the message belongs to.
        #[arg(long)]
        session: Option<String>,
    },
    /// Add or remove an emoji reaction on a message.
    React {
        /// ID of the message to react to.
        #[arg(long)]
        message_id: String,
        /// Emoji to add (e.g. "👍", ":thumbsup:", "heart").
        #[arg(long)]
        emoji: String,
        /// Remove the reaction instead of adding it.
        #[arg(long)]
        remove: bool,
        /// Channel adapter the message belongs to.
        #[arg(long)]
        channel: Option<String>,
        /// Session ID the message belongs to.
        #[arg(long)]
        session: Option<String>,
    },
    /// Edit an existing message's text content.
    Edit {
        /// ID of the message to edit.
        #[arg(long)]
        message_id: String,
        /// Channel adapter the message belongs to.
        #[arg(long)]
        channel: String,
        /// New message body text.
        #[arg(long)]
        text: String,
        /// Session ID the message belongs to.
        #[arg(long)]
        session: Option<String>,
    },
    /// Pin or unpin a message in a channel.
    Pin {
        /// ID of the message to pin or unpin.
        #[arg(long)]
        message_id: String,
        /// Unpin the message instead of pinning it.
        #[arg(long)]
        unpin: bool,
        /// Channel adapter the message belongs to.
        #[arg(long)]
        channel: Option<String>,
        /// Session ID the message belongs to.
        #[arg(long)]
        session: Option<String>,
    },
    /// List or reply to message threads.
    Thread {
        #[command(subcommand)]
        action: MessageThreadAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum MessageThreadAction {
    /// List messages within a thread.
    List {
        /// Thread ID to list messages from.
        #[arg(long)]
        thread_id: String,
        /// Channel adapter the thread belongs to.
        #[arg(long)]
        channel: Option<String>,
        /// Session ID the thread belongs to.
        #[arg(long)]
        session: Option<String>,
        /// Maximum number of messages to return.
        #[arg(long, default_value_t = 50)]
        limit: u64,
    },
    /// Reply to an existing thread.
    Reply {
        /// Thread ID to reply to.
        #[arg(long)]
        thread_id: String,
        /// Channel adapter the thread belongs to.
        #[arg(long)]
        channel: String,
        /// Reply message text.
        #[arg(long)]
        text: String,
        /// Session ID the thread belongs to.
        #[arg(long)]
        session: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CronDeliveryMode {
    None,
    Announce,
    Webhook,
}

impl CronDeliveryMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Announce => "announce",
            Self::Webhook => "webhook",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WakeMode {
    NextHeartbeat,
    Now,
}

impl WakeMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NextHeartbeat => "next-heartbeat",
            Self::Now => "now",
        }
    }
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
    fn parse_models_auth() {
        let cli = Cli::try_parse_from(["rune", "models", "auth"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Models {
                action: ModelsAction::Auth
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
    fn parse_models_set_image() {
        let cli =
            Cli::try_parse_from(["rune", "models", "set-image", "hamza-eastus2/dall-e-3"]).unwrap();
        match cli.command {
            Command::Models {
                action: ModelsAction::SetImage { model },
            } => assert_eq!(model, "hamza-eastus2/dall-e-3"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_models_fallbacks() {
        let cli = Cli::try_parse_from(["rune", "models", "fallbacks"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Models {
                action: ModelsAction::Fallbacks
            }
        ));
    }

    #[test]
    fn parse_models_image_fallbacks() {
        let cli = Cli::try_parse_from(["rune", "models", "image-fallbacks"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Models {
                action: ModelsAction::ImageFallbacks
            }
        ));
    }

    #[test]
    fn parse_models_scan() {
        let cli = Cli::try_parse_from(["rune", "models", "scan"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Models {
                action: ModelsAction::Scan
            }
        ));
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
    fn parse_system_event_inject() {
        let cli = Cli::try_parse_from([
            "rune",
            "system",
            "event",
            "inject",
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
                        action:
                            SystemEventAction::Inject {
                                text,
                                mode,
                                context_messages,
                            },
                    },
            } => {
                assert_eq!(text, "Reminder: check Rune");
                assert_eq!(mode, WakeMode::Now);
                assert_eq!(context_messages, Some(2));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_system_event_inject_defaults() {
        let cli = Cli::try_parse_from([
            "rune",
            "system",
            "event",
            "inject",
            "--text",
            "ping",
        ])
        .unwrap();
        match cli.command {
            Command::System {
                action:
                    SystemAction::Event {
                        action:
                            SystemEventAction::Inject {
                                text,
                                mode,
                                context_messages,
                            },
                    },
            } => {
                assert_eq!(text, "ping");
                assert_eq!(mode, WakeMode::NextHeartbeat);
                assert_eq!(context_messages, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_system_event_schedule() {
        let cli = Cli::try_parse_from([
            "rune",
            "system",
            "event",
            "schedule",
            "--text",
            "deploy check",
            "--at",
            "2026-04-01T09:00:00Z",
            "--name",
            "morning-check",
            "--session-target",
            "isolated",
            "--delivery-mode",
            "announce",
        ])
        .unwrap();
        match cli.command {
            Command::System {
                action:
                    SystemAction::Event {
                        action:
                            SystemEventAction::Schedule {
                                text,
                                at,
                                name,
                                session_target,
                                delivery_mode,
                                webhook_url,
                            },
                    },
            } => {
                assert_eq!(text, "deploy check");
                assert_eq!(at, "2026-04-01T09:00:00Z");
                assert_eq!(name.as_deref(), Some("morning-check"));
                assert_eq!(session_target, "isolated");
                assert_eq!(delivery_mode, CronDeliveryMode::Announce);
                assert!(webhook_url.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_system_event_schedule_defaults() {
        let cli = Cli::try_parse_from([
            "rune",
            "system",
            "event",
            "schedule",
            "--text",
            "hello",
            "--at",
            "2026-04-01T09:00:00Z",
        ])
        .unwrap();
        match cli.command {
            Command::System {
                action:
                    SystemAction::Event {
                        action:
                            SystemEventAction::Schedule {
                                session_target,
                                delivery_mode,
                                name,
                                ..
                            },
                    },
            } => {
                assert_eq!(session_target, "main");
                assert_eq!(delivery_mode, CronDeliveryMode::None);
                assert!(name.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_system_event_list() {
        let cli = Cli::try_parse_from(["rune", "system", "event", "list"]).unwrap();
        match cli.command {
            Command::System {
                action:
                    SystemAction::Event {
                        action: SystemEventAction::List { include_disabled },
                    },
            } => {
                assert!(!include_disabled);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_system_event_list_include_disabled() {
        let cli = Cli::try_parse_from([
            "rune",
            "system",
            "event",
            "list",
            "--include-disabled",
        ])
        .unwrap();
        match cli.command {
            Command::System {
                action:
                    SystemAction::Event {
                        action: SystemEventAction::List { include_disabled },
                    },
            } => {
                assert!(include_disabled);
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
            "--delivery-mode",
            "announce",
        ])
        .unwrap();
        match cli.command {
            Command::Cron {
                action:
                    CronAction::Add {
                        text,
                        at,
                        session_target,
                        delivery_mode,
                        ..
                    },
            } => {
                assert_eq!(text, "hello");
                assert_eq!(at, "2026-03-13T13:00:00Z");
                assert_eq!(session_target, "main");
                assert_eq!(delivery_mode, CronDeliveryMode::Announce);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_cron_show() {
        let cli = Cli::try_parse_from(["rune", "cron", "show", "job-1"]).unwrap();
        match cli.command {
            Command::Cron {
                action: CronAction::Show { id },
            } => assert_eq!(id, "job-1"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_cron_edit() {
        let cli = Cli::try_parse_from([
            "rune",
            "cron",
            "edit",
            "job-1",
            "--name",
            "renamed",
            "--delivery-mode",
            "webhook",
        ])
        .unwrap();
        match cli.command {
            Command::Cron {
                action:
                    CronAction::Edit {
                        id,
                        name,
                        delivery_mode,
                        ..
                    },
            } => {
                assert_eq!(id, "job-1");
                assert_eq!(name.as_deref(), Some("renamed"));
                assert_eq!(delivery_mode, Some(CronDeliveryMode::Webhook));
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
                assert_eq!(mode, WakeMode::Now);
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
    fn parse_completion_generate_bash() {
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
    fn parse_completion_generate_zsh() {
        let cli = Cli::try_parse_from(["rune", "completion", "generate", "zsh"]).unwrap();
        match cli.command {
            Command::Completion {
                action: CompletionAction::Generate { shell },
            } => assert_eq!(shell, CompletionShell::Zsh),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_completion_generate_fish() {
        let cli = Cli::try_parse_from(["rune", "completion", "generate", "fish"]).unwrap();
        match cli.command {
            Command::Completion {
                action: CompletionAction::Generate { shell },
            } => assert_eq!(shell, CompletionShell::Fish),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_completion_generate_all_shells() {
        for shell_name in ["bash", "zsh", "fish", "elvish", "power-shell"] {
            let cli =
                Cli::try_parse_from(["rune", "completion", "generate", shell_name]).unwrap();
            assert!(
                matches!(
                    cli.command,
                    Command::Completion {
                        action: CompletionAction::Generate { .. }
                    }
                ),
                "failed to parse completion generate for {shell_name}",
            );
        }
    }

    #[test]
    fn completion_generate_rejects_unknown_shell() {
        let result = Cli::try_parse_from(["rune", "completion", "generate", "nushell"]);
        assert!(result.is_err());
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

    // ── Trusted-environment bypass flags (#64) ───────────────────────

    #[test]
    fn parse_yolo_flag() {
        let cli = Cli::try_parse_from(["rune", "--yolo", "status"]).unwrap();
        assert!(cli.yolo);
        assert!(!cli.no_sandbox);
    }

    #[test]
    fn parse_no_sandbox_flag() {
        let cli = Cli::try_parse_from(["rune", "--no-sandbox", "doctor"]).unwrap();
        assert!(cli.no_sandbox);
        assert!(!cli.yolo);
    }

    #[test]
    fn parse_yolo_and_no_sandbox_combined() {
        let cli = Cli::try_parse_from(["rune", "--yolo", "--no-sandbox", "status"]).unwrap();
        assert!(cli.yolo);
        assert!(cli.no_sandbox);
    }

    #[test]
    fn bypass_flags_default_to_false() {
        let cli = Cli::try_parse_from(["rune", "status"]).unwrap();
        assert!(!cli.yolo);
        assert!(!cli.no_sandbox);
    }

    #[test]
    fn yolo_flag_works_after_subcommand() {
        // clap global flags can appear before or after the subcommand.
        let cli = Cli::try_parse_from(["rune", "gateway", "status", "--yolo"]).unwrap();
        assert!(cli.yolo);
    }

    // ── Message family (#74) ─────────────────────────────────────────

    #[test]
    fn parse_message_send() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "send",
            "--channel",
            "telegram",
            "--text",
            "Hello from Rune",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Send {
                        channel,
                        text,
                        session,
                        thread,
                    },
            } => {
                assert_eq!(channel, "telegram");
                assert_eq!(text, "Hello from Rune");
                assert!(session.is_none());
                assert!(thread.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_send_with_session_and_thread() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "send",
            "--channel",
            "discord",
            "--text",
            "threaded reply",
            "--session",
            "sess-42",
            "--thread",
            "thread-99",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Send {
                        channel,
                        text,
                        session,
                        thread,
                    },
            } => {
                assert_eq!(channel, "discord");
                assert_eq!(text, "threaded reply");
                assert_eq!(session.as_deref(), Some("sess-42"));
                assert_eq!(thread.as_deref(), Some("thread-99"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_send_requires_channel_and_text() {
        // Missing --text
        assert!(
            Cli::try_parse_from(["rune", "message", "send", "--channel", "telegram"]).is_err()
        );
        // Missing --channel
        assert!(
            Cli::try_parse_from(["rune", "message", "send", "--text", "hello"]).is_err()
        );
        // Missing both
        assert!(Cli::try_parse_from(["rune", "message", "send"]).is_err());
    }

    #[test]
    fn parse_message_search() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "search",
            "deploy failed",
            "--channel",
            "telegram",
            "--session",
            "sess-1",
            "--limit",
            "10",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Search {
                        query,
                        channel,
                        session,
                        limit,
                    },
            } => {
                assert_eq!(query, "deploy failed");
                assert_eq!(channel.as_deref(), Some("telegram"));
                assert_eq!(session.as_deref(), Some("sess-1"));
                assert_eq!(limit, 10);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_search_defaults() {
        let cli =
            Cli::try_parse_from(["rune", "message", "search", "hello world"]).unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Search {
                        query,
                        channel,
                        session,
                        limit,
                    },
            } => {
                assert_eq!(query, "hello world");
                assert!(channel.is_none());
                assert!(session.is_none());
                assert_eq!(limit, 25);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_search_requires_query() {
        assert!(Cli::try_parse_from(["rune", "message", "search"]).is_err());
    }

    #[test]
    fn parse_message_broadcast() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "broadcast",
            "--text",
            "System maintenance in 10 minutes",
            "--channels",
            "telegram,discord,slack",
            "--session",
            "sess-99",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Broadcast {
                        text,
                        channels,
                        session,
                    },
            } => {
                assert_eq!(text, "System maintenance in 10 minutes");
                assert_eq!(channels, vec!["telegram", "discord", "slack"]);
                assert_eq!(session.as_deref(), Some("sess-99"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_broadcast_no_channels() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "broadcast",
            "--text",
            "Hello everyone",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Broadcast {
                        text,
                        channels,
                        session,
                    },
            } => {
                assert_eq!(text, "Hello everyone");
                assert!(channels.is_empty());
                assert!(session.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_broadcast_requires_text() {
        assert!(Cli::try_parse_from(["rune", "message", "broadcast"]).is_err());
    }

    #[test]
    fn parse_message_broadcast_single_channel() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "broadcast",
            "--text",
            "alert",
            "--channels",
            "telegram",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Broadcast {
                        channels, ..
                    },
            } => {
                assert_eq!(channels, vec!["telegram"]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_react() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "react",
            "--message-id",
            "msg-42",
            "--emoji",
            "👍",
            "--channel",
            "telegram",
            "--session",
            "sess-7",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::React {
                        message_id,
                        emoji,
                        remove,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-42");
                assert_eq!(emoji, "👍");
                assert!(!remove);
                assert_eq!(channel.as_deref(), Some("telegram"));
                assert_eq!(session.as_deref(), Some("sess-7"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_react_remove() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "react",
            "--message-id",
            "msg-99",
            "--emoji",
            "heart",
            "--remove",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::React {
                        message_id,
                        emoji,
                        remove,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-99");
                assert_eq!(emoji, "heart");
                assert!(remove);
                assert!(channel.is_none());
                assert!(session.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_edit() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "edit",
            "--message-id",
            "msg-42",
            "--channel",
            "telegram",
            "--text",
            "Updated text",
            "--session",
            "sess-7",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Edit {
                        message_id,
                        channel,
                        text,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-42");
                assert_eq!(channel, "telegram");
                assert_eq!(text, "Updated text");
                assert_eq!(session.as_deref(), Some("sess-7"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_edit_without_session() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "edit",
            "--message-id",
            "msg-99",
            "--channel",
            "discord",
            "--text",
            "Fixed typo",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Edit {
                        message_id,
                        channel,
                        text,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-99");
                assert_eq!(channel, "discord");
                assert_eq!(text, "Fixed typo");
                assert!(session.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_edit_requires_message_id_channel_and_text() {
        // Missing --text
        assert!(Cli::try_parse_from([
            "rune",
            "message",
            "edit",
            "--message-id",
            "msg-1",
            "--channel",
            "telegram",
        ])
        .is_err());
        // Missing --channel
        assert!(Cli::try_parse_from([
            "rune",
            "message",
            "edit",
            "--message-id",
            "msg-1",
            "--text",
            "hello",
        ])
        .is_err());
        // Missing --message-id
        assert!(Cli::try_parse_from([
            "rune",
            "message",
            "edit",
            "--channel",
            "telegram",
            "--text",
            "hello",
        ])
        .is_err());
        // Missing all
        assert!(Cli::try_parse_from(["rune", "message", "edit"]).is_err());
    }

    #[test]
    fn parse_message_pin() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "pin",
            "--message-id",
            "msg-50",
            "--channel",
            "telegram",
            "--session",
            "sess-3",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Pin {
                        message_id,
                        unpin,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-50");
                assert!(!unpin);
                assert_eq!(channel.as_deref(), Some("telegram"));
                assert_eq!(session.as_deref(), Some("sess-3"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_unpin() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "pin",
            "--message-id",
            "msg-77",
            "--unpin",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Pin {
                        message_id,
                        unpin,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-77");
                assert!(unpin);
                assert!(channel.is_none());
                assert!(session.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_pin_requires_message_id() {
        assert!(Cli::try_parse_from(["rune", "message", "pin"]).is_err());
        assert!(Cli::try_parse_from(["rune", "message", "pin", "--unpin"]).is_err());
    }

    #[test]
    fn message_react_requires_message_id_and_emoji() {
        assert!(Cli::try_parse_from(["rune", "message", "react"]).is_err());
        assert!(Cli::try_parse_from([
            "rune",
            "message",
            "react",
            "--message-id",
            "msg-1",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "rune",
            "message",
            "react",
            "--emoji",
            "👍",
        ])
        .is_err());
    }

    #[test]
    fn parse_message_delete() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "delete",
            "--message-id",
            "msg-42",
            "--channel",
            "telegram",
            "--session",
            "sess-7",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Delete {
                        message_id,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-42");
                assert_eq!(channel, "telegram");
                assert_eq!(session.as_deref(), Some("sess-7"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_delete_without_session() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "delete",
            "--message-id",
            "msg-99",
            "--channel",
            "discord",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Delete {
                        message_id,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-99");
                assert_eq!(channel, "discord");
                assert!(session.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_delete_requires_message_id_and_channel() {
        assert!(Cli::try_parse_from(["rune", "message", "delete"]).is_err());
        assert!(Cli::try_parse_from([
            "rune",
            "message",
            "delete",
            "--message-id",
            "msg-1",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "rune",
            "message",
            "delete",
            "--channel",
            "telegram",
        ])
        .is_err());
    }

    // ── Message read (#74) ──────────────────────────────────────────

    #[test]
    fn parse_message_read() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "read",
            "--message-id",
            "msg-42",
            "--channel",
            "telegram",
            "--session",
            "sess-7",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Read {
                        message_id,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-42");
                assert_eq!(channel, "telegram");
                assert_eq!(session.as_deref(), Some("sess-7"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_read_without_session() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "read",
            "--message-id",
            "msg-99",
            "--channel",
            "discord",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Read {
                        message_id,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-99");
                assert_eq!(channel, "discord");
                assert!(session.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_read_requires_message_id_and_channel() {
        assert!(Cli::try_parse_from(["rune", "message", "read"]).is_err());
        assert!(Cli::try_parse_from([
            "rune",
            "message",
            "read",
            "--message-id",
            "msg-1",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "rune",
            "message",
            "read",
            "--channel",
            "telegram",
        ])
        .is_err());
    }

    // ── Message thread (#74) ────────────────────────────────────────

    #[test]
    fn parse_message_thread_list() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "thread",
            "list",
            "--thread-id",
            "thr-42",
            "--channel",
            "telegram",
            "--session",
            "sess-7",
            "--limit",
            "10",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Thread {
                        action:
                            MessageThreadAction::List {
                                thread_id,
                                channel,
                                session,
                                limit,
                            },
                    },
            } => {
                assert_eq!(thread_id, "thr-42");
                assert_eq!(channel.as_deref(), Some("telegram"));
                assert_eq!(session.as_deref(), Some("sess-7"));
                assert_eq!(limit, 10);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_thread_list_defaults() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "thread",
            "list",
            "--thread-id",
            "thr-1",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Thread {
                        action:
                            MessageThreadAction::List {
                                thread_id,
                                channel,
                                session,
                                limit,
                            },
                    },
            } => {
                assert_eq!(thread_id, "thr-1");
                assert!(channel.is_none());
                assert!(session.is_none());
                assert_eq!(limit, 50);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_thread_list_requires_thread_id() {
        assert!(
            Cli::try_parse_from(["rune", "message", "thread", "list"]).is_err()
        );
    }

    #[test]
    fn parse_message_thread_reply() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "thread",
            "reply",
            "--thread-id",
            "thr-42",
            "--channel",
            "telegram",
            "--text",
            "Thanks for the update",
            "--session",
            "sess-7",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Thread {
                        action:
                            MessageThreadAction::Reply {
                                thread_id,
                                channel,
                                text,
                                session,
                            },
                    },
            } => {
                assert_eq!(thread_id, "thr-42");
                assert_eq!(channel, "telegram");
                assert_eq!(text, "Thanks for the update");
                assert_eq!(session.as_deref(), Some("sess-7"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_thread_reply_without_session() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "thread",
            "reply",
            "--thread-id",
            "thr-99",
            "--channel",
            "discord",
            "--text",
            "reply text",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Thread {
                        action:
                            MessageThreadAction::Reply {
                                thread_id,
                                channel,
                                text,
                                session,
                            },
                    },
            } => {
                assert_eq!(thread_id, "thr-99");
                assert_eq!(channel, "discord");
                assert_eq!(text, "reply text");
                assert!(session.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_thread_reply_requires_thread_id_channel_text() {
        assert!(
            Cli::try_parse_from(["rune", "message", "thread", "reply"]).is_err()
        );
        assert!(Cli::try_parse_from([
            "rune",
            "message",
            "thread",
            "reply",
            "--thread-id",
            "thr-1",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "rune",
            "message",
            "thread",
            "reply",
            "--thread-id",
            "thr-1",
            "--channel",
            "telegram",
        ])
        .is_err());
    }
}
