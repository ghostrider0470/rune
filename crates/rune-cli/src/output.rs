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
        if json {
            Self::Json
        } else {
            Self::Human
        }
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
    #[serde(default = "default_kind")]
    pub kind: String,
    pub status: String,
    pub channel: Option<String>,
    pub requester_session_id: Option<String>,
    pub created_at: Option<String>,
    pub turn_count: Option<u32>,
    pub usage_prompt_tokens: Option<u64>,
    pub usage_completion_tokens: Option<u64>,
    pub latest_model: Option<String>,
}

fn default_kind() -> String {
    "direct".to_string()
}

impl fmt::Display for SessionSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]", self.id, self.status)?;
        if self.kind != "direct" {
            write!(f, " kind={}", self.kind)?;
        }
        if let Some(ref ch) = self.channel {
            write!(f, " ({ch})")?;
        }
        if let Some(ref parent) = self.requester_session_id {
            write!(f, " parent={parent}")?;
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
    #[serde(default = "default_kind")]
    pub kind: String,
    pub status: String,
    pub channel: Option<String>,
    pub requester_session_id: Option<String>,
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
        writeln!(f, "  Kind:    {}", self.kind)?;
        writeln!(f, "  Status:  {}", self.status)?;
        if let Some(ref ch) = self.channel {
            writeln!(f, "  Channel: {ch}")?;
        }
        if let Some(ref parent) = self.requester_session_id {
            writeln!(f, "  Parent:  {parent}")?;
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

/// A single node in a session delegation tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTreeNode {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub channel: Option<String>,
    pub created_at: Option<String>,
    pub turn_count: Option<u32>,
    pub children: Vec<SessionTreeNode>,
}

/// Session delegation tree response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTreeResponse {
    pub root: SessionTreeNode,
}

impl SessionTreeNode {
    fn fmt_tree(&self, f: &mut fmt::Formatter<'_>, prefix: &str, is_last: bool) -> fmt::Result {
        let connector = if prefix.is_empty() {
            ""
        } else if is_last {
            "└── "
        } else {
            "├── "
        };
        write!(f, "{prefix}{connector}{} [{}]", self.id, self.status)?;
        if self.kind != "direct" {
            write!(f, " kind={}", self.kind)?;
        }
        if let Some(ref ch) = self.channel {
            write!(f, " ({ch})")?;
        }
        if let Some(turns) = self.turn_count {
            write!(f, " turns={turns}")?;
        }
        writeln!(f)?;

        let child_prefix = if prefix.is_empty() {
            String::new()
        } else if is_last {
            format!("{prefix}    ")
        } else {
            format!("{prefix}│   ")
        };

        for (i, child) in self.children.iter().enumerate() {
            let last = i == self.children.len() - 1;
            child.fmt_tree(f, &child_prefix, last)?;
        }
        Ok(())
    }
}

impl fmt::Display for SessionTreeResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.root.fmt_tree(f, "", false)
    }
}

/// Subagent summary for `rune agents list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    pub id: String,
    pub status: String,
    pub parent_session_id: Option<String>,
    pub created_at: Option<String>,
    pub turn_count: Option<u32>,
    pub usage_prompt_tokens: Option<u64>,
    pub usage_completion_tokens: Option<u64>,
    pub latest_model: Option<String>,
}

impl fmt::Display for AgentSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]", self.id, self.status)?;
        if let Some(ref parent) = self.parent_session_id {
            write!(f, " parent={parent}")?;
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

/// Agent list response for `rune agents list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentListResponse {
    pub agents: Vec<AgentSummary>,
}

impl fmt::Display for AgentListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.agents.is_empty() {
            return write!(f, "No active subagent sessions.");
        }
        for a in &self.agents {
            writeln!(f, "  {a}")?;
        }
        Ok(())
    }
}

/// Detailed subagent view for `rune agents show`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDetailResponse {
    pub id: String,
    pub status: String,
    pub parent_session_id: Option<String>,
    pub created_at: Option<String>,
    pub turn_count: Option<u32>,
    pub latest_model: Option<String>,
    pub usage_prompt_tokens: Option<u64>,
    pub usage_completion_tokens: Option<u64>,
    pub last_turn_started_at: Option<String>,
    pub last_turn_ended_at: Option<String>,
}

impl fmt::Display for AgentDetailResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Agent: {}", self.id)?;
        writeln!(f, "  Status:  {}", self.status)?;
        if let Some(ref parent) = self.parent_session_id {
            writeln!(f, "  Parent:  {parent}")?;
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

/// A node in the agent delegation tree for `rune agents tree`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTreeNode {
    pub id: String,
    pub kind: String,
    pub status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<AgentTreeNode>,
}

impl AgentTreeNode {
    fn fmt_tree(&self, f: &mut fmt::Formatter<'_>, prefix: &str, connector: &str) -> fmt::Result {
        writeln!(
            f,
            "{prefix}{connector}{} [{}] ({})",
            self.id, self.status, self.kind
        )?;
        let child_prefix = format!(
            "{prefix}{}",
            if connector.is_empty() {
                ""
            } else if connector.starts_with('└') {
                "    "
            } else {
                "│   "
            }
        );
        for (i, child) in self.children.iter().enumerate() {
            let is_last = i == self.children.len() - 1;
            let child_connector = if is_last { "└── " } else { "├── " };
            child.fmt_tree(f, &child_prefix, child_connector)?;
        }
        Ok(())
    }
}

/// Response for `rune agents tree`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTreeResponse {
    pub roots: Vec<AgentTreeNode>,
}

impl fmt::Display for AgentTreeResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.roots.is_empty() {
            return write!(f, "No sessions found.");
        }
        for (i, root) in self.roots.iter().enumerate() {
            root.fmt_tree(f, "", "")?;
            if i < self.roots.len() - 1 {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

/// A single agent template entry for display in `rune agents templates`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateSummary {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub mode: String,
    pub spells: Vec<String>,
}

impl fmt::Display for TemplateSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  {:<22} {:<24} [{}] mode={}",
            self.slug, self.description, self.category, self.mode
        )
    }
}

/// Response for `rune agents templates`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateListResponse {
    pub templates: Vec<TemplateSummary>,
}

impl fmt::Display for TemplateListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.templates.is_empty() {
            return write!(f, "No templates available.");
        }
        writeln!(
            f,
            "  {:<22} {:<24} {:<12} MODE",
            "SLUG", "DESCRIPTION", "CATEGORY"
        )?;
        for t in &self.templates {
            writeln!(
                f,
                "  {:<22} {:<24} {:<12} {}",
                t.slug, t.description, t.category, t.mode
            )?;
        }
        Ok(())
    }
}

/// Response for `rune agents start --template <slug>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateStartResponse {
    pub session_id: String,
    pub template_slug: String,
    pub template_name: String,
    pub mode: String,
    pub status: String,
}

impl fmt::Display for TemplateStartResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Session started from template.")?;
        writeln!(f, "  Session:  {}", self.session_id)?;
        writeln!(f, "  Template: {} ({})", self.template_name, self.template_slug)?;
        writeln!(f, "  Mode:     {}", self.mode)?;
        write!(f, "  Status:   {}", self.status)
    }
}

/// A single installed skill entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub source_dir: String,
    pub binary_path: Option<String>,
}

impl fmt::Display for SkillSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} [{}]",
            self.name,
            if self.enabled { "enabled" } else { "disabled" }
        )?;
        writeln!(f, "  Description: {}", self.description)?;
        writeln!(f, "  Source: {}", self.source_dir)?;
        write!(
            f,
            "  Binary: {}",
            self.binary_path.as_deref().unwrap_or("-")
        )
    }
}

