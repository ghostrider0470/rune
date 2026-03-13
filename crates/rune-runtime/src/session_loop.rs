//! The session loop: bridges inbound channel events to the turn executor.
//!
//! This is the core pipeline: channel message → find/create session → execute turn → reply.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{debug, error, info};
use uuid::Uuid;

use rune_channels::{ChannelAdapter, InboundEvent, OutboundAction};
use rune_core::SessionKind;
use rune_store::models::SessionRow;
use rune_store::repos::SessionRepo;

use crate::engine::SessionEngine;
use crate::error::RuntimeError;
use crate::executor::TurnExecutor;

/// Maps channel routing key → session_id.
type SessionIndex = HashMap<String, Uuid>;

/// The main session loop that ties channels to the runtime.
pub struct SessionLoop {
    engine: Arc<SessionEngine>,
    turn_executor: Arc<TurnExecutor>,
    session_repo: Arc<dyn SessionRepo>,
    channel: Arc<Mutex<Box<dyn ChannelAdapter>>>,
    sessions: Mutex<SessionIndex>,
}

impl SessionLoop {
    pub fn new(
        engine: Arc<SessionEngine>,
        turn_executor: Arc<TurnExecutor>,
        session_repo: Arc<dyn SessionRepo>,
        channel: Box<dyn ChannelAdapter>,
    ) -> Self {
        Self {
            engine,
            turn_executor,
            session_repo,
            channel: Arc::new(Mutex::new(channel)),
            sessions: Mutex::new(HashMap::new()),
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
                let routing_key = format!("{}:{}", msg.channel_id, msg.sender);
                debug!(routing_key = %routing_key, "received inbound message");

                let session = self
                    .find_or_create_session(&routing_key, &msg.channel_id.to_string())
                    .await?;

                info!(session_id = %session.id, len = msg.content.len(), "executing turn");

                match self
                    .turn_executor
                    .execute(session.id, &msg.content, None)
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
                                    reply_to: msg.provider_message_id,
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
                                reply_to: msg.provider_message_id,
                                content: format!("⚠️ Turn failed: {e}"),
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

    async fn find_or_create_session(
        &self,
        routing_key: &str,
        channel_ref: &str,
    ) -> Result<SessionRow, RuntimeError> {
        {
            let index = self.sessions.lock().await;
            if let Some(session_id) = index.get(routing_key) {
                if let Ok(session) = self.session_repo.find_by_id(*session_id).await {
                    return Ok(session);
                }
            }
        }

        let session = self
            .engine
            .create_session_full(
                SessionKind::Direct,
                None,
                None,
                Some(channel_ref.to_string()),
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
