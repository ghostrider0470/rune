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
    pub turn_count: Option<u32>,
    pub usage_prompt_tokens: Option<u64>,
    pub usage_completion_tokens: Option<u64>,
    pub latest_model: Option<String>,
}

impl fmt::Display for SessionSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]", self.id, self.status)?;
        if let Some(ref ch) = self.channel {
            write!(f, " ({ch})")?;
        }
        if let Some(turns) = self.turn_count {
            write!(f, " turns={turns}")?;
        }
        if let Some(ref model) = self.latest_model {
            write!(f, " model={model}")?;
        }
        if let (Some(prompt), Some(completion)) =
            (self.usage_prompt_tokens, self.usage_completion_tokens)
        {
            write!(f, " tokens={}/{}", prompt, completion)?;
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
    pub latest_model: Option<String>,
    pub usage_prompt_tokens: Option<u64>,
    pub usage_completion_tokens: Option<u64>,
    pub last_turn_started_at: Option<String>,
    pub last_turn_ended_at: Option<String>,
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
            writeln!(f, "  Turns:   {n}")?;
        }
        if let Some(ref model) = self.latest_model {
            writeln!(f, "  Model:   {model}")?;
        }
        if let (Some(prompt), Some(completion)) =
            (self.usage_prompt_tokens, self.usage_completion_tokens)
        {
            writeln!(f, "  Tokens:  {prompt}/{completion}")?;
        }
        if let Some(ref started_at) = self.last_turn_started_at {
            writeln!(f, "  Last started: {started_at}")?;
        }
        if let Some(ref ended_at) = self.last_turn_ended_at {
            writeln!(f, "  Last ended:   {ended_at}")?;
        }
        Ok(())
    }
}

/// First-class `/status` / `session_status` parity card for an individual session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatusCard {
    pub session_id: Option<String>,
    pub runtime: Option<String>,
    pub status: String,
    pub current_model: Option<String>,
    pub model_override: Option<String>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub estimated_cost: Option<String>,
    pub turn_count: Option<u32>,
    pub uptime_seconds: Option<u64>,
    pub last_turn_started_at: Option<String>,
    pub last_turn_ended_at: Option<String>,
    pub reasoning: Option<String>,
    pub verbose: Option<bool>,
    pub elevated: Option<bool>,
    pub approval_mode: Option<String>,
    pub security_mode: Option<String>,
    pub subagent_lifecycle: Option<String>,
    pub subagent_runtime_status: Option<String>,
    pub subagent_runtime_attached: Option<bool>,
    pub subagent_status_updated_at: Option<String>,
    pub subagent_last_note: Option<String>,
    pub unresolved: Vec<String>,
}

/// Operator-facing durable approval request detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestSummary {
    pub id: String,
    pub subject_type: String,
    pub subject_id: String,
    pub reason: String,
    pub decision: Option<String>,
    pub decided_by: Option<String>,
    pub decided_at: Option<String>,
    pub approval_status: Option<String>,
    pub approval_status_updated_at: Option<String>,
    pub resumed_at: Option<String>,
    pub completed_at: Option<String>,
    pub resume_result_summary: Option<String>,
    pub command: Option<String>,
    pub presented_payload: serde_json::Value,
    pub created_at: String,
}

impl fmt::Display for ApprovalRequestSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Approval: {}", self.id)?;
        writeln!(
            f,
            "  Subject:         {} {}",
            self.subject_type, self.subject_id
        )?;
        writeln!(f, "  Reason:          {}", self.reason)?;
        if let Some(decision) = &self.decision {
            writeln!(f, "  Decision:        {decision}")?;
        }
        if let Some(decided_by) = &self.decided_by {
            writeln!(f, "  Decided by:      {decided_by}")?;
        }
        if let Some(decided_at) = &self.decided_at {
            writeln!(f, "  Decided at:      {decided_at}")?;
        }
        if let Some(status) = &self.approval_status {
            writeln!(f, "  Approval status: {status}")?;
        }
        if let Some(updated_at) = &self.approval_status_updated_at {
            writeln!(f, "  Status updated:  {updated_at}")?;
        }
        if let Some(resumed_at) = &self.resumed_at {
            writeln!(f, "  Resumed at:      {resumed_at}")?;
        }
        if let Some(completed_at) = &self.completed_at {
            writeln!(f, "  Completed at:    {completed_at}")?;
        }
        if let Some(summary) = &self.resume_result_summary {
            writeln!(f, "  Result:          {summary}")?;
        }
        if let Some(command) = &self.command {
            writeln!(f, "  Command:         {command}")?;
        }
        write!(f, "  Created at:      {}", self.created_at)
    }
}

/// Approval request list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalListResponse {
    pub approvals: Vec<ApprovalRequestSummary>,
}

impl fmt::Display for ApprovalListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.approvals.is_empty() {
            return write!(f, "No pending approvals.");
        }
        for approval in &self.approvals {
            writeln!(
                f,
                "{} [{}] status={} created={}",
                approval.id,
                approval.reason,
                approval.approval_status.as_deref().unwrap_or("unknown"),
                approval.created_at
            )?;
            if let Some(command) = &approval.command {
                writeln!(f, "  command: {command}")?;
            }
        }
        Ok(())
    }
}

