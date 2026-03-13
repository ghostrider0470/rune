//! Approval system for tool execution.
//!
//! Implements multi-tier approval policies:
//! - `AlwaysAllow` — no approval needed (elevated mode)
//! - `PolicyBased` — approval required based on tool category and action
//!
//! The approval payload is intentionally structured so the exact operator-visible
//! payload can be persisted and later resolved without widening scope.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::definition::ToolCall;
use crate::error::ToolError;
use crate::executor::ApprovalCheck;

/// Approval decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    /// Approved for this single exact invocation.
    AllowOnce,
    /// Approved for all future invocations within the same tool scope.
    AllowAlways,
    /// Denied.
    Deny,
}

/// Scope used when recording approval state.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    /// Exact tool name + exact canonicalized argument payload.
    ExactCall,
    /// Tool-wide approval, used for allow-always semantics.
    Tool,
}

/// Structured approval request sent to the operator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub tool_name: String,
    pub risk_level: RiskLevel,
    pub scope: ApprovalScope,
    /// Exact canonicalized payload presented to the operator.
    pub presented_payload: serde_json::Value,
    /// Extracted shell command or similarly operator-relevant summary when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

impl ApprovalRequest {
    /// Build a structured approval request from a tool call.
    #[must_use]
    pub fn from_call(call: &ToolCall) -> Self {
        Self {
            tool_name: call.tool_name.clone(),
            risk_level: classify_risk(&call.tool_name, call),
            scope: ApprovalScope::ExactCall,
            presented_payload: canonicalize_json(&call.arguments),
            command: extract_command(call),
        }
    }

    /// Stable key used for exact-call approval binding.
    #[must_use]
    pub fn exact_call_key(&self) -> String {
        format!(
            "{}:{}",
            self.tool_name,
            serde_json::to_string(&self.presented_payload).unwrap_or_default()
        )
    }
}

/// Risk level for a tool invocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// No risk — read-only operations.
    Low,
    /// Moderate risk — file writes, process execution.
    Medium,
    /// High risk — destructive operations, external communication.
    High,
}

/// Classify the risk level of a tool invocation.
pub fn classify_risk(tool_name: &str, call: &ToolCall) -> RiskLevel {
    match tool_name {
        "read" | "memory_search" | "memory_get" | "sessions_list" | "sessions_history"
        | "session_status" | "cron" => RiskLevel::Low,

        "write" | "edit" | "exec" | "process" => {
            if call
                .arguments
                .get("elevated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                RiskLevel::High
            } else {
                RiskLevel::Medium
            }
        }

        "sessions_spawn" | "sessions_send" | "subagents" | "message" => RiskLevel::Medium,

        "gateway" => {
            let action = call
                .arguments
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match action {
                "status" => RiskLevel::Low,
                _ => RiskLevel::High,
            }
        }

        _ => RiskLevel::Medium,
    }
}

/// Policy-based approval that auto-allows low-risk calls and tracks explicit approvals.
pub struct PolicyBasedApproval {
    /// Tools that are always allowed (e.g. trusted bootstrap allowlist).
    always_allow: HashSet<String>,
    /// Tool names globally approved via allow-always semantics.
    approved_tools: Arc<Mutex<HashSet<String>>>,
    /// Exact-call approvals keyed by canonicalized payload.
    approved_once: Arc<Mutex<HashMap<String, u32>>>,
}

