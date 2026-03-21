//! Cron/scheduler tool implementation.
//!
//! Uses a trait-based abstraction to avoid circular dependencies
//! with rune-runtime. The runtime wires the real Scheduler behind
//! the SchedulerOps trait.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Trait for scheduler operations, implemented by the runtime layer.
#[async_trait]
pub trait SchedulerOps: Send + Sync {
    /// List jobs as JSON.
    async fn list_jobs(&self, include_disabled: bool) -> Result<String, String>;
    /// Add a job from JSON definition. Returns JSON with jobId.
    async fn add_job(&self, job_json: serde_json::Value) -> Result<String, String>;
    /// Remove a job by ID. Returns JSON confirmation.
    async fn remove_job(&self, job_id: &str) -> Result<String, String>;
    /// Update a job by ID with a JSON patch.
    async fn update_job(&self, job_id: &str, patch: serde_json::Value) -> Result<String, String>;
    /// Trigger immediate run of a job.
    async fn run_job(&self, job_id: &str) -> Result<String, String>;
    /// Get run history for a job.
    async fn get_runs(&self, job_id: &str) -> Result<String, String>;
    /// Get scheduler status summary.
    async fn status(&self) -> Result<String, String>;
    /// Inject a wake event into the runtime scheduler/session layer.
    async fn wake(
        &self,
        text: &str,
        mode: Option<&str>,
        context_messages: Option<u64>,
    ) -> Result<String, String>;
}

/// Tool executor for cron/scheduler operations.
pub struct CronToolExecutor<S: SchedulerOps> {
    scheduler: Arc<S>,
}

impl<S: SchedulerOps> CronToolExecutor<S> {
    /// Create a new cron tool executor.
    pub fn new(scheduler: Arc<S>) -> Self {
        Self { scheduler }
    }

    #[instrument(skip(self, call), fields(tool = "cron"))]
    async fn handle(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let action = call
            .arguments
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArgument("missing required parameter: action".into())
            })?;

        let result = match action {
            "list" => {
                let include_disabled = call
                    .arguments
                    .get("includeDisabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.scheduler.list_jobs(include_disabled).await
            }
            "add" => {
                let job = call.arguments.get("job").cloned().ok_or_else(|| {
                    ToolError::InvalidArgument("add requires 'job' parameter".into())
                })?;
                self.scheduler.add_job(job).await
            }
            "remove" => {
                let id = self.get_job_id(call)?;
                self.scheduler.remove_job(&id).await
            }
            "update" => {
                let id = self.get_job_id(call)?;
                let patch = call.arguments.get("patch").cloned().unwrap_or_default();
                self.scheduler.update_job(&id, patch).await
            }
            "run" => {
                let id = self.get_job_id(call)?;
                self.scheduler.run_job(&id).await
            }
            "runs" => {
                let id = self.get_job_id(call)?;
                self.scheduler.get_runs(&id).await
            }
            "status" => self.scheduler.status().await,
            "wake" => {
                let text = call
                    .arguments
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidArgument("wake requires 'text' parameter".into())
                    })?;
                let mode = call.arguments.get("mode").and_then(|v| v.as_str());
                let context_messages = call
                    .arguments
                    .get("contextMessages")
                    .and_then(|v| v.as_u64());
                self.scheduler.wake(text, mode, context_messages).await
            }
            other => {
                return Err(ToolError::InvalidArgument(format!(
                    "unknown cron action: {other}"
                )));
            }
        };

        match result {
            Ok(output) => Ok(ToolResult {
                tool_call_id: call.tool_call_id.clone(),
                output,
                is_error: false,
                tool_execution_id: None,
            }),
            Err(e) => Ok(ToolResult {
                tool_call_id: call.tool_call_id.clone(),
                output: e,
                is_error: true,
                tool_execution_id: None,
            }),
        }
    }

    fn get_job_id(&self, call: &ToolCall) -> Result<String, ToolError> {
        call.arguments
            .get("jobId")
            .or_else(|| call.arguments.get("id"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| ToolError::InvalidArgument("missing jobId parameter".into()))
    }
}

#[async_trait]
impl<S: SchedulerOps> ToolExecutor for CronToolExecutor<S> {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "cron" => self.handle(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;

    struct MockScheduler;

    #[async_trait]
    impl SchedulerOps for MockScheduler {
        async fn list_jobs(&self, _include_disabled: bool) -> Result<String, String> {
            Ok("[{\"name\": \"test-job\"}]".into())
        }
        async fn add_job(&self, _job: serde_json::Value) -> Result<String, String> {
            Ok("{\"jobId\": \"abc-123\", \"status\": \"created\"}".into())
        }
        async fn remove_job(&self, id: &str) -> Result<String, String> {
            Ok(format!("{{\"jobId\": \"{id}\", \"removed\": true}}"))
        }
        async fn update_job(&self, id: &str, _patch: serde_json::Value) -> Result<String, String> {
            Ok(format!("{{\"jobId\": \"{id}\", \"updated\": true}}"))
        }
        async fn run_job(&self, id: &str) -> Result<String, String> {
            Ok(format!(
                "{{\"jobId\": \"{id}\", \"status\": \"triggered\"}}"
            ))
        }
        async fn get_runs(&self, _id: &str) -> Result<String, String> {
            Ok("[]".into())
        }
        async fn status(&self) -> Result<String, String> {
            Ok("{\"totalJobs\": 1, \"enabled\": 1}".into())
        }
        async fn wake(
            &self,
            text: &str,
            mode: Option<&str>,
            context_messages: Option<u64>,
        ) -> Result<String, String> {
            Ok(format!(
                "{{\"status\":\"queued\",\"text\":{text:?},\"mode\":{mode:?},\"contextMessages\":{context_messages:?}}}"
            ))
        }
    }

    fn make_call(args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "cron".into(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn list_returns_jobs() {
        let exec = CronToolExecutor::new(Arc::new(MockScheduler));
        let call = make_call(serde_json::json!({"action": "list"}));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("test-job"));
    }

    #[tokio::test]
    async fn add_returns_job_id() {
        let exec = CronToolExecutor::new(Arc::new(MockScheduler));
        let call = make_call(serde_json::json!({
            "action": "add",
            "job": {"schedule": {}, "payload": {}}
        }));
        let result = exec.execute(call).await.unwrap();
        assert!(result.output.contains("abc-123"));
    }

    #[tokio::test]
    async fn status_returns_counts() {
        let exec = CronToolExecutor::new(Arc::new(MockScheduler));
        let call = make_call(serde_json::json!({"action": "status"}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.output.contains("totalJobs"));
    }

    #[tokio::test]
    async fn wake_returns_queued_payload() {
        let exec = CronToolExecutor::new(Arc::new(MockScheduler));
        let call = make_call(serde_json::json!({
            "action": "wake",
            "text": "Reminder: check Rune",
            "mode": "now",
            "contextMessages": 3
        }));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("queued"));
        assert!(result.output.contains("Reminder: check Rune"));
        assert!(result.output.contains("now"));
    }

    #[tokio::test]
    async fn missing_action_rejected() {
        let exec = CronToolExecutor::new(Arc::new(MockScheduler));
        let call = make_call(serde_json::json!({}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }
}