/// Tool-level approval policy detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalPolicySummary {
    pub tool_name: String,
    pub decision: String,
    pub decided_at: String,
}

impl fmt::Display for ApprovalPolicySummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.tool_name, self.decision, self.decided_at
        )
    }
}

/// Approval policy list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalPoliciesResponse {
    pub policies: Vec<ApprovalPolicySummary>,
}

impl fmt::Display for ApprovalPoliciesResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.policies.is_empty() {
            return write!(f, "No approval policies.");
        }
        for policy in &self.policies {
            writeln!(f, "  {policy}")?;
        }
        Ok(())
    }
}

impl fmt::Display for SessionStatusCard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Session status")?;
        writeln!(f, "  Status:          {}", self.status)?;
        if let Some(session_id) = &self.session_id {
            writeln!(f, "  Session:         {session_id}")?;
        }
        if let Some(runtime) = &self.runtime {
            writeln!(f, "  Runtime:         {runtime}")?;
        }
        if let Some(model) = &self.current_model {
            writeln!(f, "  Model:           {model}")?;
        }
        if let Some(model_override) = &self.model_override {
            writeln!(f, "  Model override:  {model_override}")?;
        }
        if let Some(turn_count) = self.turn_count {
            writeln!(f, "  Turns:           {turn_count}")?;
        }
        if let Some(uptime_seconds) = self.uptime_seconds {
            writeln!(f, "  Uptime:          {uptime_seconds}s")?;
        }
        match (
            self.prompt_tokens,
            self.completion_tokens,
            self.total_tokens,
        ) {
            (Some(prompt), Some(completion), Some(total)) => {
                writeln!(f, "  Tokens:          {prompt}/{completion} total={total}")?;
            }
            (Some(prompt), Some(completion), None) => {
                writeln!(f, "  Tokens:          {prompt}/{completion}")?;
            }
            _ => {}
        }
        if let Some(estimated_cost) = &self.estimated_cost {
            writeln!(f, "  Cost:            {estimated_cost}")?;
        }
        if let Some(reasoning) = &self.reasoning {
            writeln!(f, "  Reasoning:       {reasoning}")?;
        }
        if let Some(verbose) = self.verbose {
            writeln!(f, "  Verbose:         {verbose}")?;
        }
        if let Some(elevated) = self.elevated {
            writeln!(f, "  Elevated:        {elevated}")?;
        }
        if let Some(approval_mode) = &self.approval_mode {
            writeln!(f, "  Approval mode:   {approval_mode}")?;
        }
        if let Some(security_mode) = &self.security_mode {
            writeln!(f, "  Security mode:   {security_mode}")?;
        }
        if let Some(subagent_lifecycle) = &self.subagent_lifecycle {
            writeln!(f, "  Subagent state:  {subagent_lifecycle}")?;
        }
        if let Some(subagent_runtime_status) = &self.subagent_runtime_status {
            writeln!(f, "  Subagent runtime:{:>3}{subagent_runtime_status}", "")?;
        }
        if let Some(subagent_runtime_attached) = self.subagent_runtime_attached {
            writeln!(f, "  Runtime attached:{:>3}{subagent_runtime_attached}", "")?;
        }
        if let Some(subagent_status_updated_at) = &self.subagent_status_updated_at {
            writeln!(f, "  Subagent update: {subagent_status_updated_at}")?;
        }
        if let Some(subagent_last_note) = &self.subagent_last_note {
            writeln!(f, "  Subagent note:   {subagent_last_note}")?;
        }
        if let Some(last_started) = &self.last_turn_started_at {
            writeln!(f, "  Last started:    {last_started}")?;
        }
        if let Some(last_ended) = &self.last_turn_ended_at {
            writeln!(f, "  Last ended:      {last_ended}")?;
        }
        if !self.unresolved.is_empty() {
            writeln!(f, "  Unresolved:")?;
            for item in &self.unresolved {
                writeln!(f, "    - {item}")?;
            }
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

/// Per-provider auth-management detail for `models auth`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAuthProviderDetail {
    pub provider: String,
    pub provider_kind: String,
    pub credential_source: String,
    pub credentials_ready: bool,
    pub api_key_configured: bool,
    pub api_key_env: Option<String>,
    pub auth_order: Vec<String>,
    pub notes: Vec<String>,
}

impl fmt::Display for ModelAuthProviderDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [{}] source={} creds={}",
            self.provider,
            self.provider_kind,
            self.credential_source,
            if self.credentials_ready { "ready" } else { "missing" }
        )
    }
}

/// Response for `models auth`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAuthResponse {
    pub providers: Vec<ModelAuthProviderDetail>,
}

