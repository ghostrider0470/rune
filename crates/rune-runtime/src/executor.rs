use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use tracing::{debug, error, warn};
use uuid::Uuid;

use rune_core::{
    ApprovalDecision, ApprovalId, NormalizedMessage, SessionKind, ToolCallId, TranscriptItem,
    TurnId, TurnStatus,
};
use rune_models::{CompletionRequest, ModelProvider};
use rune_store::models::{NewTranscriptItem, NewTurn, TranscriptItemRow, TurnRow};
use rune_store::repos::{SessionRepo, TranscriptRepo, TurnRepo};
use rune_tools::{ToolCall, ToolExecutor, ToolRegistry, ToolResult};

use crate::compaction::CompactionStrategy;
use crate::context::ContextAssembler;
use crate::error::RuntimeError;
use crate::memory::MemoryLoader;
use crate::usage::UsageAccumulator;
use crate::workspace::WorkspaceLoader;

/// Maximum tool-call loop iterations before aborting.
const DEFAULT_MAX_TOOL_ITERATIONS: u32 = 25;

/// Executes a single turn: load context → prompt → model → tool loop → persist.
pub struct TurnExecutor {
    session_repo: Arc<dyn SessionRepo>,
    turn_repo: Arc<dyn TurnRepo>,
    transcript_repo: Arc<dyn TranscriptRepo>,
    model_provider: Arc<dyn ModelProvider>,
    tool_executor: Arc<dyn ToolExecutor>,
    tool_registry: Arc<ToolRegistry>,
    context_assembler: ContextAssembler,
    compaction: Arc<dyn CompactionStrategy>,
    default_model: Option<String>,
    max_tool_iterations: u32,
}

