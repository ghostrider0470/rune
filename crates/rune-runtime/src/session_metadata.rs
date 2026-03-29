use rune_store::models::SessionRow;
use serde_json::{Value, json};

use crate::context::ContextAssemblyReport;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryHierarchySnapshot {
    pub l0_loaded: bool,
    pub l1_prompt_cache_enabled: bool,
    pub l2_vector_memory_enabled: bool,
    pub l3_cold_storage_enabled: bool,
}

pub(crate) const SELECTED_MODEL_KEY: &str = "selected_model";
pub(crate) const SESSION_MODE_KEY: &str = "mode";
pub(crate) const CONTEXT_TIERS_KEY: &str = "context_tiers";
pub(crate) const CONTEXT_TOKEN_USAGE_KEY: &str = "context_token_usage";
pub(crate) const MEMORY_HIERARCHY_KEY: &str = "memory_hierarchy";

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

pub(crate) fn set_memory_hierarchy(
    metadata: &Value,
    snapshot: &MemoryHierarchySnapshot,
) -> Value {
    let mut next = metadata.as_object().cloned().unwrap_or_default();
    next.insert(
        MEMORY_HIERARCHY_KEY.to_string(),
        json!({
            "l0": {
                "loaded": snapshot.l0_loaded,
                "description": "active in-context prompt window"
            },
            "l1": {
                "loaded": snapshot.l1_prompt_cache_enabled,
                "description": "stable prompt-cache prefixes enabled"
            },
            "l2": {
                "loaded": snapshot.l2_vector_memory_enabled,
                "description": "warm vector memory available"
            },
            "l3": {
                "loaded": snapshot.l3_cold_storage_enabled,
                "description": "cold transcript storage / compaction handoff available"
            }
        }),
    );
    Value::Object(next)
}
