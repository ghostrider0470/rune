//! Output formatting: JSON and human-readable modes.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Output format selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

impl OutputFormat {
    /// Create from the `--json` flag value.
    #[must_use]
    pub fn from_json_flag(json: bool) -> Self {
        if json { Self::Json } else { Self::Human }
    }
}

/// Render a serializable value according to the chosen format.
///
/// For JSON mode, outputs compact JSON to stdout.
/// For human mode, uses the `Display` implementation.
pub fn render<T: Serialize + fmt::Display>(value: &T, format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(value)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}")),
        OutputFormat::Human => value.to_string(),
    }
}

/// A simple status response from the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub status: String,
    pub version: Option<String>,
    pub uptime_seconds: Option<u64>,
}

impl fmt::Display for StatusResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Status: {}", self.status)?;
        if let Some(ref v) = self.version {
            write!(f, "\nVersion: {v}")?;
        }
        if let Some(u) = self.uptime_seconds {
            write!(f, "\nUptime: {u}s")?;
        }
        Ok(())
    }
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub healthy: bool,
    pub message: String,
}

impl fmt::Display for HealthResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.healthy { "✓" } else { "✗" };
        write!(f, "{icon} {}", self.message)
    }
}

/// A single diagnostic check result used by `rune doctor`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

impl fmt::Display for DoctorCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.passed { "✓" } else { "✗" };
        write!(f, "  {icon} {}: {}", self.name, self.detail)
    }
}

/// Full doctor report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

impl fmt::Display for DoctorReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Doctor Report")?;
        writeln!(f, "─────────────")?;
        for check in &self.checks {
            writeln!(f, "{check}")?;
        }
        let passed = self.checks.iter().filter(|c| c.passed).count();
        let total = self.checks.len();
        write!(f, "\n{passed}/{total} checks passed")
    }
}

/// Session summary for list output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub status: String,
    pub channel: Option<String>,
    pub created_at: Option<String>,
}

impl fmt::Display for SessionSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]", self.id, self.status)?;
        if let Some(ref ch) = self.channel {
            write!(f, " ({ch})")?;
        }
        Ok(())
    }
}

/// Session list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionSummary>,
}

impl fmt::Display for SessionListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.sessions.is_empty() {
            return write!(f, "No active sessions.");
        }
        for s in &self.sessions {
            writeln!(f, "  {s}")?;
        }
        Ok(())
    }
}

/// Detailed session view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetailResponse {
    pub id: String,
    pub status: String,
    pub channel: Option<String>,
    pub created_at: Option<String>,
    pub turn_count: Option<u32>,
}

impl fmt::Display for SessionDetailResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Session: {}", self.id)?;
        writeln!(f, "  Status:  {}", self.status)?;
        if let Some(ref ch) = self.channel {
            writeln!(f, "  Channel: {ch}")?;
        }
        if let Some(ref t) = self.created_at {
            writeln!(f, "  Created: {t}")?;
        }
        if let Some(n) = self.turn_count {
            write!(f, "  Turns:   {n}")?;
        }
        Ok(())
    }
}

/// Per-channel configuration/availability detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelDetail {
    pub name: String,
    pub enabled: bool,
    pub configured: bool,
    pub status: String,
    pub capabilities: Vec<String>,
    pub notes: Option<String>,
}

impl fmt::Display for ChannelDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{} | enabled={} configured={}]",
            self.name, self.status, self.enabled, self.configured
        )
    }
}

/// Response for `channels list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelListResponse {
    pub channels: Vec<ChannelDetail>,
}

impl fmt::Display for ChannelListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.channels.is_empty() {
            return write!(f, "No channels configured.");
        }
        for channel in &self.channels {
            writeln!(f, "  {channel}")?;
        }
        Ok(())
    }
}

/// Response for `channels status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStatusResponse {
    pub total: usize,
    pub enabled: usize,
    pub configured: usize,
    pub ready: usize,
    pub channels: Vec<ChannelDetail>,
}