impl fmt::Display for ModelAuthResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.providers.is_empty() {
            return write!(f, "No model auth providers configured.");
        }
        writeln!(f, "Model auth")?;
        for provider in &self.providers {
            writeln!(f, "  - {provider}")?;
            writeln!(
                f,
                "    api_key configured: {}",
                provider.api_key_configured
            )?;
            writeln!(
                f,
                "    api_key_env: {}",
                provider.api_key_env.as_deref().unwrap_or("(default/provider-specific)")
            )?;
            if !provider.auth_order.is_empty() {
                writeln!(f, "    auth_order: {}", provider.auth_order.join(" -> "))?;
            }
            for note in &provider.notes {
                writeln!(f, "    note: {note}")?;
            }
        }
        Ok(())
    }
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

/// Result of updating the default image model in local config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSetImageResponse {
    pub changed: bool,
    pub config_path: String,
    pub previous_image_model: Option<String>,
    pub default_image_model: String,
    pub note: String,
}

impl fmt::Display for ModelSetImageResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Default image model set to {} in {}",
            self.default_image_model, self.config_path
        )?;
        if let Some(previous) = &self.previous_image_model {
            write!(f, "\nPrevious: {previous}")?;
        }
        if !self.note.is_empty() {
            write!(f, "\nNote: {}", self.note)?;
        }
        Ok(())
    }
}

/// A single named fallback chain entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFallbackChainDetail {
    pub name: String,
    pub kind: String,
    pub chain: Vec<String>,
}

impl fmt::Display for ModelFallbackChainDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]: {}", self.name, self.kind, self.chain.join(" -> "))
    }
}

/// Response for `models fallbacks` / `models image-fallbacks`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFallbacksResponse {
    pub text_chains: Vec<ModelFallbackChainDetail>,
    pub image_chains: Vec<ModelFallbackChainDetail>,
}

impl fmt::Display for ModelFallbacksResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.text_chains.is_empty() && self.image_chains.is_empty() {
            return write!(f, "No fallback chains configured.");
        }
        if !self.text_chains.is_empty() {
            writeln!(f, "Text fallback chains:")?;
            for chain in &self.text_chains {
                writeln!(f, "  {chain}")?;
            }
        }
        if !self.image_chains.is_empty() {
            if !self.text_chains.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, "Image fallback chains:")?;
            for chain in &self.image_chains {
                writeln!(f, "  {chain}")?;
            }
        }
        Ok(())
    }
}

/// A single model discovered from a provider scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannedModelDetail {
    pub name: String,
    pub size: Option<u64>,
    pub modified_at: Option<String>,
}

impl fmt::Display for ScannedModelDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if let Some(size) = self.size {
            write!(f, " size={size}")?;
        }
        if let Some(modified_at) = &self.modified_at {
            write!(f, " modified={modified_at}")?;
        }
        Ok(())
    }
}

/// Provider-level scan response for `models scan`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelScanProviderResult {
    pub provider: String,
    pub models: Vec<ScannedModelDetail>,
}

impl fmt::Display for ModelScanProviderResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}:", self.provider)?;
        if self.models.is_empty() {
            writeln!(f, "  (no models reported)")?;
        } else {
            for model in &self.models {
                writeln!(f, "  - {model}")?;
            }
        }
        Ok(())
    }
}

/// Response for `models scan`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelScanResponse {
    pub providers: Vec<ModelScanProviderResult>,
}

impl fmt::Display for ModelScanResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.providers.is_empty() {
            return write!(f, "No local model providers returned scan results.");
        }
        writeln!(f, "Discovered models:")?;
        for provider in &self.providers {
            write!(f, "{provider}")?;
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

/// Compact operator dashboard summary spanning gateway, scheduler, sessions, models, channels, and memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardResponse {
    pub gateway: StatusResponse,
    pub health: HealthResponse,
    pub cron: CronStatusResponse,
    pub sessions: DashboardSessionsSummary,
    pub models: DashboardModelsSummary,
    pub channels: DashboardChannelsSummary,
    pub memory: MemoryStatusResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSessionsSummary {
    pub total: usize,
    pub sample: Vec<SessionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardModelsSummary {
    pub total: usize,
    pub credentials_ready: usize,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardChannelsSummary {
    pub total: usize,
    pub enabled: usize,
    pub configured: usize,
    pub ready: usize,
}

impl fmt::Display for DashboardResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Dashboard")?;
        writeln!(f, "  Gateway:   {}", self.gateway.status)?;
        if let Some(version) = &self.gateway.version {
            writeln!(f, "  Version:   {version}")?;
        }
        if let Some(uptime_seconds) = self.gateway.uptime_seconds {
            writeln!(f, "  Uptime:    {uptime_seconds}s")?;
        }
        writeln!(
            f,
            "  Health:    {}",
            if self.health.healthy {
                "healthy"
            } else {
                "degraded"
            }
        )?;
        writeln!(f, "  Sessions:  {} total", self.sessions.total)?;
        if !self.sessions.sample.is_empty() {
            writeln!(f, "  Recent:")?;
            for session in &self.sessions.sample {
                writeln!(f, "    - {session}")?;
            }
        }
        writeln!(
            f,
            "  Cron:      {} total / {} enabled / {} due",
            self.cron.total_jobs, self.cron.enabled_jobs, self.cron.due_jobs
        )?;
        writeln!(
            f,
            "  Models:    {} providers / {} ready",
            self.models.total, self.models.credentials_ready
        )?;
        writeln!(
            f,
            "  Default:   {}",
            self.models
                .default_model
                .as_deref()
                .unwrap_or("(not configured)")
        )?;
        writeln!(
            f,
            "  Channels:  {} total / {} enabled / {} configured / {} ready",
            self.channels.total,
            self.channels.enabled,
            self.channels.configured,
            self.channels.ready
        )?;
        writeln!(
            f,
            "  Memory:    daily={} long-term={} level={}",
            self.memory.daily_file_count, self.memory.long_term_exists, self.memory.memory_level
        )?;
        write!(
            f,
            "  Latest:    {}",
            self.memory.latest_daily_file.as_deref().unwrap_or("(none)")
        )
    }
}

/// Summary status for workspace memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStatusResponse {
    pub workspace_root: String,
    pub memory_dir: String,
    pub memory_level: String,
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
        writeln!(f, "  Configured level:        {}", self.memory_level)?;
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

/// Result of probing gateway reachability/auth semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayProbeResponse {
    pub gateway_url: String,
    pub status_http_ok: bool,
    pub health_http_ok: bool,
    pub auth_required: bool,
    pub auth_valid: Option<bool>,
    pub note: String,
}

