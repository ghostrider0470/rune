use rune_store::models::SessionRow;
use serde_json::{Value, json};

pub(crate) const SELECTED_MODEL_KEY: &str = "selected_model";
pub(crate) const SESSION_MODE_KEY: &str = "mode";

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
