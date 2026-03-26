//! Hook event system for plugin lifecycle integration.
//!
//! Plugins register handlers for specific hook events. When the executor
//! emits a hook, all registered handlers for that event are called in order,
//! receiving a mutable context that they can inspect or modify.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Hook events emitted during session and tool lifecycle.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HookEvent {
    /// Emitted before a tool call is executed.
    PreToolCall,
    /// Emitted after a tool call completes.
    PostToolCall,
    /// Emitted before the model is called each iteration.
    PreTurn,
    /// Emitted after a model response is processed.
    PostTurn,
    /// Emitted when a new session is created.
    SessionCreated,
    /// Emitted when a session completes.
    SessionCompleted,
    /// Emitted when the agent stops (Claude Code stop hook).
    Stop,
    /// Emitted when a subagent stops.
    SubagentStop,
    /// Emitted when a user prompt is submitted.
    UserPromptSubmit,
    /// Emitted before context compaction occurs.
    PreCompact,
    /// Emitted to deliver a notification.
    Notification,
}

impl HookEvent {
    /// Convert an event to its string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            HookEvent::PreToolCall => "pre_tool_call",
            HookEvent::PostToolCall => "post_tool_call",
            HookEvent::PreTurn => "pre_turn",
            HookEvent::PostTurn => "post_turn",
            HookEvent::SessionCreated => "session_created",
            HookEvent::SessionCompleted => "session_completed",
            HookEvent::Stop => "stop",
            HookEvent::SubagentStop => "subagent_stop",
            HookEvent::UserPromptSubmit => "user_prompt_submit",
            HookEvent::PreCompact => "pre_compact",
            HookEvent::Notification => "notification",
        }
    }

    /// Parse a string into a HookEvent.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pre_tool_call" => Some(HookEvent::PreToolCall),
            "post_tool_call" => Some(HookEvent::PostToolCall),
            "pre_turn" => Some(HookEvent::PreTurn),
            "post_turn" => Some(HookEvent::PostTurn),
            "session_created" => Some(HookEvent::SessionCreated),
            "session_completed" => Some(HookEvent::SessionCompleted),
            "stop" | "Stop" => Some(HookEvent::Stop),
            "subagent_stop" | "SubagentStop" => Some(HookEvent::SubagentStop),
            "user_prompt_submit" | "UserPromptSubmit" => Some(HookEvent::UserPromptSubmit),
            "pre_compact" | "PreCompact" => Some(HookEvent::PreCompact),
            "notification" | "Notification" => Some(HookEvent::Notification),
            _ => None,
        }
    }

    /// All known hook events.
    pub fn all() -> &'static [HookEvent] {
        &[
            HookEvent::PreToolCall,
            HookEvent::PostToolCall,
            HookEvent::PreTurn,
            HookEvent::PostTurn,
            HookEvent::SessionCreated,
            HookEvent::SessionCompleted,
            HookEvent::Stop,
            HookEvent::SubagentStop,
            HookEvent::UserPromptSubmit,
            HookEvent::PreCompact,
            HookEvent::Notification,
        ]
    }
}

// ---------------------------------------------------------------------------
// Handler trait
// ---------------------------------------------------------------------------

/// Trait for hook handlers that receive events with mutable context.
#[async_trait::async_trait]
pub trait HookHandler: Send + Sync {
    /// Handle a hook event. Handlers may modify the context.
    async fn handle(
        &self,
        event: &HookEvent,
        context: &mut serde_json::Value,
    ) -> Result<(), String>;

    /// Name of the plugin that owns this handler (for logging).
    fn plugin_name(&self) -> &str;

