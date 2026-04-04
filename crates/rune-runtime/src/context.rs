use std::collections::HashSet;

use serde::{Deserialize, Serialize};

const STABLE_PREFIX_PADDING: &str = concat!(
    "## Prompt Cache Padding\n\n",
    "This stable prefix padding exists to help upstream providers like Azure OpenAI\n",
    "cross the automatic prompt-prefix caching threshold. Keep this text deterministic\n",
    "for a given runtime build so repeated turns share the same cached prefix.\n\n",
    "Cache padding block 01. Cache padding block 02. Cache padding block 03. Cache padding block 04.\n",
    "Cache padding block 05. Cache padding block 06. Cache padding block 07. Cache padding block 08.\n",
    "Cache padding block 09. Cache padding block 10. Cache padding block 11. Cache padding block 12.\n",
    "Cache padding block 13. Cache padding block 14. Cache padding block 15. Cache padding block 16.\n",
    "Cache padding block 17. Cache padding block 18. Cache padding block 19. Cache padding block 20.\n",
    "Cache padding block 21. Cache padding block 22. Cache padding block 23. Cache padding block 24.\n",
    "Cache padding block 25. Cache padding block 26. Cache padding block 27. Cache padding block 28.\n",
    "Cache padding block 29. Cache padding block 30. Cache padding block 31. Cache padding block 32.\n",
    "Cache padding block 33. Cache padding block 34. Cache padding block 35. Cache padding block 36.\n",
    "Cache padding block 37. Cache padding block 38. Cache padding block 39. Cache padding block 40.\n",
    "Cache padding block 41. Cache padding block 42. Cache padding block 43. Cache padding block 44.\n",
    "Cache padding block 45. Cache padding block 46. Cache padding block 47. Cache padding block 48.\n",
    "Cache padding block 49. Cache padding block 50. Cache padding block 51. Cache padding block 52.\n",
    "Cache padding block 53. Cache padding block 54. Cache padding block 55. Cache padding block 56.\n",
    "Cache padding block 57. Cache padding block 58. Cache padding block 59. Cache padding block 60.\n"
);

use rune_core::{AttachmentRef, SessionKind, TranscriptItem};
use rune_models::{ChatMessage, FunctionCall, ImageUrlPart, MessagePart, Role, ToolCallRequest};
use rune_store::models::TranscriptItemRow;
use tracing::warn;

use crate::compaction::CompactionStrategy;
use crate::memory::MemoryContext;
use crate::workspace::WorkspaceContext;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextTierKind {
    Identity,
    ActiveTask,
    Project,
    Shared,
    Historical,
}

impl ContextTierKind {
    #[must_use]
    pub fn default_priority(self) -> u8 {
        match self {
            Self::Identity => 0,
            Self::ActiveTask => 1,
            Self::Project => 2,
            Self::Shared => 3,
            Self::Historical => 4,
        }
    }

