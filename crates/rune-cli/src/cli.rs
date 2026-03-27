//! Clap-based CLI definition with all subcommands.

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

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

    /// Skip the interactive first-use confirmation prompt for bypass modes.
    ///
    /// When `--yolo` or `--no-sandbox` is used for the first time, Rune
    /// normally requires an interactive acknowledgment.  Pass `--accept-risk`
    /// to auto-acknowledge (useful in CI/scripts).
    #[arg(long, global = true)]
    pub accept_risk: bool,

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
    /// Run gateway-backed diagnostic checks and inspect recent results.
    Doctor {
        #[command(subcommand)]
        action: Option<DoctorAction>,
    },
    /// Show a compact operator dashboard summary.
    Dashboard,
    /// Query, tail, search, and export structured gateway logs.
    Logs {
        #[command(subcommand)]
        action: LogsAction,
    },
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
    /// Inspect and manage subagent sessions.
    Agents {
        #[command(subcommand)]
        action: AgentsAction,
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
    /// Inspect installed prompt skills.
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
    /// Inspect installed spells discovered by the gateway.
    Spells {
        #[command(subcommand)]
        action: SpellsAction,
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
    /// Inspect and manage background processes.
    Process {
        #[command(subcommand)]
        action: ProcessAction,
    },
    /// Manage reminders (one-shot scheduled messages).
    Reminders {
        #[command(subcommand)]
        action: RemindersAction,
    },
    /// Manage registered project workspaces.
    Projects {
        #[command(subcommand)]
        action: ProjectsAction,
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
    /// Run first-time setup wizard (legacy name kept as an alias for quick start).
    Init {
        /// Target workspace/config directory (defaults to current directory).
        #[arg(long, default_value = ".")]
        path: String,
        /// API key/token for the selected provider.
        #[arg(long)]
        api_key: Option<String>,
        /// Provider kind/name (for example: openai, anthropic, azure, groq, mistral, deepseek, ollama).
        #[arg(long)]
        provider: Option<String>,
        /// Model id to configure as default.
        #[arg(long)]
        model: Option<String>,
        /// Telegram bot token to enable Telegram during setup.
        #[arg(long)]
        telegram_token: Option<String>,
        /// Enable the browser WebChat flow after writing config.
        #[arg(long, default_value_t = true)]
        webchat: bool,
        /// Start the gateway after writing config.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        start: bool,
        /// Skip starting the gateway after writing config.
        #[arg(long = "no-start", default_value_t = false)]
        no_start: bool,
        /// Open the chat URL in the default browser after startup.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        open: bool,
        /// Skip opening the browser after startup.
        #[arg(long = "no-open", default_value_t = false)]
        no_open: bool,
        /// Print the launch URL instead of opening a browser.
        #[arg(long, default_value_t = false)]
        print_url: bool,
        /// Do not prompt; derive missing values from defaults/environment where possible.
        #[arg(long)]
        non_interactive: bool,
        /// Install a service definition after writing config.
        #[arg(long)]
        install_service: bool,
        /// Service manager target for --install-service.
        #[arg(long, value_enum, default_value = "systemd")]
        service_target: ServiceTarget,
        /// Service label/name to use for --install-service.
        #[arg(long, default_value = "rune-gateway")]
        service_name: String,
        /// Enable the service immediately after install.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        service_enable: bool,
        /// Skip enabling the service after install.
        #[arg(long = "no-service-enable", default_value_t = false)]
        no_service_enable: bool,
        /// Start the service immediately after install.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        service_start: bool,
        /// Skip starting the service after install.
        #[arg(long = "no-service-start", default_value_t = false)]
        no_service_start: bool,
    },
    /// Manage configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Run security audits and inspect security posture.
    Security {
        #[command(subcommand)]
        action: SecurityAction,
    },
    /// Inspect and manage filesystem sandbox boundaries.
    Sandbox {
        #[command(subcommand)]
        action: SandboxAction,
    },
    /// Manage runtime secrets lifecycle.
    Secrets {
        #[command(subcommand)]
        action: SecretsAction,
    },
    /// Run the interactive setup wizard.
    Configure,
    /// Run the local first-run init wizard, optionally install a service, and open WebChat.
    Wizard {
        /// Target workspace/config directory (defaults to current directory).
        #[arg(long, default_value = ".")]
        path: String,
        /// API key/token for the selected provider.
        #[arg(long)]
        api_key: Option<String>,
        /// Provider kind/name (for example: openai, anthropic, azure, groq, mistral, deepseek, ollama).
        #[arg(long)]
        provider: Option<String>,
        /// Model id to configure as default.
        #[arg(long)]
        model: Option<String>,
        /// Telegram bot token to enable Telegram during setup.
        #[arg(long)]
        telegram_token: Option<String>,
        /// Enable the browser WebChat flow after writing config.
        #[arg(long, default_value_t = true)]
        webchat: bool,
        /// Start the gateway after writing config.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        start: bool,
        /// Skip starting the gateway after writing config.
        #[arg(long = "no-start", default_value_t = false)]
        no_start: bool,
        /// Open the chat URL in the default browser after startup.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        open: bool,
        /// Skip opening the browser after startup.
        #[arg(long = "no-open", default_value_t = false)]
        no_open: bool,
        /// Print the launch URL instead of opening a browser.
        #[arg(long, default_value_t = false)]
        print_url: bool,
        /// Do not prompt; derive missing values from defaults/environment where possible.
        #[arg(long)]
        non_interactive: bool,
        /// Install a service definition after writing config.
        #[arg(long)]
        install_service: bool,
        /// Service manager target for --install-service.
        #[arg(long, value_enum, default_value = "systemd")]
        service_target: ServiceTarget,
        /// Service label/name to use for --install-service.
        #[arg(long, default_value = "rune-gateway")]
        service_name: String,
        /// Enable the service immediately after install.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        service_enable: bool,
        /// Skip enabling the service after install.
        #[arg(long = "no-service-enable", default_value_t = false)]
        no_service_enable: bool,
        /// Start the service immediately after install.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        service_start: bool,
        /// Skip starting the service after install.
        #[arg(long = "no-service-start", default_value_t = false)]
        no_service_start: bool,
    },
    /// Direct agent-turn invocation — send a single instruction to a session.
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Run the MCP memory server (stdio transport) for Claude Code / Codex integration.
    McpMemoryServer {
        /// Rune gateway URL (default: http://127.0.0.1:8787).
        #[arg(long, default_value = "http://127.0.0.1:8787")]
        rune_url: String,
    },
    /// Agent Communication Protocol (ACP) bridge commands.
    Acp {
        #[command(subcommand)]
        action: AcpAction,
    },
    /// Manage installed plugins (executable extensions).
    Plugins {
        #[command(subcommand)]
        action: PluginsAction,
    },
    /// Manage lifecycle hooks (pre/post event handlers).
    Hooks {
        #[command(subcommand)]
        action: HooksAction,
    },
    /// Run first-time setup wizard (alias for `wizard` with safe defaults).
    Setup {
        /// Target workspace/config directory (defaults to current directory).
        #[arg(long, default_value = ".")]
        path: String,
        /// API key/token for the selected provider.
        #[arg(long)]
        api_key: Option<String>,
        /// Provider kind/name (for example: openai, anthropic, azure, groq, mistral, deepseek, ollama).
        #[arg(long)]
        provider: Option<String>,
        /// Model id to configure as default.
        #[arg(long)]
        model: Option<String>,
        /// Telegram bot token to enable Telegram during setup.
        #[arg(long)]
        telegram_token: Option<String>,
        /// Enable the browser WebChat flow after writing config.
        #[arg(long, default_value_t = true)]
        webchat: bool,
        /// Start the gateway after writing config.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        start: bool,
        /// Skip starting the gateway after writing config.
        #[arg(long = "no-start", default_value_t = false)]
        no_start: bool,
        /// Open the chat URL in the default browser after startup.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        open: bool,
        /// Skip opening the browser after startup.
        #[arg(long = "no-open", default_value_t = false)]
        no_open: bool,
        /// Print the launch URL instead of opening a browser.
        #[arg(long, default_value_t = false)]
        print_url: bool,
        /// Do not prompt; derive missing values from defaults/environment where possible.
        #[arg(long)]
        non_interactive: bool,
        /// Install a service definition after writing config.
        #[arg(long)]
        install_service: bool,
        /// Service manager target for --install-service.
        #[arg(long, value_enum, default_value = "systemd")]
        service_target: ServiceTarget,
        /// Service label/name to use for --install-service.
        #[arg(long, default_value = "rune-gateway")]
        service_name: String,
        /// Enable the service immediately after install.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        service_enable: bool,
        /// Skip enabling the service after install.
        #[arg(long = "no-service-enable", default_value_t = false)]
        no_service_enable: bool,
        /// Start the service immediately after install.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        service_start: bool,
        /// Skip starting the service after install.
        #[arg(long = "no-service-start", default_value_t = false)]
        no_service_start: bool,
    },
    /// First-run onboarding alias for the local setup wizard.
    Onboard {
        /// Target workspace/config directory (defaults to current directory).
        #[arg(long, default_value = ".")]
        path: String,
        /// API key/token for the selected provider.
        #[arg(long)]
        api_key: Option<String>,
        /// Provider kind/name (for example: openai, anthropic, azure, groq, mistral, deepseek, ollama).
        #[arg(long)]
        provider: Option<String>,
        /// Model id to configure as default.
        #[arg(long)]
        model: Option<String>,
        /// Telegram bot token to enable Telegram during setup.
        #[arg(long)]
        telegram_token: Option<String>,
        /// Enable the browser WebChat flow after writing config.
        #[arg(long, default_value_t = true)]
        webchat: bool,
        /// Start the gateway after writing config.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        start: bool,
        /// Skip starting the gateway after writing config.
        #[arg(long = "no-start", default_value_t = false)]
        no_start: bool,
        /// Open the chat URL in the default browser after startup.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        open: bool,
        /// Skip opening the browser after startup.
        #[arg(long = "no-open", default_value_t = false)]
        no_open: bool,
        /// Print the launch URL instead of opening a browser.
        #[arg(long, default_value_t = false)]
        print_url: bool,
        /// Do not prompt; derive missing values from defaults/environment where possible.
        #[arg(long)]
        non_interactive: bool,
        /// Install a service definition after writing config.
        #[arg(long)]
        install_service: bool,
        /// Service manager target for --install-service.
        #[arg(long, value_enum, default_value = "systemd")]
        service_target: ServiceTarget,
        /// Service label/name to use for --install-service.
        #[arg(long, default_value = "rune-gateway")]
        service_name: String,
        /// Enable the service immediately after install.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        service_enable: bool,
        /// Skip enabling the service after install.
        #[arg(long = "no-service-enable", default_value_t = false)]
        no_service_enable: bool,
        /// Start the service immediately after install.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        service_start: bool,
        /// Skip starting the service after install.
        #[arg(long = "no-service-start", default_value_t = false)]
        no_service_start: bool,
    },
    /// Manage backups of durable state.
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    /// Manage gateway updates.
    #[command(alias = "self-update")]
    Update {
        #[command(subcommand)]
        action: UpdateAction,
    },
    /// Generate and inspect OS service definitions.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Factory-reset all state (requires confirmation).
    Reset {
        /// Confirm the destructive reset operation.
        #[arg(long)]
        confirm: bool,
    },
    /// Microsoft 365 integration surfaces (mail, calendar, files).
    #[command(name = "ms365")]
    Ms365 {
        #[command(subcommand)]
        action: Ms365Action,
    },
}

// ── Microsoft 365 ─────────────────────────────────────────────────

/// Top-level MS365 subcommands.
#[derive(Debug, Subcommand)]
pub enum Ms365Action {
    /// Inspect Microsoft 365 auth/config readiness.
    Auth {
        #[command(subcommand)]
        action: Ms365AuthAction,
    },
    /// Mail operations.
    Mail {
        #[command(subcommand)]
        action: Ms365MailAction,
    },
    /// Calendar operations.
    Calendar {
        #[command(subcommand)]
        action: Ms365CalendarAction,
    },
    /// OneDrive file operations.
    Files {
        #[command(subcommand)]
        action: Ms365FilesAction,
    },
    /// Users and organization inspection.
    Users {
        #[command(subcommand)]
        action: Ms365UsersAction,
    },
    /// Planner plans and tasks.
    Planner {
        #[command(subcommand)]
        action: Ms365PlannerAction,
    },
    /// Microsoft To-Do lists and tasks.
    Todo {
        #[command(subcommand)]
        action: Ms365TodoAction,
    },
    /// SharePoint sites inspection.
    Sites {
        #[command(subcommand)]
        action: Ms365SitesAction,
    },
    /// Microsoft Teams inspection.
    Teams {
        #[command(subcommand)]
        action: Ms365TeamsAction,
    },
}

/// Auth/config inspection subcommands.
#[derive(Debug, Subcommand)]
pub enum Ms365AuthAction {
    /// Show current authentication and configuration status.
    Status,
}

/// Mail subcommands.
#[derive(Debug, Subcommand)]
pub enum Ms365MailAction {
    /// List unread messages from the authenticated mailbox.
    Unread {
        /// Maximum number of messages to return.
        #[arg(long, default_value_t = 25)]
        limit: u32,
        /// Folder to query (default: Inbox).
        #[arg(long, default_value = "Inbox")]
        folder: String,
    },
    /// Read a single mail message by ID.
    Read {
        /// Message ID to retrieve.
        #[arg(long)]
        id: String,
    },
    /// List mail folders in the authenticated mailbox.
    Folders,
    /// Send a new mail message.
    Send {
        /// Recipient email address(es), comma-separated.
        #[arg(long, required = true)]
        to: String,
        /// Message subject.
        #[arg(long)]
        subject: String,
        /// Message body (plain text).
        #[arg(long)]
        body: String,
        /// CC recipient email address(es), comma-separated.
        #[arg(long)]
        cc: Option<String>,
    },
    /// Reply to an existing mail message.
    Reply {
        /// Message ID to reply to.
        #[arg(long)]
        id: String,
        /// Reply body (plain text).
        #[arg(long)]
        body: String,
        /// Reply to all recipients instead of just the sender.
        #[arg(long)]
        reply_all: bool,
    },
    /// Forward an existing mail message.
    Forward {
        /// Message ID to forward.
        #[arg(long)]
        id: String,
        /// Recipient email address(es), comma-separated.
        #[arg(long, required = true)]
        to: String,
        /// Optional comment to include with the forwarded message.
        #[arg(long)]
        comment: Option<String>,
    },
    /// List attachments on a mail message.
    Attachments {
        /// Message ID whose attachments to list.
        #[arg(long)]
        id: String,
    },
    /// Read metadata of a single mail attachment.
    #[command(name = "attachment-read")]
    AttachmentRead {
        /// Message ID that owns the attachment.
        #[arg(long)]
        message_id: String,
        /// Attachment ID to retrieve.
        #[arg(long)]
        id: String,
    },
    /// Download attachment content to a local file.
    #[command(name = "attachment-download")]
    AttachmentDownload {
        /// Message ID that owns the attachment.
        #[arg(long)]
        message_id: String,
        /// Attachment ID to download.
        #[arg(long)]
        id: String,
        /// Output file path (defaults to attachment name in current directory).
        #[arg(long)]
        output: Option<String>,
    },
}

/// Calendar subcommands.
#[derive(Debug, Subcommand)]
pub enum Ms365CalendarAction {
    /// List upcoming calendar events.
    Upcoming {
        /// Maximum number of events to return.
        #[arg(long, default_value_t = 10)]
        limit: u32,
        /// Look-ahead window in hours.
        #[arg(long, default_value_t = 24)]
        hours: u32,
    },
    /// Read a single calendar event by ID.
    Read {
        /// Event ID to retrieve.
        #[arg(long)]
        id: String,
    },
    /// Create a new calendar event.
    Create {
        /// Event subject/title.
        #[arg(long)]
        subject: String,
        /// Start date-time (ISO 8601, e.g. 2026-03-21T14:00:00).
        #[arg(long)]
        start: String,
        /// End date-time (ISO 8601, e.g. 2026-03-21T15:00:00).
        #[arg(long)]
        end: String,
        /// Attendee email address(es), comma-separated.
        #[arg(long)]
        attendees: Option<String>,
        /// Location description.
        #[arg(long)]
        location: Option<String>,
        /// Event body/description (plain text).
        #[arg(long)]
        body: Option<String>,
    },
    /// Update a calendar event by ID.
    Update {
        /// Event ID to update.
        #[arg(long)]
        id: String,
        /// Updated event subject/title.
        #[arg(long)]
        subject: Option<String>,
        /// Updated start date-time (ISO 8601).
        #[arg(long)]
        start: Option<String>,
        /// Updated end date-time (ISO 8601).
        #[arg(long)]
        end: Option<String>,
        /// Updated attendee email address(es), comma-separated.
        #[arg(long)]
        attendees: Option<String>,
        /// Updated location description. Use empty string to clear.
        #[arg(long)]
        location: Option<String>,
        /// Updated body/description (plain text). Use empty string to clear.
        #[arg(long)]
        body: Option<String>,
    },
    /// Delete a calendar event by ID.
    Delete {
        /// Event ID to delete.
        #[arg(long)]
        id: String,
    },
    /// Respond to a calendar event invitation (accept, decline, or tentatively accept).
    Respond {
        /// Event ID to respond to.
        #[arg(long)]
        id: String,
        /// Response: accept, decline, or tentative.
        #[arg(long)]
        response: String,
        /// Optional comment to include with the response.
        #[arg(long)]
        comment: Option<String>,
    },
}

/// OneDrive file subcommands.
#[derive(Debug, Subcommand)]
pub enum Ms365FilesAction {
    /// List files in a OneDrive folder.
    List {
        /// Folder path (default: root).
        #[arg(long, default_value = "/")]
        path: String,
        /// Maximum number of items to return.
        #[arg(long, default_value_t = 25)]
        limit: u32,
    },
    /// Read metadata for a single file by ID.
    Read {
        /// File/item ID to retrieve.
        #[arg(long)]
        id: String,
    },
    /// Search for files in OneDrive by name or content.
    Search {
        /// Search query string.
        #[arg(long)]
        query: String,
        /// Maximum number of results to return.
        #[arg(long, default_value_t = 25)]
        limit: u32,
    },
    /// Download a file's content to a local file.
    Download {
        /// File/item ID to download.
        #[arg(long)]
        id: String,
        /// Output file path (defaults to the file's name in the current directory).
        #[arg(long)]
        output: Option<String>,
    },
}

/// Users/org inspection subcommands.
#[derive(Debug, Subcommand)]
pub enum Ms365UsersAction {
    /// Show the authenticated user's profile.
    Me,
    /// List users in the organization directory.
    List {
        /// Maximum number of users to return.
        #[arg(long, default_value_t = 25)]
        limit: u32,
    },
    /// Read a single user's profile by ID or UPN.
    Read {
        /// User ID or user principal name.
        #[arg(long)]
        id: String,
    },
}

/// Planner subcommands.
#[derive(Debug, Subcommand)]
pub enum Ms365PlannerAction {
    /// List plans accessible to the authenticated user.
    Plans {
        /// Maximum number of plans to return.
        #[arg(long, default_value_t = 25)]
        limit: u32,
    },
    /// List tasks in a plan.
    Tasks {
        /// Plan ID to list tasks from.
        #[arg(long)]
        plan_id: String,
        /// Maximum number of tasks to return.
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Read a single task by ID.
    #[command(name = "task-read")]
    TaskRead {
        /// Task ID to retrieve.
        #[arg(long)]
        id: String,
    },
    /// Create a new Planner task.
    Create {
        /// Plan ID that will own the task.
        #[arg(long)]
        plan_id: String,
        /// Task title.
        #[arg(long)]
        title: String,
        /// Optional bucket ID for the new task.
        #[arg(long)]
        bucket_id: Option<String>,
        /// Optional due date-time (ISO 8601).
        #[arg(long)]
        due_date: Option<String>,
        /// Optional task description/body.
        #[arg(long)]
        description: Option<String>,
    },
    /// Update Planner task details or progress.
    #[command(group(
        ArgGroup::new("planner_task_update_changes")
            .required(true)
            .multiple(true)
            .args(["title", "bucket_id", "due_date", "description", "percent_complete"])
    ))]
    Update {
        /// Task ID to update.
        #[arg(long)]
        id: String,
        /// New task title.
        #[arg(long)]
        title: Option<String>,
        /// New bucket ID.
        #[arg(long)]
        bucket_id: Option<String>,
        /// New due date-time (ISO 8601).
        #[arg(long)]
        due_date: Option<String>,
        /// New task description/body.
        #[arg(long)]
        description: Option<String>,
        /// Progress percentage from 0 to 100.
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        percent_complete: Option<u8>,
    },
    /// Mark a Planner task complete.
    Complete {
        /// Task ID to mark complete.
        #[arg(long)]
        id: String,
    },
}

