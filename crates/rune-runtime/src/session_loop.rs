//! The session loop: bridges inbound channel events to the turn executor.
//!
//! This is the core pipeline: channel message → find/create session → execute turn → reply.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use rune_channels::{ChannelAdapter, InboundEvent, OutboundAction};
use rune_config::{AgentsConfig, ModelsConfig};
use rune_core::SessionKind;
use rune_store::models::SessionRow;
use rune_store::repos::SessionRepo;
use rune_stt::SttEngine;

use crate::engine::SessionEngine;
use crate::error::RuntimeError;
use crate::executor::TurnExecutor;
use crate::session_metadata::{selected_model, set_selected_model};

/// Trait for downloading files from Telegram (or any provider using `telegram-file:` URLs).
///
/// Implemented outside the runtime crate so the session loop stays generic.
#[async_trait]
pub trait TelegramFileDownloader: Send + Sync {
    /// Download a file by its provider-specific file ID.
    /// Returns the raw bytes on success.
    async fn download(&self, file_id: &str) -> Result<Vec<u8>, String>;
}

const HELP_TEXT: &str = "\
Rune commands:
/start - show welcome text
/help - show this help
/status - show runtime status
/model - switch model (or list available)
/reset - clear session history";

/// Maps channel routing key → session_id.
type SessionIndex = HashMap<String, Uuid>;

type EventPriority = u8;
pub const PRIORITY_IMMEDIATE: EventPriority = 0;
pub const PRIORITY_USER_MESSAGE: EventPriority = 1;
pub const PRIORITY_COMMS_DIRECTIVE: EventPriority = 2;
pub const PRIORITY_CRON: EventPriority = 3;
pub const PRIORITY_BACKGROUND: EventPriority = 4;

/// The main session loop that ties channels to the runtime.
pub struct SessionLoop {
    engine: Arc<SessionEngine>,
    turn_executor: Arc<TurnExecutor>,
    session_repo: Arc<dyn SessionRepo>,
    channel: Arc<Mutex<Box<dyn ChannelAdapter>>>,
    sessions: Mutex<SessionIndex>,
    resumed_session_notifications: Mutex<HashMap<String, String>>,
    restored_session_routes: Mutex<HashMap<String, String>>,
    agents: AgentsConfig,
    models: ModelsConfig,
    stt_engine: Option<Arc<RwLock<SttEngine>>>,
    file_downloader: Option<Arc<dyn TelegramFileDownloader>>,
    command_registry: Option<Arc<crate::command_registry::CommandRegistry>>,
}

fn classify_message_priority(content: &str) -> EventPriority {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return PRIORITY_BACKGROUND;
    }

    if is_comms_directive_message(trimmed) {
        return PRIORITY_COMMS_DIRECTIVE;
    }

    if is_cron_message(trimmed) {
        return PRIORITY_CRON;
    }

    PRIORITY_USER_MESSAGE
}

fn is_comms_directive_message(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let directive_prefixes = [
        "directive:",
        "comms directive:",
        "[directive]",
        "[comms] directive",
    ];
    directive_prefixes.iter().any(|prefix| lower.starts_with(prefix))
}

fn is_cron_message(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let cron_prefixes = ["cron:", "scheduled:", "heartbeat:", "reminder:"];
    cron_prefixes.iter().any(|prefix| lower.starts_with(prefix))
}

/// MIME types considered audio for transcription purposes.
const AUDIO_MIME_TYPES: &[&str] = &[
    "audio/ogg",
    "audio/opus",
    "application/ogg",
    "audio/mpeg",
    "audio/mp3",
    "audio/mp4",
    "audio/m4a",
    "audio/wav",
    "audio/x-wav",
    "audio/webm",
    "audio/flac",
    "audio/x-flac",
    "audio/aac",
];

/// MIME type prefixes considered images.
const IMAGE_MIME_PREFIX: &str = "image/";