impl fmt::Display for ChannelStatusResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Channels")?;
        writeln!(f, "  Total:      {}", self.total)?;
        writeln!(f, "  Enabled:    {}", self.enabled)?;
        writeln!(f, "  Configured: {}", self.configured)?;
        writeln!(f, "  Ready:      {}", self.ready)?;
        for channel in &self.channels {
            writeln!(f, "  - {channel}")?;
            if !channel.capabilities.is_empty() {
                writeln!(f, "    capabilities: {}", channel.capabilities.join(", "))?;
            }
            if let Some(notes) = &channel.notes {
                writeln!(f, "    note: {notes}")?;
            }
        }
        Ok(())
    }
}

/// Response for `channels capabilities`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCapabilitiesResponse {
    pub channels: Vec<ChannelDetail>,
}

impl fmt::Display for ChannelCapabilitiesResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.channels.is_empty() {
            return write!(f, "No channel capabilities available.");
        }
        for channel in &self.channels {
            writeln!(f, "{}:", channel.name)?;
            for capability in &channel.capabilities {
                writeln!(f, "  - {capability}")?;
            }
        }
        Ok(())
    }
}

/// Resolution result for a channel name/alias.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelResolveResponse {
    pub target: String,
    pub matched: bool,
    pub channel: Option<ChannelDetail>,
    pub note: Option<String>,
}

impl fmt::Display for ChannelResolveResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(channel) = &self.channel {
            writeln!(f, "Resolved `{}` -> {}", self.target, channel.name)?;
            writeln!(f, "  status: {}", channel.status)?;
            writeln!(f, "  enabled: {}", channel.enabled)?;
            writeln!(f, "  configured: {}", channel.configured)?;
            if !channel.capabilities.is_empty() {
                writeln!(f, "  capabilities: {}", channel.capabilities.join(", "))?;
            }
            if let Some(notes) = &channel.notes {
                writeln!(f, "  note: {notes}")?;
            }
            Ok(())
        } else {
            write!(f, "No configured channel matched `{}`", self.target)?;
            if let Some(note) = &self.note {
                write!(f, "\nNote: {note}")?;
            }
            Ok(())
        }
    }
}

/// Single local log file summary for channel diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelLogFile {
    pub path: String,
    pub modified_at: Option<String>,
    pub size_bytes: u64,
}

impl fmt::Display for ChannelLogFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({} bytes", self.path, self.size_bytes)?;
        if let Some(modified_at) = &self.modified_at {
            write!(f, ", modified {modified_at}")?;
        }
        write!(f, ")")
    }
}

/// Response for `channels logs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelLogsResponse {
    pub logs_dir: String,
    pub filter: Option<String>,
    pub files: Vec<ChannelLogFile>,
    pub note: Option<String>,
}

impl fmt::Display for ChannelLogsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Channel logs")?;
        writeln!(f, "  Logs dir: {}", self.logs_dir)?;
        writeln!(
            f,
            "  Filter:   {}",
            self.filter.as_deref().unwrap_or("(all channels)")
        )?;
        if self.files.is_empty() {
            writeln!(f, "  Files:    none")?;
        } else {
            writeln!(f, "  Files:")?;
            for file in &self.files {
                writeln!(f, "    - {file}")?;
            }
        }
        if let Some(note) = &self.note {
            write!(f, "  Note:     {note}")?;
        }
        Ok(())
    }
}

/// Per-provider model configuration detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProviderDetail {
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub default_model: Option<String>,
    pub model_alias: Option<String>,
    pub deployment_name: Option<String>,
    pub api_version: Option<String>,
    pub credential_source: String,
    pub credentials_ready: bool,
    pub notes: Option<String>,
}

impl fmt::Display for ModelProviderDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{}] alias={} creds={} default={}",
            self.name,
            self.kind,
            self.model_alias.as_deref().unwrap_or("-"),
            if self.credentials_ready {
                "ready"
            } else {
                "missing"
            },
            self.default_model.as_deref().unwrap_or("-")
        )
    }
}

