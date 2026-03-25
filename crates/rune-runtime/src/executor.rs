use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use rune_core::{
    ApprovalDecision, ApprovalId, NormalizedMessage, SessionKind, ToolCallId, TranscriptItem,
    TriggerKind, TurnId, TurnStatus,
};
use rune_models::{CompletionRequest, ModelProvider, StreamEvent};
use rune_store::models::{NewApproval, NewTranscriptItem, NewTurn, TranscriptItemRow, TurnRow};
use rune_store::repos::{
    ApprovalRepo, SessionRepo, ToolApprovalPolicyRepo, TranscriptRepo, TurnRepo,
};
use rune_tools::{ToolCall, ToolExecutor, ToolRegistry, ToolResult};

use crate::compaction::CompactionStrategy;
use crate::context::ContextAssembler;
use crate::error::RuntimeError;
use crate::hooks::{HookEvent, HookRegistry};
use crate::lane_queue::{Lane, LaneQueue};
use crate::mem0::Mem0Engine;
use crate::memory::MemoryLoader;
use crate::session_metadata::selected_model;
use crate::skill::SkillRegistry;
use crate::usage::UsageAccumulator;
use crate::workspace::WorkspaceLoader;

/// Maximum tool-call loop iterations before aborting.
const DEFAULT_MAX_TOOL_ITERATIONS: u32 = 200;

/// Executes a single turn: load context → prompt → model → tool loop → persist.
///
/// When a `LaneQueue` is attached, each turn acquires a lane permit before
/// execution begins, enforcing per-lane concurrency caps. Without a lane
/// queue the executor behaves as before (sequential, no concurrency limit).
#[derive(Clone)]
pub struct TurnExecutor {
    session_repo: Arc<dyn SessionRepo>,
    turn_repo: Arc<dyn TurnRepo>,
    transcript_repo: Arc<dyn TranscriptRepo>,
    approval_repo: Arc<dyn ApprovalRepo>,
    model_provider: Arc<dyn ModelProvider>,
    tool_executor: Arc<dyn ToolExecutor>,
    tool_registry: Arc<ToolRegistry>,
    context_assembler: ContextAssembler,
    compaction: Arc<dyn CompactionStrategy>,
    default_model: Option<String>,
    max_tool_iterations: u32,
    lane_queue: Option<Arc<LaneQueue>>,
    skill_registry: Option<Arc<SkillRegistry>>,
    tool_approval_policy_repo: Option<Arc<dyn ToolApprovalPolicyRepo>>,
    hook_registry: Option<Arc<HookRegistry>>,
    /// Mem0 auto-capture/recall engine for persistent cross-session memory.
    mem0: Option<Arc<Mem0Engine>>,
    /// Global approval mode — "yolo" auto-approves all tool calls.
    approval_mode: String,
}