impl PolicyBasedApproval {
    /// Create with a set of always-allowed tool names.
    pub fn new(always_allow: HashSet<String>) -> Self {
        Self {
            always_allow,
            approved_tools: Arc::new(Mutex::new(HashSet::new())),
            approved_once: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create with all tools allowed (elevated mode).
    pub fn elevated() -> Self {
        Self::new(HashSet::new())
    }

    /// Record that a tool has been approved for all future calls.
    pub async fn approve_always(&self, tool_name: &str) {
        self.approved_tools
            .lock()
            .await
            .insert(tool_name.to_string());
    }

    /// Record a single exact-call approval.
    pub async fn approve_once(&self, request: &ApprovalRequest) {
        let mut approved_once = self.approved_once.lock().await;
        *approved_once.entry(request.exact_call_key()).or_insert(0) += 1;
    }

    /// Check if a tool has been globally approved.
    pub async fn is_tool_approved(&self, tool_name: &str) -> bool {
        self.always_allow.contains(tool_name)
            || self.approved_tools.lock().await.contains(tool_name)
    }

    async fn consume_once_if_present(&self, request: &ApprovalRequest) -> bool {
        let mut approved_once = self.approved_once.lock().await;
        if let Some(count) = approved_once.get_mut(&request.exact_call_key()) {
            if *count > 0 {
                *count -= 1;
                if *count == 0 {
                    approved_once.remove(&request.exact_call_key());
                }
                return true;
            }
        }
        false
    }
}

#[async_trait]
impl ApprovalCheck for PolicyBasedApproval {
    async fn check(&self, call: &ToolCall, _requires_approval: bool) -> Result<(), ToolError> {
        let request = ApprovalRequest::from_call(call);

        if request.risk_level == RiskLevel::Low {
            return Ok(());
        }

        if self.is_tool_approved(&call.tool_name).await {
            return Ok(());
        }

        if self.consume_once_if_present(&request).await {
            return Ok(());
        }

        Err(ToolError::ApprovalRequired {
            tool: call.tool_name.clone(),
            details: serde_json::to_string(&request)
                .unwrap_or_else(|_| call.arguments.to_string()),
        })
    }
}

fn extract_command(call: &ToolCall) -> Option<String> {
    call.arguments
        .get("command")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

fn canonicalize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut items: Vec<_> = map.iter().collect();
            items.sort_by(|a, b| a.0.cmp(b.0));
            let canonical = items
                .into_iter()
                .map(|(key, value)| (key.clone(), canonicalize_json(value)))
                .collect();
            serde_json::Value::Object(canonical)
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonicalize_json).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: name.into(),
            arguments: args,
        }
    }

    #[test]
    fn risk_classification() {
        let read_call = make_call("read", serde_json::json!({}));
        assert_eq!(classify_risk("read", &read_call), RiskLevel::Low);

        let exec_call = make_call("exec", serde_json::json!({"command": "ls"}));
        assert_eq!(classify_risk("exec", &exec_call), RiskLevel::Medium);

        let elevated_call = make_call(
            "exec",
            serde_json::json!({"command": "rm -rf /", "elevated": true}),
        );
        assert_eq!(classify_risk("exec", &elevated_call), RiskLevel::High);

        let gw_status = make_call("gateway", serde_json::json!({"action": "status"}));
        assert_eq!(classify_risk("gateway", &gw_status), RiskLevel::Low);

        let gw_restart = make_call("gateway", serde_json::json!({"action": "restart"}));
        assert_eq!(classify_risk("gateway", &gw_restart), RiskLevel::High);
    }

    #[test]
    fn request_from_call_extracts_exact_payload_and_command() {
        let call = make_call(
            "exec",
            serde_json::json!({"timeout": 30, "command": "ls -la", "workdir": "/tmp"}),
        );
        let request = ApprovalRequest::from_call(&call);
        assert_eq!(request.tool_name, "exec");
        assert_eq!(request.risk_level, RiskLevel::Medium);
        assert_eq!(request.scope, ApprovalScope::ExactCall);
        assert_eq!(request.command.as_deref(), Some("ls -la"));
        assert_eq!(request.presented_payload["workdir"], "/tmp");
    }

    #[tokio::test]
    async fn low_risk_always_allowed() {
        let policy = PolicyBasedApproval::new(HashSet::new());
        let call = make_call("read", serde_json::json!({}));
        assert!(policy.check(&call, false).await.is_ok());
    }

    #[tokio::test]
    async fn medium_risk_requires_approval() {
        let policy = PolicyBasedApproval::new(HashSet::new());
        let call = make_call("exec", serde_json::json!({"command": "ls"}));
        assert!(policy.check(&call, false).await.is_err());
    }

    #[tokio::test]
    async fn allowlist_overrides() {
        let mut allow = HashSet::new();
        allow.insert("exec".to_string());
        let policy = PolicyBasedApproval::new(allow);

        let call = make_call("exec", serde_json::json!({"command": "ls"}));
        assert!(policy.check(&call, false).await.is_ok());
    }

    #[tokio::test]
    async fn approve_always_persists() {
        let policy = PolicyBasedApproval::new(HashSet::new());
        let call = make_call("write", serde_json::json!({"path": "/tmp/x"}));

        assert!(policy.check(&call, false).await.is_err());
        policy.approve_always("write").await;
        assert!(policy.check(&call, false).await.is_ok());
    }

    #[tokio::test]
    async fn approve_once_binds_to_exact_payload_only_once() {
        let policy = PolicyBasedApproval::new(HashSet::new());
        let call = make_call("exec", serde_json::json!({"command": "ls", "workdir": "/tmp"}));
        let request = ApprovalRequest::from_call(&call);

        policy.approve_once(&request).await;
        assert!(policy.check(&call, false).await.is_ok());
        assert!(policy.check(&call, false).await.is_err());
    }

    #[tokio::test]
    async fn approve_once_does_not_expand_to_different_payload() {
        let policy = PolicyBasedApproval::new(HashSet::new());
        let approved_call = make_call("exec", serde_json::json!({"command": "ls", "workdir": "/tmp"}));
        let different_call = make_call("exec", serde_json::json!({"command": "pwd", "workdir": "/tmp"}));

        let request = ApprovalRequest::from_call(&approved_call);
        policy.approve_once(&request).await;

        assert!(policy.check(&approved_call, false).await.is_ok());
        assert!(policy.check(&different_call, false).await.is_err());
    }
}