    /// Session kinds this handler applies to. None = all kinds.
    fn session_kinds_filter(&self) -> Option<&[String]> {
        None
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Handler map: event → ordered list of handlers.
type HandlerMap = HashMap<HookEvent, Vec<Box<dyn HookHandler>>>;

/// Maps hook events to registered handlers. Thread-safe.
#[derive(Clone)]
pub struct HookRegistry {
    handlers: Arc<RwLock<HandlerMap>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a handler for a specific event.
    pub async fn register(&self, event: HookEvent, handler: Box<dyn HookHandler>) {
        let plugin = handler.plugin_name().to_string();
        self.handlers
            .write()
            .await
            .entry(event.clone())
            .or_default()
            .push(handler);
        debug!(event = %event.as_str(), plugin = %plugin, "hook handler registered");
    }

    /// Emit a hook event, calling all registered handlers in order.
    ///
    /// Handlers receive a mutable reference to the context and may modify it
    /// (e.g., PreToolCall handlers can adjust tool arguments).
    ///
    /// Handler errors are logged but do not stop subsequent handlers from running.
    pub async fn emit(&self, event: &HookEvent, context: &mut serde_json::Value) {
        let handlers = self.handlers.read().await;
        let Some(event_handlers) = handlers.get(event) else {
            return;
        };

        let session_kind = context
            .get("session_kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();

        debug!(
            event = %event.as_str(),
            handler_count = event_handlers.len(),
            "emitting hook event"
        );

        for handler in event_handlers {
            if let Some(allowed) = handler.session_kinds_filter() {
                if !session_kind.is_empty()
                    && !allowed
                        .iter()
                        .any(|k| k.eq_ignore_ascii_case(&session_kind))
                {
                    debug!(
                        event = %event.as_str(),
                        plugin = %handler.plugin_name(),
                        session_kind = %session_kind,
                        "skipping hook handler (session kind filtered)"
                    );
                    continue;
                }
            }

            if let Err(e) = handler.handle(event, context).await {
                warn!(
                    event = %event.as_str(),
                    plugin = %handler.plugin_name(),
                    error = %e,
                    "hook handler failed, continuing"
                );
            }
        }
    }

    /// Number of handlers registered for a specific event.
    pub async fn handler_count(&self, event: &HookEvent) -> usize {
        self.handlers.read().await.get(event).map_or(0, |v| v.len())
    }

    /// Total number of handlers across all events.
    pub async fn total_handlers(&self) -> usize {
        self.handlers.read().await.values().map(|v| v.len()).sum()
    }

    /// Clear all handlers (used during plugin reload).
    pub async fn clear(&self) {
        self.handlers.write().await.clear();
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TestHandler {
        name: String,
        call_count: Arc<AtomicU32>,
    }

    #[async_trait::async_trait]
    impl HookHandler for TestHandler {
        async fn handle(
            &self,
            _event: &HookEvent,
            context: &mut serde_json::Value,
        ) -> Result<(), String> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            // Modify context to prove handlers can mutate it
            if let Some(obj) = context.as_object_mut() {
                obj.insert(
                    format!("handled_by_{}", self.name),
                    serde_json::Value::Bool(true),
                );
            }
            Ok(())
        }

        fn plugin_name(&self) -> &str {
            &self.name
        }
    }

    struct FailingHandler;

    #[async_trait::async_trait]
    impl HookHandler for FailingHandler {
        async fn handle(
            &self,
            _event: &HookEvent,
            _context: &mut serde_json::Value,
        ) -> Result<(), String> {
            Err("intentional failure".to_string())
        }

        fn plugin_name(&self) -> &str {
            "failing-plugin"
        }
    }

    #[test]
    fn hook_event_roundtrip() {
        for event in HookEvent::all() {
            let s = event.as_str();
            let parsed = HookEvent::from_str(s).unwrap();
            assert_eq!(*event, parsed);
        }
    }

    #[test]
    fn hook_event_unknown_returns_none() {
        assert!(HookEvent::from_str("unknown_event").is_none());
        assert!(HookEvent::from_str("").is_none());
    }

    #[tokio::test]
    async fn emit_calls_registered_handlers() {
        let registry = HookRegistry::new();
        let count = Arc::new(AtomicU32::new(0));

        registry
            .register(
                HookEvent::PreToolCall,
                Box::new(TestHandler {
                    name: "plugin-a".into(),
                    call_count: count.clone(),
                }),
            )
            .await;

        registry
            .register(
                HookEvent::PreToolCall,
                Box::new(TestHandler {
                    name: "plugin-b".into(),
                    call_count: count.clone(),
                }),
            )
            .await;

        let mut ctx = serde_json::json!({"tool_name": "bash"});
        registry.emit(&HookEvent::PreToolCall, &mut ctx).await;

        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(ctx["handled_by_plugin-a"], true);
        assert_eq!(ctx["handled_by_plugin-b"], true);
    }

    #[tokio::test]
    async fn emit_no_handlers_is_noop() {
        let registry = HookRegistry::new();
        let mut ctx = serde_json::json!({"key": "value"});
        registry.emit(&HookEvent::PostToolCall, &mut ctx).await;
        assert_eq!(ctx, serde_json::json!({"key": "value"}));
    }

    #[tokio::test]
    async fn emit_continues_after_handler_failure() {
        let registry = HookRegistry::new();
        let count = Arc::new(AtomicU32::new(0));

        // Register a failing handler first
        registry
            .register(HookEvent::PreToolCall, Box::new(FailingHandler))
            .await;

        // Register a succeeding handler second
        registry
            .register(
                HookEvent::PreToolCall,
                Box::new(TestHandler {
                    name: "survivor".into(),
                    call_count: count.clone(),
                }),
            )
            .await;

        let mut ctx = serde_json::json!({});
        registry.emit(&HookEvent::PreToolCall, &mut ctx).await;

        // The surviving handler should still have been called
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(ctx["handled_by_survivor"], true);
    }

    #[tokio::test]
    async fn handler_count_tracking() {
        let registry = HookRegistry::new();
        assert_eq!(registry.handler_count(&HookEvent::PreToolCall).await, 0);
        assert_eq!(registry.total_handlers().await, 0);

        let count = Arc::new(AtomicU32::new(0));
        registry
            .register(
                HookEvent::PreToolCall,
                Box::new(TestHandler {
                    name: "a".into(),
                    call_count: count.clone(),
                }),
            )
            .await;
        registry
            .register(
                HookEvent::PostToolCall,
                Box::new(TestHandler {
                    name: "b".into(),
                    call_count: count,
                }),
            )
            .await;

        assert_eq!(registry.handler_count(&HookEvent::PreToolCall).await, 1);
        assert_eq!(registry.handler_count(&HookEvent::PostToolCall).await, 1);
        assert_eq!(registry.total_handlers().await, 2);
    }

    #[tokio::test]
    async fn clear_removes_all_handlers() {
        let registry = HookRegistry::new();
        let count = Arc::new(AtomicU32::new(0));

        registry
            .register(
                HookEvent::PreToolCall,
                Box::new(TestHandler {
                    name: "a".into(),
                    call_count: count,
                }),
            )
            .await;

        assert_eq!(registry.total_handlers().await, 1);
        registry.clear().await;
        assert_eq!(registry.total_handlers().await, 0);
    }

    #[tokio::test]
    async fn handlers_can_modify_context() {
        let registry = HookRegistry::new();
        let count = Arc::new(AtomicU32::new(0));

        registry
            .register(
                HookEvent::PreToolCall,
                Box::new(TestHandler {
                    name: "modifier".into(),
                    call_count: count,
                }),
            )
            .await;

        let mut ctx = serde_json::json!({
            "tool_name": "bash",
            "arguments": {"command": "ls"}
        });

        registry.emit(&HookEvent::PreToolCall, &mut ctx).await;

        // Handler should have added its marker
        assert_eq!(ctx["handled_by_modifier"], true);
        // Original data should be preserved
        assert_eq!(ctx["tool_name"], "bash");
    }
}