    #[must_use]
    pub fn default_staleness_policy(self) -> ContextStalenessPolicy {
        match self {
            Self::Identity => ContextStalenessPolicy::AlwaysFresh,
            Self::ActiveTask => ContextStalenessPolicy::PerTurn,
            Self::Project => ContextStalenessPolicy::PerSession,
            Self::Shared => ContextStalenessPolicy::OnDemand,
            Self::Historical => ContextStalenessPolicy::RetrievalOnly,
        }
    }

    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Identity => "Identity",
            Self::ActiveTask => "Active task",
            Self::Project => "Project context",
            Self::Shared => "Shared knowledge",
            Self::Historical => "Historical",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextStalenessPolicy {
    AlwaysFresh,
    PerTurn,
    PerSession,
    OnDemand,
    RetrievalOnly,
}

impl ContextStalenessPolicy {
    #[must_use]
    pub fn requires_refresh(&self) -> bool {
        matches!(self, Self::AlwaysFresh | Self::PerTurn)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextTierSpec {
    pub kind: ContextTierKind,
    pub token_budget: usize,
    pub priority: u8,
    pub staleness_policy: ContextStalenessPolicy,
}

impl ContextTierSpec {
    #[must_use]
    pub fn new(kind: ContextTierKind, token_budget: usize) -> Self {
        let priority = kind.clone().default_priority();
        let staleness_policy = kind.clone().default_staleness_policy();
        Self {
            kind,
            token_budget,
            priority,
            staleness_policy,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ContextTierUsage {
    pub kind: ContextTierKind,
    pub token_budget: usize,
    pub estimated_tokens: usize,
    pub priority: u8,
    pub staleness_policy: ContextStalenessPolicy,
    pub loaded: bool,
    pub refresh_required: bool,
    pub source: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ContextAssemblyReport {
    pub total_estimated_tokens: usize,
    pub total_budget: usize,
    pub compaction_trigger_tokens: usize,
    pub over_budget: bool,
    pub over_compaction_threshold: bool,
    pub compaction_required: bool,
    pub l3_cold_storage_enabled: bool,
    pub tiers: Vec<ContextTierUsage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextTierSnapshot {
    pub kind: ContextTierKind,
    pub token_budget: usize,
    pub priority: u8,
    pub staleness_policy: ContextStalenessPolicy,
    pub loaded: bool,
    pub refresh_required: bool,
    pub estimated_tokens: usize,
    pub source: String,
}

impl From<&ContextTierUsage> for ContextTierSnapshot {
    fn from(value: &ContextTierUsage) -> Self {
        Self {
            kind: value.kind.clone(),
            token_budget: value.token_budget,
            priority: value.priority,
            staleness_policy: value.staleness_policy.clone(),
            loaded: value.loaded,
            refresh_required: value.refresh_required,
            estimated_tokens: value.estimated_tokens,
            source: value.source.to_string(),
        }
    }
}

impl ContextAssemblyReport {
    #[must_use]
    pub fn snapshots(&self) -> Vec<ContextTierSnapshot> {
        self.tiers.iter().map(ContextTierSnapshot::from).collect()
    }

    #[must_use]
    pub fn identity_tokens(&self) -> usize {
        self.tokens_for(ContextTierKind::Identity)
    }

    #[must_use]
    pub fn project_tokens(&self) -> usize {
        self.tokens_for(ContextTierKind::Project)
    }

    #[must_use]
    pub fn tokens_for(&self, kind: ContextTierKind) -> usize {
        self.tiers
            .iter()
            .find(|tier| tier.kind == kind)
            .map(|tier| tier.estimated_tokens)
            .unwrap_or_default()
    }
}

fn parse_staleness_policy(value: &str) -> Option<ContextStalenessPolicy> {
    let normalized = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match normalized.as_str() {
        "always_fresh" => Some(ContextStalenessPolicy::AlwaysFresh),
        "per_turn" => Some(ContextStalenessPolicy::PerTurn),
        "per_session" => Some(ContextStalenessPolicy::PerSession),
        "on_demand" => Some(ContextStalenessPolicy::OnDemand),
        "retrieval_only" => Some(ContextStalenessPolicy::RetrievalOnly),
        _ => None,
    }
}

fn estimate_tokens(text: &str) -> usize {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        0
    } else {
        trimmed.chars().count().div_ceil(4)
    }
}

/// Builds the prompt messages from session history, system instructions, and context.
#[derive(Clone)]
pub struct ContextAssembler {
    system_instructions: String,
    tier_specs: Vec<ContextTierSpec>,
}

impl ContextAssembler {
    #[must_use]
    pub fn delegation_context_section(
        &self,
        delegation_context: &serde_json::Value,
    ) -> Option<String> {
        if !delegation_context.is_object() {
            return None;
        }

        let pretty = serde_json::to_string_pretty(delegation_context).ok()?;
        Some(format!(
            "## Delegation Context\n\nThe orchestrator preloaded this context slice. Use it before re-reading files.\n\n```json\n{pretty}\n```"
        ))
    }

    #[must_use]
    pub fn shared_scratchpad_section(
        &self,
        shared_scratchpad: &serde_json::Value,
    ) -> Option<String> {
        let path = shared_scratchpad.get("path")?.as_str()?.trim();
        if path.is_empty() {
            return None;
        }

        Some(format!(
            "## Shared Scratchpad\n\nCoordinate via this shared scratchpad path when you need to leave structured findings for the orchestrator:\n- `{path}`"
        ))
    }

    #[must_use]
    pub fn session_metadata_sections(
        &self,
        session_kind: SessionKind,
        session_metadata: &serde_json::Value,
    ) -> Vec<String> {
        if !matches!(session_kind, SessionKind::Subagent) {
            return Vec::new();
        }

        let mut sections = Vec::new();
        if let Some(section) = session_metadata
            .get("delegation_context")
            .and_then(|value| self.delegation_context_section(value))
        {
            sections.push(section);
        }
        if let Some(section) = session_metadata
            .get("shared_scratchpad")
            .and_then(|value| self.shared_scratchpad_section(value))
        {
            sections.push(section);
        }
        sections
    }

    pub fn new(system_instructions: impl Into<String>) -> Self {
        Self {
            system_instructions: system_instructions.into(),
            tier_specs: Self::default_tier_specs(),
        }
    }

    fn default_tier_specs() -> Vec<ContextTierSpec> {
        vec![
            ContextTierSpec::new(ContextTierKind::Identity, 1_000),
            ContextTierSpec::new(ContextTierKind::ActiveTask, 10_000),
            ContextTierSpec::new(ContextTierKind::Project, 20_000),
            ContextTierSpec::new(ContextTierKind::Shared, 5_000),
            ContextTierSpec::new(ContextTierKind::Historical, 8_000),
        ]
    }

    #[must_use]
    pub fn with_context_config(mut self, config: &rune_config::ContextConfig) -> Self {
        self.tier_specs = vec![
            ContextTierSpec {
                kind: ContextTierKind::Identity,
                token_budget: config.identity,
                priority: config.identity_priority,
                staleness_policy: parse_staleness_policy(&config.identity_staleness_policy)
                    .unwrap_or(ContextTierKind::Identity.default_staleness_policy()),
            },
            ContextTierSpec {
                kind: ContextTierKind::ActiveTask,
                token_budget: config.task,
                priority: config.task_priority,
                staleness_policy: parse_staleness_policy(&config.task_staleness_policy)
                    .unwrap_or(ContextTierKind::ActiveTask.default_staleness_policy()),
            },
            ContextTierSpec {
                kind: ContextTierKind::Project,
                token_budget: config.project,
                priority: config.project_priority,
                staleness_policy: parse_staleness_policy(&config.project_staleness_policy)
                    .unwrap_or(ContextTierKind::Project.default_staleness_policy()),
            },
            ContextTierSpec {
                kind: ContextTierKind::Shared,
                token_budget: config.shared,
                priority: config.shared_priority,
                staleness_policy: parse_staleness_policy(&config.shared_staleness_policy)
                    .unwrap_or(ContextTierKind::Shared.default_staleness_policy()),
            },
            ContextTierSpec {
                kind: ContextTierKind::Historical,
                token_budget: config.historical,
                priority: config.historical_priority,
                staleness_policy: parse_staleness_policy(&config.historical_staleness_policy)
                    .unwrap_or(ContextTierKind::Historical.default_staleness_policy()),
            },
        ];
        self
    }

    pub fn with_tier_budgets(
        mut self,
        identity: usize,
        active_task: usize,
        project: usize,
        shared: usize,
    ) -> Self {
        self.tier_specs = vec![
            ContextTierSpec::new(ContextTierKind::Identity, identity),
            ContextTierSpec::new(ContextTierKind::ActiveTask, active_task),
            ContextTierSpec::new(ContextTierKind::Project, project),
            ContextTierSpec::new(ContextTierKind::Shared, shared),
            ContextTierSpec::new(ContextTierKind::Historical, 8_000),
        ];
        self
    }

    #[must_use]
    pub fn with_tier_specs(mut self, tier_specs: Vec<ContextTierSpec>) -> Self {
        self.tier_specs = tier_specs;
        self
    }

    #[must_use]
    pub fn tier_specs(&self) -> &[ContextTierSpec] {
        &self.tier_specs
    }

    #[must_use]
    pub fn analyze_context_usage(
        &self,
        workspace: Option<&WorkspaceContext>,
        memory: Option<&MemoryContext>,
        extra_system_sections: &[String],
        compaction_trigger_tokens: usize,
        l3_loaded: bool,
    ) -> ContextAssemblyReport {
        let identity_section = self.system_instructions.trim();
        let active_task_section = extra_system_sections
            .iter()
            .filter(|section| !section.trim().is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n");
        let project_section = workspace
            .map(|workspace| WorkspaceContext {
                files: workspace.project_tier_files(),
            })
            .map(|workspace| workspace.format_for_prompt())
            .unwrap_or_default();
        let shared_section = memory
            .map(MemoryContext::format_for_prompt)
            .unwrap_or_default();

        let tiers = self
            .tier_specs
            .iter()
            .map(|spec| {
                let (estimated_tokens, loaded, source) = match spec.kind {
                    ContextTierKind::Identity => (
                        estimate_tokens(identity_section),
                        !identity_section.is_empty(),
                        "system_instructions",
                    ),
                    ContextTierKind::ActiveTask => (
                        estimate_tokens(&active_task_section),
                        !active_task_section.is_empty(),
                        "extra_system_sections",
                    ),
                    ContextTierKind::Project => (
                        estimate_tokens(&project_section),
                        !project_section.is_empty(),
                        "workspace_context",
                    ),
                    ContextTierKind::Shared => (
                        estimate_tokens(&shared_section),
                        !shared_section.is_empty(),
                        "memory_context",
                    ),
                    ContextTierKind::Historical => (0, l3_loaded, "transcript_history"),
                };

                ContextTierUsage {
                    kind: spec.kind.clone(),
                    token_budget: spec.token_budget,
                    estimated_tokens,
                    priority: spec.priority,
                    staleness_policy: spec.staleness_policy.clone(),
                    loaded,
                    refresh_required: loaded && spec.staleness_policy.requires_refresh(),
                    source,
                }
            })
            .collect::<Vec<_>>();

        let total_estimated_tokens = tiers.iter().map(|tier| tier.estimated_tokens).sum();
        let total_budget = self
            .tier_specs
            .iter()
            .map(|spec| spec.token_budget)
            .sum::<usize>();
        let over_budget = total_budget > 0 && total_estimated_tokens > total_budget;
        let over_compaction_threshold =
            compaction_trigger_tokens > 0 && total_estimated_tokens > compaction_trigger_tokens;

        ContextAssemblyReport {
            total_estimated_tokens,
            total_budget,
            compaction_trigger_tokens,
            over_budget,
            over_compaction_threshold,
            compaction_required: over_budget || over_compaction_threshold,
            l3_cold_storage_enabled: l3_loaded,
            tiers,
        }
    }

    /// Assemble prompt messages from persisted transcript rows.
    ///
    /// Produces: [system (with optional workspace + memory context)] + transcript items
    /// converted to ChatMessages, then passed through the compaction strategy.
    pub fn assemble(
        &self,
        transcript_rows: &[TranscriptItemRow],
        compaction: &dyn CompactionStrategy,
        workspace: Option<&WorkspaceContext>,
        memory: Option<&MemoryContext>,
        extra_system_sections: &[String],
    ) -> Vec<ChatMessage> {
        let mut messages = Vec::with_capacity(transcript_rows.len() + 1);

        // System message with optional workspace + memory context
        let mut sections = vec![self.system_instructions.clone()];

        let context_report = self.analyze_context_usage(
            workspace,
            memory,
            extra_system_sections,
            0,
            compaction.persists_compacted_context(),
        );
        let extra_task_sections = extra_system_sections
            .iter()
            .filter(|section| !section.trim().is_empty())
            .cloned()
            .collect::<Vec<_>>();

        for tier in &context_report.tiers {
            match tier.kind {
                ContextTierKind::Identity | ContextTierKind::Historical => {}
                ContextTierKind::ActiveTask => {
                    sections.extend(extra_task_sections.iter().cloned());
                }
                ContextTierKind::Project => {
                    if let Some(workspace) = workspace {
                        let workspace_section = workspace.format_for_prompt();
                        if !workspace_section.is_empty() {
                            sections.push(workspace_section);
                        }
                    }
                }
                ContextTierKind::Shared => {
                    if let Some(mem) = memory {
                        let mem_section = mem.format_for_prompt();
                        if !mem_section.is_empty() {
                            sections.push(mem_section);
                        }
                    }
                }
            }
        }

        sections.push(STABLE_PREFIX_PADDING.to_string());

        let system_content = sections.join("\n\n");

        messages.push(ChatMessage {
            role: Role::System,
            content: Some(system_content),
            content_parts: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        });

        // Convert transcript rows to chat messages.
        // Group consecutive ToolRequest items into a single Assistant message
        // with multiple tool_calls, as the OpenAI API requires.
        let mut i = 0;
        while i < transcript_rows.len() {
            let item: TranscriptItem =
                match serde_json::from_value(transcript_rows[i].payload.clone()) {
                    Ok(item) => item,
                    Err(_) => {
                        i += 1;
                        continue;
                    }
                };

            if matches!(item, TranscriptItem::ToolRequest { .. }) {
                // Collect consecutive ToolRequests into one assistant message
                let mut tool_calls = Vec::new();
                while i < transcript_rows.len() {
                    let inner: TranscriptItem =
                        match serde_json::from_value(transcript_rows[i].payload.clone()) {
                            Ok(item) => item,
                            Err(_) => break,
                        };
                    if let TranscriptItem::ToolRequest {
                        tool_call_id,
                        tool_name,
                        arguments,
                    } = inner
                    {
                        tool_calls.push(ToolCallRequest {
                            id: tool_call_id.to_string(),
                            call_type: "function".to_string(),
                            function: FunctionCall {
                                name: tool_name,
                                arguments: arguments.to_string(),
                            },
                        });
                        i += 1;
                    } else {
                        break;
                    }
                }
                if !tool_calls.is_empty() {
                    messages.push(ChatMessage {
                        role: Role::Assistant,
                        content: None,
                        content_parts: None,
                        name: None,
                        tool_call_id: None,
                        tool_calls: Some(tool_calls),
                    });
                }
            } else if let Some(msg) = self.item_to_chat_message(item) {
                messages.push(msg);
                i += 1;
            } else {
                i += 1;
            }
        }

        sanitize_tool_calls(&mut messages);
        compaction.compact(messages)
    }

    fn item_to_chat_message(&self, item: TranscriptItem) -> Option<ChatMessage> {
        match item {
            TranscriptItem::UserMessage { message } => {
                let content = render_user_message_content(&message.content, &message.attachments);
                let content_parts =
                    build_user_message_parts(&message.content, &message.attachments);
                if content.trim().is_empty() && content_parts.is_none() {
                    return None;
                }
                Some(ChatMessage {
                    role: Role::User,
                    content: Some(content),
                    content_parts,
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                })
            }
            TranscriptItem::AssistantMessage { content } => {
                if content.trim().is_empty() {
                    return None;
                }
                Some(ChatMessage {
                    role: Role::Assistant,
                    content: Some(content),
                    content_parts: None,
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                })
            }
            // ToolRequest is handled by the grouping logic in assemble()
            TranscriptItem::ToolRequest { .. } => None,
            TranscriptItem::ToolResult {
                tool_call_id,
                output,
                ..
            } => Some(ChatMessage {
                role: Role::Tool,
                content: Some(output),
                content_parts: None,
                name: None,
                tool_call_id: Some(tool_call_id.to_string()),
                tool_calls: None,
            }),
            _ => None,
        }
    }
}

/// Ensures tool call/result sequences obey the provider contract:
/// an assistant message with `tool_calls` must be followed by a contiguous
/// block of matching `Role::Tool` messages, and `Role::Tool` messages may not
/// appear anywhere else. Missing tool responses are synthesized, stray/late
/// tool results are dropped.
fn sanitize_tool_calls(messages: &mut Vec<ChatMessage>) {
    let original = std::mem::take(messages);
    let mut sanitized = Vec::with_capacity(original.len());
    let mut stray_results = 0usize;
    let mut synthesized_results = 0usize;
    let mut i = 0usize;

    while i < original.len() {
        let msg = original[i].clone();

        match msg.role {
            Role::Assistant
                if msg
                    .tool_calls
                    .as_ref()
                    .is_some_and(|calls| !calls.is_empty()) =>
            {
                let pending_ids: Vec<String> = msg
                    .tool_calls
                    .as_ref()
                    .into_iter()
                    .flat_map(|calls| calls.iter().map(|tc| tc.id.clone()))
                    .collect();
                let pending_set: HashSet<String> = pending_ids.iter().cloned().collect();
                let mut seen = HashSet::new();
                let mut tool_block = Vec::new();
                let mut j = i + 1;

                while j < original.len() && original[j].role == Role::Tool {
                    let tool_msg = original[j].clone();
                    match tool_msg.tool_call_id.as_ref() {
                        Some(id) if pending_set.contains(id) && seen.insert(id.clone()) => {
                            tool_block.push(tool_msg);
                        }
                        _ => {
                            stray_results += 1;
                        }
                    }
                    j += 1;
                }

                sanitized.push(msg);

                let mut missing: Vec<String> = pending_ids
                    .into_iter()
                    .filter(|id| !seen.contains(id))
                    .collect();
                missing.sort();
                synthesized_results += missing.len();

                sanitized.extend(tool_block);
                for id in missing {
                    sanitized.push(ChatMessage {
                        role: Role::Tool,
                        content: Some("[Tool call interrupted — no result available]".to_string()),
                        content_parts: None,
                        name: None,
                        tool_call_id: Some(id),
                        tool_calls: None,
                    });
                }

                i = j;
            }
            Role::Tool => {
                stray_results += 1;
                i += 1;
            }
            _ => {
                sanitized.push(msg);
                i += 1;
            }
        }
    }

    if stray_results > 0 {
        warn!(
            stray_results,
            "removing tool result messages outside contiguous assistant tool_call blocks"
        );
    }
    if synthesized_results > 0 {
        warn!(
            synthesized_results,
            "injecting synthetic tool responses for interrupted tool_call blocks"
        );
    }

    *messages = sanitized;
}

fn build_user_message_parts(
    content: &str,
    attachments: &[AttachmentRef],
) -> Option<Vec<MessagePart>> {
    let trimmed = content.trim();
    let image_refs = collect_image_refs(attachments);
    let mut parts = Vec::new();

    let text = multimodal_user_text(trimmed, !image_refs.is_empty());
    if !text.is_empty() {
        parts.push(MessagePart::Text { text });
    }

    for image_ref in image_refs {
        parts.push(MessagePart::ImageUrl {
            image_url: ImageUrlPart { url: image_ref },
        });
    }

    if parts.is_empty() { None } else { Some(parts) }
}

fn collect_image_refs(attachments: &[AttachmentRef]) -> Vec<String> {
    attachments
        .iter()
        .filter_map(attachment_image_ref)
        .collect()
}

fn multimodal_user_text(content: &str, has_images: bool) -> String {
    if has_images {
        if content.is_empty() {
            "The user sent this image. Describe what you see and respond to their message.".into()
        } else {
            format!(
                "The user sent this image. Describe what you see and respond to their message. User message: {content}"
            )
        }
    } else {
        content.to_string()
    }
}

fn attachment_image_ref(attachment: &AttachmentRef) -> Option<String> {
    let mime = attachment.mime_type.as_deref().unwrap_or("");
    if !mime.starts_with("image/") {
        return None;
    }

    if let Some(url) = attachment.url.as_deref() {
        if !url.starts_with("telegram-file:") {
            return Some(url.to_string());
        }
    }

    attachment
        .provider_file_id
        .as_ref()
        .map(|file_id| format!("provider-file:{file_id}"))
}

fn render_user_message_content(content: &str, attachments: &[AttachmentRef]) -> String {
    let trimmed = content.trim();
    if attachments.is_empty() {
        return trimmed.to_string();
    }

    let mut rendered = String::new();
    if !trimmed.is_empty() {
        rendered.push_str(trimmed);
        rendered.push_str("\n\n");
    }

    rendered.push_str("[Attachments]\n");
    for attachment in attachments {
        rendered.push_str("- ");
        rendered.push_str(&format_attachment_ref(attachment));
        rendered.push('\n');
    }

    rendered.trim_end().to_string()
}

fn format_attachment_ref(attachment: &AttachmentRef) -> String {
    let mut line = attachment.name.clone();
    let mut details = Vec::new();

    if let Some(mime) = attachment.mime_type.as_deref() {
        details.push(mime.to_string());
    }
    if let Some(size_bytes) = attachment.size_bytes {
        details.push(format!("{} bytes", size_bytes));
    }
    if let Some(provider_file_id) = attachment.provider_file_id.as_deref() {
        details.push(format!("provider_file_id={provider_file_id}"));
    }
    if let Some(url) = attachment.url.as_deref() {
        details.push(format!("url={url}"));
    }

    if !details.is_empty() {
        line.push_str(" (");
        line.push_str(&details.join(", "));
        line.push(')');
    }

    line
}

#[cfg(test)]
mod attachment_prompt_tests {
    use super::{
        ContextAssembler, attachment_image_ref, build_user_message_parts, format_attachment_ref,
        render_user_message_content,
    };
    use rune_core::{AttachmentRef, NormalizedMessage, TranscriptItem};
    use rune_models::{MessagePart, Role};
    use rune_store::models::TranscriptItemRow;
    use uuid::Uuid;

    use crate::NoOpCompaction;

    #[test]
    fn formats_attachment_only_user_messages_into_prompt_content() {
        let item = TranscriptItem::UserMessage {
            message: NormalizedMessage {
                channel_id: None,
                sender_id: "user".into(),
                sender_display_name: None,
                message_id: Some("m1".into()),
                reply_to_message_id: None,
                content: String::new(),
                attachments: vec![AttachmentRef {
                    name: "invoice.pdf".into(),
                    mime_type: Some("application/pdf".into()),
                    size_bytes: Some(1234),
                    url: Some("https://example.test/invoice.pdf".into()),
                    provider_file_id: Some("file_123".into()),
                }],
                metadata: serde_json::Value::Null,
            },
        };
        let row = TranscriptItemRow {
            id: Uuid::now_v7(),
            session_id: Uuid::now_v7(),
            turn_id: None,
            seq: 1,
            kind: "user_message".into(),
            payload: serde_json::to_value(item).unwrap(),
            created_at: chrono::Utc::now(),
        };

        let assembler = ContextAssembler::new("system");
        let messages = assembler.assemble(&[row], &NoOpCompaction, None, None, &[]);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].role, Role::User);
        let content = messages[1].content.as_deref().unwrap();
        assert!(content.contains("[Attachments]"));
        assert!(content.contains("invoice.pdf"));
        assert!(content.contains("application/pdf"));
        assert!(content.contains("provider_file_id=file_123"));
    }

    #[test]
    fn appends_attachment_summary_after_text_content() {
        let rendered = render_user_message_content(
            "Please summarize this",
            &[AttachmentRef {
                name: "notes.txt".into(),
                mime_type: Some("text/plain".into()),
                size_bytes: None,
                url: None,
                provider_file_id: None,
            }],
        );

        assert!(rendered.starts_with("Please summarize this\n\n[Attachments]\n- notes.txt"));
    }

    #[test]
    fn formats_attachment_ref_compactly() {
        let formatted = format_attachment_ref(&AttachmentRef {
            name: "photo.jpg".into(),
            mime_type: Some("image/jpeg".into()),
            size_bytes: Some(42),
            url: None,
            provider_file_id: Some("abc".into()),
        });

        assert_eq!(
            formatted,
            "photo.jpg (image/jpeg, 42 bytes, provider_file_id=abc)"
        );
    }
    #[test]
    fn prefers_attachment_url_for_http_image_parts() {
        let attachment = AttachmentRef {
            name: "photo.jpg".into(),
            mime_type: Some("image/jpeg".into()),
            size_bytes: Some(42),
            url: Some("https://example.test/photo.jpg".into()),
            provider_file_id: Some("file_123".into()),
        };

        assert_eq!(
            attachment_image_ref(&attachment).as_deref(),
            Some("https://example.test/photo.jpg")
        );
    }

    #[test]
    fn prefers_provider_file_id_over_telegram_file_urls_for_image_parts() {
        let attachment = AttachmentRef {
            name: "photo.jpg".into(),
            mime_type: Some("image/jpeg".into()),
            size_bytes: Some(42),
            url: Some("telegram-file:file_123".into()),
            provider_file_id: Some("file_123".into()),
        };

        assert_eq!(
            attachment_image_ref(&attachment).as_deref(),
            Some("provider-file:file_123")
        );
    }

    #[test]
    fn falls_back_to_provider_file_id_for_image_parts_when_url_missing() {
        let attachment = AttachmentRef {
            name: "photo.jpg".into(),
            mime_type: Some("image/jpeg".into()),
            size_bytes: Some(42),
            url: None,
            provider_file_id: Some("file_123".into()),
        };

        assert_eq!(
            attachment_image_ref(&attachment).as_deref(),
            Some("provider-file:file_123")
        );
    }

    #[test]
    fn builds_multimodal_parts_for_user_text_and_image_attachments() {
        let parts = build_user_message_parts(
            "Describe this image",
            &[
                AttachmentRef {
                    name: "photo.jpg".into(),
                    mime_type: Some("image/jpeg".into()),
                    size_bytes: Some(42),
                    url: Some("https://example.test/photo.jpg".into()),
                    provider_file_id: None,
                },
                AttachmentRef {
                    name: "notes.txt".into(),
                    mime_type: Some("text/plain".into()),
                    size_bytes: None,
                    url: Some("https://example.test/notes.txt".into()),
                    provider_file_id: None,
                },
            ],
        )
        .expect("expected multimodal parts");

        assert!(
            matches!(&parts[0], MessagePart::Text { text } if text == "The user sent this image. Describe what you see and respond to their message. User message: Describe this image")
        );
        assert!(
            matches!(&parts[1], MessagePart::ImageUrl { image_url } if image_url.url == "https://example.test/photo.jpg")
        );
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn builds_multimodal_parts_for_attachment_only_image_messages() {
        let parts = build_user_message_parts(
            "",
            &[AttachmentRef {
                name: "photo.jpg".into(),
                mime_type: Some("image/jpeg".into()),
                size_bytes: Some(42),
                url: Some("https://example.test/photo.jpg".into()),
                provider_file_id: None,
            }],
        )
        .expect("expected multimodal parts");

        assert!(
            matches!(&parts[0], MessagePart::Text { text } if text == "The user sent this image. Describe what you see and respond to their message.")
        );
        assert!(
            matches!(&parts[1], MessagePart::ImageUrl { image_url } if image_url.url == "https://example.test/photo.jpg")
        );
        assert_eq!(parts.len(), 2);
    }
}

#[cfg(test)]
mod tool_call_sanitizer_tests {
    use super::*;
    use rune_models::{FunctionCall, ToolCallRequest};

    fn assistant_with_tool_calls(ids: &[&str]) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: None,
            content_parts: None,
            name: None,
            tool_call_id: None,
            tool_calls: Some(
                ids.iter()
                    .map(|id| ToolCallRequest {
                        id: (*id).to_string(),
                        call_type: "function".to_string(),
                        function: FunctionCall {
                            name: "read".to_string(),
                            arguments: "{}".to_string(),
                        },
                    })
                    .collect(),
            ),
        }
    }

    fn tool_result(id: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: Role::Tool,
            content: Some(content.to_string()),
            content_parts: None,
            name: None,
            tool_call_id: Some(id.to_string()),
            tool_calls: None,
        }
    }

    fn user_message(content: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: Some(content.to_string()),
            content_parts: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    #[test]
    fn sanitize_tool_calls_drops_late_tool_results_outside_assistant_block() {
        let mut messages = vec![
            assistant_with_tool_calls(&["call-a"]),
            user_message("intervening user message"),
            tool_result("call-a", "late tool result"),
        ];

        sanitize_tool_calls(&mut messages);

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, Role::Assistant);
        assert_eq!(messages[1].role, Role::Tool);
        assert_eq!(messages[1].tool_call_id.as_deref(), Some("call-a"));
        assert_eq!(
            messages[1].content.as_deref(),
            Some("[Tool call interrupted — no result available]")
        );
        assert_eq!(messages[2].role, Role::User);
    }

    #[test]
    fn sanitize_tool_calls_keeps_matching_contiguous_tool_results() {
        let mut messages = vec![
            assistant_with_tool_calls(&["call-a", "call-b"]),
            tool_result("call-a", "result a"),
            tool_result("call-b", "result b"),
            user_message("next"),
        ];

        sanitize_tool_calls(&mut messages);

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, Role::Assistant);
        assert_eq!(messages[1].tool_call_id.as_deref(), Some("call-a"));
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call-b"));
        assert_eq!(messages[3].role, Role::User);
    }

    #[test]
    fn sanitize_tool_calls_replaces_invalid_tool_block_entries_with_synthetic_results() {
        let mut messages = vec![
            assistant_with_tool_calls(&["call-a", "call-b"]),
            tool_result("call-a", "result a"),
            tool_result("other", "wrong block result"),
            user_message("next"),
        ];

        sanitize_tool_calls(&mut messages);

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, Role::Assistant);
        assert_eq!(messages[1].tool_call_id.as_deref(), Some("call-a"));
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call-b"));
        assert_eq!(
            messages[2].content.as_deref(),
            Some("[Tool call interrupted — no result available]")
        );
        assert_eq!(messages[3].role, Role::User);
    }
}

#[cfg(test)]
mod context_tier_tests {
    use super::*;

