use rune_store::models::SessionRow;
use serde_json::{Value, json};

use crate::context::ContextAssemblyReport;

pub(crate) const SELECTED_MODEL_KEY: &str = "selected_model";
pub(crate) const SESSION_MODE_KEY: &str = "mode";
pub(crate) const CONTEXT_TIERS_KEY: &str = "context_tiers";
pub(crate) const CONTEXT_TOKEN_USAGE_KEY: &str = "context_token_usage";

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