impl TurnExecutor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_repo: Arc<dyn SessionRepo>,
        turn_repo: Arc<dyn TurnRepo>,
        transcript_repo: Arc<dyn TranscriptRepo>,
        model_provider: Arc<dyn ModelProvider>,
        tool_executor: Arc<dyn ToolExecutor>,
        tool_registry: Arc<ToolRegistry>,
        context_assembler: ContextAssembler,
        compaction: Arc<dyn CompactionStrategy>,
    ) -> Self {
        Self {
            session_repo,
            turn_repo,
            transcript_repo,
            model_provider,
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
            default_model: None,
            max_tool_iterations: DEFAULT_MAX_TOOL_ITERATIONS,
        }
    }

    /// Set the default model name for completion requests.
    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = Some(model.into());
        self
    }

    /// Access the transcript repo (used by session loop to read replies).
    pub fn transcript_repo(&self) -> &Arc<dyn TranscriptRepo> {
        &self.transcript_repo
    }

    /// Override the max tool iterations limit.
    pub fn with_max_tool_iterations(mut self, max: u32) -> Self {
        self.max_tool_iterations = max;
        self
    }

    /// Execute a turn for the given session, triggered by a user message.
    ///
    /// Returns the completed turn row and accumulated usage.
    pub async fn execute(
        &self,
        session_id: Uuid,
        user_message: &str,
        model_ref: Option<&str>,
    ) -> Result<(TurnRow, UsageAccumulator), RuntimeError> {
        let turn_id = TurnId::new();
        let now = Utc::now();

        // 1. Create turn in Started state
        let turn = self
            .turn_repo
            .create(NewTurn {
                id: turn_id.into_uuid(),
                session_id,
                trigger_kind: "user_message".to_string(),
                status: status_str(TurnStatus::Started).to_string(),
                model_ref: model_ref.map(String::from),
                started_at: now,
                ended_at: None,
                usage_prompt_tokens: None,
                usage_completion_tokens: None,
            })
            .await?;

        debug!(turn_id = %turn_id, "turn created");

        // 2. Persist user message to transcript
        let user_item = TranscriptItem::UserMessage {
            message: NormalizedMessage::new("user", user_message),
        };
        self.append_transcript(session_id, Some(turn_id.into_uuid()), &user_item)
            .await?;

        // 3. Run the model/tool loop
        let mut usage = UsageAccumulator::new();
        let result = self.run_turn_loop(session_id, turn_id, &mut usage).await;

        // 4. Finalize turn status
        let (final_status, ended_at) = match &result {
            Ok(_) => (TurnStatus::Completed, Some(Utc::now())),
            Err(_) => (TurnStatus::Failed, Some(Utc::now())),
        };

        let final_turn = self
            .turn_repo
            .update_status(turn.id, status_str(final_status), ended_at)
            .await?;

        // If the loop failed, propagate the error
        result?;

        Ok((final_turn, usage))
    }

    /// The core model → tool → model loop.
    async fn run_turn_loop(
        &self,
        session_id: Uuid,
        turn_id: TurnId,
        usage: &mut UsageAccumulator,
    ) -> Result<(), RuntimeError> {
        let session = self.session_repo.find_by_id(session_id).await?;
        let session_kind = parse_session_kind(&session.kind)?;
        let workspace_root = session
            .workspace_root
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let workspace_context = WorkspaceLoader::new(&workspace_root, session_kind).load().await;
        let memory_context = MemoryLoader::new(&workspace_root).load(session_kind).await;

        let mut iterations: u32 = 0;

        loop {
            if iterations >= self.max_tool_iterations {
                return Err(RuntimeError::MaxToolIterations(self.max_tool_iterations));
            }
            iterations += 1;

            // Transition: → ModelCalling
            self.turn_repo
                .update_status(
                    turn_id.into_uuid(),
                    status_str(TurnStatus::ModelCalling),
                    None,
                )
                .await?;

            // Load transcript and assemble prompt
            let transcript_rows = self.transcript_repo.list_by_session(session_id).await?;

            let messages = self.context_assembler.assemble(
                &transcript_rows,
                self.compaction.as_ref(),
                Some(&workspace_context),
                Some(&memory_context),
            );

            // Build tool definitions for the request
            let tool_defs: Vec<rune_models::ToolDefinition> = self
                .tool_registry
                .list()
                .iter()
                .map(|t| rune_models::ToolDefinition {
                    tool_type: "function".to_string(),
                    function: rune_models::FunctionDefinition {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.parameters.clone(),
                    },
                })
                .collect();

            let request = CompletionRequest {
                messages,
                model: self.default_model.clone(),
                temperature: None,
                max_tokens: None,
                tools: if tool_defs.is_empty() {
                    None
                } else {
                    Some(tool_defs)
                },
            };

            // Call model
            let response = match self.model_provider.complete(&request).await {
                Ok(resp) => resp,
                Err(e) => {
                    error!(error = %e, "model call failed");
                    return Err(RuntimeError::Model(e));
                }
            };

            usage.add(&response.usage);

            // If model returned tool calls → execute them and loop
            if !response.tool_calls.is_empty() {
                // Persist assistant message with tool calls as transcript items
                for tc in &response.tool_calls {
                    let args: serde_json::Value =
                        serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                    let tool_call_id = ToolCallId::new();
                    let req_item = TranscriptItem::ToolRequest {
                        tool_call_id,
                        tool_name: tc.function.name.clone(),
                        arguments: args.clone(),
                    };
                    self.append_transcript(session_id, Some(turn_id.into_uuid()), &req_item)
                        .await?;

                    // Transition: → ToolExecuting
                    self.turn_repo
                        .update_status(
                            turn_id.into_uuid(),
                            status_str(TurnStatus::ToolExecuting),
                            None,
                        )
                        .await?;

                    // Execute tool
                    let call = ToolCall {
                        tool_call_id,
                        tool_name: tc.function.name.clone(),
                        arguments: args,
                    };

                    let tool_result = match self.tool_executor.execute(call).await {
                        Ok(result) => result,
                        Err(rune_tools::ToolError::ApprovalRequired { tool, details }) => {
                            warn!(tool = %tool, "tool execution requires approval");

                            let approval_id = ApprovalId::new();
                            let approval_request = TranscriptItem::ApprovalRequest {
                                approval_id,
                                summary: details.clone(),
                                command: extract_approval_command(&details),
                            };
                            self.append_transcript(
                                session_id,
                                Some(turn_id.into_uuid()),
                                &approval_request,
                            )
                            .await?;

                            let approval_response = TranscriptItem::ApprovalResponse {
                                approval_id,
                                decision: ApprovalDecision::Deny,
                                note: Some("approval required before execution".to_string()),
                            };
                            self.append_transcript(
                                session_id,
                                Some(turn_id.into_uuid()),
                                &approval_response,
                            )
                            .await?;

                            ToolResult {
                                tool_call_id,
                                output: format!("Approval required for tool {tool}: {details}"),
                                is_error: true,
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, tool = %tc.function.name, "tool execution failed");
                            ToolResult {
                                tool_call_id,
                                output: format!("Tool error: {e}"),
                                is_error: true,
                            }
                        }
                    };

                    // Persist tool result
                    let result_item = TranscriptItem::ToolResult {
                        tool_call_id: tool_result.tool_call_id,
                        output: tool_result.output,
                        is_error: tool_result.is_error,
                    };
                    self.append_transcript(session_id, Some(turn_id.into_uuid()), &result_item)
                        .await?;
                }

                // Loop back to model
                continue;
            }

            // Model returned a final text response — persist and finish
            if let Some(content) = &response.content {
                let assistant_item = TranscriptItem::AssistantMessage {
                    content: content.clone(),
                };
                self.append_transcript(session_id, Some(turn_id.into_uuid()), &assistant_item)
                    .await?;
            }

            return Ok(());
        }
    }

    /// Append a transcript item, auto-incrementing the sequence number.
    async fn append_transcript(
        &self,
        session_id: Uuid,
        turn_id: Option<Uuid>,
        item: &TranscriptItem,
    ) -> Result<TranscriptItemRow, RuntimeError> {
        // Get current count for sequence
        let existing = self.transcript_repo.list_by_session(session_id).await?;
        let seq = existing.len() as i32;

        let kind = transcript_item_kind(item);
        let payload = serde_json::to_value(item).map_err(|e| {
            RuntimeError::ContextAssembly(format!("failed to serialize transcript item: {e}"))
        })?;

        let row = self
            .transcript_repo
            .append(NewTranscriptItem {
                id: Uuid::now_v7(),
                session_id,
                turn_id,
                seq,
                kind: kind.to_string(),
                payload,
                created_at: Utc::now(),
            })
            .await?;

        Ok(row)
    }
}