/// Response for `rune skills list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillListResponse {
    pub skills: Vec<SkillSummary>,
}

impl fmt::Display for SkillListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.skills.is_empty() {
            return write!(f, "No skills found.");
        }

        for (idx, skill) in self.skills.iter().enumerate() {
            if idx > 0 {
                writeln!(f)?;
                writeln!(f)?;
            }
            write!(f, "{skill}")?;
        }

        Ok(())
    }
}

/// Response for `rune skills check`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCheckResponse {
    pub success: bool,
    pub discovered: usize,
    pub loaded: usize,
    pub removed: usize,
}

impl fmt::Display for SkillCheckResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.success {
            write!(
                f,
                "Skill scan complete — discovered {}, loaded {}, removed {}",
                self.discovered, self.loaded, self.removed
            )
        } else {
            write!(f, "Skill scan failed.")
        }
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
            if self.credentials_ready {
                "ready"
            } else {
                "missing"
            }
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
            writeln!(f, "    api_key configured: {}", provider.api_key_configured)?;
            writeln!(
                f,
                "    api_key_env: {}",
                provider
                    .api_key_env
                    .as_deref()
                    .unwrap_or("(default/provider-specific)")
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
        write!(
            f,
            "{} [{}]: {}",
            self.name,
            self.kind,
            self.chain.join(" -> ")
        )
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

/// Result of a bulk session cleanup operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCleanupResponse {
    pub deleted: usize,
    pub failed: usize,
    pub dry_run: bool,
    pub sessions: Vec<SessionCleanupItem>,
}

/// Individual session affected by a cleanup operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCleanupItem {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub result: String,
}

impl fmt::Display for SessionCleanupResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.dry_run {
            writeln!(
                f,
                "Dry run — {} session(s) would be deleted:",
                self.sessions.len()
            )?;
        } else {
            writeln!(
                f,
                "Cleanup complete — {} deleted, {} failed",
                self.deleted, self.failed
            )?;
        }
        for s in &self.sessions {
            let icon = match s.result.as_str() {
                "deleted" => "✓",
                "would_delete" => "~",
                _ => "✗",
            };
            writeln!(f, "  {icon} {} ({}, {})", s.id, s.kind, s.status)?;
        }
        Ok(())
    }
}

// ── Session export ────────────────────────────────────────────────────────────

/// A single transcript entry returned by the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub id: String,
    pub turn_id: Option<String>,
    pub seq: i32,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

impl fmt::Display for TranscriptEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let turn = self.turn_id.as_deref().unwrap_or("-");
        write!(f, "  [{:>3}] {:<20} turn={}", self.seq, self.kind, turn)?;
        // For user/assistant messages show a content preview.
        match self.kind.as_str() {
            "user_message" => {
                if let Some(msg) = self.payload["message"].as_str() {
                    let preview: String = msg.chars().take(80).collect();
                    write!(f, "  {preview}")?;
                    if msg.len() > 80 {
                        write!(f, "…")?;
                    }
                }
            }
            "assistant_message" => {
                if let Some(content) = self.payload["content"].as_str() {
                    let preview: String = content.chars().take(80).collect();
                    write!(f, "  {preview}")?;
                    if content.len() > 80 {
                        write!(f, "…")?;
                    }
                }
            }
            "tool_request" => {
                if let Some(name) = self.payload["tool_name"].as_str() {
                    write!(f, "  tool={name}")?;
                }
            }
            "tool_result" => {
                let is_err = self.payload["is_error"].as_bool().unwrap_or(false);
                if is_err {
                    write!(f, "  (error)")?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

// ── Session history ──────────────────────────────────────────────────────────

/// Response for `rune sessions history` — a focused, filterable transcript view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistoryResponse {
    pub session_id: String,
    pub total_entries: usize,
    pub shown_entries: usize,
    pub entries: Vec<TranscriptEntry>,
}

impl fmt::Display for SessionHistoryResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Session history: {} (showing {}/{})",
            self.session_id, self.shown_entries, self.total_entries
        )?;
        writeln!(f, "{}", "─".repeat(72))?;
        for entry in &self.entries {
            let ts = &entry.created_at;
            let turn_label = entry.turn_id.as_deref().unwrap_or("-");
            match entry.kind.as_str() {
                "user_message" => {
                    let msg = entry.payload["message"].as_str().unwrap_or("<no message>");
                    writeln!(f, "[{ts}] turn={turn_label}")?;
                    writeln!(f, "  ▶ User:")?;
                    for line in msg.lines() {
                        writeln!(f, "    {line}")?;
                    }
                }
                "assistant_message" => {
                    let content = entry.payload["content"].as_str().unwrap_or("<no content>");
                    writeln!(f, "[{ts}] turn={turn_label}")?;
                    writeln!(f, "  ◀ Assistant:")?;
                    for line in content.lines() {
                        writeln!(f, "    {line}")?;
                    }
                }
                "tool_request" => {
                    let tool = entry.payload["tool_name"].as_str().unwrap_or("unknown");
                    writeln!(f, "[{ts}] turn={turn_label}")?;
                    writeln!(f, "  ⚙ Tool call: {tool}")?;
                }
                "tool_result" => {
                    let is_err = entry.payload["is_error"].as_bool().unwrap_or(false);
                    let label = if is_err {
                        "✗ Tool error"
                    } else {
                        "✓ Tool result"
                    };
                    writeln!(f, "[{ts}] turn={turn_label}")?;
                    writeln!(f, "  {label}")?;
                }
                other => {
                    writeln!(f, "[{ts}] turn={turn_label}")?;
                    writeln!(f, "  ({other})")?;
                }
            }
        }
        if self.shown_entries == 0 {
            writeln!(f, "  (no matching entries)")?;
        }
        write!(f, "{}", "─".repeat(72))
    }
}

/// Full session export bundle: session detail + transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExportBundle {
    pub session: SessionDetailResponse,
    pub transcript: Vec<TranscriptEntry>,
}

impl fmt::Display for SessionExportBundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.session)?;
        writeln!(f, "  Transcript ({} items):", self.transcript.len())?;
        for entry in &self.transcript {
            writeln!(f, "{entry}")?;
        }
        Ok(())
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

/// Response for `message send`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSendResponse {
    pub success: bool,
    pub channel: String,
    pub message_id: Option<String>,
    pub detail: String,
}

impl fmt::Display for MessageSendResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        write!(f, "{icon} [{}] {}", self.channel, self.detail)?;
        if let Some(ref id) = self.message_id {
            write!(f, " (id={id})")?;
        }
        Ok(())
    }
}

/// A single hit returned by `message search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSearchHit {
    pub id: String,
    pub channel: Option<String>,
    pub session: Option<String>,
    pub sender: Option<String>,
    pub text: String,
    pub timestamp: Option<String>,
    pub score: Option<f64>,
}

impl fmt::Display for MessageSearchHit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ch = self.channel.as_deref().unwrap_or("?");
        let ts = self.timestamp.as_deref().unwrap_or("?");
        let sender = self.sender.as_deref().unwrap_or("unknown");
        write!(f, "[{ch}] {ts} {sender}: {}", self.text)?;
        if let Some(score) = self.score {
            write!(f, " (score={score:.2})")?;
        }
        Ok(())
    }
}

