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
use rune_stt::SttEngine;
use rune_store::models::SessionRow;
use rune_store::repos::SessionRepo;

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

/// The main session loop that ties channels to the runtime.
pub struct SessionLoop {
    engine: Arc<SessionEngine>,
    turn_executor: Arc<TurnExecutor>,
    session_repo: Arc<dyn SessionRepo>,
    channel: Arc<Mutex<Box<dyn ChannelAdapter>>>,
    sessions: Mutex<SessionIndex>,
    agents: AgentsConfig,
    models: ModelsConfig,
    stt_engine: Option<Arc<RwLock<SttEngine>>>,
    file_downloader: Option<Arc<dyn TelegramFileDownloader>>,
}

/// MIME types considered audio for transcription purposes.
const AUDIO_MIME_TYPES: &[&str] = &["audio/ogg", "audio/mpeg", "audio/mp4"];

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
            agents,
            models,
            stt_engine: None,
            file_downloader: None,
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

                // Enrich content by downloading/transcribing media attachments.
                let enriched_content = self.enrich_media_content(&msg).await;

                info!(session_id = %session.id, len = enriched_content.len(), "executing turn");

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

                match self
                    .turn_executor
                    .execute(session.id, &enriched_content, None)
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
    /// message content with transcriptions (audio) or placeholders (images).
    async fn enrich_media_content(&self, msg: &rune_channels::ChannelMessage) -> String {
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
                info!(file_id = %file_id, mime = %mime, "image attachment detected");
                extra_parts.push("[Image attached]".to_string());
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