/// Response for `models list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelListResponse {
    pub default_model: Option<String>,
    pub providers: Vec<ModelProviderDetail>,
}

impl fmt::Display for ModelListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(default_model) = &self.default_model {
            writeln!(f, "Default model: {default_model}")?;
        }
        if self.providers.is_empty() {
            return write!(f, "No model providers configured.");
        }
        for provider in &self.providers {
            writeln!(f, "  {provider}")?;
            writeln!(f, "    endpoint: {}", provider.base_url)?;
            if let Some(deployment) = &provider.deployment_name {
                writeln!(f, "    deployment: {deployment}")?;
            }
            if let Some(version) = &provider.api_version {
                writeln!(f, "    api_version: {version}")?;
            }
            if let Some(notes) = &provider.notes {
                writeln!(f, "    note: {notes}")?;
            }
        }
        Ok(())
    }
}

/// Response for `models status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStatusResponse {
    pub default_model: Option<String>,
    pub total: usize,
    pub credentials_ready: usize,
    pub providers: Vec<ModelProviderDetail>,
}

/// Single configured model alias mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAliasDetail {
    pub alias: String,
    pub provider: String,
    pub target_model: Option<String>,
    pub provider_kind: String,
    pub base_url: String,
    pub deployment_name: Option<String>,
    pub api_version: Option<String>,
    pub credentials_ready: bool,
    pub note: Option<String>,
}

impl fmt::Display for ModelAliasDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} -> {}{} [{}] creds={}",
            self.alias,
            self.provider,
            self.target_model
                .as_deref()
                .map(|model| format!("/{model}"))
                .unwrap_or_default(),
            self.provider_kind,
            if self.credentials_ready {
                "ready"
            } else {
                "missing"
            }
        )
    }
}

/// Response for `models aliases`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAliasesResponse {
    pub aliases: Vec<ModelAliasDetail>,
}

impl fmt::Display for ModelAliasesResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.aliases.is_empty() {
            return write!(f, "No model aliases configured.");
        }
        for alias in &self.aliases {
            writeln!(f, "  {alias}")?;
            writeln!(f, "    endpoint: {}", alias.base_url)?;
            if let Some(deployment) = &alias.deployment_name {
                writeln!(f, "    deployment: {deployment}")?;
            }
            if let Some(version) = &alias.api_version {
                writeln!(f, "    api_version: {version}")?;
            }
            if let Some(note) = &alias.note {
                writeln!(f, "    note: {note}")?;
            }
        }
        Ok(())
    }
}

/// Result of updating local model routing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSetResponse {
    pub changed: bool,
    pub config_path: String,
    pub previous_model: Option<String>,
    pub default_model: String,
    pub note: String,
}

impl fmt::Display for ModelSetResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Default model set to {} in {}",
            self.default_model, self.config_path
        )?;
        if let Some(previous) = &self.previous_model {
            write!(f, "\nPrevious: {previous}")?;
        }
        if !self.note.is_empty() {
            write!(f, "\nNote: {}", self.note)?;
        }
        Ok(())
    }
}

impl fmt::Display for ModelStatusResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Models")?;
        writeln!(f, "  Total providers:      {}", self.total)?;
        writeln!(f, "  Credentials ready:   {}", self.credentials_ready)?;
        writeln!(
            f,
            "  Default model:       {}",
            self.default_model.as_deref().unwrap_or("(not configured)")
        )?;
        for provider in &self.providers {
            writeln!(f, "  - {provider}")?;
            writeln!(f, "    credential_source: {}", provider.credential_source)?;
            if let Some(notes) = &provider.notes {
                writeln!(f, "    note: {notes}")?;
            }
        }
        Ok(())
    }
}