impl fmt::Display for GatewayProbeResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Gateway probe")?;
        writeln!(f, "  URL:           {}", self.gateway_url)?;
        writeln!(f, "  /status OK:    {}", self.status_http_ok)?;
        writeln!(f, "  /health OK:    {}", self.health_http_ok)?;
        writeln!(f, "  Auth required: {}", self.auth_required)?;
        if let Some(auth_valid) = self.auth_valid {
            writeln!(f, "  Auth valid:    {}", auth_valid)?;
        }
        write!(f, "  Note:          {}", self.note)
    }
}

/// Operator-facing discovery data for the current gateway binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayDiscoverResponse {
    pub gateway_url: String,
    pub health_url: String,
    pub websocket_url: String,
    pub config_path: String,
    pub auth_enabled: bool,
    pub note: String,
}

impl fmt::Display for GatewayDiscoverResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Gateway discovery")?;
        writeln!(f, "  Gateway URL:   {}", self.gateway_url)?;
        writeln!(f, "  Health URL:    {}", self.health_url)?;
        writeln!(f, "  WebSocket URL: {}", self.websocket_url)?;
        writeln!(f, "  Config path:   {}", self.config_path)?;
        writeln!(f, "  Auth enabled:  {}", self.auth_enabled)?;
        write!(f, "  Note:          {}", self.note)
    }
}

/// Raw gateway HTTP call result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayCallResponse {
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub content_type: Option<String>,
    pub body: String,
}

impl fmt::Display for GatewayCallResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} {} -> HTTP {}",
            self.method, self.path, self.status_code
        )?;
        if let Some(content_type) = &self.content_type {
            writeln!(f, "Content-Type: {content_type}")?;
        }
        write!(f, "{}", self.body)
    }
}

/// Aggregate token-usage summary from persisted session turns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayUsageCostResponse {
    pub total_sessions: usize,
    pub total_turns: usize,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub note: String,
}

impl fmt::Display for GatewayUsageCostResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Gateway usage")?;
        writeln!(f, "  Sessions:          {}", self.total_sessions)?;
        writeln!(f, "  Turns:             {}", self.total_turns)?;
        writeln!(f, "  Prompt tokens:     {}", self.prompt_tokens)?;
        writeln!(f, "  Completion tokens: {}", self.completion_tokens)?;
        writeln!(f, "  Total tokens:      {}", self.total_tokens)?;
        write!(f, "  Note:              {}", self.note)
    }
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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CronScheduleSummary {
    At {
        at: String,
    },
    Every {
        every_ms: u64,
        #[serde(default)]
        anchor_ms: Option<u64>,
    },
    Cron {
        expr: String,
        #[serde(default)]
        tz: Option<String>,
    },
}

