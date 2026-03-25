use rune_core::TurnStatus;

use crate::error::StoreError;

pub fn validate_turn_transition(current: &str, target: &str) -> Result<(), StoreError> {
    let current = parse_status(current)?;
    let target = parse_status(target)?;

    if can_transition(current, target) {
        Ok(())
    } else {
        Err(StoreError::InvalidTransition(format!(
            "invalid transition: {} -> {}",
            status_str(current),
            status_str(target)
        )))
    }
}

fn parse_status(raw: &str) -> Result<TurnStatus, StoreError> {
    match raw {
        "started" => Ok(TurnStatus::Started),
        "model_calling" => Ok(TurnStatus::ModelCalling),
        "tool_executing" => Ok(TurnStatus::ToolExecuting),
        "completed" => Ok(TurnStatus::Completed),
        "failed" => Ok(TurnStatus::Failed),
        "cancelled" => Ok(TurnStatus::Cancelled),
        other => Err(StoreError::InvalidTransition(format!(
            "unknown turn status: {other}"
        ))),
    }
}

fn can_transition(current: TurnStatus, target: TurnStatus) -> bool {
    matches!(
        (current, target),
        (TurnStatus::Started, TurnStatus::ModelCalling)
            | (TurnStatus::Started, TurnStatus::Failed)
            | (TurnStatus::Started, TurnStatus::Cancelled)
            | (TurnStatus::ModelCalling, TurnStatus::ToolExecuting)
            | (TurnStatus::ModelCalling, TurnStatus::Completed)
            | (TurnStatus::ModelCalling, TurnStatus::Failed)
            | (TurnStatus::ModelCalling, TurnStatus::Cancelled)
            | (TurnStatus::ToolExecuting, TurnStatus::ModelCalling)
            | (TurnStatus::ToolExecuting, TurnStatus::ToolExecuting)
            | (TurnStatus::ToolExecuting, TurnStatus::Completed)
            | (TurnStatus::ToolExecuting, TurnStatus::Failed)
            | (TurnStatus::ToolExecuting, TurnStatus::Cancelled)
    )
}

fn status_str(status: TurnStatus) -> &'static str {
    match status {
        TurnStatus::Started => "started",
        TurnStatus::ModelCalling => "model_calling",
        TurnStatus::ToolExecuting => "tool_executing",
        TurnStatus::Completed => "completed",
        TurnStatus::Failed => "failed",
        TurnStatus::Cancelled => "cancelled",
    }
}