/// Microsoft To-Do subcommands.
#[derive(Debug, Subcommand)]
pub enum Ms365TodoAction {
    /// List To-Do task lists.
    Lists {
        /// Maximum number of lists to return.
        #[arg(long, default_value_t = 25)]
        limit: u32,
    },
    /// List tasks in a To-Do list.
    Tasks {
        /// Task list ID to list tasks from.
        #[arg(long)]
        list_id: String,
        /// Maximum number of tasks to return.
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Read a single To-Do task by ID.
    #[command(name = "task-read")]
    TaskRead {
        /// Task list ID that contains the task.
        #[arg(long)]
        list_id: String,
        /// Task ID to retrieve.
        #[arg(long)]
        id: String,
    },
    /// Create a new To-Do task.
    Create {
        /// Task list ID that will own the task.
        #[arg(long)]
        list_id: String,
        /// Task title.
        #[arg(long)]
        title: String,
        /// Optional due date-time (ISO 8601).
        #[arg(long)]
        due_date: Option<String>,
        /// Optional task importance (for example `low`, `normal`, `high`).
        #[arg(long)]
        importance: Option<String>,
        /// Optional task body/notes.
        #[arg(long)]
        body: Option<String>,
    },
    /// Update To-Do task details or workflow status.
    #[command(group(
        ArgGroup::new("todo_task_update_changes")
            .required(true)
            .multiple(true)
            .args(["title", "status", "importance", "due_date", "body"])
    ))]
    Update {
        /// Task list ID that contains the task.
        #[arg(long)]
        list_id: String,
        /// Task ID to update.
        #[arg(long)]
        id: String,
        /// New task title.
        #[arg(long)]
        title: Option<String>,
        /// New workflow status (for example `notStarted`, `inProgress`, `completed`).
        #[arg(long)]
        status: Option<String>,
        /// New task importance.
        #[arg(long)]
        importance: Option<String>,
        /// New due date-time (ISO 8601).
        #[arg(long)]
        due_date: Option<String>,
        /// New task body/notes.
        #[arg(long)]
        body: Option<String>,
    },
    /// Mark a To-Do task complete.
    Complete {
        /// Task list ID that contains the task.
        #[arg(long)]
        list_id: String,
        /// Task ID to mark complete.
        #[arg(long)]
        id: String,
    },
}