impl CronScheduleSummary {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::At { .. } => "at",
            Self::Every { .. } => "every",
            Self::Cron { .. } => "cron",
        }
    }

    fn render_human(&self) -> String {
        match self {
            Self::At { at } => format!("at {at}"),
            Self::Every {
                every_ms,
                anchor_ms,
            } => {
                let mut summary = format!("every {every_ms}ms");
                if let Some(anchor_ms) = anchor_ms {
                    summary.push_str(&format!(" anchor={anchor_ms}"));
                }
                summary
            }
            Self::Cron { expr, tz } => match tz {
                Some(tz) => format!("cron {expr} tz={tz}"),
                None => format!("cron {expr}"),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CronPayloadSummary {
    SystemEvent {
        text: String,
    },
    AgentTurn {
        message: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
}

impl CronPayloadSummary {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::SystemEvent { .. } => "system_event",
            Self::AgentTurn { .. } => "agent_turn",
        }
    }

    fn render_human(&self) -> String {
        match self {
            Self::SystemEvent { text } => format!("system_event text={}", quoted(text)),
            Self::AgentTurn {
                message,
                model,
                timeout_seconds,
            } => {
                let mut summary = format!("agent_turn message={}", quoted(message));
                if let Some(model) = model {
                    summary.push_str(&format!(" model={model}"));
                }
                if let Some(timeout_seconds) = timeout_seconds {
                    summary.push_str(&format!(" timeout={}s", timeout_seconds));
                }
                summary
            }
        }
    }
}

fn quoted(value: &str) -> String {
    format!("{value:?}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobSummary {
    pub id: String,
    pub name: Option<String>,
    pub schedule: CronScheduleSummary,
    pub payload: CronPayloadSummary,
    pub delivery_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    pub enabled: bool,
    pub session_target: String,
    pub created_at: String,
    pub last_run_at: Option<String>,
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
        write!(
            f,
            " delivery={} payload={} schedule={}",
            self.delivery_mode,
            self.payload.kind(),
            self.schedule.kind()
        )?;
        if let Some(next) = &self.next_run_at {
            write!(f, " next={next}")?;
        }
        Ok(())
    }
}

/// Detailed cron job inspection response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobDetailResponse {
    #[serde(flatten)]
    pub job: CronJobSummary,
}

impl fmt::Display for CronJobDetailResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Cron job")?;
        writeln!(f, "  Id:             {}", self.job.id)?;
        writeln!(
            f,
            "  Name:           {}",
            self.job.name.as_deref().unwrap_or("(unnamed)")
        )?;
        writeln!(f, "  Enabled:        {}", self.job.enabled)?;
        writeln!(f, "  Session target: {}", self.job.session_target)?;
        writeln!(f, "  Delivery mode:  {}", self.job.delivery_mode)?;
        if let Some(url) = &self.job.webhook_url {
            writeln!(f, "  Webhook URL:    {url}")?;
        }
        writeln!(f, "  Schedule:       {}", self.job.schedule.render_human())?;
        writeln!(f, "  Payload:        {}", self.job.payload.render_human())?;
        writeln!(f, "  Created:        {}", self.job.created_at)?;
        writeln!(
            f,
            "  Last run:       {}",
            self.job.last_run_at.as_deref().unwrap_or("(never)")
        )?;
        writeln!(
            f,
            "  Next run:       {}",
            self.job.next_run_at.as_deref().unwrap_or("(none)")
        )?;
        write!(f, "  Runs:           {}", self.job.run_count)
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

/// System event list response (filtered cron jobs with `system_event` payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEventListResponse {
    pub events: Vec<CronJobSummary>,
}

impl fmt::Display for SystemEventListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.events.is_empty() {
            return write!(f, "No system event jobs.");
        }
        for job in &self.events {
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
    /// Lifecycle status: pending, delivered, cancelled, missed.
    #[serde(default)]
    pub status: String,
    /// When the reminder reached a terminal outcome.
    pub outcome_at: Option<String>,
    /// Last recorded terminal error, if any.
    pub last_error: Option<String>,
}

