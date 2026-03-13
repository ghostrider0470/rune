//! The session loop: bridges inbound channel events to the turn executor.
//!
//! This is the core pipeline: channel message → find/create session → execute turn → reply.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use rune_channels::{ChannelAdapter, InboundEvent, OutboundAction};
use rune_config::AgentsConfig;
use rune_core::SessionKind;
use rune_store::models::SessionRow;
use rune_store::repos::SessionRepo;

use crate::engine::SessionEngine;
use crate::error::RuntimeError;
use crate::executor::TurnExecutor;

const HELP_TEXT: &str = "\
Rune commands:
/start - show welcome text
/help - show this help
/status - show runtime status
/model - switch model (or list available)
/reset - clear session history";

/// Maps channel routing key → session_id.
type SessionIndex = HashMap<String, Uuid>;

/// The main session loop that ties channels to the runtime.
pub struct SessionLoop {
    engine: Arc<SessionEngine>,
    turn_executor: Arc<TurnExecutor>,
    session_repo: Arc<dyn SessionRepo>,
    channel: Arc<Mutex<Box<dyn ChannelAdapter>>>,
    sessions: Mutex<SessionIndex>,
    agents: AgentsConfig,
    /// Per-session model overrides set via /model command.
    model_overrides: Mutex<HashMap<Uuid, String>>,
}

impl SessionLoop {
    pub fn new(
        engine: Arc<SessionEngine>,
        turn_executor: Arc<TurnExecutor>,
        session_repo: Arc<dyn SessionRepo>,
        channel: Box<dyn ChannelAdapter>,
        agents: AgentsConfig,
    ) -> Self {
        Self {
            engine,
            turn_executor,
            session_repo,
            channel: Arc::new(Mutex::new(channel)),
            sessions: Mutex::new(HashMap::new()),
            agents,
            model_overrides: Mutex::new(HashMap::new()),
        }
    }

