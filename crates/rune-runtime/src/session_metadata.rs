use rune_store::models::SessionRow;
use serde_json::{Value, json};

use crate::context::ContextAssemblyReport;

pub(crate) const SELECTED_MODEL_KEY: &str = "selected_model";
pub(crate) const SESSION_MODE_KEY: &str = "mode";
pub(crate) const PROJECT_ID_KEY: &str = "project_id";
pub(crate) const CONTEXT_TIERS_KEY: &str = "context_tiers";
pub(crate) const CONTEXT_TOKEN_USAGE_KEY: &str = "context_token_usage";
pub(crate) const CHANNEL_SOURCE_PRIORITY_KEY: &str = "channel_source_priority";
pub(crate) const ANTI_THRASH_KEY: &str = "anti_thrash";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryBudgetState {
    pub failure_fingerprint: String,
    pub retry_count: u32,
    pub budget_exhausted: bool,
    pub suppression_reason: Option<String>,
    pub stall_reason: Option<String>,
    pub operator_note: Option<String>,
    pub next_retry_at: Option<String>,
    pub last_error: Option<String>,
}

pub(crate) fn selected_model(session: &SessionRow) -> Option<&str> {
    session
        .metadata
        .get(SELECTED_MODEL_KEY)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

pub(crate) fn set_selected_model(metadata: &Value, model: &str) -> Value {
    let mut next = metadata.as_object().cloned().unwrap_or_default();
    next.insert(SELECTED_MODEL_KEY.to_string(), json!(model));
    Value::Object(next)
}

pub(crate) fn set_session_mode(metadata: &Value, mode: &str) -> Value {
    let mut next = metadata.as_object().cloned().unwrap_or_default();
    next.insert(SESSION_MODE_KEY.to_string(), json!(mode));
    Value::Object(next)
}

pub(crate) fn set_project_id(metadata: &Value, project_id: &str) -> Value {
    let mut next = metadata.as_object().cloned().unwrap_or_default();
    next.insert(PROJECT_ID_KEY.to_string(), json!(project_id));
    Value::Object(next)
}

pub(crate) fn set_context_tiers(metadata: &Value, report: &ContextAssemblyReport) -> Value {
    let mut next = metadata.as_object().cloned().unwrap_or_default();
    next.insert(
        CONTEXT_TIERS_KEY.to_string(),
        serde_json::to_value(report.snapshots()).unwrap_or_else(|_| Value::Array(Vec::new())),
    );
    next.insert(
        CONTEXT_TOKEN_USAGE_KEY.to_string(),
        json!({
            "total_estimated_tokens": report.total_estimated_tokens,
            "total_budget": report.total_budget,
            "compaction_trigger_tokens": report.compaction_trigger_tokens,
            "over_budget": report.over_budget,
            "over_compaction_threshold": report.over_compaction_threshold,
            "compaction_required": report.compaction_required,
        }),
    );
    Value::Object(next)
}

pub(crate) fn set_channel_source_priority(metadata: &Value, source: &str, priority: u8) -> Value {
    let mut next = metadata.as_object().cloned().unwrap_or_default();
    let mut map = next
        .remove(CHANNEL_SOURCE_PRIORITY_KEY)
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    map.insert(source.to_string(), json!(priority));
    next.insert(CHANNEL_SOURCE_PRIORITY_KEY.to_string(), Value::Object(map));
    Value::Object(next)
}

pub(crate) fn anti_thrash_state(session: &SessionRow) -> Option<RetryBudgetState> {
    let anti = session.metadata.get(ANTI_THRASH_KEY)?;
    Some(RetryBudgetState {
        failure_fingerprint: anti.get("failure_fingerprint")?.as_str()?.to_string(),
        retry_count: anti
            .get("retry_count")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        budget_exhausted: anti
            .get("budget_exhausted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        suppression_reason: anti
            .get("suppression_reason")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        stall_reason: anti
            .get("stall_reason")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        operator_note: anti
            .get("operator_note")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        next_retry_at: anti
            .get("next_retry_at")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        last_error: anti
            .get("last_error")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    })
}

pub(crate) fn set_anti_thrash_state(metadata: &Value, state: &RetryBudgetState) -> Value {
    let mut next = metadata.as_object().cloned().unwrap_or_default();
    next.insert(
        ANTI_THRASH_KEY.to_string(),
        json!({
            "failure_fingerprint": state.failure_fingerprint,
            "retry_count": state.retry_count,
            "budget_exhausted": state.budget_exhausted,
            "suppression_reason": state.suppression_reason,
            "stall_reason": state.stall_reason,
            "operator_note": state.operator_note,
            "next_retry_at": state.next_retry_at,
            "last_error": state.last_error,
        }),
    );
    Value::Object(next)
}

pub(crate) fn clear_anti_thrash_state(metadata: &Value) -> Value {
    let mut next = metadata.as_object().cloned().unwrap_or_default();
    next.remove(ANTI_THRASH_KEY);
    Value::Object(next)
}