/// Response for `message search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSearchResponse {
    pub query: String,
    pub total: usize,
    pub hits: Vec<MessageSearchHit>,
}

impl fmt::Display for MessageSearchResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.hits.is_empty() {
            return write!(f, "No messages found for query: {}", self.query);
        }
        writeln!(
            f,
            "Message search: {} result{} for \"{}\"",
            self.total,
            if self.total == 1 { "" } else { "s" },
            self.query,
        )?;
        for hit in &self.hits {
            writeln!(f, "  {hit}")?;
        }
        Ok(())
    }
}

/// Per-channel result within a broadcast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBroadcastChannelResult {
    pub channel: String,
    pub success: bool,
    pub message_id: Option<String>,
    pub detail: String,
}

impl fmt::Display for MessageBroadcastChannelResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        write!(f, "{icon} [{}] {}", self.channel, self.detail)?;
        if let Some(id) = &self.message_id {
            write!(f, " (id: {id})")?;
        }
        Ok(())
    }
}

/// Response for `message broadcast`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBroadcastResponse {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub results: Vec<MessageBroadcastChannelResult>,
}

impl fmt::Display for MessageBroadcastResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Broadcast: {}/{} channel{} succeeded",
            self.succeeded,
            self.total,
            if self.total == 1 { "" } else { "s" },
        )?;
        for result in &self.results {
            writeln!(f, "  {result}")?;
        }
        Ok(())
    }
}

/// Response for `message react`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReactResponse {
    pub success: bool,
    pub message_id: String,
    pub emoji: String,
    pub removed: bool,
    pub detail: String,
}

impl fmt::Display for MessageReactResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        let verb = if self.removed { "removed" } else { "added" };
        write!(
            f,
            "{icon} {verb} {} on message {}",
            self.emoji, self.message_id,
        )?;
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}

/// Response from a message pin/unpin operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePinResponse {
    pub success: bool,
    pub message_id: String,
    pub pinned: bool,
    pub detail: String,
}

impl fmt::Display for MessagePinResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        let verb = if self.pinned { "pinned" } else { "unpinned" };
        write!(f, "{icon} {verb} message {}", self.message_id)?;
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}

/// Result of editing a message's text content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEditResponse {
    pub success: bool,
    pub message_id: String,
    pub channel: String,
    pub detail: String,
}

impl fmt::Display for MessageEditResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        write!(
            f,
            "{icon} message {} on {}: {}",
            self.message_id, self.channel, self.detail,
        )
    }
}

/// Result of deleting a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeleteResponse {
    pub success: bool,
    pub message_id: String,
    pub channel: String,
    pub detail: String,
}

impl fmt::Display for MessageDeleteResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        write!(
            f,
            "{icon} message {} on {}: {}",
            self.message_id, self.channel, self.detail,
        )
    }
}

/// Response for `message read` — fetch a single message by ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReadResponse {
    pub success: bool,
    pub message_id: String,
    pub channel: String,
    pub sender: Option<String>,
    pub text: Option<String>,
    pub timestamp: Option<String>,
    pub thread_id: Option<String>,
    pub detail: String,
}

impl fmt::Display for MessageReadResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.success {
            return write!(f, "✗ message {}: {}", self.message_id, self.detail);
        }
        let sender = self.sender.as_deref().unwrap_or("unknown");
        let ts = self.timestamp.as_deref().unwrap_or("?");
        let text = self.text.as_deref().unwrap_or("");
        write!(f, "[{}] {} {sender}: {text}", self.channel, ts)?;
        if let Some(ref tid) = self.thread_id {
            write!(f, " (thread={tid})")?;
        }
        write!(f, " (id={})", self.message_id)
    }
}

/// A single message within a thread listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMessage {
    pub id: String,
    pub sender: Option<String>,
    pub text: String,
    pub timestamp: Option<String>,
}

impl fmt::Display for ThreadMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sender = self.sender.as_deref().unwrap_or("unknown");
        let ts = self.timestamp.as_deref().unwrap_or("?");
        write!(f, "{ts} {sender}: {}", self.text)
    }
}

/// Response for `message thread list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageThreadListResponse {
    pub thread_id: String,
    pub total: usize,
    pub messages: Vec<ThreadMessage>,
}

impl fmt::Display for MessageThreadListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.messages.is_empty() {
            return write!(f, "No messages in thread {}", self.thread_id);
        }
        writeln!(
            f,
            "Thread {}: {} message{}",
            self.thread_id,
            self.total,
            if self.total == 1 { "" } else { "s" },
        )?;
        for msg in &self.messages {
            writeln!(f, "  {msg}")?;
        }
        Ok(())
    }
}

/// Response for `message thread reply`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageThreadReplyResponse {
    pub success: bool,
    pub thread_id: String,
    pub message_id: Option<String>,
    pub detail: String,
}

impl fmt::Display for MessageThreadReplyResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        write!(f, "{icon} thread {}: {}", self.thread_id, self.detail)?;
        if let Some(ref id) = self.message_id {
            write!(f, " (id={id})")?;
        }
        Ok(())
    }
}

/// Response for `message voice send`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageVoiceSendResponse {
    pub success: bool,
    pub channel: String,
    pub bytes_synthesized: usize,
    pub output_path: Option<String>,
    pub channel_delivered: bool,
    pub message_id: Option<String>,
    pub detail: String,
}

impl fmt::Display for MessageVoiceSendResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        write!(f, "{icon} [{}] {}", self.channel, self.detail)?;
        if let Some(ref path) = self.output_path {
            write!(f, " (saved: {path})")?;
        }
        if let Some(ref id) = self.message_id {
            write!(f, " (id={id})")?;
        }
        Ok(())
    }
}

/// A TTS voice entry from the engine's available voices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsVoiceDetail {
    pub id: String,
    pub name: String,
    pub language: String,
}

impl fmt::Display for TtsVoiceDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}, {})", self.id, self.name, self.language)
    }
}

/// Response for `message voice status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageVoiceStatusResponse {
    pub enabled: bool,
    pub provider: String,
    pub voice: String,
    pub model: String,
    pub auto_mode: String,
    pub voices: Vec<TtsVoiceDetail>,
}

impl fmt::Display for MessageVoiceStatusResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.enabled { "✓" } else { "✗" };
        writeln!(
            f,
            "{icon} TTS engine: {}",
            if self.enabled { "enabled" } else { "disabled" },
        )?;
        writeln!(f, "  Provider: {}", self.provider)?;
        writeln!(f, "  Voice:    {}", self.voice)?;
        writeln!(f, "  Model:    {}", self.model)?;
        writeln!(f, "  Auto:     {}", self.auto_mode)?;
        if !self.voices.is_empty() {
            writeln!(f, "  Available voices:")?;
            for v in &self.voices {
                writeln!(f, "    {v}")?;
            }
        }
        Ok(())
    }
}

/// Response for `message tag add` / `message tag remove`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTagResponse {
    pub success: bool,
    pub message_id: String,
    pub tag: String,
    pub added: bool,
    pub detail: String,
}

impl fmt::Display for MessageTagResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        let verb = if self.added { "tagged" } else { "untagged" };
        write!(
            f,
            "{icon} {verb} message {} with \"{}\"",
            self.message_id, self.tag,
        )?;
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}

/// Response for `message tag list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTagListResponse {
    pub message_id: String,
    pub tags: Vec<String>,
}