impl SessionLoop {
    pub fn new(
        engine: Arc<SessionEngine>,
        turn_executor: Arc<TurnExecutor>,
        session_repo: Arc<dyn SessionRepo>,
        channel: Box<dyn ChannelAdapter>,
        agents: AgentsConfig,
        models: ModelsConfig,
    ) -> Self {
        Self {
            engine,
            turn_executor,
            session_repo,
            channel: Arc::new(Mutex::new(channel)),
            sessions: Mutex::new(HashMap::new()),
            resumed_session_notifications: Mutex::new(HashMap::new()),
            restored_session_routes: Mutex::new(HashMap::new()),
            agents,
            models,
            stt_engine: None,
            file_downloader: None,
            command_registry: None,
        }
    }

    /// Attach an STT engine for voice/audio transcription.
    pub fn with_stt(mut self, stt_engine: Arc<RwLock<SttEngine>>) -> Self {
        self.stt_engine = Some(stt_engine);
        self
    }

    /// Attach a file downloader for resolving `telegram-file:` URLs.
    pub fn with_file_downloader(mut self, downloader: Arc<dyn TelegramFileDownloader>) -> Self {
        self.file_downloader = Some(downloader);
        self
    }

    /// Attach a command registry for plugin slash commands.
    pub fn with_command_registry(
        mut self,
        registry: Arc<crate::command_registry::CommandRegistry>,
    ) -> Self {
        self.command_registry = Some(registry);
        self
    }

    pub(crate) async fn run_startup_restore(&self) -> Result<(), RuntimeError> {
        let sessions = self.session_repo.list_active_channel_sessions().await?;
        let mut index = self.sessions.lock().await;
        let mut restored = self.restored_session_routes.lock().await;
        for session in &sessions {
            if let Some(ref channel_ref) = session.channel_ref {
                index.insert(channel_ref.clone(), session.id);
                restored.insert(channel_ref.clone(), session.last_activity_at.to_rfc3339());
            }
        }
        if !sessions.is_empty() {
            info!(
                count = sessions.len(),
                "restored active channel sessions from DB"
            );
        }
        Ok(())
    }