fn parse_session_kind(kind: &str) -> Result<SessionKind, RuntimeError> {
    match kind {
        "direct" => Ok(SessionKind::Direct),
        "channel" => Ok(SessionKind::Channel),
        "scheduled" => Ok(SessionKind::Scheduled),
        "subagent" => Ok(SessionKind::Subagent),
        other => Err(RuntimeError::ContextAssembly(format!(
            "unknown session kind: {other}"
        ))),
    }
}

fn status_str(status: TurnStatus) -> &'static str {
    match status {
        TurnStatus::Started => "started",
        TurnStatus::ModelCalling => "model_calling",
        TurnStatus::ToolExecuting => "tool_executing",
        TurnStatus::Completed => "completed",
        TurnStatus::Failed => "failed",
        TurnStatus::Cancelled => "cancelled",
    }
}

fn transcript_item_kind(item: &TranscriptItem) -> &'static str {
    match item {
        TranscriptItem::UserMessage { .. } => "user_message",
        TranscriptItem::AssistantMessage { .. } => "assistant_message",
        TranscriptItem::ToolRequest { .. } => "tool_request",
        TranscriptItem::ToolResult { .. } => "tool_result",
        TranscriptItem::ApprovalRequest { .. } => "approval_request",
        TranscriptItem::ApprovalResponse { .. } => "approval_response",
        TranscriptItem::StatusNote { .. } => "status_note",
        TranscriptItem::SubagentResult { .. } => "subagent_result",
    }
}

fn extract_approval_command(details: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(details) {
        if let Some(command) = value.get("command").and_then(|v| v.as_str()) {
            return Some(command.to_string());
        }

        if let Some(arguments_summary) = value.get("arguments_summary").and_then(|v| v.as_str()) {
            if let Ok(args) = serde_json::from_str::<serde_json::Value>(arguments_summary) {
                if let Some(command) = args.get("command").and_then(|v| v.as_str()) {
                    return Some(command.to_string());
                }
            }
        }
    }

    details
        .lines()
        .find_map(|line| line.trim().strip_prefix("command:").map(|s| s.trim().to_string()))
}