impl TurnExecutor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_repo: Arc<dyn SessionRepo>,
        turn_repo: Arc<dyn TurnRepo>,
        transcript_repo: Arc<dyn TranscriptRepo>,
        approval_repo: Arc<dyn ApprovalRepo>,
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
            approval_repo,
            model_provider,
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
            default_model: None,
            max_tool_iterations: DEFAULT_MAX_TOOL_ITERATIONS,
            lane_queue: None,
            skill_registry: None,
            tool_approval_policy_repo: None,
            hook_registry: None,
            mem0: None,
            approval_mode: "on-miss".to_string(),
        }
    }

    /// Set the global approval mode. When set to "yolo", all tool approval
    /// gates are automatically bypassed in the turn loop.
    pub fn with_approval_mode(mut self, mode: impl Into<String>) -> Self {
        self.approval_mode = mode.into();
        self
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

    /// Attach a lane queue for concurrency-limited execution.
    ///
    /// When set, each call to [`execute`] or [`resume_approval`] will
    /// acquire a permit from the appropriate lane before proceeding.
    pub fn with_lane_queue(mut self, queue: Arc<LaneQueue>) -> Self {
        self.lane_queue = Some(queue);
        self
    }

    /// Expose lane queue stats for operator surfaces when configured.
    pub fn lane_stats(&self) -> Option<crate::lane_queue::LaneStats> {
        self.lane_queue.as_ref().map(|queue| queue.stats())
    }

    /// Attach a dynamic skill registry whose enabled skills are injected into the system prompt.
    pub fn with_skill_registry(mut self, registry: Arc<SkillRegistry>) -> Self {
        self.skill_registry = Some(registry);
        self
    }

    /// Attach a durable tool approval policy store for persisting allow-always decisions.
    pub fn with_tool_approval_policy_repo(mut self, repo: Arc<dyn ToolApprovalPolicyRepo>) -> Self {
        self.tool_approval_policy_repo = Some(repo);
        self
    }

    /// Attach a hook registry for emitting pre/post tool call and session lifecycle events.
    pub fn with_hook_registry(mut self, registry: Arc<HookRegistry>) -> Self {
        self.hook_registry = Some(registry);
        self
    }

    /// Attach a Mem0 engine for auto-recall and auto-capture of persistent memories.
    pub fn with_mem0(mut self, engine: Arc<Mem0Engine>) -> Self {
        self.mem0 = Some(engine);
        self
    }

    /// Access the Mem0 engine (if connected).
    pub fn mem0(&self) -> Option<&Arc<Mem0Engine>> {
        self.mem0.as_ref()
    }

    /// Execute a turn for the given session, triggered by a user message.
    ///
    /// Returns the completed turn row and accumulated usage.
    ///
    /// When a [`LaneQueue`] is attached, this method will wait for a
    /// lane permit before beginning execution. The permit is held for
    /// the entire turn and released automatically when the turn ends.
    pub async fn execute(
        &self,
        session_id: Uuid,
        user_message: &str,
        model_ref: Option<&str>,
    ) -> Result<(TurnRow, UsageAccumulator), RuntimeError> {
        self.execute_triggered(
            session_id,
            user_message,
            model_ref,
            TriggerKind::UserMessage,
            None,
        )
        .await
    }

    /// Execute a turn with streaming — text deltas are forwarded to `chunk_tx`
    /// as they arrive from the model provider. The caller can progressively
    /// update a UI (e.g. edit a Telegram message) while the model generates.
    ///
    /// Falls back to non-streaming transparently if the provider does not
    /// support streaming or if the response contains tool calls.
    pub async fn execute_streaming(
        &self,
        session_id: Uuid,
        user_message: &str,
        model_ref: Option<&str>,
        chunk_tx: tokio::sync::mpsc::Sender<String>,
    ) -> Result<(TurnRow, UsageAccumulator), RuntimeError> {
        self.execute_triggered(
            session_id,
            user_message,
            model_ref,
            TriggerKind::UserMessage,
            Some(chunk_tx),
        )
        .await
    }

    /// Execute a turn for the given session using an explicit trigger kind.
    pub async fn execute_triggered(
        &self,
        session_id: Uuid,
        user_message: &str,
        model_ref: Option<&str>,
        trigger_kind: TriggerKind,
        chunk_tx: Option<tokio::sync::mpsc::Sender<String>>,
    ) -> Result<(TurnRow, UsageAccumulator), RuntimeError> {
        let turn_id = TurnId::new();
        let now = Utc::now();
        let session = self.session_repo.find_by_id(session_id).await?;
        let effective_model = model_ref
            .map(str::to_owned)
            .or_else(|| selected_model(&session).map(str::to_owned))
            .or_else(|| self.default_model.clone());

        // Acquire a lane permit if a lane queue is configured.
        let _lane_permit = if let Some(ref lq) = self.lane_queue {
            let session_kind = parse_session_kind(&session.kind)?;
            let lane = Lane::from_session_kind(&session_kind);
            debug!(
                turn_id = %turn_id,
                lane = %lane,
                "waiting for lane permit"
            );
            Some(lq.acquire(lane).await)
        } else {
            None
        };

        // 1. Transition session Ready → Running
        self.session_repo
            .update_status(session_id, "running", Utc::now())
            .await?;

        // 2. Create turn in Started state
        let turn = self
            .turn_repo
            .create(NewTurn {
                id: turn_id.into_uuid(),
                session_id,
                trigger_kind: trigger_kind.as_str().to_string(),
                status: status_str(TurnStatus::Started).to_string(),
                model_ref: effective_model.clone(),
                started_at: now,
                ended_at: None,
                usage_prompt_tokens: None,
                usage_completion_tokens: None,
            })
            .await?;

        debug!(turn_id = %turn_id, "turn created");

        // 3. Persist user message to transcript
        let user_item = TranscriptItem::UserMessage {
            message: NormalizedMessage::new("user", user_message),
        };
        self.append_transcript(session_id, Some(turn_id.into_uuid()), &user_item)
            .await?;

        // 4. Run the model/tool loop
        let mut usage = UsageAccumulator::new();
        let result = self
            .run_turn_loop(
                session_id,
                turn_id,
                effective_model.as_deref(),
                &mut usage,
                chunk_tx.as_ref(),
            )
            .await;

        // Persist usage totals even on failed turns so operator surfaces remain durable.
        let prompt_tokens = i32::try_from(usage.prompt_tokens).unwrap_or(i32::MAX);
        let completion_tokens = i32::try_from(usage.completion_tokens).unwrap_or(i32::MAX);
        let _ = self
            .turn_repo
            .update_usage(turn.id, prompt_tokens, completion_tokens)
            .await?;

        // 5. Finalize turn status
        let (final_status, ended_at) = match &result {
            Ok(TurnLoopOutcome::Completed) => (TurnStatus::Completed, Some(Utc::now())),
            Ok(TurnLoopOutcome::WaitingForApproval) => (TurnStatus::ToolExecuting, None),
            Err(_) => (TurnStatus::Failed, Some(Utc::now())),
        };

        let final_turn = self
            .turn_repo
            .update_status(turn.id, status_str(final_status), ended_at)
            .await?;

        // Update latest_turn_id on the session for quick access.
        if matches!(final_status, TurnStatus::Completed | TurnStatus::Failed) {
            let _ = self
                .session_repo
                .update_latest_turn(session_id, turn.id, Utc::now())
                .await;
        }

        // If the loop failed, propagate the error
        result?;

        // Mem0 capture: extract and store facts from this turn (background).
        if let Some(ref mem0) = self.mem0 {
            if matches!(final_status, TurnStatus::Completed) {
                let mem0 = mem0.clone();
                let user_msg = user_message.to_string();
                let sess_id = session_id;

                // Grab the assistant's final response from transcript
                let transcript_repo = self.transcript_repo.clone();
                tokio::spawn(async move {
                    let assistant_msg = match transcript_repo.list_by_session(sess_id).await {
                        Ok(rows) => rows
                            .iter()
                            .rev()
                            .find_map(|row| {
                                let item: TranscriptItem =
                                    serde_json::from_value(row.payload.clone()).ok()?;
                                if let TranscriptItem::AssistantMessage { content } = item {
                                    Some(content)
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_default(),
                        Err(e) => {
                            warn!(error = %e, "mem0 capture: failed to read transcript");
                            return;
                        }
                    };

                    if !user_msg.is_empty() && !assistant_msg.is_empty() {
                        mem0.capture(&user_msg, &assistant_msg, sess_id).await;
                    }
                });
            }
        }

        Ok((final_turn, usage))
    }

    /// Resume a pending approval-backed tool execution and continue the blocked turn.
    pub async fn resume_approval(
        &self,
        approval_id: Uuid,
    ) -> Result<(TurnRow, UsageAccumulator), RuntimeError> {
        let approval = self.approval_repo.find_by_id(approval_id).await?;
        let decision_raw = approval.decision.clone().ok_or_else(|| {
            RuntimeError::Aborted("approval has not been decided yet".to_string())
        })?;
        let resumed_at = Utc::now();
        let decision = parse_approval_decision(&decision_raw)?;
        let payload = approval.presented_payload.clone();

        let session_id = payload
            .get("session_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| RuntimeError::Aborted("approval payload missing session_id".to_string()))
            .and_then(parse_uuid_runtime)?;

        // Acquire a lane permit if a lane queue is configured.
        let _lane_permit = if let Some(ref lq) = self.lane_queue {
            let session = self.session_repo.find_by_id(session_id).await?;
            let session_kind = parse_session_kind(&session.kind)?;
            let lane = Lane::from_session_kind(&session_kind);
            debug!(
                approval_id = %approval_id,
                lane = %lane,
                "waiting for lane permit (approval resume)"
            );
            Some(lq.acquire(lane).await)
        } else {
            None
        };

        let turn_uuid = payload
            .get("turn_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| RuntimeError::Aborted("approval payload missing turn_id".to_string()))
            .and_then(parse_uuid_runtime)?;
        let tool_call_id = payload
            .get("tool_call_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                RuntimeError::Aborted("approval payload missing tool_call_id".to_string())
            })
            .and_then(parse_uuid_runtime)?;
        let tool_name = payload
            .get("tool_name")
            .and_then(|value| value.as_str())
            .ok_or_else(|| RuntimeError::Aborted("approval payload missing tool_name".to_string()))?
            .to_string();
        let arguments = payload.get("arguments").cloned().ok_or_else(|| {
            RuntimeError::Aborted("approval payload missing arguments".to_string())
        })?;

        let mut usage = UsageAccumulator::new();
        let turn = self.turn_repo.find_by_id(turn_uuid).await?;

        match decision {
            ApprovalDecision::Deny => {
                self.update_approval_progress(
                    approval.id,
                    resumed_at,
                    "denied",
                    Some(format!("operator denied execution for tool {tool_name}")),
                )
                .await?;
                let response = TranscriptItem::ApprovalResponse {
                    approval_id: ApprovalId::from(approval.id),
                    decision: ApprovalDecision::Deny,
                    note: Some("operator denied execution".to_string()),
                };
                self.append_transcript(session_id, Some(turn_uuid), &response)
                    .await?;

                let result_item = TranscriptItem::ToolResult {
                    tool_call_id: ToolCallId::from(tool_call_id),
                    output: format!("Tool error: approval denied for tool {tool_name}"),
                    is_error: true,
                    tool_execution_id: None,
                };
                self.append_transcript(session_id, Some(turn_uuid), &result_item)
                    .await?;

                self.turn_repo
                    .update_status(
                        turn_uuid,
                        status_str(TurnStatus::Completed),
                        Some(Utc::now()),
                    )
                    .await?;
                let final_turn = self.turn_repo.find_by_id(turn_uuid).await?;
                self.session_repo
                    .update_status(session_id, "running", Utc::now())
                    .await?;
                return Ok((final_turn, usage));
            }
            ApprovalDecision::AllowAlways | ApprovalDecision::AllowOnce => {
                self.update_approval_progress(
                    approval.id,
                    resumed_at,
                    "resuming",
                    Some(format!("resuming approved tool call for {tool_name}")),
                )
                .await?;

                // Persist allow-always decisions durably so they survive restarts.
                if decision == ApprovalDecision::AllowAlways {
                    if let Some(ref repo) = self.tool_approval_policy_repo {
                        if let Err(e) = repo.set_policy(&tool_name, "allow_always").await {
                            warn!(
                                tool = %tool_name,
                                error = %e,
                                "failed to persist allow-always policy"
                            );
                        }
                    }
                }

                let response = TranscriptItem::ApprovalResponse {
                    approval_id: ApprovalId::from(approval.id),
                    decision,
                    note: Some("operator approved execution".to_string()),
                };
                self.append_transcript(session_id, Some(turn_uuid), &response)
                    .await?;
            }
        }

        self.session_repo
            .update_status(session_id, "running", Utc::now())
            .await?;
        self.turn_repo
            .update_status(turn_uuid, status_str(TurnStatus::ToolExecuting), None)
            .await?;

        let mut arguments = arguments;
        if let Some(obj) = arguments.as_object_mut() {
            obj.insert(
                "__approval_resume".to_string(),
                serde_json::Value::Bool(true),
            );
        }

        let call = ToolCall {
            tool_call_id: ToolCallId::from(tool_call_id),
            tool_name: tool_name.clone(),
            arguments,
        };

        let tool_result = match self.tool_executor.execute(call.clone()).await {
            Ok(result) => result,
            Err(rune_tools::ToolError::ApprovalRequired { .. }) => {
                self.update_approval_progress(
                    approval.id,
                    resumed_at,
                    "failed",
                    Some("approved tool call still requested approval during resume".to_string()),
                )
                .await?;
                return Err(RuntimeError::Aborted(
                    "approved tool call still requested approval during resume".to_string(),
                ));
            }
            Err(e) => ToolResult {
                tool_call_id: call.tool_call_id,
                output: format!("Tool error: {e}"),
                is_error: true,
                tool_execution_id: None,
            },
        };

        let tool_result_was_error = tool_result.is_error;
        let resume_output = tool_result.output.clone();
        let result_item = TranscriptItem::ToolResult {
            tool_call_id: tool_result.tool_call_id,
            output: tool_result.output,
            is_error: tool_result.is_error,
            tool_execution_id: tool_result.tool_execution_id,
        };
        self.append_transcript(session_id, Some(turn_uuid), &result_item)
            .await?;

        self.update_approval_progress(
            approval.id,
            resumed_at,
            "resuming",
            Some(format!("tool execution resumed: {resume_output}")),
        )
        .await?;

        let session = self.session_repo.find_by_id(session_id).await?;
        let effective_model = selected_model(&session)
            .map(str::to_owned)
            .or_else(|| turn.model_ref.clone())
            .or_else(|| self.default_model.clone());

        let result = self
            .run_turn_loop(
                session_id,
                TurnId::from(turn_uuid),
                effective_model.as_deref(),
                &mut usage,
                None,
            )
            .await;

        let prompt_tokens = i32::try_from(usage.prompt_tokens).unwrap_or(i32::MAX);
        let completion_tokens = i32::try_from(usage.completion_tokens).unwrap_or(i32::MAX);
        let _ = self
            .turn_repo
            .update_usage(turn_uuid, prompt_tokens, completion_tokens)
            .await?;

        let (final_status, ended_at, approval_status, approval_summary) = match &result {
            Ok(TurnLoopOutcome::Completed) => {
                let approval_status = if tool_result_was_error {
                    "completed_error"
                } else {
                    "completed"
                };
                (
                    TurnStatus::Completed,
                    Some(Utc::now()),
                    approval_status,
                    Some(resume_output),
                )
            }
            Ok(TurnLoopOutcome::WaitingForApproval) => (
                TurnStatus::ToolExecuting,
                None,
                "waiting_for_approval",
                Some("resumed turn encountered another approval gate".to_string()),
            ),
            Err(error) => (
                TurnStatus::Failed,
                Some(Utc::now()),
                "failed",
                Some(format!("post-approval continuation failed: {error}")),
            ),
        };
        self.update_approval_progress(approval.id, resumed_at, approval_status, approval_summary)
            .await?;
        let final_turn = self
            .turn_repo
            .update_status(turn_uuid, status_str(final_status), ended_at)
            .await?;

        result?;
        Ok((final_turn, usage))
    }

    /// The core model → tool → model loop.
    async fn run_turn_loop(
        &self,
        session_id: Uuid,
        turn_id: TurnId,
        model_ref: Option<&str>,
        usage: &mut UsageAccumulator,
        chunk_tx: Option<&tokio::sync::mpsc::Sender<String>>,
    ) -> Result<TurnLoopOutcome, RuntimeError> {
        let session = self.session_repo.find_by_id(session_id).await?;
        let session_kind = parse_session_kind(&session.kind)?;
        let workspace_root = session
            .workspace_root
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let workspace_context = WorkspaceLoader::new(&workspace_root, session_kind)
            .load()
            .await;
        let memory_context = MemoryLoader::new(&workspace_root).load(session_kind).await;

        // Mem0 recall: find semantically similar memories before the first
        // model call. We fetch the user message from the most recent
        // transcript entry so we can embed the current query.
        let mem0_prompt_section = if let Some(ref mem0) = self.mem0 {
            let transcript_rows = self.transcript_repo.list_by_session(session_id).await?;
            let user_msg = transcript_rows
                .iter()
                .rev()
                .find_map(|row| {
                    let item: rune_core::TranscriptItem =
                        serde_json::from_value(row.payload.clone()).ok()?;
                    if let rune_core::TranscriptItem::UserMessage { message } = item {
                        Some(message.content)
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

            if !user_msg.is_empty() {
                let memories = mem0.recall(&user_msg).await;
                let section = Mem0Engine::format_for_prompt(&memories);
                if section.is_empty() {
                    None
                } else {
                    Some(section)
                }
            } else {
                None
            }
        } else {
            None
        };

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

            let skill_prompt_fragment = match &self.skill_registry {
                Some(registry) => registry.system_prompt_fragment().await,
                None => None,
            };
            let mut extra_system_sections: Vec<String> =
                skill_prompt_fragment.into_iter().collect();

            // Inject recalled mem0 memories into the system prompt
            if let Some(ref section) = mem0_prompt_section {
                extra_system_sections.push(section.clone());
            }

            let messages = self.context_assembler.assemble(
                &transcript_rows,
                self.compaction.as_ref(),
                Some(&workspace_context),
                Some(&memory_context),
                &extra_system_sections,
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
                model: model_ref.map(str::to_owned),
                temperature: None,
                max_tokens: None,
                tools: if tool_defs.is_empty() {
                    None
                } else {
                    Some(tool_defs)
                },
            };

            // Call model — try streaming when a chunk sender is available,
            // otherwise use the non-streaming path with retries.
            let response = if let Some(tx) = chunk_tx {
                match self.model_provider.complete_stream(&request).await {
                    Ok(mut rx) => {
                        let mut final_response = None;
                        while let Some(event) = rx.recv().await {
                            match event {
                                StreamEvent::TextDelta(delta) => {
                                    let _ = tx.send(delta).await;
                                }
                                StreamEvent::Done(resp) => {
                                    final_response = Some(resp);
                                    break;
                                }
                            }
                        }
                        match final_response {
                            Some(resp) => resp,
                            None => {
                                warn!(
                                    "stream ended without Done event, falling back to non-streaming"
                                );
                                self.complete_with_retries(&request).await?
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "streaming request failed, falling back to non-streaming");
                        self.complete_with_retries(&request).await?
                    }
                }
            } else {
                self.complete_with_retries(&request).await?
            };

            usage.add(&response.usage);

            // If model returned tool calls → execute them and loop
            if !response.tool_calls.is_empty() {
                // Persist assistant message with tool calls as transcript items
                for tc in &response.tool_calls {
                    let args: serde_json::Value =
                        serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                    let tool_call_id = ToolCallId::from_model(&tc.id);
                    let req_item = TranscriptItem::ToolRequest {
                        tool_call_id: tool_call_id.clone(),
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
                    let mut args = args;
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert(
                            "__session_id".to_string(),
                            serde_json::Value::String(session_id.to_string()),
                        );
                        obj.insert(
                            "__turn_id".to_string(),
                            serde_json::Value::String(turn_id.into_uuid().to_string()),
                        );
                    }

                    // Emit PreToolCall hook
                    if let Some(ref hook_reg) = self.hook_registry {
                        let mut hook_ctx = serde_json::json!({
                            "tool_name": tc.function.name,
                            "arguments": &args,
                            "session_id": session_id.to_string(),
                            "turn_id": turn_id.into_uuid().to_string(),
                        });
                        hook_reg.emit(&HookEvent::PreToolCall, &mut hook_ctx).await;
                        // Allow hooks to modify arguments
                        if let Some(modified_args) = hook_ctx.get("arguments").cloned() {
                            args = modified_args;
                        }
                    }

                    let call = ToolCall {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tc.function.name.clone(),
                        arguments: args,
                    };

                    let tool_result = match self.tool_executor.execute(call.clone()).await {
                        Ok(result) => result,
                        Err(rune_tools::ToolError::ApprovalRequired { tool, details }) => {
                            // Yolo mode: auto-approve everything without
                            // persisting an approval record or checking policies.
                            let is_yolo = self.approval_mode == "yolo";

                            // Check for a persisted allow-always policy before
                            // halting the turn loop.  This lets a prior
                            // allow-always decision auto-approve matching future
                            // tool calls without operator intervention.
                            let has_allow_always = if !is_yolo {
                                if let Some(ref repo) = self.tool_approval_policy_repo {
                                    matches!(
                                        repo.get_policy(&tool).await,
                                        Ok(Some(ref p)) if p.decision == "allow_always"
                                    )
                                } else {
                                    false
                                }
                            } else {
                                false
                            };

                            if is_yolo || has_allow_always {
                                if is_yolo {
                                    info!(
                                        tool = %tool,
                                        "auto-approved by yolo mode"
                                    );
                                } else {
                                    info!(
                                        tool = %tool,
                                        "auto-approved by persisted allow-always policy"
                                    );
                                }

                                // Audit: record auto-approval in the transcript
                                // so it is visible in the conversation history.
                                let auto_approval_id = ApprovalId::new();
                                let decision = if is_yolo {
                                    ApprovalDecision::AllowOnce
                                } else {
                                    ApprovalDecision::AllowAlways
                                };
                                let note = if is_yolo {
                                    format!("auto-approved: yolo mode for {tool}")
                                } else {
                                    format!(
                                        "auto-approved: persisted allow-always policy for {tool}"
                                    )
                                };
                                let response = TranscriptItem::ApprovalResponse {
                                    approval_id: auto_approval_id,
                                    decision,
                                    note: Some(note),
                                };
                                self.append_transcript(
                                    session_id,
                                    Some(turn_id.into_uuid()),
                                    &response,
                                )
                                .await?;

                                // Re-execute the tool with the approval bypass
                                // flag, mirroring the manual resume path.
                                let mut auto_args = call.arguments.clone();
                                if let Some(obj) = auto_args.as_object_mut() {
                                    obj.insert(
                                        "__approval_resume".to_string(),
                                        serde_json::Value::Bool(true),
                                    );
                                }

                                let auto_call = ToolCall {
                                    tool_call_id: call.tool_call_id.clone(),
                                    tool_name: call.tool_name.clone(),
                                    arguments: auto_args,
                                };

                                match self.tool_executor.execute(auto_call).await {
                                    Ok(result) => result,
                                    Err(e) => {
                                        warn!(
                                            error = %e,
                                            tool = %tool,
                                            "auto-approved tool execution failed"
                                        );
                                        ToolResult {
                                            tool_call_id,
                                            output: format!("Tool error: {e}"),
                                            is_error: true,
                                            tool_execution_id: None,
                                        }
                                    }
                                }
                            } else {
                                // No allow-always policy — standard approval gate.
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

                                self.approval_repo
                                    .create(NewApproval {
                                        id: approval_id.into_uuid(),
                                        subject_type: "tool_call".to_string(),
                                        subject_id: tool_call_id.clone().into_uuid(),
                                        reason: tool.clone(),
                                        presented_payload: approval_payload(
                                            session_id,
                                            turn_id,
                                            tool_call_id.clone(),
                                            &tc.function.name,
                                            &details,
                                            &call.arguments,
                                        ),
                                        created_at: Utc::now(),
                                        handle_ref: None,
                                        host_ref: None,
                                    })
                                    .await?;

                                self.session_repo
                                    .update_status(session_id, "waiting_for_approval", Utc::now())
                                    .await?;

                                return Ok(TurnLoopOutcome::WaitingForApproval);
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, tool = %tc.function.name, "tool execution failed");
                            ToolResult {
                                tool_call_id,
                                output: format!("Tool error: {e}"),
                                is_error: true,
                                tool_execution_id: None,
                            }
                        }
                    };

                    // Persist tool result
                    let result_item = TranscriptItem::ToolResult {
                        tool_call_id: tool_result.tool_call_id,
                        output: tool_result.output.clone(),
                        is_error: tool_result.is_error,
                        tool_execution_id: tool_result.tool_execution_id,
                    };
                    self.append_transcript(session_id, Some(turn_id.into_uuid()), &result_item)
                        .await?;

                    // Emit PostToolCall hook
                    if let Some(ref hook_reg) = self.hook_registry {
                        let mut hook_ctx = serde_json::json!({
                            "tool_name": tc.function.name,
                            "session_id": session_id.to_string(),
                            "turn_id": turn_id.into_uuid().to_string(),
                            "output": tool_result.output,
                            "is_error": tool_result.is_error,
                        });
                        hook_reg.emit(&HookEvent::PostToolCall, &mut hook_ctx).await;
                    }
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

            return Ok(TurnLoopOutcome::Completed);
        }
    }

    /// Non-streaming model call with transient-error retries and exponential backoff.
    async fn complete_with_retries(
        &self,
        request: &CompletionRequest,
    ) -> Result<rune_models::CompletionResponse, RuntimeError> {
        const MAX_RETRIES: u32 = 3;
        const BACKOFF_SECS: [u64; 3] = [1, 3, 10];
        let mut last_err = None;

        for attempt in 0..=MAX_RETRIES {
            match self.model_provider.complete(request).await {
                Ok(r) => return Ok(r),
                Err(e) => {
                    if attempt < MAX_RETRIES && e.is_retriable() {
                        let delay = BACKOFF_SECS[attempt as usize];
                        warn!(
                            error = %e,
                            attempt = attempt + 1,
                            max_retries = MAX_RETRIES,
                            backoff_secs = delay,
                            "transient model error, retrying"
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                        last_err = Some(e);
                    } else {
                        error!(error = %e, "model call failed");
                        return Err(RuntimeError::Model(e));
                    }
                }
            }
        }

        Err(RuntimeError::Model(last_err.unwrap()))
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

    async fn update_approval_progress(
        &self,
        approval_id: Uuid,
        resumed_at: chrono::DateTime<Utc>,
        approval_status: &str,
        result_summary: Option<String>,
    ) -> Result<(), RuntimeError> {
        let approval = self.approval_repo.find_by_id(approval_id).await?;
        let mut payload = approval.presented_payload;
        let Some(obj) = payload.as_object_mut() else {
            return Ok(());
        };

        obj.insert(
            "resumed_at".to_string(),
            serde_json::Value::String(resumed_at.to_rfc3339()),
        );
        obj.insert(
            "resume_status".to_string(),
            serde_json::Value::String(approval_status.to_string()),
        );
        obj.insert(
            "approval_status".to_string(),
            serde_json::Value::String(approval_status.to_string()),
        );
        obj.insert(
            "approval_status_updated_at".to_string(),
            serde_json::Value::String(Utc::now().to_rfc3339()),
        );
        if matches!(
            approval_status,
            "completed" | "completed_error" | "failed" | "denied"
        ) {
            obj.insert(
                "completed_at".to_string(),
                serde_json::Value::String(Utc::now().to_rfc3339()),
            );
        }
        if let Some(summary) = result_summary {
            obj.insert(
                "resume_result_summary".to_string(),
                serde_json::Value::String(summary),
            );
        }

        self.approval_repo
            .update_presented_payload(approval_id, payload)
            .await?;
        Ok(())
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
        TranscriptItem::SystemInstruction { .. } => "system_instruction",
        TranscriptItem::ChannelDeliveryNote { .. } => "channel_delivery_note",
        TranscriptItem::CronHeartbeatNote { .. } => "cron_heartbeat_note",
    }
}

enum TurnLoopOutcome {
    Completed,
    WaitingForApproval,
}

fn approval_payload(
    session_id: Uuid,
    turn_id: TurnId,
    tool_call_id: ToolCallId,
    tool_name: &str,
    details: &str,
    arguments: &serde_json::Value,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "session_id": session_id,
        "turn_id": turn_id.into_uuid(),
        "tool_call_id": tool_call_id.into_uuid(),
        "tool_name": tool_name,
        "arguments": arguments,
        "command": extract_approval_command(details),
        "details": details,
        "resume_status": "pending",
        "approval_status": "pending",
    });

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(details) {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("approval_request".to_string(), parsed);
        }
    }

    payload
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

    details.lines().find_map(|line| {
        line.trim()
            .strip_prefix("command:")
            .map(|s| s.trim().to_string())
    })
}

fn parse_uuid_runtime(raw: &str) -> Result<Uuid, RuntimeError> {
    Uuid::parse_str(raw)
        .map_err(|e| RuntimeError::Aborted(format!("invalid UUID in approval payload: {e}")))
}

fn parse_approval_decision(raw: &str) -> Result<ApprovalDecision, RuntimeError> {
    match raw {
        "allow_once" => Ok(ApprovalDecision::AllowOnce),
        "allow_always" => Ok(ApprovalDecision::AllowAlways),
        "deny" => Ok(ApprovalDecision::Deny),
        other => Err(RuntimeError::Aborted(format!(
            "unsupported approval decision: {other}"
        ))),
    }
}