    #[test]
    fn default_tier_specs_match_story_budgets() {
        let assembler = ContextAssembler::new("You are Rune.");
        let specs = assembler.tier_specs();
        assert_eq!(specs.len(), 5);
        assert_eq!(
            specs[0],
            ContextTierSpec::new(ContextTierKind::Identity, 1_000)
        );
        assert_eq!(
            specs[1],
            ContextTierSpec::new(ContextTierKind::ActiveTask, 10_000)
        );
        assert_eq!(
            specs[2],
            ContextTierSpec::new(ContextTierKind::Project, 20_000)
        );
        assert_eq!(
            specs[3],
            ContextTierSpec::new(ContextTierKind::Shared, 5_000)
        );
        assert_eq!(
            specs[4],
            ContextTierSpec::new(ContextTierKind::Historical, 8_000)
        );
    }

    #[test]
    fn context_usage_marks_per_turn_tiers_for_refresh() {
        let assembler = ContextAssembler::new("You are Rune.");
        let report = assembler.analyze_context_usage(
            None,
            None,
            &["## Active task

Ship this slice."
                .into()],
            0,
            false,
        );

        let identity = report
            .tiers
            .iter()
            .find(|tier| tier.kind == ContextTierKind::Identity)
            .unwrap();
        assert!(identity.loaded);
        assert!(identity.refresh_required);

        let active_task = report
            .tiers
            .iter()
            .find(|tier| tier.kind == ContextTierKind::ActiveTask)
            .unwrap();
        assert!(active_task.loaded);
        assert!(active_task.refresh_required);

        let project = report
            .tiers
            .iter()
            .find(|tier| tier.kind == ContextTierKind::Project)
            .unwrap();
        assert!(!project.loaded);
        assert!(!project.refresh_required);

        let historical = report
            .tiers
            .iter()
            .find(|tier| tier.kind == ContextTierKind::Historical)
            .unwrap();
        assert!(!historical.refresh_required);
    }