    /// Run the session loop forever, processing inbound events.
    pub async fn run(&self) -> Result<(), RuntimeError> {
        // Pre-populate the session index from DB so existing Channel
        // sessions resume after a gateway restart.
        if let Err(e) = self.run_startup_restore().await {
            warn!(error = %e, "failed to restore channel sessions from DB, starting fresh");
        }

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

    fn classify_event_priority(event: &InboundEvent) -> EventPriority {
        match event {
            InboundEvent::Message(msg) => classify_message_priority(&msg.content),
            InboundEvent::Reaction { .. }
            | InboundEvent::Edit { .. }
            | InboundEvent::Delete { .. }
            | InboundEvent::MemberJoin { .. }
            | InboundEvent::MemberLeave { .. } => PRIORITY_BACKGROUND,
        }
    }

    pub fn classify_source_priority(source: &str) -> EventPriority {
        match source.trim().to_ascii_lowercase().as_str() {
            "heartbeat" | "health" | "health-check" | "health_check" => PRIORITY_IMMEDIATE,
            "telegram" | "telegram-message" | "telegram_message" | "user" => {
                PRIORITY_USER_MESSAGE
            }
            "comms" | "directive" | "comms-directive" | "comms_directive" => {
                PRIORITY_COMMS_DIRECTIVE
            }
            "cron" | "scheduler" | "scheduled" => PRIORITY_CRON,
            _ => PRIORITY_BACKGROUND,
        }
    }

    #[must_use]
    pub fn event_priority_for_test(event: &InboundEvent) -> EventPriority {
        Self::classify_event_priority(event)
    }

    #[must_use]
    pub fn source_priority_for_test(source: &str) -> EventPriority {
        Self::classify_source_priority(source)
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
                self.maybe_send_resumed_session_notice(&msg, &routing_key, &session)
                    .await;

                // Enrich content by downloading/transcribing media attachments.
                let enriched_content = self.enrich_media_content(&msg).await;

                // Expand plugin slash commands into their full prompt body.
                let final_text = if enriched_content.starts_with('/') {
                    if let Some(ref registry) = self.command_registry {
                        let (cmd, args) = match enriched_content.split_once(' ') {
                            Some((c, a)) => (c.trim_start_matches('/'), a.trim()),
                            None => (enriched_content.trim_start_matches('/'), ""),
                        };
                        if let Some(command) = registry.get(cmd).await {
                            info!(command = cmd, plugin = %command.plugin_name, "expanding plugin command");
                            command.expand(args)
                        } else {
                            enriched_content.clone()
                        }
                    } else {
                        enriched_content.clone()
                    }
                } else {
                    enriched_content.clone()
                };

                info!(session_id = %session.id, len = final_text.len(), "executing turn");

                // Send a "Thinking…" placeholder reply so the user gets immediate feedback.
                let placeholder_id = {
                    let ch = self.channel.lock().await;
                    // Also triggers typing indicator implicitly via the message.
                    let _ = ch
                        .send(OutboundAction::SendTypingIndicator {
                            channel_id: msg.channel_id,
                            chat_id: msg.raw_chat_id.clone(),
                        })
                        .await;
                    match ch
                        .send(OutboundAction::Reply {
                            channel_id: msg.channel_id,
                            chat_id: msg.raw_chat_id.clone(),
                            reply_to: msg.provider_message_id.clone(),
                            content: "Thinking…".to_string(),
                        })
                        .await
                    {
                        Ok(receipt) => Some(receipt.provider_message_id),
                        Err(e) => {
                            warn!(error = %e, "failed to send placeholder message");
                            None
                        }
                    }
                };

                // Set up a streaming channel so we can progressively edit
                // the placeholder message as the model generates text.
                let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel::<String>(64);

                // Spawn a background task that edits the placeholder with
                // accumulated text, throttled to at most one edit per 500ms
                // to stay within Telegram rate limits.
                let edit_handle = if let Some(ref ph_id) = placeholder_id {
                    let channel = self.channel.clone();
                    let channel_id = msg.channel_id;
                    let chat_id = msg.raw_chat_id.clone();
                    let ph_id = ph_id.clone();

                    Some(tokio::spawn(progressive_edit_loop(
                        channel, channel_id, chat_id, ph_id, chunk_rx,
                    )))
                } else {
                    // No placeholder — drain chunks so the sender never blocks.
                    Some(tokio::spawn(drain_chunks(chunk_rx)))
                };

                let result = self
                    .turn_executor
                    .execute_streaming_with_attachments(
                        session.id,
                        &final_text,
                        msg.attachments.clone(),
                        None,
                        chunk_tx,
                    )
                    .await;

                // Wait for the progressive edit task to finish (it exits when
                // the chunk_tx sender is dropped, i.e. when execute_streaming
                // returns above).
                if let Some(handle) = edit_handle {
                    let _ = handle.await;
                }

                match result {
                    Ok((turn, usage)) => {
                        debug!(
                            turn_id = %turn.id,
                            prompt_tokens = usage.prompt_tokens,
                            completion_tokens = usage.completion_tokens,
                            "turn completed"
                        );

                        if let Some(reply) = self.get_last_assistant_message(session.id).await {
                            let ch = self.channel.lock().await;
                            // Final edit with the authoritative response from
                            // the transcript (ensures consistency after tool
                            // calls or partial streaming).
                            let result = if let Some(ref ph_id) = placeholder_id {
                                ch.send(OutboundAction::Edit {
                                    channel_id: msg.channel_id,
                                    chat_id: msg.raw_chat_id.clone(),
                                    message_id: ph_id.clone(),
                                    new_content: reply,
                                })
                                .await
                            } else {
                                ch.send(OutboundAction::Reply {
                                    channel_id: msg.channel_id,
                                    chat_id: msg.raw_chat_id.clone(),
                                    reply_to: msg.provider_message_id.clone(),
                                    content: reply,
                                })
                                .await
                            };
                            if let Err(e) = result {
                                error!(error = %e, "failed to send reply");
                            }
                        } else if let Some(ref ph_id) = placeholder_id {
                            // No assistant message — remove the placeholder.
                            let ch = self.channel.lock().await;
                            let _ = ch
                                .send(OutboundAction::Delete {
                                    channel_id: msg.channel_id,
                                    chat_id: msg.raw_chat_id.clone(),
                                    message_id: ph_id.clone(),
                                })
                                .await;
                        }
                    }
                    Err(e) => {
                        error!(session_id = %session.id, error = %e, "turn failed");
                        let brief = {
                            let full = e.to_string();
                            if full.len() > 200 {
                                format!("{}…", &full[..200])
                            } else {
                                full
                            }
                        };
                        let error_content =
                            format!("Sorry, I encountered an error: {brief}. Please try again.");
                        let ch = self.channel.lock().await;
                        // Edit the placeholder with the error, or send a new reply.
                        if let Some(ref ph_id) = placeholder_id {
                            let _ = ch
                                .send(OutboundAction::Edit {
                                    channel_id: msg.channel_id,
                                    chat_id: msg.raw_chat_id.clone(),
                                    message_id: ph_id.clone(),
                                    new_content: error_content,
                                })
                                .await;
                        } else {
                            let _ = ch
                                .send(OutboundAction::Reply {
                                    channel_id: msg.channel_id,
                                    chat_id: msg.raw_chat_id.clone(),
                                    reply_to: msg.provider_message_id.clone(),
                                    content: error_content,
                                })
                                .await;
                        }
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
                self.send_command_reply(
                    msg,
                    "Rune is online. Send a message to start a session, or use /help for commands.",
                )
                .await;
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
            "/commands" => {
                let mut text = String::from("Available commands:\n\n");
                text.push_str("/start - show welcome\n/help - show help\n/status - runtime status\n/model - switch model\n/reset - clear history\n/commands - this list\n");
                if let Some(ref registry) = self.command_registry {
                    let commands = registry.list().await;
                    if !commands.is_empty() {
                        text.push_str("\nPlugin commands:\n");
                        for cmd in &commands {
                            text.push_str(&format!(
                                "/{} - {}\n",
                                cmd.short_name(),
                                cmd.description
                            ));
                        }
                    }
                }
                self.send_command_reply(msg, &text).await;
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
                .available_models()
                .into_iter()
                .map(|model| (model.clone(), format!("/model {model}")))
                .collect();

            if buttons.is_empty() {
                self.send_command_reply(
                    msg,
                    "No configured models found. Add provider inventories under [models.providers].",
                )
                .await;
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
            let resolved = match self.models.resolve_model(args) {
                Ok(model) => model,
                Err(_) => {
                    self.send_command_reply(
                        msg,
                        &format!("Unknown model `{args}`. Use `/model` to list configured models."),
                    )
                    .await;
                    return Ok(());
                }
            };

            // Model specified → set it for this session
            let routing_key = format!("{}:{}", msg.raw_chat_id, msg.sender);
            let session = self.find_or_create_session(&routing_key).await?;
            let model_id = resolved.canonical_model_id();
            let metadata = set_selected_model(&session.metadata, &model_id);
            self.session_repo
                .update_metadata(session.id, metadata, chrono::Utc::now())
                .await?;

            info!(session_id = %session.id, model = %model_id, "model override set");
            self.send_command_reply(
                msg,
                &format!("Model switched to `{model_id}` for this session."),
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
            }
        }

        info!(routing_key = %routing_key, "session reset");
        self.send_command_reply(
            msg,
            "Session reset. Your next message starts a fresh conversation.",
        )
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
            let session = {
                let session_id = {
                    let index = self.sessions.lock().await;
                    index.get(routing_key).copied()
                };

                if let Some(session_id) = session_id {
                    self.session_repo.find_by_id(session_id).await.ok()
                } else {
                    self.session_repo.find_by_channel_ref(routing_key).await?
                }
            };

            session.and_then(|session| selected_model(&session).map(str::to_owned))
        };

        let default_model = self
            .agents
            .default_agent()
            .and_then(|a| self.agents.effective_model(a))
            .or(self.models.default_model.as_deref())
            .unwrap_or("(not configured)");

        let model_display = current_model.as_deref().unwrap_or(default_model);
        let model_origin = if current_model.is_some() {
            "session override"
        } else {
            "default"
        };

        let session_info = {
            if let Some(session) = self.session_repo.find_by_channel_ref(routing_key).await? {
                format!("session={}", session.id)
            } else {
                "session=none".to_string()
            }
        };

        Ok(format!(
            "Rune status: ok\nactive_model: {model_display} ({model_origin})\ndefault_model: {default_model}\n{session_info}\nsessions: {} total, {active} active",
            sessions.len()
        ))
    }

    async fn find_or_create_session(&self, routing_key: &str) -> Result<SessionRow, RuntimeError> {
        // 1. Check in-memory cache
        {
            let index = self.sessions.lock().await;
            if let Some(session_id) = index.get(routing_key) {
                if let Ok(session) = self.session_repo.find_by_id(*session_id).await {
                    return Ok(session);
                }
            }
        }

        // 2. Serialize DB/create path per routing key to avoid duplicate channel
        // sessions when multiple inbound events race before the cache is hydrated.
        let mut index = self.sessions.lock().await;
        if let Some(session_id) = index.get(routing_key).copied() {
            drop(index);
            if let Ok(session) = self.session_repo.find_by_id(session_id).await {
                return Ok(session);
            }
            index = self.sessions.lock().await;
        }

        if let Some(session) = self.session_repo.find_by_channel_ref(routing_key).await? {
            info!(session_id = %session.id, routing_key = %routing_key, "resumed existing session from DB");
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
                None,
                None,
            )
            .await?;

        info!(session_id = %session.id, routing_key = %routing_key, "created new session");

        {
            let mut index = self.sessions.lock().await;
            index.insert(routing_key.to_string(), session.id);
        }

        Ok(session)
    }

    pub(crate) async fn maybe_send_resumed_session_notice(
        &self,
        msg: &rune_channels::ChannelMessage,
        routing_key: &str,
        session: &SessionRow,
    ) {
        if session.kind != "channel" {
            return;
        }

        let fingerprint = session.last_activity_at.to_rfc3339();
        let restored = {
            let restored = self.restored_session_routes.lock().await;
            restored.get(routing_key).cloned()
        };
        if restored.as_deref() != Some(fingerprint.as_str()) {
            return;
        }

        let last_notified = {
            let notices = self.resumed_session_notifications.lock().await;
            notices.get(routing_key).cloned()
        };

        if last_notified.as_deref() == Some(fingerprint.as_str()) {
            return;
        }

        let text = crate::restart_continuity::RESUMED_SESSION_NOTICE_TEMPLATE
            .replace("{session_id}", &session.id.to_string());

        let ch = self.channel.lock().await;
        match ch
            .send(OutboundAction::Reply {
                channel_id: msg.channel_id,
                chat_id: msg.raw_chat_id.clone(),
                reply_to: msg.provider_message_id.clone(),
                content: text,
            })
            .await
        {
            Ok(_) => {
                let mut notices = self.resumed_session_notifications.lock().await;
                notices.insert(routing_key.to_string(), fingerprint);
            }
            Err(e) => {
                warn!(error = %e, session_id = %session.id, "failed to send resumed-session notice");
            }
        }
    }

    fn available_models(&self) -> Vec<String> {
        let mut models = self.models.model_ids();

        if models.is_empty() {
            if let Some(default_model) = &self.models.default_model {
                models.push(default_model.clone());
            }

            for agent in &self.agents.list {
                if let Some(model) = self.agents.effective_model(agent) {
                    models.push(model.to_string());
                }
            }

            models.sort();
            models.dedup();
        }

        models
    }

    /// Inspect message attachments for `telegram-file:` URLs and enrich the
    /// message content with transcriptions (audio). Image attachments are kept
    /// as first-class multimodal parts and should not be flattened into text
    /// placeholders here.
    pub(crate) async fn enrich_media_content(&self, msg: &rune_channels::ChannelMessage) -> String {
        let mut extra_parts: Vec<String> = Vec::new();

        for attachment in &msg.attachments {
            let url = match attachment.url.as_deref() {
                Some(u) if u.starts_with("telegram-file:") => u,
                _ => continue,
            };
            let file_id = &url["telegram-file:".len()..];
            let mime = attachment.mime_type.as_deref().unwrap_or("");

            if AUDIO_MIME_TYPES.contains(&mime) {
                // Attempt download + transcription
                info!(file_id = %file_id, mime = %mime, "downloading audio attachment for transcription");
                match self.download_and_transcribe(file_id, mime).await {
                    Ok(text) => {
                        info!(file_id = %file_id, chars = text.len(), "transcription succeeded");
                        extra_parts.push(format!("[Voice transcription]: {text}"));
                    }
                    Err(e) => {
                        warn!(file_id = %file_id, error = %e, "transcription failed");
                        extra_parts.push(format!("[Voice message — transcription failed: {e}]"));
                    }
                }
            } else if mime.starts_with(IMAGE_MIME_PREFIX) {
                info!(file_id = %file_id, mime = %mime, "image attachment detected; preserving multimodal attachment without text placeholder");
            }
        }

        if extra_parts.is_empty() {
            return msg.content.clone();
        }

        let enrichment = extra_parts.join("\n");

        if msg.content.is_empty() {
            // Voice-only message: transcription IS the content (strip the prefix
            // when there is exactly one audio part so the turn sees clean text).
            if extra_parts.len() == 1 && extra_parts[0].starts_with("[Voice transcription]: ") {
                extra_parts[0]["[Voice transcription]: ".len()..].to_string()
            } else {
                enrichment
            }
        } else {
            format!("{}\n{enrichment}", msg.content)
        }
    }

    /// Download a file via the downloader and transcribe via the STT engine.
    async fn download_and_transcribe(&self, file_id: &str, mime: &str) -> Result<String, String> {
        let downloader = self
            .file_downloader
            .as_ref()
            .ok_or_else(|| "no file downloader configured".to_string())?;

        let audio_bytes = downloader.download(file_id).await?;

        let stt = self
            .stt_engine
            .as_ref()
            .ok_or_else(|| "no STT engine configured".to_string())?;

        let engine = stt.read().await;
        let result = engine
            .transcribe(&audio_bytes, mime)
            .await
            .map_err(|e| format!("{e}"))?;

        Ok(result.text)
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

#[cfg(test)]
pub(crate) async fn enrich_media_content_for_test(
    session_loop: &SessionLoop,
    msg: &rune_channels::ChannelMessage,
) -> String {
    session_loop.enrich_media_content(msg).await
}
// ── Streaming helpers ───────────────────────────────────────────────

/// Progressively edit a Telegram placeholder message as text chunks arrive.
///
/// Throttles edits to at most one every 500 ms to stay within Telegram Bot API
/// rate limits (~30 messages per second per chat). The very first chunk always
/// triggers an immediate edit so the user sees output as fast as possible.
async fn progressive_edit_loop(
    channel: Arc<Mutex<Box<dyn rune_channels::ChannelAdapter>>>,
    channel_id: rune_core::ChannelId,
    chat_id: String,
    message_id: String,
    mut chunk_rx: tokio::sync::mpsc::Receiver<String>,
) {
    use std::time::Duration;
    use tokio::time::Instant;

    let throttle = Duration::from_millis(500);
    // Set last_edit far enough in the past so the first chunk triggers an edit.
    let mut last_edit = Instant::now() - throttle;
    let mut accumulated = String::new();

    loop {
        match chunk_rx.recv().await {
            Some(chunk) => {
                accumulated.push_str(&chunk);

                if last_edit.elapsed() >= throttle {
                    let ch = channel.lock().await;
                    let _ = ch
                        .send(OutboundAction::Edit {
                            channel_id,
                            chat_id: chat_id.clone(),
                            message_id: message_id.clone(),
                            new_content: accumulated.clone(),
                        })
                        .await;
                    last_edit = Instant::now();
                }
            }
            None => {
                // Sender dropped — streaming finished. One final edit to
                // flush any remaining text that was accumulated after the
                // last throttled edit.
                if !accumulated.is_empty() {
                    let ch = channel.lock().await;
                    let _ = ch
                        .send(OutboundAction::Edit {
                            channel_id,
                            chat_id: chat_id.clone(),
                            message_id: message_id.clone(),
                            new_content: accumulated,
                        })
                        .await;
                }
                break;
            }
        }
    }
}

/// Drain a chunk receiver without doing anything. Used when there is no
/// placeholder message to edit (so the sender never blocks).
async fn drain_chunks(mut rx: tokio::sync::mpsc::Receiver<String>) {
    while rx.recv().await.is_some() {}
}

#[cfg(test)]
mod media_enrichment_tests {
    use super::*;
    use chrono::Utc;
    use rune_channels::ChannelMessage;
    use rune_core::{AttachmentRef, ChannelId};

    fn image_message(content: &str, mime: &str) -> ChannelMessage {
        ChannelMessage {
            channel_id: ChannelId::new(),
            raw_chat_id: "chat-test".to_string(),
            sender: "user-test".to_string(),
            content: content.to_string(),
            attachments: vec![AttachmentRef {
                name: "image".to_string(),
                mime_type: Some(mime.to_string()),
                size_bytes: Some(16),
                url: Some("telegram-file:file-123".to_string()),
                provider_file_id: Some("file-123".to_string()),
            }],
            timestamp: Utc::now(),
            provider_message_id: "provider-msg".to_string(),
        }
    }

    #[test]
    fn image_attachment_is_appended_to_caption_text() {
        let msg = image_message("What is in this photo?", "image/jpeg");
        let mut extra_parts = Vec::new();

        for attachment in &msg.attachments {
            let url = match attachment.url.as_deref() {
                Some(u) if u.starts_with("telegram-file:") => u,
                _ => continue,
            };
            let _file_id = &url["telegram-file:".len()..];
            let mime = attachment.mime_type.as_deref().unwrap_or("");
            if mime.starts_with(IMAGE_MIME_PREFIX) {
                extra_parts.push("[Image attached]".to_string());
            }
        }

        let enriched = if extra_parts.is_empty() {
            msg.content.clone()
        } else {
            let enrichment = extra_parts.join("\n");
            if msg.content.is_empty() {
                enrichment
            } else {
                format!("{}\n{enrichment}", msg.content)
            }
        };

        assert_eq!(enriched, "What is in this photo?\n[Image attached]");
    }

    #[test]
    fn image_only_message_still_marks_image_presence() {
        let msg = image_message("", "image/png");
        let mut extra_parts = Vec::new();

        for attachment in &msg.attachments {
            let url = match attachment.url.as_deref() {
                Some(u) if u.starts_with("telegram-file:") => u,
                _ => continue,
            };
            let _file_id = &url["telegram-file:".len()..];
            let mime = attachment.mime_type.as_deref().unwrap_or("");
            if mime.starts_with(IMAGE_MIME_PREFIX) {
                extra_parts.push("[Image attached]".to_string());
            }
        }

        let enriched = if extra_parts.is_empty() {
            msg.content.clone()
        } else {
            extra_parts.join("\n")
        };

        assert_eq!(enriched, "[Image attached]");
    }
}