/// Summary status for workspace memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStatusResponse {
    pub workspace_root: String,
    pub memory_dir: String,
    pub semantic_search_enabled: bool,
    pub long_term_exists: bool,
    pub daily_file_count: usize,
    pub latest_daily_file: Option<String>,
}

impl fmt::Display for MemoryStatusResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Memory")?;
        writeln!(f, "  Workspace:               {}", self.workspace_root)?;
        writeln!(f, "  Memory dir:              {}", self.memory_dir)?;
        writeln!(
            f,
            "  Semantic search enabled: {}",
            self.semantic_search_enabled
        )?;
        writeln!(f, "  MEMORY.md present:       {}", self.long_term_exists)?;
        writeln!(f, "  Daily files:             {}", self.daily_file_count)?;
        write!(
            f,
            "  Latest daily file:       {}",
            self.latest_daily_file.as_deref().unwrap_or("(none)")
        )
    }
}

/// A single memory search hit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchHit {
    pub path: String,
    pub line: usize,
    pub score: f64,
    pub snippet: String,
}

impl fmt::Display for MemorySearchHit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}:{} (score {:.2})", self.path, self.line, self.score)?;
        write!(f, "{}", self.snippet)
    }
}

/// Search results for workspace memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchResponse {
    pub query: String,
    pub total: usize,
    pub hits: Vec<MemorySearchHit>,
}

impl fmt::Display for MemorySearchResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.hits.is_empty() {
            return write!(f, "No results found for query: {}", self.query);
        }
        writeln!(f, "Memory search: {} ({} hits)", self.query, self.total)?;
        for (idx, hit) in self.hits.iter().enumerate() {
            if idx > 0 {
                writeln!(f, "\n---")?;
            }
            writeln!(f, "{hit}")?;
        }
        Ok(())
    }
}

/// Bounded snippet read from a memory file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGetResponse {
    pub path: String,
    pub from: usize,
    pub lines: usize,
    pub content: String,
}

impl fmt::Display for MemoryGetResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} (from line {}, {} lines)",
            self.path, self.from, self.lines
        )?;
        write!(f, "{}", self.content)
    }
}

/// Config validation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

impl fmt::Display for ConfigValidationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.valid {
            write!(f, "✓ Configuration is valid.")
        } else {
            writeln!(f, "✗ Configuration errors:")?;
            for e in &self.errors {
                writeln!(f, "  - {e}")?;
            }
            Ok(())
        }
    }
}

/// Location of the local config file used for mutations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFileResponse {
    pub path: String,
    pub exists: bool,
}

impl fmt::Display for ConfigFileResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Config file")?;
        writeln!(f, "  Path:   {}", self.path)?;
        write!(f, "  Exists: {}", self.exists)
    }
}

/// Result of reading a config key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigGetResponse {
    pub key: String,
    pub found: bool,
    pub value: Option<serde_json::Value>,
    pub source_path: String,
}

impl fmt::Display for ConfigGetResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Config key: {}", self.key)?;
        writeln!(f, "  File:   {}", self.source_path)?;
        writeln!(f, "  Found:  {}", self.found)?;
        if let Some(value) = &self.value {
            write!(
                f,
                "  Value:  {}",
                serde_json::to_string_pretty(value).unwrap_or_default()
            )
        } else {
            write!(f, "  Value:  <unset>")
        }
    }
}

/// Result of mutating a config key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigMutationResponse {
    pub key: String,
    pub changed: bool,
    pub action: String,
    pub source_path: String,
    pub value: Option<serde_json::Value>,
    pub note: Option<String>,
}

impl fmt::Display for ConfigMutationResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Config {}", self.action)?;
        writeln!(f, "  Key:     {}", self.key)?;
        writeln!(f, "  File:    {}", self.source_path)?;
        writeln!(f, "  Changed: {}", self.changed)?;
        if let Some(value) = &self.value {
            writeln!(
                f,
                "  Value:   {}",
                serde_json::to_string_pretty(value).unwrap_or_default()
            )?;
        }
        if let Some(note) = &self.note {
            write!(f, "  Note:    {note}")?;
        }
        Ok(())
    }
}