    #[test]
    fn context_snapshots_preserve_refresh_required_flag() {
        let assembler = ContextAssembler::new("system");
        let report = assembler.analyze_context_usage(None, None, &[], 0, true);
        let snapshots = report.snapshots();
        let identity = snapshots
            .iter()
            .find(|tier| tier.kind == ContextTierKind::Identity)
            .unwrap();
        assert!(identity.refresh_required);
        let historical = snapshots
            .iter()
            .find(|tier| tier.kind == ContextTierKind::Historical)
            .unwrap();
        assert!(!historical.refresh_required);
    }

    #[test]
    fn with_context_config_uses_runtime_context_tiers() {
        let config = rune_config::ContextConfig {
            identity: 111,
            identity_priority: 9,
            identity_staleness_policy: "always_fresh".into(),
            task: 222,
            task_priority: 8,
            task_staleness_policy: "per_turn".into(),
            project: 333,
            project_priority: 7,
            project_staleness_policy: "per_session".into(),
            shared: 444,
            shared_priority: 6,
            shared_staleness_policy: "on_demand".into(),
            historical: 555,
            historical_priority: 5,
            historical_staleness_policy: "retrieval_only".into(),
        };

        let assembler = ContextAssembler::new("Identity instructions").with_context_config(&config);
        let specs = assembler.tier_specs();

        assert_eq!(specs.len(), 5);
        assert_eq!(specs[0].token_budget, 111);
        assert_eq!(specs[0].priority, 9);
        assert_eq!(
            specs[0].staleness_policy,
            ContextStalenessPolicy::AlwaysFresh
        );
        assert_eq!(specs[1].token_budget, 222);
        assert_eq!(specs[1].priority, 8);
        assert_eq!(specs[1].staleness_policy, ContextStalenessPolicy::PerTurn);
        assert_eq!(specs[2].token_budget, 333);
        assert_eq!(specs[2].priority, 7);
        assert_eq!(
            specs[2].staleness_policy,
            ContextStalenessPolicy::PerSession
        );
        assert_eq!(specs[3].token_budget, 444);
        assert_eq!(specs[3].priority, 6);
        assert_eq!(specs[3].staleness_policy, ContextStalenessPolicy::OnDemand);
        assert_eq!(specs[4].token_budget, 555);
        assert_eq!(specs[4].priority, 5);
        assert_eq!(
            specs[4].staleness_policy,
            ContextStalenessPolicy::RetrievalOnly
        );
    }

