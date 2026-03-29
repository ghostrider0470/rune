use rune_store::models::SessionRow;
use serde_json::{Value, json};

use crate::context::ContextAssemblyReport;

pub(crate) const SELECTED_MODEL_KEY: &str = "selected_model";
pub(crate) const SESSION_MODE_KEY: &str = "mode";
pub(crate) const CONTEXT_TIERS_KEY: &str = "context_tiers";
pub(crate) const CONTEXT_TOKEN_USAGE_KEY: &str = "context_token_usage";
pub(crate) const THINKING_LEVEL_KEY: &str = "thinking_level";
pub(crate) const REASONING_KEY: &str = "reasoning";
pub(crate) const VERBOSE_KEY: &str = "verbose";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionThinkingLevel {
    Off,
    Low,
    Medium,
    High,
}

impl SessionThinkingLevel {
    pub(crate) fn from_metadata_value(value: Option<&str>) -> Self {
        match value.unwrap_or("off").trim().to_ascii_lowercase().as_str() {
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            _ => Self::Off,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub(crate) fn prompt_directive(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Low => Some("Keep reasoning light. Use short internal deliberation, avoid over-explaining, and move quickly to the answer or action."),
            Self::Medium => Some("Use moderate reasoning depth. Verify key assumptions before acting, but stay concise and execution-focused."),
            Self::High => Some("Use high reasoning depth for this session. Think carefully through edge cases, validate assumptions before acting, and prefer robust solutions over fast guesses."),
        }
    }
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


pub(crate) fn metadata_string<'a>(metadata: &'a Value, key: &str) -> Option<&'a str> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn session_thinking_level(metadata: &Value) -> SessionThinkingLevel {
    let thinking = SessionThinkingLevel::from_metadata_value(metadata_string(metadata, THINKING_LEVEL_KEY));
    if thinking != SessionThinkingLevel::Off {
        return thinking;
    }
    SessionThinkingLevel::from_metadata_value(metadata_string(metadata, REASONING_KEY))
}

pub(crate) fn session_verbose(metadata: &Value) -> bool {
    metadata
        .get(VERBOSE_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn session_mode_prompt_section(metadata: &Value) -> Option<String> {
    let thinking = session_thinking_level(metadata);
    let verbose = session_verbose(metadata);
    if thinking == SessionThinkingLevel::Off && !verbose {
        return None;
    }

    let mut lines = vec![String::from("## Session Operating Mode")];
    if let Some(directive) = thinking.prompt_directive() {
        lines.push(format!("- Thinking level: {}", thinking.label()));
        lines.push(format!("- {}", directive));
    }
    if verbose {
        lines.push(String::from("- Verbose mode is enabled. When useful, surface concise progress notes, tradeoffs, and validation evidence instead of only the final conclusion."));
    }

    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_mode_prompt_uses_thinking_level_and_verbose_flags() {
        let section = session_mode_prompt_section(&json!({
            "thinking_level": "high",
            "verbose": true,
        }))
        .expect("section");

        assert!(section.contains("## Session Operating Mode"));
        assert!(section.contains("Thinking level: high"));
        assert!(section.contains("Verbose mode is enabled"));
    }

    #[test]
    fn session_mode_prompt_falls_back_to_reasoning_alias() {
        let section = session_mode_prompt_section(&json!({
            "reasoning": "medium",
        }))
        .expect("section");

        assert!(section.contains("Thinking level: medium"));
    }

    #[test]
    fn session_mode_prompt_absent_when_disabled() {
        assert!(session_mode_prompt_section(&json!({})).is_none());
    }
}