impl fmt::Display for MessageTagListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.tags.is_empty() {
            return write!(f, "No tags on message {}", self.message_id);
        }
        write!(
            f,
            "Message {}: {} tag{}",
            self.message_id,
            self.tags.len(),
            if self.tags.len() == 1 { "" } else { "s" },
        )?;
        for tag in &self.tags {
            write!(f, "\n  {tag}")?;
        }
        Ok(())
    }
}

/// Response for `message ack` — acknowledge (mark as read/received) a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAckResponse {
    pub success: bool,
    pub message_id: String,
    pub channel: String,
    pub detail: String,
}

impl fmt::Display for MessageAckResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        write!(
            f,
            "{icon} acknowledged message {} on {}",
            self.message_id, self.channel,
        )?;
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}

/// A single reaction on a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionDetail {
    pub emoji: String,
    pub count: u64,
    pub users: Vec<String>,
}

/// Response for `message list-reactions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReactionListResponse {
    pub message_id: String,
    pub reactions: Vec<ReactionDetail>,
}

impl fmt::Display for MessageReactionListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.reactions.is_empty() {
            return write!(f, "No reactions on message {}", self.message_id);
        }
        write!(
            f,
            "Message {}: {} reaction{}",
            self.message_id,
            self.reactions.len(),
            if self.reactions.len() == 1 { "" } else { "s" },
        )?;
        for r in &self.reactions {
            if r.users.is_empty() {
                write!(f, "\n  {} ×{}", r.emoji, r.count)?;
            } else {
                write!(f, "\n  {} ×{} ({})", r.emoji, r.count, r.users.join(", "),)?;
            }
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
            if self.delivered {
                "delivered"
            } else {
                "pending"
            }
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
    fn render_agent_list_empty() {
        let l = AgentListResponse { agents: vec![] };
        assert_eq!(
            render(&l, OutputFormat::Human),
            "No active subagent sessions."
        );
    }

    #[test]
    fn render_agent_list_with_entry() {
        let l = AgentListResponse {
            agents: vec![AgentSummary {
                id: "sub-1".into(),
                status: "running".into(),
                parent_session_id: Some("parent-abc".into()),
                created_at: Some("2026-03-19T00:00:00Z".into()),
                turn_count: Some(3),
                usage_prompt_tokens: Some(100),
                usage_completion_tokens: Some(50),
                latest_model: Some("gpt-5".into()),
            }],
        };
        let out = render(&l, OutputFormat::Human);
        assert!(out.contains("sub-1"));
        assert!(out.contains("parent=parent-abc"));
        assert!(out.contains("turns=3"));
    }

    #[test]
    fn render_agent_detail() {
        let detail = AgentDetailResponse {
            id: "sub-1".into(),
            status: "running".into(),
            parent_session_id: Some("parent-abc".into()),
            created_at: Some("2026-03-19T00:00:00Z".into()),
            turn_count: Some(3),
            latest_model: Some("gpt-5".into()),
            usage_prompt_tokens: Some(100),
            usage_completion_tokens: Some(50),
            last_turn_started_at: None,
            last_turn_ended_at: None,
        };
        let out = render(&detail, OutputFormat::Human);
        assert!(out.contains("Agent: sub-1"));
        assert!(out.contains("Parent:  parent-abc"));
    }

    #[test]
    fn render_agent_tree_empty() {
        let tree = AgentTreeResponse { roots: vec![] };
        assert_eq!(render(&tree, OutputFormat::Human), "No sessions found.");
    }

    #[test]
    fn render_agent_tree_hierarchy() {
        let tree = AgentTreeResponse {
            roots: vec![AgentTreeNode {
                id: "root-1".into(),
                kind: "direct".into(),
                status: "running".into(),
                children: vec![
                    AgentTreeNode {
                        id: "child-a".into(),
                        kind: "subagent".into(),
                        status: "running".into(),
                        children: vec![AgentTreeNode {
                            id: "grandchild-1".into(),
                            kind: "subagent".into(),
                            status: "idle".into(),
                            children: vec![],
                        }],
                    },
                    AgentTreeNode {
                        id: "child-b".into(),
                        kind: "subagent".into(),
                        status: "idle".into(),
                        children: vec![],
                    },
                ],
            }],
        };
        let out = render(&tree, OutputFormat::Human);
        assert!(out.contains("root-1 [running] (direct)"));
        assert!(out.contains("child-a [running] (subagent)"));
        assert!(out.contains("grandchild-1 [idle] (subagent)"));
        assert!(out.contains("child-b [idle] (subagent)"));
        // Verify tree connectors are present
        assert!(out.contains("├──") || out.contains("└──"));
    }

    #[test]
    fn render_agent_tree_json() {
        let tree = AgentTreeResponse {
            roots: vec![AgentTreeNode {
                id: "root-1".into(),
                kind: "direct".into(),
                status: "running".into(),
                children: vec![],
            }],
        };
        let out = render(&tree, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["roots"][0]["id"], "root-1");
        assert_eq!(parsed["roots"][0]["kind"], "direct");
    }

    #[test]
    fn render_template_list_empty() {
        let list = TemplateListResponse { templates: vec![] };
        assert_eq!(
            render(&list, OutputFormat::Human),
            "No templates available."
        );
    }

    #[test]
    fn render_template_list_with_entries() {
        let list = TemplateListResponse {
            templates: vec![
                TemplateSummary {
                    slug: "coding-agent".into(),
                    name: "Coding Agent".into(),
                    description: "Implements features".into(),
                    category: "developer".into(),
                    mode: "coder".into(),
                    spells: vec!["file-tools".into(), "exec-tools".into()],
                },
                TemplateSummary {
                    slug: "monitor-agent".into(),
                    name: "Monitor Agent".into(),
                    description: "Watches health".into(),
                    category: "operator".into(),
                    mode: "ask".into(),
                    spells: vec!["status-tools".into()],
                },
            ],
        };
        let out = render(&list, OutputFormat::Human);
        assert!(out.contains("coding-agent"));
        assert!(out.contains("monitor-agent"));
        assert!(out.contains("developer"));
        assert!(out.contains("operator"));
        assert!(out.contains("SLUG"));
    }

    #[test]
    fn render_template_list_json() {
        let list = TemplateListResponse {
            templates: vec![TemplateSummary {
                slug: "coding-agent".into(),
                name: "Coding Agent".into(),
                description: "Implements features".into(),
                category: "developer".into(),
                mode: "coder".into(),
                spells: vec!["file-tools".into()],
            }],
        };
        let out = render(&list, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["templates"][0]["slug"], "coding-agent");
        assert_eq!(parsed["templates"][0]["spells"][0], "file-tools");
    }

    #[test]
    fn render_template_start_human() {
        let resp = TemplateStartResponse {
            session_id: "abc-123".into(),
            template_slug: "coding-agent".into(),
            template_name: "Coding Agent".into(),
            mode: "coder".into(),
            status: "idle".into(),
        };
        let out = render(&resp, OutputFormat::Human);
        assert!(out.contains("Session started from template."));
        assert!(out.contains("abc-123"));
        assert!(out.contains("Coding Agent"));
        assert!(out.contains("coding-agent"));
        assert!(out.contains("coder"));
        assert!(out.contains("idle"));
    }

    #[test]
    fn render_template_start_json() {
        let resp = TemplateStartResponse {
            session_id: "abc-123".into(),
            template_slug: "coding-agent".into(),
            template_name: "Coding Agent".into(),
            mode: "coder".into(),
            status: "idle".into(),
        };
        let out = render(&resp, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["session_id"], "abc-123");
        assert_eq!(parsed["template_slug"], "coding-agent");
        assert_eq!(parsed["mode"], "coder");
    }

    #[test]
    fn render_skill_list_empty() {
        let response = SkillListResponse { skills: vec![] };
        assert_eq!(render(&response, OutputFormat::Human), "No skills found.");
    }

    #[test]
    fn render_skill_list_human() {
        let response = SkillListResponse {
            skills: vec![
                SkillSummary {
                    name: "alpha".into(),
                    description: "First skill".into(),
                    enabled: true,
                    source_dir: "/data/skills/alpha".into(),
                    binary_path: Some("/data/skills/alpha/run.sh".into()),
                },
                SkillSummary {
                    name: "beta".into(),
                    description: "Second skill".into(),
                    enabled: false,
                    source_dir: "/data/skills/beta".into(),
                    binary_path: None,
                },
            ],
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("alpha [enabled]"));
        assert!(out.contains("beta [disabled]"));
        assert!(out.contains("Description: First skill"));
        assert!(out.contains("Source: /data/skills/alpha"));
        assert!(out.contains("Binary: /data/skills/alpha/run.sh"));
        assert!(out.contains("Binary: -"));
    }

    #[test]
    fn render_skill_list_json() {
        let response = SkillListResponse {
            skills: vec![SkillSummary {
                name: "alpha".into(),
                description: "First skill".into(),
                enabled: true,
                source_dir: "/data/skills/alpha".into(),
                binary_path: Some("/data/skills/alpha/run.sh".into()),
            }],
        };
        let out = render(&response, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["skills"][0]["name"], "alpha");
        assert_eq!(parsed["skills"][0]["enabled"], true);
        assert_eq!(
            parsed["skills"][0]["binary_path"],
            "/data/skills/alpha/run.sh"
        );
    }

    #[test]
    fn render_skill_check_human() {
        let response = SkillCheckResponse {
            success: true,
            discovered: 3,
            loaded: 2,
            removed: 1,
        };
        let out = render(&response, OutputFormat::Human);
        assert!(out.contains("Skill scan complete"));
        assert!(out.contains("discovered 3"));
        assert!(out.contains("loaded 2"));
        assert!(out.contains("removed 1"));
    }

    #[test]
    fn render_skill_check_json() {
        let response = SkillCheckResponse {
            success: true,
            discovered: 3,
            loaded: 2,
            removed: 1,
        };
        let out = render(&response, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["discovered"], 3);
        assert_eq!(parsed["loaded"], 2);
        assert_eq!(parsed["removed"], 1);
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
        assert!(
            out.contains("hamza-eastus2 [azure-openai] source=env:OPENAI_API_KEY creds=missing")
        );
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
                    kind: "direct".into(),
                    status: "running".into(),
                    channel: Some("telegram".into()),
                    requester_session_id: None,
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

    // ── Message family (#74) ─────────────────────────────────────────

    #[test]
    fn render_message_send_success() {
        let r = MessageSendResponse {
            success: true,
            channel: "telegram".into(),
            message_id: Some("msg-42".into()),
            detail: "Message sent".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.starts_with('✓'));
        assert!(out.contains("[telegram]"));
        assert!(out.contains("(id=msg-42)"));
    }

    #[test]
    fn render_message_send_failure() {
        let r = MessageSendResponse {
            success: false,
            channel: "discord".into(),
            message_id: None,
            detail: "Gateway returned HTTP 503".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.starts_with('✗'));
        assert!(out.contains("[discord]"));
        assert!(out.contains("503"));
        assert!(!out.contains("(id="));
    }

    #[test]
    fn render_message_send_json() {
        let r = MessageSendResponse {
            success: true,
            channel: "slack".into(),
            message_id: Some("msg-99".into()),
            detail: "Message sent".into(),
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(v["channel"], "slack");
        assert_eq!(v["message_id"], "msg-99");
    }

    #[test]
    fn render_message_search_empty() {
        let r = MessageSearchResponse {
            query: "nonexistent".into(),
            total: 0,
            hits: vec![],
        };
        let out = render(&r, OutputFormat::Human);
        assert_eq!(out, "No messages found for query: nonexistent");
    }

    #[test]
    fn render_message_search_with_results() {
        let r = MessageSearchResponse {
            query: "deploy".into(),
            total: 2,
            hits: vec![
                MessageSearchHit {
                    id: "msg-1".into(),
                    channel: Some("telegram".into()),
                    session: Some("sess-1".into()),
                    sender: Some("hamza".into()),
                    text: "deploy to staging".into(),
                    timestamp: Some("2026-03-19T10:00:00Z".into()),
                    score: Some(0.95),
                },
                MessageSearchHit {
                    id: "msg-2".into(),
                    channel: Some("discord".into()),
                    session: None,
                    sender: None,
                    text: "deploy rollback".into(),
                    timestamp: None,
                    score: None,
                },
            ],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("Message search: 2 results for \"deploy\""));
        assert!(out.contains("[telegram] 2026-03-19T10:00:00Z hamza: deploy to staging"));
        assert!(out.contains("(score=0.95)"));
        assert!(out.contains("[discord] ? unknown: deploy rollback"));
    }

    #[test]
    fn render_message_search_json() {
        let r = MessageSearchResponse {
            query: "test".into(),
            total: 1,
            hits: vec![MessageSearchHit {
                id: "msg-3".into(),
                channel: Some("slack".into()),
                session: Some("sess-2".into()),
                sender: Some("bot".into()),
                text: "test passed".into(),
                timestamp: Some("2026-03-19T12:00:00Z".into()),
                score: Some(0.88),
            }],
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["query"], "test");
        assert_eq!(v["total"], 1);
        assert_eq!(v["hits"][0]["id"], "msg-3");
        assert_eq!(v["hits"][0]["channel"], "slack");
        assert_eq!(v["hits"][0]["session"], "sess-2");
        assert_eq!(v["hits"][0]["text"], "test passed");
        assert!(v["hits"][0]["score"].as_f64().unwrap() > 0.87);
    }

    #[test]
    fn render_message_search_single_result_grammar() {
        let r = MessageSearchResponse {
            query: "one".into(),
            total: 1,
            hits: vec![MessageSearchHit {
                id: "msg-4".into(),
                channel: None,
                session: None,
                sender: None,
                text: "one match".into(),
                timestamp: None,
                score: None,
            }],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("1 result for"));
        assert!(!out.contains("results for"));
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

    #[test]
    fn render_message_broadcast_success() {
        let r = MessageBroadcastResponse {
            total: 2,
            succeeded: 2,
            failed: 0,
            results: vec![
                MessageBroadcastChannelResult {
                    channel: "telegram".into(),
                    success: true,
                    message_id: Some("msg-1".into()),
                    detail: "Message sent".into(),
                },
                MessageBroadcastChannelResult {
                    channel: "discord".into(),
                    success: true,
                    message_id: Some("msg-2".into()),
                    detail: "Message sent".into(),
                },
            ],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("2/2 channels succeeded"));
        assert!(out.contains("✓ [telegram]"));
        assert!(out.contains("✓ [discord]"));
    }

    #[test]
    fn render_message_broadcast_partial_failure() {
        let r = MessageBroadcastResponse {
            total: 2,
            succeeded: 1,
            failed: 1,
            results: vec![
                MessageBroadcastChannelResult {
                    channel: "telegram".into(),
                    success: true,
                    message_id: Some("msg-1".into()),
                    detail: "Message sent".into(),
                },
                MessageBroadcastChannelResult {
                    channel: "slack".into(),
                    success: false,
                    message_id: None,
                    detail: "Channel not configured".into(),
                },
            ],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("1/2 channels succeeded"));
        assert!(out.contains("✓ [telegram]"));
        assert!(out.contains("✗ [slack]"));
    }

    #[test]
    fn render_message_broadcast_json() {
        let r = MessageBroadcastResponse {
            total: 1,
            succeeded: 1,
            failed: 0,
            results: vec![MessageBroadcastChannelResult {
                channel: "telegram".into(),
                success: true,
                message_id: Some("msg-10".into()),
                detail: "Message sent".into(),
            }],
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["total"], 1);
        assert_eq!(v["succeeded"], 1);
        assert_eq!(v["failed"], 0);
        assert_eq!(v["results"][0]["channel"], "telegram");
        assert!(v["results"][0]["success"].as_bool().unwrap());
        assert_eq!(v["results"][0]["message_id"], "msg-10");
    }

    #[test]
    fn render_message_broadcast_single_channel_grammar() {
        let r = MessageBroadcastResponse {
            total: 1,
            succeeded: 1,
            failed: 0,
            results: vec![MessageBroadcastChannelResult {
                channel: "telegram".into(),
                success: true,
                message_id: None,
                detail: "sent".into(),
            }],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("1/1 channel succeeded"));
        assert!(!out.contains("channels"));
    }

    #[test]
    fn render_message_react_add_success() {
        let r = MessageReactResponse {
            success: true,
            message_id: "msg-42".into(),
            emoji: "👍".into(),
            removed: false,
            detail: "Reaction added".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✓"));
        assert!(out.contains("added"));
        assert!(out.contains("👍"));
        assert!(out.contains("msg-42"));
    }

    #[test]
    fn render_message_react_remove_success() {
        let r = MessageReactResponse {
            success: true,
            message_id: "msg-99".into(),
            emoji: "heart".into(),
            removed: true,
            detail: "Reaction removed".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✓"));
        assert!(out.contains("removed"));
        assert!(out.contains("heart"));
        assert!(out.contains("msg-99"));
    }

    #[test]
    fn render_message_react_failure() {
        let r = MessageReactResponse {
            success: false,
            message_id: "msg-1".into(),
            emoji: "👎".into(),
            removed: false,
            detail: "Gateway returned HTTP 404: Message not found".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✗"));
        assert!(out.contains("added"));
        assert!(out.contains("404"));
    }

    #[test]
    fn render_message_react_json() {
        let r = MessageReactResponse {
            success: true,
            message_id: "msg-42".into(),
            emoji: "👍".into(),
            removed: false,
            detail: "Reaction added".into(),
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["success"].as_bool().unwrap());
        assert_eq!(v["message_id"], "msg-42");
        assert_eq!(v["emoji"], "👍");
        assert!(!v["removed"].as_bool().unwrap());
        assert_eq!(v["detail"], "Reaction added");
    }

    #[test]
    fn render_message_react_empty_detail() {
        let r = MessageReactResponse {
            success: true,
            message_id: "msg-1".into(),
            emoji: "fire".into(),
            removed: false,
            detail: String::new(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✓ added fire on message msg-1"));
        assert!(!out.contains(":"));
    }

    // ── MessageEditResponse ──────────────────────────────────────────

    #[test]
    fn message_edit_response_human_success() {
        let r = MessageEditResponse {
            success: true,
            message_id: "msg-42".into(),
            channel: "telegram".into(),
            detail: "Message edited".into(),
        };
        let out = r.to_string();
        assert!(out.contains("✓"));
        assert!(out.contains("msg-42"));
        assert!(out.contains("telegram"));
        assert!(out.contains("Message edited"));
    }

    #[test]
    fn message_edit_response_human_failure() {
        let r = MessageEditResponse {
            success: false,
            message_id: "msg-99".into(),
            channel: "discord".into(),
            detail: "Gateway returned HTTP 404: Message not found".into(),
        };
        let out = r.to_string();
        assert!(out.contains("✗"));
        assert!(out.contains("msg-99"));
        assert!(out.contains("discord"));
        assert!(out.contains("404"));
    }

    #[test]
    fn message_edit_response_json() {
        let r = MessageEditResponse {
            success: true,
            message_id: "msg-42".into(),
            channel: "telegram".into(),
            detail: "Message edited".into(),
        };
        let json_out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&json_out).unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(v["message_id"], "msg-42");
        assert_eq!(v["channel"], "telegram");
        assert_eq!(v["detail"], "Message edited");
    }

    // ── MessagePinResponse ──────────────────────────────────────────

    #[test]
    fn message_pin_response_human_pin() {
        let r = MessagePinResponse {
            success: true,
            message_id: "msg-50".into(),
            pinned: true,
            detail: "Message pinned".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✓ pinned message msg-50"));
        assert!(out.contains("Message pinned"));
    }

    #[test]
    fn message_pin_response_human_unpin() {
        let r = MessagePinResponse {
            success: true,
            message_id: "msg-77".into(),
            pinned: false,
            detail: "Message unpinned".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✓ unpinned message msg-77"));
        assert!(out.contains("Message unpinned"));
    }

    #[test]
    fn message_pin_response_human_failure() {
        let r = MessagePinResponse {
            success: false,
            message_id: "msg-99".into(),
            pinned: true,
            detail: "Gateway returned HTTP 404: Message not found".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✗ pinned message msg-99"));
        assert!(out.contains("404"));
    }

    #[test]
    fn message_pin_response_json() {
        let r = MessagePinResponse {
            success: true,
            message_id: "msg-50".into(),
            pinned: true,
            detail: "Message pinned".into(),
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(v["message_id"], "msg-50");
        assert_eq!(v["pinned"], true);
        assert_eq!(v["detail"], "Message pinned");
    }

    #[test]
    fn message_pin_response_empty_detail() {
        let r = MessagePinResponse {
            success: true,
            message_id: "msg-1".into(),
            pinned: true,
            detail: String::new(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✓ pinned message msg-1"));
        assert!(!out.contains(":"));
    }

    // ── MessageDeleteResponse ───────────────────────────────────────

    #[test]
    fn render_message_delete_success() {
        let r = MessageDeleteResponse {
            success: true,
            message_id: "msg-42".into(),
            channel: "telegram".into(),
            detail: "Message deleted".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✓"));
        assert!(out.contains("msg-42"));
        assert!(out.contains("telegram"));
        assert!(out.contains("Message deleted"));
    }

    #[test]
    fn render_message_delete_failure() {
        let r = MessageDeleteResponse {
            success: false,
            message_id: "msg-99".into(),
            channel: "discord".into(),
            detail: "Gateway returned HTTP 404: Message not found".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✗"));
        assert!(out.contains("msg-99"));
        assert!(out.contains("404"));
    }

    #[test]
    fn render_message_delete_json() {
        let r = MessageDeleteResponse {
            success: true,
            message_id: "msg-42".into(),
            channel: "telegram".into(),
            detail: "Message deleted".into(),
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["success"].as_bool().unwrap());
        assert_eq!(v["message_id"], "msg-42");
        assert_eq!(v["channel"], "telegram");
        assert_eq!(v["detail"], "Message deleted");
    }

    // ── MessageReadResponse ──────────────────────────────────────────

    #[test]
    fn render_message_read_success_human() {
        let r = MessageReadResponse {
            success: true,
            message_id: "msg-42".into(),
            channel: "telegram".into(),
            sender: Some("alice".into()),
            text: Some("Hello world".into()),
            timestamp: Some("2026-03-19T12:00:00Z".into()),
            thread_id: None,
            detail: "Message retrieved".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("[telegram]"));
        assert!(out.contains("alice"));
        assert!(out.contains("Hello world"));
        assert!(out.contains("(id=msg-42)"));
    }

    #[test]
    fn render_message_read_success_with_thread() {
        let r = MessageReadResponse {
            success: true,
            message_id: "msg-77".into(),
            channel: "discord".into(),
            sender: Some("bob".into()),
            text: Some("threaded msg".into()),
            timestamp: Some("2026-03-19T14:00:00Z".into()),
            thread_id: Some("thr-10".into()),
            detail: "Message retrieved".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("(thread=thr-10)"));
        assert!(out.contains("bob"));
    }

    #[test]
    fn render_message_read_failure_human() {
        let r = MessageReadResponse {
            success: false,
            message_id: "msg-404".into(),
            channel: "telegram".into(),
            sender: None,
            text: None,
            timestamp: None,
            thread_id: None,
            detail: "Gateway returned HTTP 404: not found".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.starts_with("✗"));
        assert!(out.contains("msg-404"));
        assert!(out.contains("404"));
    }

    #[test]
    fn render_message_read_json() {
        let r = MessageReadResponse {
            success: true,
            message_id: "msg-42".into(),
            channel: "telegram".into(),
            sender: Some("alice".into()),
            text: Some("Hello world".into()),
            timestamp: Some("2026-03-19T12:00:00Z".into()),
            thread_id: None,
            detail: "Message retrieved".into(),
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["success"].as_bool().unwrap());
        assert_eq!(v["message_id"], "msg-42");
        assert_eq!(v["channel"], "telegram");
        assert_eq!(v["sender"], "alice");
        assert_eq!(v["text"], "Hello world");
    }

    #[test]
    fn render_message_read_success_no_sender() {
        let r = MessageReadResponse {
            success: true,
            message_id: "msg-50".into(),
            channel: "slack".into(),
            sender: None,
            text: Some("system message".into()),
            timestamp: None,
            thread_id: None,
            detail: "Message retrieved".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("unknown"));
        assert!(out.contains("system message"));
    }

    // ── MessageTagResponse ──────────────────────────────────────────

    #[test]
    fn render_message_tag_add_success() {
        let r = MessageTagResponse {
            success: true,
            message_id: "msg-42".into(),
            tag: "urgent".into(),
            added: true,
            detail: "Tag added".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✓"));
        assert!(out.contains("tagged"));
        assert!(out.contains("msg-42"));
        assert!(out.contains("\"urgent\""));
        assert!(out.contains("Tag added"));
    }

    #[test]
    fn render_message_tag_remove_success() {
        let r = MessageTagResponse {
            success: true,
            message_id: "msg-99".into(),
            tag: "followup".into(),
            added: false,
            detail: "Tag removed".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✓"));
        assert!(out.contains("untagged"));
        assert!(out.contains("msg-99"));
        assert!(out.contains("\"followup\""));
        assert!(out.contains("Tag removed"));
    }

    #[test]
    fn render_message_tag_failure() {
        let r = MessageTagResponse {
            success: false,
            message_id: "msg-1".into(),
            tag: "resolved".into(),
            added: true,
            detail: "Gateway returned HTTP 404: Message not found".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✗"));
        assert!(out.contains("tagged"));
        assert!(out.contains("404"));
    }

    #[test]
    fn render_message_tag_json() {
        let r = MessageTagResponse {
            success: true,
            message_id: "msg-42".into(),
            tag: "urgent".into(),
            added: true,
            detail: "Tag added".into(),
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["success"].as_bool().unwrap());
        assert_eq!(v["message_id"], "msg-42");
        assert_eq!(v["tag"], "urgent");
        assert!(v["added"].as_bool().unwrap());
        assert_eq!(v["detail"], "Tag added");
    }

    #[test]
    fn render_message_tag_empty_detail() {
        let r = MessageTagResponse {
            success: true,
            message_id: "msg-1".into(),
            tag: "done".into(),
            added: true,
            detail: String::new(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("✓ tagged message msg-1 with \"done\""));
        assert!(!out.contains(":"));
    }

    // ── MessageTagListResponse ────────────────────────────────────

    #[test]
    fn render_message_tag_list_empty() {
        let r = MessageTagListResponse {
            message_id: "msg-42".into(),
            tags: vec![],
        };
        let out = render(&r, OutputFormat::Human);
        assert_eq!(out, "No tags on message msg-42");
    }

    #[test]
    fn render_message_tag_list_with_tags() {
        let r = MessageTagListResponse {
            message_id: "msg-42".into(),
            tags: vec!["urgent".into(), "followup".into()],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("Message msg-42: 2 tags"));
        assert!(out.contains("urgent"));
        assert!(out.contains("followup"));
    }

    #[test]
    fn render_message_tag_list_single_grammar() {
        let r = MessageTagListResponse {
            message_id: "msg-99".into(),
            tags: vec!["resolved".into()],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("1 tag"));
        assert!(!out.contains("tags"));
    }

    #[test]
    fn render_message_tag_list_json() {
        let r = MessageTagListResponse {
            message_id: "msg-42".into(),
            tags: vec!["urgent".into(), "followup".into()],
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["message_id"], "msg-42");
        assert_eq!(v["tags"][0], "urgent");
        assert_eq!(v["tags"][1], "followup");
    }

    // ── MessageReactionListResponse ─────────────────────────────────

    #[test]
    fn render_message_reaction_list_empty() {
        let r = MessageReactionListResponse {
            message_id: "msg-42".into(),
            reactions: vec![],
        };
        let out = render(&r, OutputFormat::Human);
        assert_eq!(out, "No reactions on message msg-42");
    }

    #[test]
    fn render_message_reaction_list_with_reactions() {
        let r = MessageReactionListResponse {
            message_id: "msg-42".into(),
            reactions: vec![
                ReactionDetail {
                    emoji: "👍".into(),
                    count: 3,
                    users: vec!["alice".into(), "bob".into(), "carol".into()],
                },
                ReactionDetail {
                    emoji: "❤️".into(),
                    count: 1,
                    users: vec!["dave".into()],
                },
            ],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("Message msg-42: 2 reactions"));
        assert!(out.contains("👍 ×3"));
        assert!(out.contains("alice, bob, carol"));
        assert!(out.contains("❤️ ×1"));
        assert!(out.contains("dave"));
    }

    #[test]
    fn render_message_reaction_list_single_grammar() {
        let r = MessageReactionListResponse {
            message_id: "msg-99".into(),
            reactions: vec![ReactionDetail {
                emoji: "🎉".into(),
                count: 5,
                users: vec![],
            }],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("1 reaction"));
        assert!(!out.contains("reactions"));
        assert!(out.contains("🎉 ×5"));
        // No users → no parenthetical
        assert!(!out.contains("("));
    }

    #[test]
    fn render_message_reaction_list_json() {
        let r = MessageReactionListResponse {
            message_id: "msg-42".into(),
            reactions: vec![ReactionDetail {
                emoji: "👍".into(),
                count: 2,
                users: vec!["alice".into(), "bob".into()],
            }],
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["message_id"], "msg-42");
        assert_eq!(v["reactions"][0]["emoji"], "👍");
        assert_eq!(v["reactions"][0]["count"], 2);
        assert_eq!(v["reactions"][0]["users"][0], "alice");
    }

    // ── MessageThreadListResponse ──────────────────────────────────

    #[test]
    fn render_message_thread_list_empty() {
        let r = MessageThreadListResponse {
            thread_id: "thr-1".into(),
            total: 0,
            messages: vec![],
        };
        let out = render(&r, OutputFormat::Human);
        assert_eq!(out, "No messages in thread thr-1");
    }

    #[test]
    fn render_message_thread_list_with_messages() {
        let r = MessageThreadListResponse {
            thread_id: "thr-42".into(),
            total: 2,
            messages: vec![
                ThreadMessage {
                    id: "msg-1".into(),
                    sender: Some("hamza".into()),
                    text: "initial message".into(),
                    timestamp: Some("2026-03-19T10:00:00Z".into()),
                },
                ThreadMessage {
                    id: "msg-2".into(),
                    sender: None,
                    text: "follow-up".into(),
                    timestamp: None,
                },
            ],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("Thread thr-42: 2 messages"));
        assert!(out.contains("2026-03-19T10:00:00Z hamza: initial message"));
        assert!(out.contains("? unknown: follow-up"));
    }

    #[test]
    fn render_message_thread_list_single_grammar() {
        let r = MessageThreadListResponse {
            thread_id: "thr-99".into(),
            total: 1,
            messages: vec![ThreadMessage {
                id: "msg-1".into(),
                sender: Some("bot".into()),
                text: "only one".into(),
                timestamp: Some("2026-03-19T12:00:00Z".into()),
            }],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("1 message"));
        assert!(!out.contains("messages"));
    }

    #[test]
    fn render_message_thread_list_json() {
        let r = MessageThreadListResponse {
            thread_id: "thr-42".into(),
            total: 1,
            messages: vec![ThreadMessage {
                id: "msg-1".into(),
                sender: Some("hamza".into()),
                text: "hello thread".into(),
                timestamp: Some("2026-03-19T10:00:00Z".into()),
            }],
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["thread_id"], "thr-42");
        assert_eq!(v["total"], 1);
        assert_eq!(v["messages"][0]["id"], "msg-1");
        assert_eq!(v["messages"][0]["sender"], "hamza");
        assert_eq!(v["messages"][0]["text"], "hello thread");
    }

    // ── MessageThreadReplyResponse ─────────────────────────────────

    #[test]
    fn render_message_thread_reply_success() {
        let r = MessageThreadReplyResponse {
            success: true,
            thread_id: "thr-42".into(),
            message_id: Some("msg-new-1".into()),
            detail: "Reply sent".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.starts_with('✓'));
        assert!(out.contains("thread thr-42"));
        assert!(out.contains("Reply sent"));
        assert!(out.contains("(id=msg-new-1)"));
    }

    #[test]
    fn render_message_thread_reply_failure() {
        let r = MessageThreadReplyResponse {
            success: false,
            thread_id: "thr-99".into(),
            message_id: None,
            detail: "Gateway returned HTTP 404: Thread not found".into(),
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.starts_with('✗'));
        assert!(out.contains("thr-99"));
        assert!(out.contains("404"));
        assert!(!out.contains("(id="));
    }

    #[test]
    fn render_message_thread_reply_json() {
        let r = MessageThreadReplyResponse {
            success: true,
            thread_id: "thr-42".into(),
            message_id: Some("msg-new-1".into()),
            detail: "Reply sent".into(),
        };
        let out = render(&r, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(v["thread_id"], "thr-42");
        assert_eq!(v["message_id"], "msg-new-1");
        assert_eq!(v["detail"], "Reply sent");
    }

    // ── Session history tests ────────────────────────────────────────────

    fn make_transcript_entry(seq: i32, kind: &str, turn_id: Option<&str>) -> TranscriptEntry {
        let payload = match kind {
            "user_message" => serde_json::json!({"message": "Hello world"}),
            "assistant_message" => serde_json::json!({"content": "Hi there"}),
            "tool_request" => serde_json::json!({"tool_name": "read_file"}),
            "tool_result" => serde_json::json!({"is_error": false}),
            _ => serde_json::json!({}),
        };
        TranscriptEntry {
            id: format!("entry-{seq}"),
            turn_id: turn_id.map(String::from),
            seq,
            kind: kind.to_string(),
            payload,
            created_at: "2026-03-20T01:00:00Z".to_string(),
        }
    }

    #[test]
    fn render_session_history_human_mixed() {
        let resp = SessionHistoryResponse {
            session_id: "sess-abc".into(),
            total_entries: 4,
            shown_entries: 4,
            entries: vec![
                make_transcript_entry(1, "user_message", Some("t1")),
                make_transcript_entry(2, "assistant_message", Some("t1")),
                make_transcript_entry(3, "tool_request", Some("t2")),
                make_transcript_entry(4, "tool_result", Some("t2")),
            ],
        };
        let out = render(&resp, OutputFormat::Human);
        assert!(out.contains("Session history: sess-abc (showing 4/4)"));
        assert!(out.contains("▶ User:"));
        assert!(out.contains("Hello world"));
        assert!(out.contains("◀ Assistant:"));
        assert!(out.contains("Hi there"));
        assert!(out.contains("⚙ Tool call: read_file"));
        assert!(out.contains("✓ Tool result"));
    }

    #[test]
    fn render_session_history_empty() {
        let resp = SessionHistoryResponse {
            session_id: "sess-empty".into(),
            total_entries: 10,
            shown_entries: 0,
            entries: vec![],
        };
        let out = render(&resp, OutputFormat::Human);
        assert!(out.contains("showing 0/10"));
        assert!(out.contains("(no matching entries)"));
    }

    #[test]
    fn render_session_history_json() {
        let resp = SessionHistoryResponse {
            session_id: "sess-j".into(),
            total_entries: 1,
            shown_entries: 1,
            entries: vec![make_transcript_entry(1, "user_message", Some("t1"))],
        };
        let out = render(&resp, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["session_id"], "sess-j");
        assert_eq!(v["total_entries"], 1);
        assert_eq!(v["shown_entries"], 1);
        assert_eq!(v["entries"][0]["kind"], "user_message");
    }

    #[test]
    fn render_session_history_tool_error() {
        let mut entry = make_transcript_entry(1, "tool_result", Some("t1"));
        entry.payload = serde_json::json!({"is_error": true});
        let resp = SessionHistoryResponse {
            session_id: "sess-err".into(),
            total_entries: 1,
            shown_entries: 1,
            entries: vec![entry],
        };
        let out = render(&resp, OutputFormat::Human);
        assert!(out.contains("✗ Tool error"));
    }

    #[test]
    fn render_session_history_unknown_kind() {
        let resp = SessionHistoryResponse {
            session_id: "sess-u".into(),
            total_entries: 1,
            shown_entries: 1,
            entries: vec![make_transcript_entry(1, "system_event", None)],
        };
        let out = render(&resp, OutputFormat::Human);
        assert!(out.contains("(system_event)"));
        assert!(out.contains("turn=-"));
    }

    #[test]
    fn render_session_history_multiline_user_message() {
        let mut entry = make_transcript_entry(1, "user_message", Some("t1"));
        entry.payload = serde_json::json!({"message": "line one\nline two\nline three"});
        let resp = SessionHistoryResponse {
            session_id: "sess-ml".into(),
            total_entries: 1,
            shown_entries: 1,
            entries: vec![entry],
        };
        let out = render(&resp, OutputFormat::Human);
        assert!(out.contains("    line one"));
        assert!(out.contains("    line two"));
        assert!(out.contains("    line three"));
    }
}