/// SharePoint sites subcommands.
#[derive(Debug, Subcommand)]
pub enum Ms365SitesAction {
    /// List SharePoint sites accessible to the authenticated user.
    List {
        /// Maximum number of sites to return.
        #[arg(long, default_value_t = 25)]
        limit: u32,
    },
    /// Read details of a single SharePoint site by ID.
    Read {
        /// Site ID to retrieve.
        #[arg(long)]
        id: String,
    },
    /// List document libraries in a SharePoint site.
    Lists {
        /// Site ID to list libraries from.
        #[arg(long)]
        site_id: String,
        /// Maximum number of lists to return.
        #[arg(long, default_value_t = 25)]
        limit: u32,
    },
    /// List items in a SharePoint list/library.
    #[command(name = "list-items")]
    ListItems {
        /// Site ID that contains the list.
        #[arg(long)]
        site_id: String,
        /// List ID to enumerate items from.
        #[arg(long)]
        list_id: String,
        /// Maximum number of items to return.
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
}

/// Microsoft Teams subcommands.
#[derive(Debug, Subcommand)]
pub enum Ms365TeamsAction {
    /// List teams the authenticated user belongs to.
    List {
        /// Maximum number of teams to return.
        #[arg(long, default_value_t = 25)]
        limit: u32,
    },
    /// List channels in a team.
    Channels {
        /// Team ID to list channels from.
        #[arg(long)]
        team_id: String,
        /// Maximum number of channels to return.
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Read details of a single channel.
    #[command(name = "channel-read")]
    ChannelRead {
        /// Team ID that contains the channel.
        #[arg(long)]
        team_id: String,
        /// Channel ID to retrieve.
        #[arg(long)]
        id: String,
    },
    /// List recent messages in a channel.
    Messages {
        /// Team ID that contains the channel.
        #[arg(long)]
        team_id: String,
        /// Channel ID to list messages from.
        #[arg(long)]
        channel_id: String,
        /// Maximum number of messages to return.
        #[arg(long, default_value_t = 25)]
        limit: u32,
    },
}

/// Direct agent-turn invocation actions.
#[derive(Debug, Subcommand)]
pub enum AgentAction {
    /// Send a single instruction to a session and return the response.
    Run {
        /// Session ID to send the instruction to.
        #[arg(long)]
        session: String,
        /// The instruction text to send.
        #[arg(long)]
        message: String,
        /// Maximum turns to allow before returning.
        #[arg(long, default_value_t = 1)]
        max_turns: u32,
        /// Wait for the agent turn to complete before returning.
        #[arg(long, default_value_t = true)]
        wait: bool,
    },
    /// Check the result of a previously submitted agent turn.
    Result {
        /// Session ID to check.
        #[arg(long)]
        session: String,
        /// Turn ID to retrieve.
        #[arg(long)]
        turn: String,
    },
}

/// ACP bridge actions for inter-agent communication.
#[derive(Debug, Subcommand)]
pub enum AcpAction {
    /// Send an ACP message to a target agent session.
    Send {
        /// Source session ID.
        #[arg(long)]
        from: String,
        /// Target session ID.
        #[arg(long)]
        to: String,
        /// Message payload (JSON string).
        #[arg(long)]
        payload: String,
    },
    /// List pending ACP messages for a session.
    Inbox {
        /// Session ID to check.
        #[arg(long)]
        session: String,
    },
    /// Acknowledge/consume an ACP message.
    Ack {
        /// Message ID to acknowledge.
        #[arg(long)]
        message_id: String,
        /// Session ID that owns the message.
        #[arg(long)]
        session: String,
    },
}

/// Plugin lifecycle actions.
#[derive(Debug, Subcommand)]
pub enum PluginsAction {
    /// List installed plugins.
    List,
    /// Show details for a specific plugin.
    Info {
        /// Plugin name.
        name: String,
    },
    /// Install a plugin from a path or URL.
    Install {
        /// Plugin source (local path or URL).
        source: String,
    },
    /// Uninstall a plugin.
    Uninstall {
        /// Plugin name.
        name: String,
    },
    /// Enable an installed plugin.
    Enable {
        /// Plugin name.
        name: String,
    },
    /// Disable an installed plugin.
    Disable {
        /// Plugin name.
        name: String,
    },
    /// Update an installed plugin.
    Update {
        /// Plugin name.
        name: String,
    },
    /// Run diagnostic checks on a plugin.
    Doctor {
        /// Plugin name.
        name: String,
    },
}

/// Hook lifecycle actions.
#[derive(Debug, Subcommand)]
pub enum HooksAction {
    /// List configured hooks.
    List,
    /// Show details for a specific hook.
    Info {
        /// Hook name.
        name: String,
    },
    /// Validate hook configuration and report issues.
    Check,
    /// Enable a configured hook.
    Enable {
        /// Hook name.
        name: String,
    },
    /// Disable a configured hook.
    Disable {
        /// Hook name.
        name: String,
    },
    /// Install a hook from a path or URL.
    Install {
        /// Hook source (local path or URL).
        source: String,
    },
    /// Update an installed hook.
    Update {
        /// Hook name.
        name: String,
    },
}

#[derive(Debug, Clone, Args)]
pub struct LogsArgs {
    /// Filter by log level.
    #[arg(long)]
    pub level: Option<String>,
    /// Filter by source/component name.
    #[arg(long)]
    pub source: Option<String>,
    /// Maximum number of entries to return.
    #[arg(long)]
    pub limit: Option<usize>,
    /// Lower-bound timestamp or relative cursor understood by the gateway.
    #[arg(long)]
    pub since: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ProjectAddArgs {
    /// Filesystem path to the project repository/workspace.
    pub path: String,
    /// Override the inferred project name.
    #[arg(long)]
    pub name: Option<String>,
    /// Override the detected remote URL.
    #[arg(long)]
    pub repo_url: Option<String>,
    /// Default branch to record for the project.
    #[arg(long, default_value = "main")]
    pub default_branch: String,
    /// Default model override for this project.
    #[arg(long)]
    pub default_model: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ProjectsAction {
    /// Register a project workspace.
    Add(ProjectAddArgs),
    /// List all registered projects.
    List,
    /// Switch the active foreground project.
    Switch {
        /// Registered project name.
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum LogsAction {
    /// Query structured gateway logs with optional filters.
    Query(LogsArgs),
    /// Tail (follow) live gateway log output.
    Tail {
        /// Filter by log level.
        #[arg(long)]
        level: Option<String>,
        /// Filter by source/component name.
        #[arg(long)]
        source: Option<String>,
        /// Enable continuous follow mode (stream new entries as they arrive).
        #[arg(long, short)]
        follow: bool,
        /// Maximum number of initial entries to show before following.
        #[arg(long, default_value_t = 50)]
        lines: usize,
    },
    /// Full-text search across historical log entries.
    Search {
        /// Query text to search for in log content.
        query: String,
        /// Filter by log level.
        #[arg(long)]
        level: Option<String>,
        /// Filter by source/component name.
        #[arg(long)]
        source: Option<String>,
        /// Maximum number of results to return.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Export logs to a file or stdout in a specified format.
    Export {
        /// Output format: `json`, `csv`, or `text`.
        #[arg(long, default_value = "json")]
        format: String,
        /// Filter by log level.
        #[arg(long)]
        level: Option<String>,
        /// Filter by source/component name.
        #[arg(long)]
        source: Option<String>,
        /// Lower-bound timestamp.
        #[arg(long)]
        since: Option<String>,
        /// Upper-bound timestamp.
        #[arg(long)]
        until: Option<String>,
        /// Maximum number of entries to export.
        #[arg(long)]
        limit: Option<usize>,
        /// Output file path (default: stdout).
        #[arg(long, short)]
        output: Option<String>,
    },
}
#[derive(Debug, Subcommand)]
pub enum GatewayAction {
    /// Query gateway status.
    Status,
    /// Run a health check against the gateway.
    Health,
    /// Inspect and mutate the live gateway config surface.
    Config {
        #[command(subcommand)]
        action: GatewayConfigAction,
    },
    /// Inspect runtime control-plane status surfaced by the gateway.
    Runtime {
        #[command(subcommand)]
        action: GatewayRuntimeAction,
    },
    /// Probe RPC/API reachability and auth separately from process status.
    Probe,
    /// Discover operator-facing runtime URLs and config binding details.
    Discover,
    /// Query structured gateway logs.
    Logs(LogsArgs),
    /// Run gateway-backed diagnostic checks and inspect recent results.
    Doctor {
        #[command(subcommand)]
        action: Option<DoctorAction>,
    },
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
pub enum GatewayConfigAction {
    /// Fetch the live gateway config with secrets redacted.
    Show,
    /// Replace the live gateway config from a JSON file or stdin (`-`).
    Apply {
        /// JSON file path, or `-` to read from stdin.
        input: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum GatewayRuntimeAction {
    /// Inspect and control the heartbeat runner.
    Heartbeat {
        #[command(subcommand)]
        action: GatewayRuntimeHeartbeatAction,
    },
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum GatewayRuntimeHeartbeatAction {
    /// Enable the heartbeat runner.
    Enable,
    /// Disable the heartbeat runner.
    Disable,
    /// Show heartbeat runner status (enabled, interval, counters).
    Status,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum DoctorAction {
    /// Execute a fresh doctor run via the gateway.
    Run,
    /// Fetch the most recent doctor report from the gateway.
    Results,
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
    /// List sessions with optional activity/channel/kind filters.
    List {
        /// Only include sessions active within the last N minutes.
        #[arg(long = "active")]
        active_minutes: Option<u64>,
        /// Only include sessions for the given channel reference.
        #[arg(long)]
        channel: Option<String>,
        /// Filter by session kind (e.g. "direct", "subagent", "scheduled").
        #[arg(long)]
        kind: Option<String>,
        /// Only show children of this parent/requester session ID.
        #[arg(long)]
        parent: Option<String>,
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
    /// Show the delegation tree rooted at a session (parent → children → grandchildren).
    Tree {
        /// Session ID to use as tree root.
        id: String,
    },
    /// Display the conversation history / transcript for a session.
    History {
        /// Session ID to inspect.
        id: String,
        /// Filter entries by kind (e.g. "user_message", "assistant_message", "tool_request").
        #[arg(long)]
        kind: Option<String>,
        /// Only show entries belonging to this turn ID.
        #[arg(long)]
        turn: Option<String>,
        /// Show only the last N entries.
        #[arg(long)]
        tail: Option<usize>,
    },
    /// Export a session as a JSON bundle (detail + transcript).
    Export {
        /// Session ID to export.
        id: String,
    },
    /// Delete a single session and its transcript history.
    Delete {
        /// Session ID to delete.
        id: String,
    },
    /// Bulk-delete sessions matching filters (completed, stale, by kind).
    Cleanup {
        /// Only delete sessions with this status (e.g. "completed", "failed", "idle").
        #[arg(long)]
        status: Option<String>,
        /// Only delete sessions of this kind (e.g. "direct", "subagent", "scheduled").
        #[arg(long)]
        kind: Option<String>,
        /// Only delete sessions inactive for at least N minutes.
        #[arg(long = "older-than")]
        older_than_minutes: Option<u64>,
        /// Preview which sessions would be deleted without removing them.
        #[arg(long)]
        dry_run: bool,
        /// Maximum number of sessions to delete.
        #[arg(long, default_value_t = 100)]
        limit: u64,
    },
}

#[derive(Debug, Subcommand)]
pub enum AgentsAction {
    /// List active subagent sessions.
    List {
        /// Only include agents active within the last N minutes.
        #[arg(long = "active")]
        active_minutes: Option<u64>,
        /// Maximum number of agents to return.
        #[arg(long, default_value_t = 100)]
        limit: u64,
    },
    /// Show details for a specific subagent session.
    Show {
        /// Subagent session ID to inspect.
        id: String,
    },
    /// Show the first-class status card for a specific subagent session.
    Status {
        /// Subagent session ID to inspect.
        id: String,
    },
    /// Display the full delegation tree (parent → children → grandchildren).
    Tree {
        /// Maximum number of sessions to fetch for tree construction.
        #[arg(long, default_value_t = 500)]
        limit: u64,
    },
    /// List available pre-built agent templates.
    Templates {
        /// Filter templates by category (developer, operator, personal).
        #[arg(long)]
        category: Option<String>,
    },
    /// Start a new agent session from a built-in template.
    Start {
        /// Template slug to launch (e.g. "coding-agent").
        #[arg(long)]
        template: String,
    },
    /// Spawn a new subagent session linked to a parent.
    Spawn {
        /// Parent session ID that owns this subagent.
        #[arg(long)]
        parent: String,
        /// Agent mode (e.g. "coding", "research", "operator").
        #[arg(long, default_value = "default")]
        mode: String,
        /// Approval policy for the child session.
        #[arg(long, default_value = "inherit")]
        policy: String,
        /// Initial task description for the subagent.
        #[arg(long)]
        task: String,
        /// Model provider override for the child session.
        #[arg(long)]
        provider: Option<String>,
    },
    /// Send a follow-up instruction to a running subagent.
    Steer {
        /// Subagent session ID to steer.
        id: String,
        /// Follow-up instruction text.
        #[arg(long)]
        message: String,
    },
    /// Terminate a running subagent session.
    Kill {
        /// Subagent session ID to kill.
        id: String,
        /// Optional reason for termination.
        #[arg(long)]
        reason: Option<String>,
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
pub enum SkillsAction {
    /// List installed skills discovered by the gateway.
    List,
    /// Show details for a single installed skill.
    Info {
        /// Skill name.
        name: String,
    },
    /// Re-scan the skills directory and report load/remove counts.
    Check,
    /// Enable a discovered skill in the gateway registry.
    Enable {
        /// Skill name.
        name: String,
    },
    /// Disable a discovered skill in the gateway registry.
    Disable {
        /// Skill name.
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SpellsAction {
    /// Search installed spells by name, description, tag, trigger, or namespace.
    Search {
        /// Query text to match against installed spell metadata.
        query: String,
    },
    /// List installed spells discovered by the gateway.
    List,
    /// Show details for a single installed spell.
    Info {
        /// Spell name.
        name: String,
    },
    /// Re-scan the spells directory and report load/remove counts.
    Check,
    /// Enable a discovered spell in the gateway registry.
    Enable {
        /// Spell name.
        name: String,
    },
    /// Disable a discovered spell in the gateway registry.
    Disable {
        /// Spell name.
        name: String,
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
    /// Pin a message in a channel.
    Pin {
        /// ID of the message to pin.
        #[arg(long)]
        message_id: String,
        /// Channel adapter the message belongs to.
        #[arg(long)]
        channel: Option<String>,
        /// Session ID the message belongs to.
        #[arg(long)]
        session: Option<String>,
    },
    /// Unpin a message in a channel.
    Unpin {
        /// ID of the message to unpin.
        #[arg(long)]
        message_id: String,
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
    /// Synthesize and send voice messages via TTS.
    Voice {
        #[command(subcommand)]
        action: MessageVoiceAction,
    },
    /// Add, remove, or list tags on a message.
    Tag {
        #[command(subcommand)]
        action: MessageTagAction,
    },
    /// Acknowledge (mark as read/received) a message.
    Ack {
        /// ID of the message to acknowledge.
        #[arg(long)]
        message_id: String,
        /// Channel adapter the message belongs to.
        #[arg(long)]
        channel: String,
        /// Session ID the message belongs to.
        #[arg(long)]
        session: Option<String>,
    },
    /// List emoji reactions on a message.
    #[command(name = "list-reactions")]
    ListReactions {
        /// ID of the message to list reactions for.
        #[arg(long)]
        message_id: String,
        /// Channel adapter the message belongs to.
        #[arg(long)]
        channel: Option<String>,
        /// Session ID the message belongs to.
        #[arg(long)]
        session: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum MessageTagAction {
    /// Add a tag to a message.
    Add {
        /// ID of the message to tag.
        #[arg(long)]
        message_id: String,
        /// Tag name to add (e.g. "urgent", "followup", "resolved").
        #[arg(long)]
        tag: String,
        /// Channel adapter the message belongs to.
        #[arg(long)]
        channel: Option<String>,
        /// Session ID the message belongs to.
        #[arg(long)]
        session: Option<String>,
    },
    /// Remove a tag from a message.
    Remove {
        /// ID of the message to untag.
        #[arg(long)]
        message_id: String,
        /// Tag name to remove.
        #[arg(long)]
        tag: String,
        /// Channel adapter the message belongs to.
        #[arg(long)]
        channel: Option<String>,
        /// Session ID the message belongs to.
        #[arg(long)]
        session: Option<String>,
    },
    /// List all tags on a message.
    List {
        /// ID of the message to list tags for.
        #[arg(long)]
        message_id: String,
        /// Channel adapter the message belongs to.
        #[arg(long)]
        channel: Option<String>,
        /// Session ID the message belongs to.
        #[arg(long)]
        session: Option<String>,
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
pub enum MessageVoiceAction {
    /// Synthesize text to speech and send as a voice message.
    Send {
        /// Text to synthesize into speech.
        #[arg(long)]
        text: String,
        /// Target channel adapter for delivery (e.g. "telegram", "discord").
        #[arg(long)]
        channel: String,
        /// Override the default TTS voice (e.g. "alloy", "nova", "shimmer").
        #[arg(long)]
        voice: Option<String>,
        /// Override the default TTS model (e.g. "tts-1", "tts-1-hd").
        #[arg(long)]
        model: Option<String>,
        /// Optional session ID to associate the message with.
        #[arg(long)]
        session: Option<String>,
        /// Save synthesized audio to a local file path.
        #[arg(long)]
        output: Option<String>,
    },
    /// Show TTS engine status and available voices.
    Status,
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
    /// Ask the gateway to reload its configuration from disk.
    Reload,
    /// Show the diff between the on-disk config and the live running config.
    Diff,
    /// Show all resolved RUNE__* environment variable overrides.
    Env,
    /// Export resolved configuration (secrets redacted) to a file or stdout.
    Export {
        /// Output file path. Omit or use `-` to write to stdout.
        #[arg(default_value = "-")]
        output: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProcessAction {
    /// List background processes.
    List,
    /// Inspect a single background process.
    Get {
        /// Process identifier.
        id: String,
    },
    /// Fetch log output from a background process.
    Log {
        /// Process identifier.
        id: String,
    },
    /// Kill a running background process.
    Kill {
        /// Process identifier.
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SecurityAction {
    /// Run a security audit against the gateway.
    Audit,
}

#[derive(Debug, Subcommand)]
pub enum SandboxAction {
    /// List active sandbox boundaries and their status.
    List,
    /// Recreate sandbox boundaries from the current configuration.
    Recreate,
    /// Explain the current sandbox policy in human-readable form.
    Explain,
}

#[derive(Debug, Subcommand)]
pub enum SecretsAction {
    /// Reload secrets from the configured secret store.
    Reload,
    /// Audit secret usage and detect stale or unused secrets.
    Audit,
    /// Show the current secret store configuration (redacted).
    Configure,
    /// Apply a secrets manifest from a JSON file or stdin (`-`).
    Apply {
        /// JSON file path, or `-` to read from stdin.
        input: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum BackupAction {
    /// Create a backup of all durable state.
    Create {
        /// Optional human-readable label for the backup.
        #[arg(long)]
        label: Option<String>,
    },
    /// List available backups.
    List,
    /// Restore from a specific backup.
    Restore {
        /// Backup identifier to restore from.
        id: String,
        /// Confirm the restore operation.
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum UpdateAction {
    /// Check for available updates.
    Check,
    /// Apply a pending update.
    /// Apply a pending update by downloading and replacing the current binary.
    Apply {
        /// Explicit release tag (for example `v0.1.0`). Defaults to the latest GitHub release.
        #[arg(long)]
        version: Option<String>,
        /// GitHub repository to download releases from.
        #[arg(long, default_value = "ghostrider0470/rune")]
        repo: String,
        /// Override the install destination for the downloaded binary.
        #[arg(long)]
        binary_path: Option<String>,
    },
    /// Show current update status.
    Status,
    /// Print a direct install script URL for one-command bootstrap.
    InstallScript {
        /// Install script URL to print.
        #[arg(
            long,
            default_value = "https://raw.githubusercontent.com/ghostrider0470/rune/main/scripts/install.sh"
        )]
        install_script_url: String,
    },
    /// Print the quickest self-update/install commands for this checkout.
    Wizard {
        /// Install script URL to print for one-command bootstrap.
        #[arg(
            long,
            default_value = "https://raw.githubusercontent.com/ghostrider0470/rune/main/scripts/install.sh"
        )]
        install_script_url: String,
        /// Branch name to use when updating/building from source.
        #[arg(long, default_value = "main")]
        branch: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ServiceTarget {
    Systemd,
    Launchd,
}

#[derive(Debug, Subcommand)]
pub enum ServiceAction {
    /// Print a service definition to stdout.
    Print {
        /// Service target format.
        #[arg(long, value_enum, default_value = "systemd")]
        target: ServiceTarget,
        /// Service name/label to embed in the unit/plist.
        #[arg(long, default_value = "rune-gateway")]
        name: String,
        /// Working directory for the service process.
        #[arg(long, default_value = ".")]
        workdir: String,
        /// Config path passed via RUNE_CONFIG.
        #[arg(long)]
        config: Option<String>,
        /// Gateway URL exposed to the service process.
        #[arg(long)]
        gateway_url: Option<String>,
        /// Start the gateway with approval.mode=yolo.
        #[arg(long)]
        yolo: bool,
        /// Disable sandboxing for the service process.
        #[arg(long)]
        no_sandbox: bool,
    },
    /// Write a service definition to disk.
    Install {
        /// Service target format.
        #[arg(long, value_enum, default_value = "systemd")]
        target: ServiceTarget,
        /// Service name/label to embed in the unit/plist.
        #[arg(long, default_value = "rune-gateway")]
        name: String,
        /// Working directory for the service process.
        #[arg(long, default_value = ".")]
        workdir: String,
        /// Config path passed via RUNE_CONFIG.
        #[arg(long)]
        config: Option<String>,
        /// Gateway URL exposed to the service process.
        #[arg(long)]
        gateway_url: Option<String>,
        /// Start the gateway with approval.mode=yolo.
        #[arg(long)]
        yolo: bool,
        /// Disable sandboxing for the service process.
        #[arg(long)]
        no_sandbox: bool,
        /// Output path for the generated file.
        #[arg(long)]
        output: Option<String>,
        /// Enable the installed service after writing the definition.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        enable: bool,
        /// Start the installed service after writing the definition.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        start: bool,
        /// Only write the definition; skip systemctl/launchctl activation even when enable/start are true.
        #[arg(long, default_value_t = false)]
        no_bootstrap: bool,
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
        assert!(matches!(cli.command, Command::Doctor { action: None }));
    }

    #[test]
    fn parse_doctor_run() {
        let cli = Cli::try_parse_from(["rune", "doctor", "run"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Doctor {
                action: Some(DoctorAction::Run)
            }
        ));
    }

    #[test]
    fn parse_doctor_results() {
        let cli = Cli::try_parse_from(["rune", "doctor", "results"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Doctor {
                action: Some(DoctorAction::Results)
            }
        ));
    }

    #[test]
    fn parse_dashboard() {
        let cli = Cli::try_parse_from(["rune", "dashboard"]).unwrap();
        assert!(matches!(cli.command, Command::Dashboard));
    }

    #[test]
    fn parse_logs_query() {
        let cli = Cli::try_parse_from([
            "rune",
            "logs",
            "query",
            "--level",
            "warn",
            "--source",
            "gateway",
            "--limit",
            "25",
            "--since",
            "2026-03-20T09:00:00Z",
        ])
        .unwrap();
        match cli.command {
            Command::Logs {
                action:
                    LogsAction::Query(LogsArgs {
                        level,
                        source,
                        limit,
                        since,
                    }),
            } => {
                assert_eq!(level.as_deref(), Some("warn"));
                assert_eq!(source.as_deref(), Some("gateway"));
                assert_eq!(limit, Some(25));
                assert_eq!(since.as_deref(), Some("2026-03-20T09:00:00Z"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_logs_tail() {
        let cli =
            Cli::try_parse_from(["rune", "logs", "tail", "--follow", "--level", "error"]).unwrap();
        match cli.command {
            Command::Logs {
                action:
                    LogsAction::Tail {
                        level,
                        source,
                        follow,
                        lines,
                    },
            } => {
                assert_eq!(level.as_deref(), Some("error"));
                assert_eq!(source, None);
                assert!(follow);
                assert_eq!(lines, 50);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_logs_search() {
        let cli = Cli::try_parse_from([
            "rune", "logs", "search", "panic", "--level", "error", "--limit", "10",
        ])
        .unwrap();
        match cli.command {
            Command::Logs {
                action:
                    LogsAction::Search {
                        query,
                        level,
                        source,
                        limit,
                    },
            } => {
                assert_eq!(query, "panic");
                assert_eq!(level.as_deref(), Some("error"));
                assert_eq!(source, None);
                assert_eq!(limit, 10);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_logs_export() {
        let cli = Cli::try_parse_from([
            "rune",
            "logs",
            "export",
            "--format",
            "csv",
            "--since",
            "2026-03-19",
            "-o",
            "out.csv",
        ])
        .unwrap();
        match cli.command {
            Command::Logs {
                action:
                    LogsAction::Export {
                        format,
                        level,
                        source,
                        since,
                        until,
                        limit,
                        output,
                    },
            } => {
                assert_eq!(format, "csv");
                assert_eq!(level, None);
                assert_eq!(source, None);
                assert_eq!(since.as_deref(), Some("2026-03-19"));
                assert_eq!(until, None);
                assert_eq!(limit, None);
                assert_eq!(output.as_deref(), Some("out.csv"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_gateway_logs() {
        let cli = Cli::try_parse_from(["rune", "gateway", "logs", "--limit", "50"]).unwrap();
        match cli.command {
            Command::Gateway {
                action:
                    GatewayAction::Logs(LogsArgs {
                        level,
                        source,
                        limit,
                        since,
                    }),
            } => {
                assert_eq!(level, None);
                assert_eq!(source, None);
                assert_eq!(limit, Some(50));
                assert_eq!(since, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_gateway_doctor_results() {
        let cli = Cli::try_parse_from(["rune", "gateway", "doctor", "results"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Doctor {
                    action: Some(DoctorAction::Results)
                }
            }
        ));
    }

    #[test]
    fn parse_skills_list() {
        let cli = Cli::try_parse_from(["rune", "skills", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Skills {
                action: SkillsAction::List
            }
        ));
    }

    #[test]
    fn parse_skills_check() {
        let cli = Cli::try_parse_from(["rune", "skills", "check"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Skills {
                action: SkillsAction::Check
            }
        ));
    }

    #[test]
    fn parse_skills_info() {
        let cli = Cli::try_parse_from(["rune", "skills", "info", "alpha"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Skills {
                action: SkillsAction::Info { name }
            } if name == "alpha"
        ));
    }

    #[test]
    fn parse_skills_enable_disable() {
        for (subcommand, matcher) in [("enable", "enable"), ("disable", "disable")] {
            let cli = Cli::try_parse_from(["rune", "skills", subcommand, "alpha"]).unwrap();
            match (matcher, cli.command) {
                (
                    "enable",
                    Command::Skills {
                        action: SkillsAction::Enable { name },
                    },
                )
                | (
                    "disable",
                    Command::Skills {
                        action: SkillsAction::Disable { name },
                    },
                ) => assert_eq!(name, "alpha"),
                other => panic!("unexpected command: {other:?}"),
            }
        }
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
    fn parse_gateway_config_show() {
        let cli = Cli::try_parse_from(["rune", "gateway", "config", "show"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Config {
                    action: GatewayConfigAction::Show
                }
            }
        ));
    }

    #[test]
    fn parse_gateway_config_apply() {
        let cli = Cli::try_parse_from(["rune", "gateway", "config", "apply", "live.json"]).unwrap();
        match cli.command {
            Command::Gateway {
                action:
                    GatewayAction::Config {
                        action: GatewayConfigAction::Apply { input },
                    },
            } => assert_eq!(input, "live.json"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_gateway_runtime_heartbeat_status() {
        let cli =
            Cli::try_parse_from(["rune", "gateway", "runtime", "heartbeat", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Gateway {
                action: GatewayAction::Runtime {
                    action: GatewayRuntimeAction::Heartbeat {
                        action: GatewayRuntimeHeartbeatAction::Status
                    }
                }
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
        let cli =
            Cli::try_parse_from(["rune", "system", "event", "inject", "--text", "ping"]).unwrap();
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
        let cli =
            Cli::try_parse_from(["rune", "system", "event", "list", "--include-disabled"]).unwrap();
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
        match cli.command {
            Command::Sessions {
                action:
                    SessionsAction::List {
                        active_minutes,
                        channel,
                        kind,
                        parent,
                        limit,
                    },
            } => {
                assert_eq!(active_minutes, None);
                assert_eq!(channel, None);
                assert_eq!(kind, None);
                assert_eq!(parent, None);
                assert_eq!(limit, 100);
            }
            other => panic!("unexpected command: {other:?}"),
        }
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
                        ..
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
    fn parse_sessions_list_kind_filter() {
        let cli = Cli::try_parse_from(["rune", "sessions", "list", "--kind", "subagent"]).unwrap();
        match cli.command {
            Command::Sessions {
                action: SessionsAction::List { kind, .. },
            } => {
                assert_eq!(kind.as_deref(), Some("subagent"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_sessions_list_parent_filter() {
        let cli = Cli::try_parse_from(["rune", "sessions", "list", "--parent", "abc-123"]).unwrap();
        match cli.command {
            Command::Sessions {
                action: SessionsAction::List { parent, .. },
            } => {
                assert_eq!(parent.as_deref(), Some("abc-123"));
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
    fn parse_sessions_tree() {
        let cli = Cli::try_parse_from(["rune", "sessions", "tree", "abc-123"]).unwrap();
        match &cli.command {
            Command::Sessions {
                action: SessionsAction::Tree { id },
            } => assert_eq!(id, "abc-123"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_agents_list() {
        let cli = Cli::try_parse_from(["rune", "agents", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Agents {
                action: AgentsAction::List {
                    active_minutes: None,
                    limit: 100
                }
            }
        ));
    }

    #[test]
    fn parse_agents_list_with_filters() {
        let cli =
            Cli::try_parse_from(["rune", "agents", "list", "--active", "15", "--limit", "50"])
                .unwrap();
        match cli.command {
            Command::Agents {
                action:
                    AgentsAction::List {
                        active_minutes,
                        limit,
                    },
            } => {
                assert_eq!(active_minutes, Some(15));
                assert_eq!(limit, 50);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_agents_show() {
        let cli = Cli::try_parse_from(["rune", "agents", "show", "agent-abc"]).unwrap();
        match &cli.command {
            Command::Agents {
                action: AgentsAction::Show { id },
            } => assert_eq!(id, "agent-abc"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_agents_status() {
        let cli = Cli::try_parse_from(["rune", "agents", "status", "agent-abc"]).unwrap();
        match &cli.command {
            Command::Agents {
                action: AgentsAction::Status { id },
            } => assert_eq!(id, "agent-abc"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_agents_tree() {
        let cli = Cli::try_parse_from(["rune", "agents", "tree"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Agents {
                action: AgentsAction::Tree { limit: 500 }
            }
        ));
    }

    #[test]
    fn parse_agents_tree_with_limit() {
        let cli = Cli::try_parse_from(["rune", "agents", "tree", "--limit", "200"]).unwrap();
        match cli.command {
            Command::Agents {
                action: AgentsAction::Tree { limit },
            } => {
                assert_eq!(limit, 200);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_agents_templates() {
        let cli = Cli::try_parse_from(["rune", "agents", "templates"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Agents {
                action: AgentsAction::Templates { category: None }
            }
        ));
    }

    #[test]
    fn parse_agents_templates_with_category() {
        let cli = Cli::try_parse_from(["rune", "agents", "templates", "--category", "developer"])
            .unwrap();
        match &cli.command {
            Command::Agents {
                action: AgentsAction::Templates { category },
            } => assert_eq!(category.as_deref(), Some("developer")),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_agents_start_with_template() {
        let cli =
            Cli::try_parse_from(["rune", "agents", "start", "--template", "coding-agent"]).unwrap();
        match &cli.command {
            Command::Agents {
                action: AgentsAction::Start { template },
            } => assert_eq!(template, "coding-agent"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_agents_start_requires_template() {
        let result = Cli::try_parse_from(["rune", "agents", "start"]);
        assert!(result.is_err(), "start without --template should fail");
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
    fn parse_config_reload() {
        let cli = Cli::try_parse_from(["rune", "config", "reload"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Config {
                action: ConfigAction::Reload
            }
        ));
    }

    #[test]
    fn parse_config_diff() {
        let cli = Cli::try_parse_from(["rune", "config", "diff"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Config {
                action: ConfigAction::Diff
            }
        ));
    }

    #[test]
    fn parse_config_env() {
        let cli = Cli::try_parse_from(["rune", "config", "env"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Config {
                action: ConfigAction::Env
            }
        ));
    }

    #[test]
    fn parse_config_export_default() {
        let cli = Cli::try_parse_from(["rune", "config", "export"]).unwrap();
        match cli.command {
            Command::Config {
                action: ConfigAction::Export { output },
            } => {
                assert_eq!(output, "-");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_config_export_file() {
        let cli = Cli::try_parse_from(["rune", "config", "export", "/tmp/out.toml"]).unwrap();
        match cli.command {
            Command::Config {
                action: ConfigAction::Export { output },
            } => {
                assert_eq!(output, "/tmp/out.toml");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_process_list() {
        let cli = Cli::try_parse_from(["rune", "process", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Process {
                action: ProcessAction::List
            }
        ));
    }

    #[test]
    fn parse_process_get() {
        let cli = Cli::try_parse_from(["rune", "process", "get", "abc123"]).unwrap();
        match cli.command {
            Command::Process {
                action: ProcessAction::Get { id },
            } => assert_eq!(id, "abc123"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_process_kill() {
        let cli = Cli::try_parse_from(["rune", "process", "kill", "abc123"]).unwrap();
        match cli.command {
            Command::Process {
                action: ProcessAction::Kill { id },
            } => assert_eq!(id, "abc123"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_process_log() {
        let cli = Cli::try_parse_from(["rune", "process", "log", "abc123"]).unwrap();
        match cli.command {
            Command::Process {
                action: ProcessAction::Log { id },
            } => assert_eq!(id, "abc123"),
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
            let cli = Cli::try_parse_from(["rune", "completion", "generate", shell_name]).unwrap();
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

    #[test]
    fn parse_accept_risk_flag() {
        let cli = Cli::try_parse_from(["rune", "--yolo", "--accept-risk", "status"]).unwrap();
        assert!(cli.yolo);
        assert!(cli.accept_risk);
    }

    #[test]
    fn accept_risk_defaults_to_false() {
        let cli = Cli::try_parse_from(["rune", "--yolo", "status"]).unwrap();
        assert!(!cli.accept_risk);
    }

    #[test]
    fn accept_risk_works_without_bypass_flags() {
        let cli = Cli::try_parse_from(["rune", "--accept-risk", "status"]).unwrap();
        assert!(cli.accept_risk);
        assert!(!cli.yolo);
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
        assert!(Cli::try_parse_from(["rune", "message", "send", "--channel", "telegram"]).is_err());
        // Missing --channel
        assert!(Cli::try_parse_from(["rune", "message", "send", "--text", "hello"]).is_err());
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
        let cli = Cli::try_parse_from(["rune", "message", "search", "hello world"]).unwrap();
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
        let cli = Cli::try_parse_from(["rune", "message", "broadcast", "--text", "Hello everyone"])
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
                action: MessageAction::Broadcast { channels, .. },
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
        assert!(
            Cli::try_parse_from([
                "rune",
                "message",
                "edit",
                "--message-id",
                "msg-1",
                "--channel",
                "telegram",
            ])
            .is_err()
        );
        // Missing --channel
        assert!(
            Cli::try_parse_from([
                "rune",
                "message",
                "edit",
                "--message-id",
                "msg-1",
                "--text",
                "hello",
            ])
            .is_err()
        );
        // Missing --message-id
        assert!(
            Cli::try_parse_from([
                "rune",
                "message",
                "edit",
                "--channel",
                "telegram",
                "--text",
                "hello",
            ])
            .is_err()
        );
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
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-50");
                assert_eq!(channel.as_deref(), Some("telegram"));
                assert_eq!(session.as_deref(), Some("sess-3"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_unpin() {
        let cli =
            Cli::try_parse_from(["rune", "message", "unpin", "--message-id", "msg-77"]).unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Unpin {
                        message_id,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-77");
                assert!(channel.is_none());
                assert!(session.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_pin_requires_message_id() {
        assert!(Cli::try_parse_from(["rune", "message", "pin"]).is_err());
    }

    #[test]
    fn message_unpin_requires_message_id() {
        assert!(Cli::try_parse_from(["rune", "message", "unpin"]).is_err());
    }

    #[test]
    fn message_pin_rejects_unpin_flag() {
        assert!(
            Cli::try_parse_from([
                "rune",
                "message",
                "pin",
                "--message-id",
                "msg-77",
                "--unpin"
            ])
            .is_err()
        );
    }

    #[test]
    fn message_react_requires_message_id_and_emoji() {
        assert!(Cli::try_parse_from(["rune", "message", "react"]).is_err());
        assert!(
            Cli::try_parse_from(["rune", "message", "react", "--message-id", "msg-1",]).is_err()
        );
        assert!(Cli::try_parse_from(["rune", "message", "react", "--emoji", "👍",]).is_err());
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
        assert!(
            Cli::try_parse_from(["rune", "message", "delete", "--message-id", "msg-1",]).is_err()
        );
        assert!(
            Cli::try_parse_from(["rune", "message", "delete", "--channel", "telegram",]).is_err()
        );
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
        assert!(
            Cli::try_parse_from(["rune", "message", "read", "--message-id", "msg-1",]).is_err()
        );
        assert!(
            Cli::try_parse_from(["rune", "message", "read", "--channel", "telegram",]).is_err()
        );
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
        let cli =
            Cli::try_parse_from(["rune", "message", "thread", "list", "--thread-id", "thr-1"])
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
        assert!(Cli::try_parse_from(["rune", "message", "thread", "list"]).is_err());
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
        assert!(Cli::try_parse_from(["rune", "message", "thread", "reply"]).is_err());
        assert!(
            Cli::try_parse_from(["rune", "message", "thread", "reply", "--thread-id", "thr-1",])
                .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "rune",
                "message",
                "thread",
                "reply",
                "--thread-id",
                "thr-1",
                "--channel",
                "telegram",
            ])
            .is_err()
        );
    }

    // ── Message voice (#74) ────────────────────────────────────────

    #[test]
    fn parse_message_voice_send() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "voice",
            "send",
            "--text",
            "Hello from Rune",
            "--channel",
            "telegram",
            "--voice",
            "alloy",
            "--model",
            "tts-1-hd",
            "--session",
            "sess-5",
            "--output",
            "/tmp/voice.mp3",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Voice {
                        action:
                            MessageVoiceAction::Send {
                                text,
                                channel,
                                voice,
                                model,
                                session,
                                output,
                            },
                    },
            } => {
                assert_eq!(text, "Hello from Rune");
                assert_eq!(channel, "telegram");
                assert_eq!(voice.as_deref(), Some("alloy"));
                assert_eq!(model.as_deref(), Some("tts-1-hd"));
                assert_eq!(session.as_deref(), Some("sess-5"));
                assert_eq!(output.as_deref(), Some("/tmp/voice.mp3"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_voice_send_minimal() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "voice",
            "send",
            "--text",
            "Hey",
            "--channel",
            "discord",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Voice {
                        action:
                            MessageVoiceAction::Send {
                                text,
                                channel,
                                voice,
                                model,
                                session,
                                output,
                            },
                    },
            } => {
                assert_eq!(text, "Hey");
                assert_eq!(channel, "discord");
                assert!(voice.is_none());
                assert!(model.is_none());
                assert!(session.is_none());
                assert!(output.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_voice_send_requires_text_and_channel() {
        assert!(
            Cli::try_parse_from(["rune", "message", "voice", "send", "--text", "hello"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["rune", "message", "voice", "send", "--channel", "telegram",])
                .is_err()
        );
        assert!(Cli::try_parse_from(["rune", "message", "voice", "send"]).is_err());
    }

    #[test]
    fn parse_message_voice_status() {
        let cli = Cli::try_parse_from(["rune", "message", "voice", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Message {
                action: MessageAction::Voice {
                    action: MessageVoiceAction::Status
                }
            }
        ));
    }

    #[test]
    fn parse_message_ack_minimal() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "ack",
            "--message-id",
            "msg-42",
            "--channel",
            "telegram",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Ack {
                        message_id,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-42");
                assert_eq!(channel, "telegram");
                assert!(session.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_message_ack_with_session() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "ack",
            "--message-id",
            "msg-99",
            "--channel",
            "discord",
            "--session",
            "sess-abc",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::Ack {
                        message_id,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-99");
                assert_eq!(channel, "discord");
                assert_eq!(session.as_deref(), Some("sess-abc"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn message_ack_requires_message_id_and_channel() {
        assert!(Cli::try_parse_from(["rune", "message", "ack", "--message-id", "msg-1"]).is_err());
        assert!(Cli::try_parse_from(["rune", "message", "ack", "--channel", "slack"]).is_err());
        assert!(Cli::try_parse_from(["rune", "message", "ack"]).is_err());
    }

    // ── Message list-reactions (#74) ────────────────────────────────

    #[test]
    fn parse_message_list_reactions_minimal() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "list-reactions",
            "--message-id",
            "msg-42",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::ListReactions {
                        message_id,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-42");
                assert_eq!(channel, None);
                assert_eq!(session, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_message_list_reactions_with_channel_and_session() {
        let cli = Cli::try_parse_from([
            "rune",
            "message",
            "list-reactions",
            "--message-id",
            "msg-99",
            "--channel",
            "discord",
            "--session",
            "sess-7",
        ])
        .unwrap();
        match cli.command {
            Command::Message {
                action:
                    MessageAction::ListReactions {
                        message_id,
                        channel,
                        session,
                    },
            } => {
                assert_eq!(message_id, "msg-99");
                assert_eq!(channel.as_deref(), Some("discord"));
                assert_eq!(session.as_deref(), Some("sess-7"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn message_list_reactions_requires_message_id() {
        assert!(Cli::try_parse_from(["rune", "message", "list-reactions"]).is_err());
        assert!(
            Cli::try_parse_from(["rune", "message", "list-reactions", "--channel", "slack"])
                .is_err()
        );
    }

    // ── Security ──────────────────────────────────────────────────────

    #[test]
    fn parse_security_audit() {
        let cli = Cli::try_parse_from(["rune", "security", "audit"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Security {
                action: SecurityAction::Audit
            }
        ));
    }

    // ── Sandbox ───────────────────────────────────────────────────────

    #[test]
    fn parse_sandbox_list() {
        let cli = Cli::try_parse_from(["rune", "sandbox", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Sandbox {
                action: SandboxAction::List
            }
        ));
    }

    #[test]
    fn parse_sandbox_recreate() {
        let cli = Cli::try_parse_from(["rune", "sandbox", "recreate"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Sandbox {
                action: SandboxAction::Recreate
            }
        ));
    }

    #[test]
    fn parse_sandbox_explain() {
        let cli = Cli::try_parse_from(["rune", "sandbox", "explain"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Sandbox {
                action: SandboxAction::Explain
            }
        ));
    }

    // ── Secrets ───────────────────────────────────────────────────────

    #[test]
    fn parse_secrets_reload() {
        let cli = Cli::try_parse_from(["rune", "secrets", "reload"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Secrets {
                action: SecretsAction::Reload
            }
        ));
    }

    #[test]
    fn parse_secrets_audit() {
        let cli = Cli::try_parse_from(["rune", "secrets", "audit"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Secrets {
                action: SecretsAction::Audit
            }
        ));
    }

    #[test]
    fn parse_secrets_configure() {
        let cli = Cli::try_parse_from(["rune", "secrets", "configure"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Secrets {
                action: SecretsAction::Configure
            }
        ));
    }

    #[test]
    fn parse_secrets_apply() {
        let cli = Cli::try_parse_from(["rune", "secrets", "apply", "secrets.json"]).unwrap();
        match cli.command {
            Command::Secrets {
                action: SecretsAction::Apply { input },
            } => assert_eq!(input, "secrets.json"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_secrets_apply_stdin() {
        let cli = Cli::try_parse_from(["rune", "secrets", "apply", "-"]).unwrap();
        match cli.command {
            Command::Secrets {
                action: SecretsAction::Apply { input },
            } => assert_eq!(input, "-"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    // ── Configure ─────────────────────────────────────────────────────

    #[test]
    fn parse_configure() {
        let cli = Cli::try_parse_from(["rune", "configure"]).unwrap();
        assert!(matches!(cli.command, Command::Configure));
    }
}

#[cfg(test)]
mod subagent_cli_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_agents_spawn() {
        let cli = Cli::try_parse_from([
            "rune", "agents", "spawn", "--parent", "sess-1", "--task", "do stuff",
        ])
        .unwrap();
        match cli.command {
            Command::Agents {
                action:
                    AgentsAction::Spawn {
                        parent,
                        task,
                        mode,
                        policy,
                        provider,
                    },
            } => {
                assert_eq!(parent, "sess-1");
                assert_eq!(task, "do stuff");
                assert_eq!(mode, "default");
                assert_eq!(policy, "inherit");
                assert!(provider.is_none());
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_agents_steer() {
        let cli = Cli::try_parse_from(["rune", "agents", "steer", "child-1", "--message", "focus"])
            .unwrap();
        match cli.command {
            Command::Agents {
                action: AgentsAction::Steer { id, message },
            } => {
                assert_eq!(id, "child-1");
                assert_eq!(message, "focus");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_agents_kill() {
        let cli = Cli::try_parse_from(["rune", "agents", "kill", "child-1", "--reason", "timeout"])
            .unwrap();
        match cli.command {
            Command::Agents {
                action: AgentsAction::Kill { id, reason },
            } => {
                assert_eq!(id, "child-1");
                assert_eq!(reason.as_deref(), Some("timeout"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_agent_run() {
        let cli =
            Cli::try_parse_from(["rune", "agent", "run", "--session", "s1", "--message", "go"])
                .unwrap();
        match cli.command {
            Command::Agent {
                action:
                    AgentAction::Run {
                        session,
                        message,
                        max_turns,
                        wait,
                    },
            } => {
                assert_eq!(session, "s1");
                assert_eq!(message, "go");
                assert_eq!(max_turns, 1);
                assert!(wait);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_agent_result() {
        let cli =
            Cli::try_parse_from(["rune", "agent", "result", "--session", "s1", "--turn", "t1"])
                .unwrap();
        match cli.command {
            Command::Agent {
                action: AgentAction::Result { session, turn },
            } => {
                assert_eq!(session, "s1");
                assert_eq!(turn, "t1");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_acp_send() {
        let cli = Cli::try_parse_from([
            "rune",
            "acp",
            "send",
            "--from",
            "a",
            "--to",
            "b",
            "--payload",
            r#"{"x":1}"#,
        ])
        .unwrap();
        match cli.command {
            Command::Acp {
                action: AcpAction::Send { from, to, payload },
            } => {
                assert_eq!(from, "a");
                assert_eq!(to, "b");
                assert_eq!(payload, r#"{"x":1}"#);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_acp_inbox() {
        let cli = Cli::try_parse_from(["rune", "acp", "inbox", "--session", "a"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Acp {
                action: AcpAction::Inbox { .. }
            }
        ));
    }

    #[test]
    fn parse_acp_ack() {
        let cli =
            Cli::try_parse_from(["rune", "acp", "ack", "--message-id", "m1", "--session", "a"])
                .unwrap();
        match cli.command {
            Command::Acp {
                action:
                    AcpAction::Ack {
                        message_id,
                        session,
                    },
            } => {
                assert_eq!(message_id, "m1");
                assert_eq!(session, "a");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_init() {
        let cli = Cli::try_parse_from(["rune", "init"]).unwrap();
        assert!(matches!(cli.command, Command::Init { .. }));
    }

    #[test]
    fn parse_setup() {
        let cli = Cli::try_parse_from(["rune", "setup"]).unwrap();
        assert!(matches!(cli.command, Command::Setup { .. }));
    }

    #[test]
    fn parse_setup_no_open() {
        let cli = Cli::try_parse_from(["rune", "setup", "--no-open"]).unwrap();
        match cli.command {
            Command::Setup { open, no_open, .. } => {
                assert!(open);
                assert!(no_open);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_setup_print_url() {
        let cli = Cli::try_parse_from(["rune", "setup", "--print-url"]).unwrap();
        match cli.command {
            Command::Setup { print_url, .. } => {
                assert!(print_url);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_wizard_no_open() {
        let cli = Cli::try_parse_from(["rune", "wizard", "--no-open"]).unwrap();
        match cli.command {
            Command::Wizard { open, no_open, .. } => {
                assert!(open);
                assert!(no_open);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_wizard_print_url() {
        let cli = Cli::try_parse_from(["rune", "wizard", "--print-url"]).unwrap();
        match cli.command {
            Command::Wizard { print_url, .. } => {
                assert!(print_url);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_onboard_no_open() {
        let cli = Cli::try_parse_from(["rune", "onboard", "--no-open"]).unwrap();
        match cli.command {
            Command::Onboard { open, no_open, .. } => {
                assert!(open);
                assert!(no_open);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_onboard_print_url() {
        let cli = Cli::try_parse_from(["rune", "onboard", "--print-url"]).unwrap();
        match cli.command {
            Command::Onboard { print_url, .. } => {
                assert!(print_url);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_backup_create() {
        let cli = Cli::try_parse_from(["rune", "backup", "create"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Backup {
                action: BackupAction::Create { label: None }
            }
        ));
    }

    #[test]
    fn parse_backup_create_with_label() {
        let cli = Cli::try_parse_from(["rune", "backup", "create", "--label", "nightly"]).unwrap();
        match cli.command {
            Command::Backup {
                action: BackupAction::Create { label },
            } => assert_eq!(label.as_deref(), Some("nightly")),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_backup_list() {
        let cli = Cli::try_parse_from(["rune", "backup", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Backup {
                action: BackupAction::List
            }
        ));
    }

    #[test]
    fn parse_backup_restore() {
        let cli =
            Cli::try_parse_from(["rune", "backup", "restore", "bk-001", "--confirm"]).unwrap();
        match cli.command {
            Command::Backup {
                action: BackupAction::Restore { id, confirm },
            } => {
                assert_eq!(id, "bk-001");
                assert!(confirm);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_onboard() {
        let cli = Cli::try_parse_from(["rune", "onboard", "--path", "~/.rune"]).unwrap();
        match cli.command {
            Command::Onboard { path, .. } => assert_eq!(path, "~/.rune"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_projects_add() {
        let cli = Cli::try_parse_from([
            "rune",
            "projects",
            "add",
            "~/Development/phoenix-iot",
            "--name",
            "phoenix-iot",
            "--default-branch",
            "develop",
            "--default-model",
            "gpt-5.4",
        ])
        .unwrap();
        match cli.command {
            Command::Projects {
                action:
                    ProjectsAction::Add(ProjectAddArgs {
                        path,
                        name,
                        default_branch,
                        default_model,
                        ..
                    }),
            } => {
                assert_eq!(path, "~/Development/phoenix-iot");
                assert_eq!(name.as_deref(), Some("phoenix-iot"));
                assert_eq!(default_branch, "develop");
                assert_eq!(default_model.as_deref(), Some("gpt-5.4"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_projects_list() {
        let cli = Cli::try_parse_from(["rune", "projects", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Projects {
                action: ProjectsAction::List
            }
        ));
    }

    #[test]
    fn parse_projects_switch() {
        let cli = Cli::try_parse_from(["rune", "projects", "switch", "phoenix-iot"]).unwrap();
        match cli.command {
            Command::Projects {
                action: ProjectsAction::Switch { name },
            } => assert_eq!(name, "phoenix-iot"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_update_check() {
        let cli = Cli::try_parse_from(["rune", "update", "check"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Update {
                action: UpdateAction::Check
            }
        ));
    }

    #[test]
    fn parse_update_apply() {
        let cli = Cli::try_parse_from(["rune", "update", "apply"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Update {
                action: UpdateAction::Apply {
                    version: None,
                    repo,
                    binary_path: None,
                }
            } if repo == "ghostrider0470/rune"
        ));
    }

    #[test]
    fn parse_update_status() {
        let cli = Cli::try_parse_from(["rune", "update", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Update {
                action: UpdateAction::Status
            }
        ));
    }

    #[test]
    fn parse_update_install_script() {
        let cli = Cli::try_parse_from(["rune", "update", "install-script"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Update {
                action: UpdateAction::InstallScript { .. }
            }
        ));
    }

    #[test]
    fn parse_update_wizard() {
        let cli = Cli::try_parse_from(["rune", "update", "wizard"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Update {
                action: UpdateAction::Wizard { .. }
            }
        ));
    }

    #[test]
    fn parse_reset_with_confirm() {
        let cli = Cli::try_parse_from(["rune", "reset", "--confirm"]).unwrap();
        match cli.command {
            Command::Reset { confirm } => assert!(confirm),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_reset_no_confirm() {
        let cli = Cli::try_parse_from(["rune", "reset"]).unwrap();
        match cli.command {
            Command::Reset { confirm } => assert!(!confirm),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_mail_read() {
        let cli = Cli::try_parse_from(["rune", "ms365", "mail", "read", "--id", "abc123"]).unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Mail {
                        action: Ms365MailAction::Read { id },
                    },
            } => {
                assert_eq!(id, "abc123");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_calendar_read() {
        let cli =
            Cli::try_parse_from(["rune", "ms365", "calendar", "read", "--id", "evt456"]).unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Calendar {
                        action: Ms365CalendarAction::Read { id },
                    },
            } => {
                assert_eq!(id, "evt456");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_calendar_update() {
        let cli = Cli::try_parse_from([
            "rune",
            "ms365",
            "calendar",
            "update",
            "--id",
            "evt456",
            "--subject",
            "Updated",
            "--start",
            "2026-03-25T11:00:00Z",
            "--end",
            "2026-03-25T12:00:00Z",
            "--attendees",
            "a@example.com,b@example.com",
            "--location",
            "Teams",
            "--body",
            "Agenda",
        ])
        .unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Calendar {
                        action:
                            Ms365CalendarAction::Update {
                                id,
                                subject,
                                start,
                                end,
                                attendees,
                                location,
                                body,
                            },
                    },
            } => {
                assert_eq!(id, "evt456");
                assert_eq!(subject.as_deref(), Some("Updated"));
                assert_eq!(start.as_deref(), Some("2026-03-25T11:00:00Z"));
                assert_eq!(end.as_deref(), Some("2026-03-25T12:00:00Z"));
                assert_eq!(attendees.as_deref(), Some("a@example.com,b@example.com"));
                assert_eq!(location.as_deref(), Some("Teams"));
                assert_eq!(body.as_deref(), Some("Agenda"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_auth_status() {
        let cli = Cli::try_parse_from(["rune", "ms365", "auth", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Ms365 {
                action: Ms365Action::Auth {
                    action: Ms365AuthAction::Status
                }
            }
        ));
    }

    #[test]
    fn parse_ms365_planner_plans() {
        let cli = Cli::try_parse_from(["rune", "ms365", "planner", "plans"]).unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Planner {
                        action: Ms365PlannerAction::Plans { limit },
                    },
            } => {
                assert_eq!(limit, 25);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_planner_tasks() {
        let cli =
            Cli::try_parse_from(["rune", "ms365", "planner", "tasks", "--plan-id", "p1"]).unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Planner {
                        action: Ms365PlannerAction::Tasks { plan_id, limit },
                    },
            } => {
                assert_eq!(plan_id, "p1");
                assert_eq!(limit, 50);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_planner_task_read() {
        let cli =
            Cli::try_parse_from(["rune", "ms365", "planner", "task-read", "--id", "t1"]).unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Planner {
                        action: Ms365PlannerAction::TaskRead { id },
                    },
            } => {
                assert_eq!(id, "t1");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_planner_create() {
        let cli = Cli::try_parse_from([
            "rune",
            "ms365",
            "planner",
            "create",
            "--plan-id",
            "plan-1",
            "--title",
            "Write summary",
            "--bucket-id",
            "bucket-1",
            "--due-date",
            "2026-03-25T12:00:00Z",
            "--description",
            "Prepare status update",
        ])
        .unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Planner {
                        action:
                            Ms365PlannerAction::Create {
                                plan_id,
                                title,
                                bucket_id,
                                due_date,
                                description,
                            },
                    },
            } => {
                assert_eq!(plan_id, "plan-1");
                assert_eq!(title, "Write summary");
                assert_eq!(bucket_id.as_deref(), Some("bucket-1"));
                assert_eq!(due_date.as_deref(), Some("2026-03-25T12:00:00Z"));
                assert_eq!(description.as_deref(), Some("Prepare status update"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_planner_update() {
        let cli = Cli::try_parse_from([
            "rune",
            "ms365",
            "planner",
            "update",
            "--id",
            "task-1",
            "--title",
            "Updated title",
            "--percent-complete",
            "60",
        ])
        .unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Planner {
                        action:
                            Ms365PlannerAction::Update {
                                id,
                                title,
                                bucket_id,
                                due_date,
                                description,
                                percent_complete,
                            },
                    },
            } => {
                assert_eq!(id, "task-1");
                assert_eq!(title.as_deref(), Some("Updated title"));
                assert!(bucket_id.is_none());
                assert!(due_date.is_none());
                assert!(description.is_none());
                assert_eq!(percent_complete, Some(60));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_planner_update_requires_change() {
        let err = Cli::try_parse_from(["rune", "ms365", "planner", "update", "--id", "task-1"])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn parse_ms365_planner_complete() {
        let cli = Cli::try_parse_from(["rune", "ms365", "planner", "complete", "--id", "task-1"])
            .unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Planner {
                        action: Ms365PlannerAction::Complete { id },
                    },
            } => {
                assert_eq!(id, "task-1");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_mail_folders() {
        let cli = Cli::try_parse_from(["rune", "ms365", "mail", "folders"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Ms365 {
                action: Ms365Action::Mail {
                    action: Ms365MailAction::Folders
                }
            }
        ));
    }

    #[test]
    fn parse_ms365_todo_lists() {
        let cli = Cli::try_parse_from(["rune", "ms365", "todo", "lists"]).unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Todo {
                        action: Ms365TodoAction::Lists { limit },
                    },
            } => {
                assert_eq!(limit, 25);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_todo_tasks() {
        let cli =
            Cli::try_parse_from(["rune", "ms365", "todo", "tasks", "--list-id", "lst1"]).unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Todo {
                        action: Ms365TodoAction::Tasks { list_id, limit },
                    },
            } => {
                assert_eq!(list_id, "lst1");
                assert_eq!(limit, 50);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_todo_task_read() {
        let cli = Cli::try_parse_from([
            "rune",
            "ms365",
            "todo",
            "task-read",
            "--list-id",
            "lst1",
            "--id",
            "task1",
        ])
        .unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Todo {
                        action: Ms365TodoAction::TaskRead { list_id, id },
                    },
            } => {
                assert_eq!(list_id, "lst1");
                assert_eq!(id, "task1");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_todo_create() {
        let cli = Cli::try_parse_from([
            "rune",
            "ms365",
            "todo",
            "create",
            "--list-id",
            "lst1",
            "--title",
            "Draft operator note",
            "--due-date",
            "2026-03-25T12:00:00Z",
            "--importance",
            "high",
            "--body",
            "Prepare the follow-on summary.",
        ])
        .unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Todo {
                        action:
                            Ms365TodoAction::Create {
                                list_id,
                                title,
                                due_date,
                                importance,
                                body,
                            },
                    },
            } => {
                assert_eq!(list_id, "lst1");
                assert_eq!(title, "Draft operator note");
                assert_eq!(due_date.as_deref(), Some("2026-03-25T12:00:00Z"));
                assert_eq!(importance.as_deref(), Some("high"));
                assert_eq!(body.as_deref(), Some("Prepare the follow-on summary."));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_todo_update() {
        let cli = Cli::try_parse_from([
            "rune",
            "ms365",
            "todo",
            "update",
            "--list-id",
            "lst1",
            "--id",
            "task1",
            "--status",
            "inProgress",
            "--importance",
            "high",
        ])
        .unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Todo {
                        action:
                            Ms365TodoAction::Update {
                                list_id,
                                id,
                                title,
                                status,
                                importance,
                                due_date,
                                body,
                            },
                    },
            } => {
                assert_eq!(list_id, "lst1");
                assert_eq!(id, "task1");
                assert!(title.is_none());
                assert_eq!(status.as_deref(), Some("inProgress"));
                assert_eq!(importance.as_deref(), Some("high"));
                assert!(due_date.is_none());
                assert!(body.is_none());
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_todo_update_requires_change() {
        let err = Cli::try_parse_from([
            "rune",
            "ms365",
            "todo",
            "update",
            "--list-id",
            "lst1",
            "--id",
            "task1",
        ])
        .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn parse_ms365_todo_complete() {
        let cli = Cli::try_parse_from([
            "rune",
            "ms365",
            "todo",
            "complete",
            "--list-id",
            "lst1",
            "--id",
            "task1",
        ])
        .unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Todo {
                        action: Ms365TodoAction::Complete { list_id, id },
                    },
            } => {
                assert_eq!(list_id, "lst1");
                assert_eq!(id, "task1");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_teams_list() {
        let cli = Cli::try_parse_from(["rune", "ms365", "teams", "list"]).unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Teams {
                        action: Ms365TeamsAction::List { limit },
                    },
            } => {
                assert_eq!(limit, 25);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_teams_channels() {
        let cli =
            Cli::try_parse_from(["rune", "ms365", "teams", "channels", "--team-id", "t1"]).unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Teams {
                        action: Ms365TeamsAction::Channels { team_id, limit },
                    },
            } => {
                assert_eq!(team_id, "t1");
                assert_eq!(limit, 50);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_teams_channel_read() {
        let cli = Cli::try_parse_from([
            "rune",
            "ms365",
            "teams",
            "channel-read",
            "--team-id",
            "t1",
            "--id",
            "ch1",
        ])
        .unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Teams {
                        action: Ms365TeamsAction::ChannelRead { team_id, id },
                    },
            } => {
                assert_eq!(team_id, "t1");
                assert_eq!(id, "ch1");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_ms365_teams_messages() {
        let cli = Cli::try_parse_from([
            "rune",
            "ms365",
            "teams",
            "messages",
            "--team-id",
            "t1",
            "--channel-id",
            "ch1",
        ])
        .unwrap();
        match cli.command {
            Command::Ms365 {
                action:
                    Ms365Action::Teams {
                        action:
                            Ms365TeamsAction::Messages {
                                team_id,
                                channel_id,
                                limit,
                            },
                    },
            } => {
                assert_eq!(team_id, "t1");
                assert_eq!(channel_id, "ch1");
                assert_eq!(limit, 25);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}

#[test]
fn parse_service_install_no_bootstrap() {
    let cli = Cli::try_parse_from([
        "rune",
        "service",
        "install",
        "--target",
        "systemd",
        "--no-bootstrap",
    ])
    .unwrap();

    match cli.command {
        Command::Service {
            action: ServiceAction::Install { no_bootstrap, .. },
        } => assert!(no_bootstrap),
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_update_self_update_alias() {
    let cli = Cli::try_parse_from(["rune", "self-update", "check"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::Update {
            action: UpdateAction::Check
        }
    ));
}
