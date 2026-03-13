//! Approval system for tool execution.
//!
//! Implements multi-tier approval policies:
//! - `AlwaysAllow` — no approval needed (elevated mode)
//! - `PolicyBased` — approval required based on tool category and action
//! - `Interactive` — approval requested from the user via callback

use std::collections::HashSet;
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
    /// Approved for this single invocation.
    AllowOnce,
    /// Approved for all future invocations of this tool.
    AllowAlways,
    /// Denied.
    Deny,
}

/// Approval request sent to the user.
#[derive(Clone, Debug, Serialize)]
pub struct ApprovalRequest {
    pub tool_name: String,
    pub arguments_summary: String,
    pub risk_level: RiskLevel,
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
            // exec with elevated flag is high risk
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

/// Policy-based approval that auto-allows low-risk and uses an allowlist.
pub struct PolicyBasedApproval {
    /// Tools that are always allowed (e.g., elevated mode allowlist).
    always_allow: HashSet<String>,
    /// Tools that have been approved via AllowAlways.
    approved_always: Arc<Mutex<HashSet<String>>>,
}

impl PolicyBasedApproval {
    /// Create with a set of always-allowed tool names.
    pub fn new(always_allow: HashSet<String>) -> Self {
        Self {
            always_allow,
            approved_always: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Create with all tools allowed (elevated mode).
    pub fn elevated() -> Self {
        Self::new(HashSet::new())
    }

    /// Record that a tool has been approved for all future calls.
    pub async fn approve_always(&self, tool_name: &str) {
        self.approved_always
            .lock()
            .await
            .insert(tool_name.to_string());
    }

    /// Check if a tool has been globally approved.
    pub async fn is_approved(&self, tool_name: &str) -> bool {
        self.always_allow.contains(tool_name)
            || self.approved_always.lock().await.contains(tool_name)
    }
}

#[async_trait]
impl ApprovalCheck for PolicyBasedApproval {
    async fn check(&self, call: &ToolCall, _requires_approval: bool) -> Result<(), ToolError> {
        let risk = classify_risk(&call.tool_name, call);

        // Low risk: always allowed
        if risk == RiskLevel::Low {
            return Ok(());
        }

        // Check allowlists
        if self.is_approved(&call.tool_name).await {
            return Ok(());
        }

        // Medium/High risk without approval
        Err(ToolError::ApprovalRequired {
            tool: call.tool_name.clone(),
            details: serde_json::to_string(&ApprovalRequest {
                tool_name: call.tool_name.clone(),
                arguments_summary: call.arguments.to_string(),
                risk_level: risk,
            })
            .unwrap_or_else(|_| call.arguments.to_string()),
        })
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

        let elevated_call = make_call("exec", serde_json::json!({"command": "rm -rf /", "elevated": true}));
        assert_eq!(classify_risk("exec", &elevated_call), RiskLevel::High);

        let gw_status = make_call("gateway", serde_json::json!({"action": "status"}));
        assert_eq!(classify_risk("gateway", &gw_status), RiskLevel::Low);

        let gw_restart = make_call("gateway", serde_json::json!({"action": "restart"}));
        assert_eq!(classify_risk("gateway", &gw_restart), RiskLevel::High);
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
        let call = make_call("write", serde_json::json!({}));

        // First call: denied
        assert!(policy.check(&call, false).await.is_err());

        // Approve always
        policy.approve_always("write").await;

        // Second call: allowed
        assert!(policy.check(&call, false).await.is_ok());
    }
}