/// Simple action acknowledgment (gateway start/stop).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    pub message: String,
}

/// HEARTBEAT.md presence/metadata response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatPresenceResponse {
    pub workspace_root: String,
    pub path: String,
    pub present: bool,
    pub modified_at: Option<String>,
    pub size_bytes: Option<u64>,
    pub note: Option<String>,
}

impl fmt::Display for HeartbeatPresenceResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Heartbeat")?;
        writeln!(f, "  Workspace: {}", self.workspace_root)?;
        writeln!(f, "  Path:      {}", self.path)?;
        writeln!(f, "  Present:   {}", self.present)?;
        if let Some(modified_at) = &self.modified_at {
            writeln!(f, "  Modified:  {modified_at}")?;
        }
        if let Some(size_bytes) = self.size_bytes {
            writeln!(f, "  Size:      {size_bytes} bytes")?;
        }
        if let Some(note) = &self.note {
            write!(f, "  Note:      {note}")?;
        }
        Ok(())
    }
}

/// Runtime heartbeat runner state from the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatStatusResponse {
    pub enabled: bool,
    pub interval_secs: u64,
    pub last_run_at: Option<String>,
    pub run_count: u64,
    pub suppressed_count: u64,
}

impl fmt::Display for HeartbeatStatusResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Heartbeat Runner")?;
        writeln!(f, "  Enabled:          {}", self.enabled)?;
        writeln!(f, "  Interval:         {}s", self.interval_secs)?;
        writeln!(f, "  Runs:             {}", self.run_count)?;
        writeln!(f, "  Suppressed no-op: {}", self.suppressed_count)?;
        if let Some(last_run_at) = &self.last_run_at {
            write!(f, "  Last run:         {last_run_at}")?;
        }
        Ok(())
    }
}

impl fmt::Display for ActionResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        write!(f, "{icon} {}", self.message)
    }
}

/// Scheduler status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronStatusResponse {
    pub total_jobs: usize,
    pub enabled_jobs: usize,
    pub due_jobs: usize,
}

impl fmt::Display for CronStatusResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Cron scheduler")?;
        writeln!(f, "  Total jobs:   {}", self.total_jobs)?;
        writeln!(f, "  Enabled jobs: {}", self.enabled_jobs)?;
        write!(f, "  Due jobs:     {}", self.due_jobs)
    }
}

/// A cron job summary/detail item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobSummary {
    pub id: String,
    pub name: Option<String>,
    pub enabled: bool,
    pub session_target: String,
    pub schedule_kind: String,
    pub next_run_at: Option<String>,
    pub run_count: u64,
}

impl fmt::Display for CronJobSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{}] {} target={} runs={}",
            self.id,
            if self.enabled { "enabled" } else { "disabled" },
            self.name.as_deref().unwrap_or("(unnamed)"),
            self.session_target,
            self.run_count
        )?;
        if let Some(next) = &self.next_run_at {
            write!(f, " next={next}")?;
        }
        Ok(())
    }
}

/// Cron list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronListResponse {
    pub jobs: Vec<CronJobSummary>,
}

impl fmt::Display for CronListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.jobs.is_empty() {
            return write!(f, "No cron jobs.");
        }
        for job in &self.jobs {
            writeln!(f, "  {job}")?;
        }
        Ok(())
    }
}

/// Cron run history item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronRunSummary {
    pub job_id: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub output: Option<String>,
}

impl fmt::Display for CronRunSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.status, self.started_at)?;
        if let Some(output) = &self.output {
            write!(f, " — {output}")?;
        }
        Ok(())
    }
}

/// Cron run history response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronRunsResponse {
    pub runs: Vec<CronRunSummary>,
}

impl fmt::Display for CronRunsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.runs.is_empty() {
            return write!(f, "No recorded runs.");
        }
        for run in &self.runs {
            writeln!(f, "  {run}")?;
        }
        Ok(())
    }
}