impl fmt::Display for ReminderSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = if self.status.is_empty() {
            if self.delivered { "delivered" } else { "pending" }
        } else {
            &self.status
        };
        write!(
            f,
            "{} [{}] {} -> {}",
            self.id, label, self.target, self.message,
        )?;
        write!(f, " at {}", self.fire_at)?;
        if let Some(ref outcome_at) = self.outcome_at {
            write!(f, " outcome={outcome_at}")?;
        }
        if let Some(ref last_error) = self.last_error {
            write!(f, " error={last_error}")?;
        }
        Ok(())
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
    fn render_session_status_card_human() {
        let status = SessionStatusCard {
            session_id: Some("main".into()),
            runtime: Some("agent=main | model=gpt-5.4".into()),
            status: "running".into(),
            current_model: Some("gpt-5.4".into()),
            model_override: None,
            prompt_tokens: Some(1200),
            completion_tokens: Some(340),
            total_tokens: Some(1540),
            estimated_cost: Some("not available".into()),
            turn_count: Some(12),
            uptime_seconds: Some(98),
            last_turn_started_at: Some("2026-03-14T01:30:00Z".into()),
            last_turn_ended_at: Some("2026-03-14T01:30:09Z".into()),
            reasoning: Some("off".into()),
            verbose: Some(false),
            elevated: Some(false),
            approval_mode: Some("on-miss".into()),
            security_mode: Some("allowlist".into()),
            subagent_lifecycle: Some("steered".into()),
            subagent_runtime_status: Some("not_attached".into()),
            subagent_runtime_attached: Some(false),
            subagent_status_updated_at: Some("2026-03-14T02:00:00Z".into()),
            subagent_last_note: Some(
                "Steering message queued for subagent/session: tighten the tests".into(),
            ),
            unresolved: vec!["cost posture is estimate-only".into()],
        };
        let out = render(&status, OutputFormat::Human);
        assert!(out.contains("Session status"));
        assert!(out.contains("Model:           gpt-5.4"));
        assert!(out.contains("Tokens:          1200/340 total=1540"));
        assert!(out.contains("Approval mode:   on-miss"));
        assert!(out.contains("Subagent state:  steered"));
        assert!(out.contains(
            "Subagent note:   Steering message queued for subagent/session: tighten the tests"
        ));
        assert!(out.contains("- cost posture is estimate-only"));
    }

    #[test]
    fn render_session_status_card_json() {
        let status = SessionStatusCard {
            session_id: Some("main".into()),
            runtime: None,
            status: "running".into(),
            current_model: Some("gpt-5.4".into()),
            model_override: Some("o3-mini".into()),
            prompt_tokens: Some(10),
            completion_tokens: Some(5),
            total_tokens: Some(15),
            estimated_cost: None,
            turn_count: Some(2),
            uptime_seconds: None,
            last_turn_started_at: None,
            last_turn_ended_at: None,
            reasoning: Some("low".into()),
            verbose: Some(true),
            elevated: Some(false),
            approval_mode: Some("always".into()),
            security_mode: Some("full".into()),
            subagent_lifecycle: Some("spawned".into()),
            subagent_runtime_status: Some("not_attached".into()),
            subagent_runtime_attached: Some(false),
            subagent_status_updated_at: None,
            subagent_last_note: None,
            unresolved: vec![],
        };
        let out = render(&status, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["session_id"], "main");
        assert_eq!(v["model_override"], "o3-mini");
        assert_eq!(v["total_tokens"], 15);
        assert_eq!(v["approval_mode"], "always");
        assert_eq!(v["subagent_lifecycle"], "spawned");
        assert_eq!(v["subagent_runtime_status"], "not_attached");
    }

    #[test]
    fn render_cron_list_empty() {
        let l = CronListResponse { jobs: vec![] };
        assert_eq!(render(&l, OutputFormat::Human), "No cron jobs.");
    }

    #[test]
    fn render_cron_list_item_includes_delivery_and_kinds() {
        let response = CronListResponse {
            jobs: vec![CronJobSummary {
                id: "job-1".into(),
                name: Some("daily-check".into()),
                schedule: CronScheduleSummary::Cron {
                    expr: "0 0 9 * * *".into(),
                    tz: Some("Europe/Sarajevo".into()),
                },
                payload: CronPayloadSummary::SystemEvent {
                    text: "run daily check".into(),
                },
                delivery_mode: "announce".into(),
                webhook_url: None,
                enabled: true,
                session_target: "main".into(),
                created_at: "2026-03-18T09:00:00Z".into(),
                last_run_at: None,
                next_run_at: Some("2026-03-18T10:00:00Z".into()),
                run_count: 3,
            }],
        };

        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("delivery=announce"));
        assert!(out.contains("payload=system_event"));
        assert!(out.contains("schedule=cron"));
    }

    #[test]
    fn render_cron_job_detail() {
        let response = CronJobDetailResponse {
            job: CronJobSummary {
                id: "job-1".into(),
                name: Some("daily-check".into()),
                schedule: CronScheduleSummary::Every {
                    every_ms: 60000,
                    anchor_ms: Some(12345),
                },
                payload: CronPayloadSummary::AgentTurn {
                    message: "check queue".into(),
                    model: Some("gpt-5.4".into()),
                    timeout_seconds: Some(30),
                },
                delivery_mode: "webhook".into(),
                webhook_url: Some("https://example.com/hook".into()),
                enabled: false,
                session_target: "isolated".into(),
                created_at: "2026-03-18T09:00:00Z".into(),
                last_run_at: Some("2026-03-18T09:30:00Z".into()),
                next_run_at: None,
                run_count: 2,
            },
        };

        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Delivery mode:  webhook"));
        assert!(out.contains("Webhook URL:    https://example.com/hook"));
        assert!(out.contains("Schedule:       every 60000ms anchor=12345"));
        assert!(out.contains(
            "Payload:        agent_turn message=\"check queue\" model=gpt-5.4 timeout=30s"
        ));
        assert!(out.contains("Next run:       (none)"));
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
    fn render_gateway_probe() {
        let response = GatewayProbeResponse {
            gateway_url: "http://127.0.0.1:8787".into(),
            status_http_ok: true,
            health_http_ok: true,
            auth_required: true,
            auth_valid: Some(true),
            note: "RPC reachable and auth accepted.".into(),
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Gateway probe"));
        assert!(out.contains("Auth required: true"));
    }

    #[test]
    fn render_gateway_discover() {
        let response = GatewayDiscoverResponse {
            gateway_url: "http://127.0.0.1:8787".into(),
            health_url: "http://127.0.0.1:8787/health".into(),
            websocket_url: "ws://127.0.0.1:8787/ws".into(),
            config_path: "config.toml".into(),
            auth_enabled: false,
            note: "Use /health for probes and /status for operator detail.".into(),
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Gateway discovery"));
        assert!(out.contains("WebSocket URL: ws://127.0.0.1:8787/ws"));
    }

    #[test]
    fn render_gateway_call() {
        let response = GatewayCallResponse {
            method: "GET".into(),
            path: "/status".into(),
            status_code: 200,
            content_type: Some("application/json".into()),
            body: "{\"status\":\"running\"}".into(),
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("GET /status -> HTTP 200"));
        assert!(out.contains("application/json"));
    }

    #[test]
    fn render_gateway_usage_cost() {
        let response = GatewayUsageCostResponse {
            total_sessions: 2,
            total_turns: 3,
            prompt_tokens: 100,
            completion_tokens: 40,
            total_tokens: 140,
            note: "Token aggregates only; monetary cost accounting is not implemented yet.".into(),
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Gateway usage"));
        assert!(out.contains("Total tokens:      140"));
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
    fn render_model_auth_response() {
        let response = ModelAuthResponse {
            providers: vec![ModelAuthProviderDetail {
                provider: "hamza-eastus2".into(),
                provider_kind: "azure-openai".into(),
                credential_source: "env:OPENAI_API_KEY".into(),
                credentials_ready: false,
                api_key_configured: false,
                api_key_env: Some("OPENAI_API_KEY".into()),
                auth_order: vec!["api_key".into(), "api_key_env".into(), "azure_cli".into()],
                notes: vec![
                    "Use `rune config set models.providers.<n>.api_key_env \"OPENAI_API_KEY\"` or set the environment variable before launch.".into(),
                ],
            }],
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Model auth"));
        assert!(out.contains("hamza-eastus2 [azure-openai] source=env:OPENAI_API_KEY creds=missing"));
        assert!(out.contains("auth_order: api_key -> api_key_env -> azure_cli"));
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
    fn render_model_set_image_response() {
        let response = ModelSetImageResponse {
            changed: true,
            config_path: "config.toml".into(),
            previous_image_model: Some("oc-01-openai/dall-e-3".into()),
            default_image_model: "hamza-eastus2/dall-e-4".into(),
            note: "Local config updated; restart gateway to apply new default image model.".into(),
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Default image model set to hamza-eastus2/dall-e-4"));
        assert!(out.contains("Previous: oc-01-openai/dall-e-3"));
    }

    #[test]
    fn render_model_fallbacks_empty() {
        let response = ModelFallbacksResponse {
            text_chains: vec![],
            image_chains: vec![],
        };
        assert_eq!(
            render(&response, OutputFormat::Human),
            "No fallback chains configured."
        );
    }

    #[test]
    fn render_model_fallbacks_with_chains() {
        let response = ModelFallbacksResponse {
            text_chains: vec![ModelFallbackChainDetail {
                name: "primary-text".into(),
                kind: "text".into(),
                chain: vec![
                    "azure/gpt-5.4".into(),
                    "openai/gpt-5.4".into(),
                    "groq/llama-4".into(),
                ],
            }],
            image_chains: vec![ModelFallbackChainDetail {
                name: "primary-image".into(),
                kind: "image".into(),
                chain: vec!["openai/dall-e-4".into(), "azure/dall-e-3".into()],
            }],
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Text fallback chains:"));
        assert!(out.contains("azure/gpt-5.4 -> openai/gpt-5.4 -> groq/llama-4"));
        assert!(out.contains("Image fallback chains:"));
        assert!(out.contains("openai/dall-e-4 -> azure/dall-e-3"));
    }

    #[test]
    fn render_model_fallbacks_json() {
        let response = ModelFallbacksResponse {
            text_chains: vec![ModelFallbackChainDetail {
                name: "default".into(),
                kind: "text".into(),
                chain: vec!["a/m1".into(), "b/m2".into()],
            }],
            image_chains: vec![],
        };
        let out = render(&response, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["text_chains"][0]["name"], "default");
        assert_eq!(v["text_chains"][0]["chain"][0], "a/m1");
        assert!(v["image_chains"].as_array().unwrap().is_empty());
    }

    #[test]
    fn render_model_scan_empty() {
        let response = ModelScanResponse { providers: vec![] };
        assert_eq!(
            render(&response, OutputFormat::Human),
            "No local model providers returned scan results."
        );
    }

    #[test]
    fn render_model_scan_with_results() {
        let response = ModelScanResponse {
            providers: vec![ModelScanProviderResult {
                provider: "ollama-local".into(),
                models: vec![ScannedModelDetail {
                    name: "llama3.2:latest".into(),
                    size: Some(123456789),
                    modified_at: Some("2026-03-19T03:00:00Z".into()),
                }],
            }],
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Discovered models:"));
        assert!(out.contains("ollama-local:"));
        assert!(out.contains("llama3.2:latest size=123456789 modified=2026-03-19T03:00:00Z"));
    }

    #[test]
    fn render_model_scan_json() {
        let response = ModelScanResponse {
            providers: vec![ModelScanProviderResult {
                provider: "ollama-local".into(),
                models: vec![ScannedModelDetail {
                    name: "llama3.2:latest".into(),
                    size: None,
                    modified_at: None,
                }],
            }],
        };
        let out = render(&response, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["providers"][0]["provider"], "ollama-local");
        assert_eq!(v["providers"][0]["models"][0]["name"], "llama3.2:latest");
    }

    #[test]
    fn render_memory_status() {
        let response = MemoryStatusResponse {
            workspace_root: "/workspace".into(),
            memory_dir: "/workspace/memory".into(),
            memory_level: "semantic".into(),
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

    #[test]
    fn render_dashboard_response() {
        let response = DashboardResponse {
            gateway: StatusResponse {
                status: "running".into(),
                version: Some("0.1.0".into()),
                uptime_seconds: Some(42),
            },
            health: HealthResponse {
                healthy: true,
                message: "Gateway is healthy.".into(),
            },
            cron: CronStatusResponse {
                total_jobs: 3,
                enabled_jobs: 2,
                due_jobs: 1,
            },
            sessions: DashboardSessionsSummary {
                total: 2,
                sample: vec![SessionSummary {
                    id: "session-1".into(),
                    status: "running".into(),
                    channel: Some("telegram".into()),
                    created_at: None,
                    turn_count: Some(1),
                    usage_prompt_tokens: Some(10),
                    usage_completion_tokens: Some(5),
                    latest_model: Some("hamza-eastus2/gpt-5.4".into()),
                }],
            },
            models: DashboardModelsSummary {
                total: 2,
                credentials_ready: 1,
                default_model: Some("hamza-eastus2/gpt-5.4".into()),
            },
            channels: DashboardChannelsSummary {
                total: 1,
                enabled: 1,
                configured: 1,
                ready: 1,
            },
            memory: MemoryStatusResponse {
                workspace_root: "/workspace".into(),
                memory_dir: "/workspace/memory".into(),
                memory_level: "semantic".into(),
                semantic_search_enabled: true,
                long_term_exists: true,
                daily_file_count: 4,
                latest_daily_file: Some("memory/2026-03-13.md".into()),
            },
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Dashboard"));
        assert!(out.contains("Cron:      3 total / 2 enabled / 1 due"));
        assert!(out.contains("Default:   hamza-eastus2/gpt-5.4"));
    }

    #[test]
    fn render_reminder_pending() {
        let r = ReminderSummary {
            id: "r-1".into(),
            message: "Stand up".into(),
            target: "main".into(),
            fire_at: "2026-04-01T09:00:00Z".into(),
            delivered: false,
            created_at: "2026-03-19T10:00:00Z".into(),
            delivered_at: None,
            status: "pending".into(),
            outcome_at: None,
            last_error: None,
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("[pending]"));
        assert!(out.contains("main -> Stand up"));
        assert!(!out.contains("outcome="));
        assert!(!out.contains("error="));
    }

    #[test]
    fn render_reminder_delivered() {
        let r = ReminderSummary {
            id: "r-2".into(),
            message: "Take meds".into(),
            target: "isolated".into(),
            fire_at: "2026-04-01T08:00:00Z".into(),
            delivered: true,
            created_at: "2026-03-19T10:00:00Z".into(),
            delivered_at: Some("2026-04-01T08:00:05Z".into()),
            status: "delivered".into(),
            outcome_at: Some("2026-04-01T08:00:05Z".into()),
            last_error: None,
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("[delivered]"));
        assert!(out.contains("isolated -> Take meds"));
        assert!(out.contains("outcome=2026-04-01T08:00:05Z"));
    }

    #[test]
    fn render_reminder_missed_shows_error() {
        let r = ReminderSummary {
            id: "r-3".into(),
            message: "Important".into(),
            target: "main".into(),
            fire_at: "2026-04-01T07:00:00Z".into(),
            delivered: false,
            created_at: "2026-03-19T10:00:00Z".into(),
            delivered_at: None,
            status: "missed".into(),
            outcome_at: Some("2026-04-01T07:01:00Z".into()),
            last_error: Some("session unavailable".into()),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("[missed]"));
        assert!(out.contains("error=session unavailable"));
        assert!(out.contains("outcome="));
    }

    #[test]
    fn render_reminder_cancelled() {
        let r = ReminderSummary {
            id: "r-4".into(),
            message: "Nevermind".into(),
            target: "main".into(),
            fire_at: "2026-04-01T12:00:00Z".into(),
            delivered: false,
            created_at: "2026-03-19T10:00:00Z".into(),
            delivered_at: None,
            status: "cancelled".into(),
            outcome_at: Some("2026-03-20T10:00:00Z".into()),
            last_error: None,
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("[cancelled]"));
    }

    #[test]
    fn render_reminder_json_includes_outcome_fields() {
        let r = ReminderSummary {
            id: "r-5".into(),
            message: "JSON test".into(),
            target: "isolated".into(),
            fire_at: "2026-04-01T09:00:00Z".into(),
            delivered: false,
            created_at: "2026-03-19T10:00:00Z".into(),
            delivered_at: None,
            status: "missed".into(),
            outcome_at: Some("2026-04-01T09:01:00Z".into()),
            last_error: Some("timeout".into()),
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["status"], "missed");
        assert_eq!(v["outcome_at"], "2026-04-01T09:01:00Z");
        assert_eq!(v["last_error"], "timeout");
        assert_eq!(v["target"], "isolated");
    }
}