    /// Run the session loop forever, processing inbound events.
    pub async fn run(&self) -> Result<(), RuntimeError> {
        info!("session loop started, waiting for inbound events");

        loop {
            let event = {
                let mut ch = self.channel.lock().await;
                match ch.receive().await {
                    Ok(event) => event,
                    Err(e) => {
                        error!(error = %e, "channel receive error, retrying in 5s");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                }
            };

            if let Err(e) = self.handle_event(event).await {
                error!(error = %e, "failed to handle inbound event");
            }
        }
    }

    async fn handle_event(&self, event: InboundEvent) -> Result<(), RuntimeError> {
        match event {
            InboundEvent::Message(msg) => {
                if self.handle_command(&msg).await? {
                    return Ok(());
                }

                let routing_key = format!("{}:{}", msg.raw_chat_id, msg.sender);
                debug!(routing_key = %routing_key, "received inbound message");

                let session = self.find_or_create_session(&routing_key).await?;

                info!(session_id = %session.id, len = msg.content.len(), "executing turn");

                // Send typing indicator before executing the turn
                {
                    let ch = self.channel.lock().await;
                    let _ = ch
                        .send(OutboundAction::SendTypingIndicator {
                            channel_id: msg.channel_id,
                            chat_id: msg.raw_chat_id.clone(),
                        })
                        .await;
                }

                // Resolve model override for this session
                let model_override = {
                    let overrides = self.model_overrides.lock().await;
                    overrides.get(&session.id).cloned()
                };

                match self
                    .turn_executor
                    .execute(
                        session.id,
                        &msg.content,
                        model_override.as_deref(),
                    )
                    .await
                {
                    Ok((turn, usage)) => {
                        debug!(
                            turn_id = %turn.id,
                            prompt_tokens = usage.prompt_tokens,
                            completion_tokens = usage.completion_tokens,
                            "turn completed"
                        );

                        if let Some(reply) = self.get_last_assistant_message(session.id).await {
                            let ch = self.channel.lock().await;
                            if let Err(e) = ch
                                .send(OutboundAction::Reply {
                                    channel_id: msg.channel_id,
                                    chat_id: msg.raw_chat_id.clone(),
                                    reply_to: msg.provider_message_id.clone(),
                                    content: reply,
                                })
                                .await
                            {
                                error!(error = %e, "failed to send reply");
                            }
                        }
                    }
                    Err(e) => {
                        error!(session_id = %session.id, error = %e, "turn failed");
                        let ch = self.channel.lock().await;
                        let _ = ch
                            .send(OutboundAction::Reply {
                                channel_id: msg.channel_id,
                                chat_id: msg.raw_chat_id.clone(),
                                reply_to: msg.provider_message_id.clone(),
                                content: format!("Turn failed: {e}"),
                            })
                            .await;
                    }
                }
            }
            _ => {
                debug!("ignoring unhandled event type");
            }
        }

        Ok(())
    }

    async fn handle_command(
        &self,
        msg: &rune_channels::ChannelMessage,
    ) -> Result<bool, RuntimeError> {
        let text = msg.content.trim();

        // Handle callback data from inline keyboards (e.g. "/model claude-sonnet-4-6")
        // and regular slash commands
        let (cmd, args) = match text.split_once(' ') {
            Some((cmd, args)) => (cmd, args.trim()),
            None => (text, ""),
        };

        match cmd {
            "/start" => {
                self.send_command_reply(msg, "Rune is online. Send a message to start a session, or use /help for commands.").await;
                Ok(true)
            }
            "/help" => {
                self.send_command_reply(msg, HELP_TEXT).await;
                Ok(true)
            }
            "/status" => {
                let routing_key = format!("{}:{}", msg.raw_chat_id, msg.sender);
                let status = self.render_status(&routing_key).await?;
                self.send_command_reply(msg, &status).await;
                Ok(true)
            }
            "/model" => {
                self.handle_model_command(msg, args).await?;
                Ok(true)
            }
            "/reset" => {
                self.handle_reset_command(msg).await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    async fn handle_model_command(
        &self,
        msg: &rune_channels::ChannelMessage,
        args: &str,
    ) -> Result<(), RuntimeError> {
        if args.is_empty() {
            // No model specified → show available models as inline keyboard
            let mut buttons: Vec<(String, String)> = self
                .agents
                .list
                .iter()
                .filter_map(|a| {
                    let model = self.agents.effective_model(a)?;
                    Some((
                        format!("{} ({})", a.id, model),
                        format!("/model {model}"),
                    ))
                })
                .collect();

            if buttons.is_empty() {
                // No agents configured, just show a help message
                self.send_command_reply(msg, "No agents configured. Use /model <name> to set a model directly.").await;
            } else {
                // Deduplicate by callback_data
                buttons.dedup_by(|a, b| a.1 == b.1);

                let ch = self.channel.lock().await;
                let _ = ch
                    .send(OutboundAction::SendInlineKeyboard {
                        channel_id: msg.channel_id,
                        chat_id: msg.raw_chat_id.clone(),
                        content: "Select a model:".to_string(),
                        buttons,
                    })
                    .await;
            }
        } else {
            // Model specified → set it for this session
            let routing_key = format!("{}:{}", msg.raw_chat_id, msg.sender);
            let session = self.find_or_create_session(&routing_key).await?;

            {
                let mut overrides = self.model_overrides.lock().await;
                overrides.insert(session.id, args.to_string());
            }

            info!(session_id = %session.id, model = %args, "model override set");
            self.send_command_reply(
                msg,
                &format!("Model switched to `{args}` for this session."),
            )
            .await;
        }
        Ok(())
    }

    async fn handle_reset_command(
        &self,
        msg: &rune_channels::ChannelMessage,
    ) -> Result<(), RuntimeError> {
        let routing_key = format!("{}:{}", msg.raw_chat_id, msg.sender);

        // Remove from in-memory cache so next message creates a new session
        {
            let mut index = self.sessions.lock().await;
            if let Some(old_id) = index.remove(&routing_key) {
                // Mark old session as completed
                if let Err(e) = self
                    .session_repo
                    .update_status(old_id, "completed", chrono::Utc::now())
                    .await
                {
                    warn!(error = %e, "failed to mark old session as completed");
                }

                // Remove model override for old session
                let mut overrides = self.model_overrides.lock().await;
                overrides.remove(&old_id);
            }
        }

        info!(routing_key = %routing_key, "session reset");
        self.send_command_reply(msg, "Session reset. Your next message starts a fresh conversation.")
            .await;
        Ok(())
    }

    async fn send_command_reply(&self, msg: &rune_channels::ChannelMessage, content: &str) {
        let ch = self.channel.lock().await;
        if let Err(e) = ch
            .send(OutboundAction::Reply {
                channel_id: msg.channel_id,
                chat_id: msg.raw_chat_id.clone(),
                reply_to: msg.provider_message_id.clone(),
                content: content.to_string(),
            })
            .await
        {
            error!(error = %e, "failed to send command reply");
        }
    }

    async fn render_status(&self, routing_key: &str) -> Result<String, RuntimeError> {
        let sessions = self.session_repo.list(10, 0).await?;
        let active = sessions
            .iter()
            .filter(|s| {
                matches!(
                    s.status.as_str(),
                    "created"
                        | "ready"
                        | "running"
                        | "waiting_for_tool"
                        | "waiting_for_approval"
                        | "waiting_for_subagent"
                )
            })
            .count();

        let current_model = {
            let index = self.sessions.lock().await;
            if let Some(session_id) = index.get(routing_key) {
                let overrides = self.model_overrides.lock().await;
                overrides.get(session_id).cloned()
            } else {
                None
            }
        };

        let default_model = self
            .agents
            .default_agent()
            .and_then(|a| self.agents.effective_model(a))
            .unwrap_or("(not configured)");

        let model_display = current_model
            .as_deref()
            .unwrap_or(default_model);

        let session_info = {
            let index = self.sessions.lock().await;
            index
                .get(routing_key)
                .map(|id| format!("session={id}"))
                .unwrap_or_else(|| "session=none".to_string())
        };

        Ok(format!(
            "Rune status: ok\nmodel: {model_display}\n{session_info}\nsessions: {} total, {active} active",
            sessions.len()
        ))
    }

    async fn find_or_create_session(
        &self,
        routing_key: &str,
    ) -> Result<SessionRow, RuntimeError> {
        // 1. Check in-memory cache
        {
            let index = self.sessions.lock().await;
            if let Some(session_id) = index.get(routing_key) {
                if let Ok(session) = self.session_repo.find_by_id(*session_id).await {
                    return Ok(session);
                }
            }
        }

        // 2. Check database (survives restarts)
        if let Some(session) = self.session_repo.find_by_channel_ref(routing_key).await? {
            info!(session_id = %session.id, routing_key = %routing_key, "resumed existing session from DB");
            let mut index = self.sessions.lock().await;
            index.insert(routing_key.to_string(), session.id);
            return Ok(session);
        }

        // 3. Create new session with routing_key as channel_ref
        let workspace = self
            .agents
            .default_agent()
            .and_then(|a| self.agents.effective_workspace(a))
            .map(String::from);
        let session = self
            .engine
            .create_session_full(
                SessionKind::Channel,
                workspace,
                None,
                Some(routing_key.to_string()),
            )
            .await?;

        info!(session_id = %session.id, routing_key = %routing_key, "created new session");

        {
            let mut index = self.sessions.lock().await;
            index.insert(routing_key.to_string(), session.id);
        }

        Ok(session)
    }

    async fn get_last_assistant_message(&self, session_id: Uuid) -> Option<String> {
        let transcript_repo = self.turn_executor.transcript_repo();
        let items = transcript_repo.list_by_session(session_id).await.ok()?;

        for item in items.iter().rev() {
            if item.kind == "assistant_message" {
                if let Ok(rune_core::TranscriptItem::AssistantMessage { content }) =
                    serde_json::from_value::<rune_core::TranscriptItem>(item.payload.clone())
                {
                    return Some(content);
                }
            }
        }
        None
    }
}