/// One-shot reminder detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderSummary {
    pub id: String,
    pub message: String,
    pub target: String,
    pub fire_at: String,
    pub delivered: bool,
    pub created_at: String,
    pub delivered_at: Option<String>,
}

impl fmt::Display for ReminderSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{}] {} -> {}",
            self.id,
            if self.delivered { "delivered" } else { "pending" },
            self.target,
            self.message
        )?;
        write!(f, " at {}", self.fire_at)
    }
}

/// Response for `reminders list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemindersListResponse {
    pub reminders: Vec<ReminderSummary>,
}

impl fmt::Display for RemindersListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.reminders.is_empty() {
            return write!(f, "No reminders.");
        }
        for reminder in &self.reminders {
            writeln!(f, "  {reminder}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_format_from_flag() {
        assert_eq!(OutputFormat::from_json_flag(true), OutputFormat::Json);
        assert_eq!(OutputFormat::from_json_flag(false), OutputFormat::Human);
    }

    #[test]
    fn render_status_human() {
        let s = StatusResponse {
            status: "running".into(),
            version: Some("0.1.0".into()),
            uptime_seconds: Some(120),
        };
        let out = render(&s, OutputFormat::Human);
        assert!(out.contains("Status: running"));
        assert!(out.contains("Version: 0.1.0"));
        assert!(out.contains("Uptime: 120s"));
    }

    #[test]
    fn render_status_json() {
        let s = StatusResponse {
            status: "running".into(),
            version: None,
            uptime_seconds: None,
        };
        let out = render(&s, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["status"], "running");
    }

    #[test]
    fn render_health_human() {
        let h = HealthResponse {
            healthy: true,
            message: "All systems go".into(),
        };
        assert_eq!(render(&h, OutputFormat::Human), "✓ All systems go");
    }

    #[test]
    fn render_health_unhealthy() {
        let h = HealthResponse {
            healthy: false,
            message: "DB unreachable".into(),
        };
        let out = render(&h, OutputFormat::Human);
        assert!(out.starts_with('✗'));
    }

    #[test]
    fn render_doctor_report() {
        let r = DoctorReport {
            checks: vec![
                DoctorCheck {
                    name: "config".into(),
                    passed: true,
                    detail: "valid".into(),
                },
                DoctorCheck {
                    name: "db".into(),
                    passed: false,
                    detail: "unreachable".into(),
                },
            ],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("1/2 checks passed"));
    }

    #[test]
    fn render_config_validation_valid() {
        let v = ConfigValidationResult {
            valid: true,
            errors: vec![],
        };
        let out = render(&v, OutputFormat::Human);
        assert!(out.contains("✓"));
    }

    #[test]
    fn render_config_validation_invalid() {
        let v = ConfigValidationResult {
            valid: false,
            errors: vec!["bad port".into()],
        };
        let out = render(&v, OutputFormat::Human);
        assert!(out.contains("bad port"));
    }

    #[test]
    fn render_session_list_empty() {
        let l = SessionListResponse { sessions: vec![] };
        assert_eq!(render(&l, OutputFormat::Human), "No active sessions.");
    }

    #[test]
    fn render_cron_list_empty() {
        let l = CronListResponse { jobs: vec![] };
        assert_eq!(render(&l, OutputFormat::Human), "No cron jobs.");
    }

    #[test]
    fn render_action_result_json() {
        let a = ActionResult {
            success: true,
            message: "started".into(),
        };
        let out = render(&a, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["success"], true);
    }

    #[test]
    fn render_heartbeat_presence() {
        let response = HeartbeatPresenceResponse {
            workspace_root: "/workspace".into(),
            path: "/workspace/HEARTBEAT.md".into(),
            present: true,
            modified_at: Some("2026-03-13T19:00:00Z".into()),
            size_bytes: Some(42),
            note: Some("Scheduled sessions load this file at startup.".into()),
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Heartbeat"));
        assert!(out.contains("Present:   true"));
        assert!(out.contains("Scheduled sessions load this file at startup."));
    }

    #[test]
    fn render_channel_resolve_miss() {
        let response = ChannelResolveResponse {
            target: "discord".into(),
            matched: false,
            channel: None,
            note: Some("Only telegram is currently configured.".into()),
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("No configured channel matched `discord`"));
        assert!(out.contains("Only telegram is currently configured."));
    }

    #[test]
    fn render_channel_logs_empty() {
        let response = ChannelLogsResponse {
            logs_dir: "/data/logs".into(),
            filter: Some("telegram".into()),
            files: vec![],
            note: Some("No matching log files found.".into()),
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Channel logs"));
        assert!(out.contains("No matching log files found."));
    }

    #[test]
    fn render_model_list_empty() {
        let response = ModelListResponse {
            default_model: None,
            providers: vec![],
        };
        assert_eq!(
            render(&response, OutputFormat::Human),
            "No model providers configured."
        );
    }

    #[test]
    fn render_model_status_includes_default() {
        let response = ModelStatusResponse {
            default_model: Some("gpt-5.4".into()),
            total: 1,
            credentials_ready: 1,
            providers: vec![ModelProviderDetail {
                name: "azure-foundry".into(),
                kind: "azure-foundry".into(),
                base_url: "https://example.invalid".into(),
                default_model: Some("gpt-5.4".into()),
                model_alias: Some("fast".into()),
                deployment_name: None,
                api_version: None,
                credential_source: "api_key".into(),
                credentials_ready: true,
                notes: None,
            }],
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Default model:       gpt-5.4"));
        assert!(out.contains("credential_source: api_key"));
    }

    #[test]
    fn render_model_aliases_response() {
        let response = ModelAliasesResponse {
            aliases: vec![ModelAliasDetail {
                alias: "fast".into(),
                provider: "hamza-eastus2".into(),
                target_model: Some("gpt-5.4-mini".into()),
                provider_kind: "azure-openai".into(),
                base_url: "https://example.openai.azure.com".into(),
                deployment_name: Some("gpt-5.4-mini".into()),
                api_version: Some("2025-01-01-preview".into()),
                credentials_ready: true,
                note: None,
            }],
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("fast -> hamza-eastus2/gpt-5.4-mini"));
        assert!(out.contains("deployment: gpt-5.4-mini"));
    }

    #[test]
    fn render_model_set_response() {
        let response = ModelSetResponse {
            changed: true,
            config_path: "config.toml".into(),
            previous_model: Some("oc-01-openai/gpt-5.4".into()),
            default_model: "hamza-eastus2/grok-4-fast-reasoning".into(),
            note: "Local config updated; restart gateway to apply new default sessions.".into(),
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Default model set to hamza-eastus2/grok-4-fast-reasoning"));
        assert!(out.contains("Previous: oc-01-openai/gpt-5.4"));
    }

    #[test]
    fn render_memory_status() {
        let response = MemoryStatusResponse {
            workspace_root: "/workspace".into(),
            memory_dir: "/workspace/memory".into(),
            semantic_search_enabled: true,
            long_term_exists: true,
            daily_file_count: 2,
            latest_daily_file: Some("memory/2026-03-13.md".into()),
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Memory"));
        assert!(out.contains("MEMORY.md present:       true"));
    }

    #[test]
    fn render_memory_search_empty() {
        let response = MemorySearchResponse {
            query: "nothing".into(),
            total: 0,
            hits: vec![],
        };
        let out = render(&response, OutputFormat::Human);
        assert_eq!(out, "No results found for query: nothing");
    }

    #[test]
    fn render_memory_get() {
        let response = MemoryGetResponse {
            path: "MEMORY.md".into(),
            from: 1,
            lines: 2,
            content: "# Memory\nEntry".into(),
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("MEMORY.md (from line 1, 2 lines)"));
        assert!(out.contains("Entry"));
    }
}