    #[test]
    fn with_tier_budgets_overrides_defaults() {
        let assembler = ContextAssembler::new("Identity instructions")
            .with_tier_budgets(750, 8_000, 16_000, 2_500);
        let specs = assembler.tier_specs();

        assert_eq!(specs.len(), 5);
        assert_eq!(specs[0].token_budget, 750);
        assert_eq!(specs[1].token_budget, 8_000);
        assert_eq!(specs[2].token_budget, 16_000);
        assert_eq!(specs[3].token_budget, 2_500);
        assert_eq!(specs[4].token_budget, 8_000);
    }

    #[test]
    fn analyze_context_usage_reports_loaded_tiers() {
        let assembler = ContextAssembler::new("Identity instructions");
        let workspace = WorkspaceContext {
            files: vec![("AGENTS.md".into(), "project rules".into())],
        };
        let memory = MemoryContext {
            today: Some("today note".into()),
            ..Default::default()
        };
        let report = assembler.analyze_context_usage(
            Some(&workspace),
            Some(&memory),
            &["Active task goes here".into()],
            50_000,
            true,
        );

        assert!(report.total_estimated_tokens > 0);
        assert_eq!(report.total_budget, 44_000);
        assert!(!report.over_budget);
        assert_eq!(report.compaction_trigger_tokens, 50_000);
        assert!(!report.over_compaction_threshold);
        assert!(report.identity_tokens() > 0);
        assert!(report.project_tokens() > 0);
        assert!(report.tokens_for(ContextTierKind::Shared) > 0);
        assert_eq!(report.tokens_for(ContextTierKind::Historical), 0);
        assert!(
            report
                .tiers
                .iter()
                .any(|tier| tier.kind == ContextTierKind::Historical && tier.loaded)
        );
        assert!(
            report
                .tiers
                .iter()
                .any(|tier| tier.kind == ContextTierKind::ActiveTask && tier.loaded)
        );
    }
}

#[cfg(test)]
mod context_budget_tests {
    use super::*;

    #[test]
    fn analyze_context_usage_marks_over_budget_when_tier_sum_exceeded() {
        let assembler = ContextAssembler::new("Identity instructions").with_tier_specs(vec![
            ContextTierSpec::new(ContextTierKind::Identity, 1),
            ContextTierSpec::new(ContextTierKind::ActiveTask, 1),
            ContextTierSpec::new(ContextTierKind::Project, 1),
            ContextTierSpec::new(ContextTierKind::Shared, 1),
            ContextTierSpec::new(ContextTierKind::Historical, 0),
        ]);
        let workspace = WorkspaceContext {
            files: vec![("AGENTS.md".into(), "project rules".into())],
        };
        let memory = MemoryContext {
            today: Some("today note".into()),
            ..Default::default()
        };

        let report = assembler.analyze_context_usage(
            Some(&workspace),
            Some(&memory),
            &["Active task goes here".into()],
            50_000,
            false,
        );

        assert!(report.over_budget);
        assert!(!report.over_compaction_threshold);
    }
    #[test]
    fn analyze_context_usage_marks_compaction_threshold_exceeded() {
        let assembler = ContextAssembler::new("Identity instructions");
        let report = assembler.analyze_context_usage(
            None,
            None,
            &["This active task section is deliberately long enough to exceed a tiny compaction trigger.".repeat(8)],
            10,
            false,
        );

        assert!(report.over_compaction_threshold);
        assert_eq!(report.compaction_trigger_tokens, 10);
    }
}

#[cfg(test)]
mod context_staleness_parse_tests {
    use super::*;

    #[test]
    fn parse_staleness_policy_accepts_hyphen_and_space_variants() {
        assert_eq!(
            parse_staleness_policy("always-fresh"),
            Some(ContextStalenessPolicy::AlwaysFresh)
        );
        assert_eq!(
            parse_staleness_policy("per turn"),
            Some(ContextStalenessPolicy::PerTurn)
        );
        assert_eq!(
            parse_staleness_policy("per-session"),
            Some(ContextStalenessPolicy::PerSession)
        );
        assert_eq!(
            parse_staleness_policy("on demand"),
            Some(ContextStalenessPolicy::OnDemand)
        );
        assert_eq!(
            parse_staleness_policy("retrieval-only"),
            Some(ContextStalenessPolicy::RetrievalOnly)
        );
    }
}
