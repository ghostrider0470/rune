//! Microsoft 365 gateway services.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const GRAPH_BASE_URL_ENV: &str = "RUNE_MS365_GRAPH_BASE_URL";
const ACCESS_TOKEN_ENV: &str = "RUNE_MS365_ACCESS_TOKEN";
const DEFAULT_GRAPH_BASE_URL: &str = "https://graph.microsoft.com/v1.0";

/// Gateway request for creating a Planner task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreatePlannerTaskRequest {
    pub plan_id: String,
    pub title: String,
    #[serde(default)]
    pub bucket_id: Option<String>,
    #[serde(default)]
    pub assigned_to: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub description: Option<String>,
}

impl CreatePlannerTaskRequest {
    pub fn validate(&self) -> Result<(), Ms365PlannerServiceError> {
        if self.plan_id.trim().is_empty() {
            return Err(Ms365PlannerServiceError::Validation(
                "planner task plan_id is required".to_string(),
            ));
        }
        if self.title.trim().is_empty() {
            return Err(Ms365PlannerServiceError::Validation(
                "planner task title is required".to_string(),
            ));
        }
        Ok(())
    }
}

/// Gateway request for updating a Planner task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdatePlannerTaskRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub bucket_id: Option<String>,
    #[serde(default)]
    pub assigned_to: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub description: Option<String>,
}

impl UpdatePlannerTaskRequest {
    pub fn validate(&self) -> Result<(), Ms365PlannerServiceError> {
        if !self.has_changes() {
            return Err(Ms365PlannerServiceError::Validation(
                "planner task update requires at least one mutable field".to_string(),
            ));
        }

        if let Some(title) = &self.title
            && title.trim().is_empty()
        {
            return Err(Ms365PlannerServiceError::Validation(
                "planner task title cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    pub fn has_changes(&self) -> bool {
        self.title.is_some()
            || self.bucket_id.is_some()
            || self.assigned_to.is_some()
            || self.due_date.is_some()
            || self.priority.is_some()
            || self.description.is_some()
    }
}

/// Gateway-facing Planner task shape used by mutation routes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerTask {
    pub id: String,
    pub title: String,
    pub plan_id: String,
    pub bucket_id: Option<String>,
    pub percent_complete: u8,
    pub assigned_to: Option<String>,
    pub due_date: Option<String>,
    pub created_at: Option<String>,
    pub priority: Option<u8>,
    pub description: Option<String>,
}

/// Gateway request for creating a Microsoft To-Do task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateTodoTaskRequest {
    pub title: String,
    #[serde(default)]
    pub body_preview: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub importance: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

impl CreateTodoTaskRequest {
    pub fn validate(&self, list_id: &str) -> Result<(), Ms365TodoServiceError> {
        validate_todo_list_id(list_id)?;

        if self.title.trim().is_empty() {
            return Err(Ms365TodoServiceError::Validation(
                "todo task title is required".to_string(),
            ));
        }

        validate_todo_importance(self.importance.as_deref())?;
        validate_todo_status(self.status.as_deref())?;
        validate_todo_datetime(self.due_date.as_deref(), "todo task due_date")?;
        Ok(())
    }
}

/// Gateway request for updating a Microsoft To-Do task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateTodoTaskRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body_preview: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub importance: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

impl UpdateTodoTaskRequest {
    pub fn validate(&self, list_id: &str, id: &str) -> Result<(), Ms365TodoServiceError> {
        validate_todo_list_id(list_id)?;
        validate_todo_task_id(id)?;

        if !self.has_changes() {
            return Err(Ms365TodoServiceError::Validation(
                "todo task update requires at least one mutable field".to_string(),
            ));
        }

        if let Some(title) = &self.title
            && title.trim().is_empty()
        {
            return Err(Ms365TodoServiceError::Validation(
                "todo task title cannot be empty".to_string(),
            ));
        }

        validate_todo_importance(self.importance.as_deref())?;
        validate_todo_status(self.status.as_deref())?;
        validate_todo_datetime(self.due_date.as_deref(), "todo task due_date")?;
        Ok(())
    }

    pub fn has_changes(&self) -> bool {
        self.title.is_some()
            || self.body_preview.is_some()
            || self.due_date.is_some()
            || self.importance.is_some()
            || self.status.is_some()
    }
}

/// Gateway-facing Microsoft To-Do task shape used by mutation routes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoTask {
    pub id: String,
    pub list_id: String,
    pub title: String,
    pub status: String,
    pub importance: String,
    pub is_reminder_on: bool,
    pub due_date: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: Option<String>,
    pub body_preview: Option<String>,
}

/// Gateway request for sending a Microsoft 365 mail message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SendMailRequest {
    #[serde(default)]
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub cc: Vec<String>,
}

impl SendMailRequest {
    pub fn validate(&self) -> Result<(), Ms365MailServiceError> {
        validate_mail_recipients(&self.to, "mail to")?;
        validate_optional_mail_recipients(&self.cc, "mail cc")?;

        if self.subject.trim().is_empty() {
            return Err(Ms365MailServiceError::Validation(
                "mail subject is required".to_string(),
            ));
        }

        if self.body.trim().is_empty() {
            return Err(Ms365MailServiceError::Validation(
                "mail body is required".to_string(),
            ));
        }

        Ok(())
    }
}

/// Gateway request for replying to a Microsoft 365 mail message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplyMailRequest {
    pub body: String,
    #[serde(default)]
    pub reply_all: bool,
}

impl ReplyMailRequest {
    pub fn validate(&self, id: &str) -> Result<(), Ms365MailServiceError> {
        validate_mail_message_id(id)?;

        if self.body.trim().is_empty() {
            return Err(Ms365MailServiceError::Validation(
                "mail reply body is required".to_string(),
            ));
        }

        Ok(())
    }
}

/// Gateway request for forwarding a Microsoft 365 mail message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForwardMailRequest {
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

impl ForwardMailRequest {
    pub fn validate(&self, id: &str) -> Result<(), Ms365MailServiceError> {
        validate_mail_message_id(id)?;
        validate_mail_recipients(&self.to, "mail forward to")?;
        Ok(())
    }
}

/// Errors surfaced by the Microsoft 365 mutation service boundaries.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum Ms365ServiceError {
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    NotConfigured(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Upstream(String),
}

pub type Ms365PlannerServiceError = Ms365ServiceError;
pub type Ms365TodoServiceError = Ms365ServiceError;
pub type Ms365MailServiceError = Ms365ServiceError;

#[async_trait]
pub trait Ms365PlannerService: Send + Sync {
    async fn create_task(
        &self,
        request: CreatePlannerTaskRequest,
    ) -> Result<PlannerTask, Ms365PlannerServiceError>;

    async fn update_task(
        &self,
        id: &str,
        request: UpdatePlannerTaskRequest,
    ) -> Result<PlannerTask, Ms365PlannerServiceError>;

    async fn complete_task(&self, id: &str) -> Result<PlannerTask, Ms365PlannerServiceError>;
}

#[async_trait]
pub trait Ms365TodoService: Send + Sync {
    async fn create_task(
        &self,
        list_id: &str,
        request: CreateTodoTaskRequest,
    ) -> Result<TodoTask, Ms365TodoServiceError>;

    async fn update_task(
        &self,
        list_id: &str,
        id: &str,
        request: UpdateTodoTaskRequest,
    ) -> Result<TodoTask, Ms365TodoServiceError>;

    async fn complete_task(
        &self,
        list_id: &str,
        id: &str,
    ) -> Result<TodoTask, Ms365TodoServiceError>;
}

#[async_trait]
pub trait Ms365MailService: Send + Sync {
    async fn send_mail(&self, request: SendMailRequest) -> Result<(), Ms365MailServiceError>;

    async fn reply_to_message(
        &self,
        id: &str,
        request: ReplyMailRequest,
    ) -> Result<(), Ms365MailServiceError>;

    async fn forward_message(
        &self,
        id: &str,
        request: ForwardMailRequest,
    ) -> Result<(), Ms365MailServiceError>;
}

/// Planner mutation service backed by Microsoft Graph.
pub struct GraphMs365PlannerService {
    client: Client,
}

impl Default for GraphMs365PlannerService {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphMs365PlannerService {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    async fn fetch_task(&self, id: &str) -> Result<GraphPlannerTask, Ms365PlannerServiceError> {
        send_graph_json(
            graph_request(
                &self.client,
                reqwest::Method::GET,
                &format!("/planner/tasks/{id}"),
            )?,
            "planner",
        )
        .await
    }

    async fn fetch_task_details(
        &self,
        id: &str,
    ) -> Result<GraphPlannerTaskDetails, Ms365PlannerServiceError> {
        send_graph_json(
            graph_request(
                &self.client,
                reqwest::Method::GET,
                &format!("/planner/tasks/{id}/details"),
            )?,
            "planner",
        )
        .await
    }

    async fn read_task(&self, id: &str) -> Result<PlannerTask, Ms365PlannerServiceError> {
        let task = self.fetch_task(id).await?;
        let details = self.fetch_task_details(id).await?;
        Ok(PlannerTask::from_graph(task, details))
    }
}

#[async_trait]
impl Ms365PlannerService for GraphMs365PlannerService {
    async fn create_task(
        &self,
        request: CreatePlannerTaskRequest,
    ) -> Result<PlannerTask, Ms365PlannerServiceError> {
        request.validate()?;

        let mut body = Map::new();
        body.insert("planId".to_string(), json!(request.plan_id.trim()));
        body.insert("title".to_string(), json!(request.title.trim()));

        if let Some(bucket_id) = request
            .bucket_id
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            body.insert("bucketId".to_string(), json!(bucket_id.trim()));
        }

        if let Some(due_date) = request
            .due_date
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            body.insert("dueDateTime".to_string(), json!(due_date.trim()));
        }

        if let Some(priority) = request.priority {
            body.insert("priority".to_string(), json!(priority));
        }

        if let Some(assignee) = request
            .assigned_to
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            body.insert(
                "assignments".to_string(),
                planner_assignments_value(assignee),
            );
        }

        let created: GraphPlannerTask = self.fetch_graph_task_created(Value::Object(body)).await?;

        if let Some(description) = request.description {
            let trimmed = description.trim().to_string();
            if !trimmed.is_empty() {
                let details = self.fetch_task_details(&created.id).await?;
                patch_graph_json(
                    &self.client,
                    &format!("/planner/tasks/{}/details", created.id),
                    Some(details.etag.as_deref().ok_or_else(|| {
                        Ms365PlannerServiceError::Upstream(
                            "planner task details response missing etag".to_string(),
                        )
                    })?),
                    json!({ "description": trimmed }),
                    "planner",
                )
                .await?;
            }
        }

        self.read_task(&created.id).await
    }

    async fn update_task(
        &self,
        id: &str,
        request: UpdatePlannerTaskRequest,
    ) -> Result<PlannerTask, Ms365PlannerServiceError> {
        if id.trim().is_empty() {
            return Err(Ms365PlannerServiceError::Validation(
                "planner task id is required".to_string(),
            ));
        }
        request.validate()?;

        let task_id = id.trim();
        let mut task_patch = Map::new();

        if let Some(title) = &request.title {
            task_patch.insert("title".to_string(), json!(title.trim()));
        }

        if let Some(bucket_id) = request
            .bucket_id
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            task_patch.insert("bucketId".to_string(), json!(bucket_id.trim()));
        }

        if let Some(due_date) = request
            .due_date
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            task_patch.insert("dueDateTime".to_string(), json!(due_date.trim()));
        }

        if let Some(priority) = request.priority {
            task_patch.insert("priority".to_string(), json!(priority));
        }

        if let Some(assignee) = request
            .assigned_to
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            task_patch.insert(
                "assignments".to_string(),
                planner_assignments_value(assignee),
            );
        }

        if !task_patch.is_empty() {
            let current = self.fetch_task(task_id).await?;
            patch_graph_json(
                &self.client,
                &format!("/planner/tasks/{task_id}"),
                Some(current.etag.as_deref().ok_or_else(|| {
                    Ms365PlannerServiceError::Upstream(
                        "planner task response missing etag".to_string(),
                    )
                })?),
                Value::Object(task_patch),
                "planner",
            )
            .await?;
        }

        if let Some(description) = request.description {
            let details = self.fetch_task_details(task_id).await?;
            patch_graph_json(
                &self.client,
                &format!("/planner/tasks/{task_id}/details"),
                Some(details.etag.as_deref().ok_or_else(|| {
                    Ms365PlannerServiceError::Upstream(
                        "planner task details response missing etag".to_string(),
                    )
                })?),
                json!({ "description": description.trim() }),
                "planner",
            )
            .await?;
        }

        self.read_task(task_id).await
    }

    async fn complete_task(&self, id: &str) -> Result<PlannerTask, Ms365PlannerServiceError> {
        if id.trim().is_empty() {
            return Err(Ms365PlannerServiceError::Validation(
                "planner task id is required".to_string(),
            ));
        }

        let task_id = id.trim();
        let current = self.fetch_task(task_id).await?;
        patch_graph_json(
            &self.client,
            &format!("/planner/tasks/{task_id}"),
            Some(current.etag.as_deref().ok_or_else(|| {
                Ms365PlannerServiceError::Upstream("planner task response missing etag".to_string())
            })?),
            json!({ "percentComplete": 100 }),
            "planner",
        )
        .await?;

        self.read_task(task_id).await
    }
}

impl GraphMs365PlannerService {
    async fn fetch_graph_task_created(
        &self,
        body: Value,
    ) -> Result<GraphPlannerTask, Ms365PlannerServiceError> {
        send_graph_json(
            graph_request(&self.client, reqwest::Method::POST, "/planner/tasks")?.json(&body),
            "planner",
        )
        .await
    }
}

fn planner_assignments_value(assignee: &str) -> Value {
    json!({
        assignee.trim(): {
            "@odata.type": "microsoft.graph.plannerAssignment",
            "orderHint": " !"
        }
    })
}

/// Microsoft To-Do mutation service backed by Microsoft Graph.
pub struct GraphMs365TodoService {
    client: Client,
}

impl Default for GraphMs365TodoService {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphMs365TodoService {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    async fn fetch_task(
        &self,
        list_id: &str,
        id: &str,
    ) -> Result<GraphTodoTask, Ms365TodoServiceError> {
        send_graph_json(
            graph_request(
                &self.client,
                reqwest::Method::GET,
                &format!("/me/todo/lists/{list_id}/tasks/{id}"),
            )?,
            "todo",
        )
        .await
    }

    async fn read_task(&self, list_id: &str, id: &str) -> Result<TodoTask, Ms365TodoServiceError> {
        let task = self.fetch_task(list_id, id).await?;
        Ok(TodoTask::from_graph(list_id, task))
    }
}

#[async_trait]
impl Ms365TodoService for GraphMs365TodoService {
    async fn create_task(
        &self,
        list_id: &str,
        request: CreateTodoTaskRequest,
    ) -> Result<TodoTask, Ms365TodoServiceError> {
        request.validate(list_id)?;

        let mut body = Map::new();
        body.insert("title".to_string(), json!(request.title.trim()));

        if let Some(body_preview) = request
            .body_preview
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            body.insert(
                "body".to_string(),
                graph_item_body_value(body_preview.trim()),
            );
        }

        if let Some(due_date) = request
            .due_date
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            body.insert("dueDateTime".to_string(), graph_datetime_value(due_date)?);
        }

        if let Some(importance) = request
            .importance
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            body.insert("importance".to_string(), json!(importance.trim()));
        }

        if let Some(status) = request
            .status
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            body.insert("status".to_string(), json!(status.trim()));
        }

        let created: GraphTodoTask = send_graph_json(
            graph_request(
                &self.client,
                reqwest::Method::POST,
                &format!("/me/todo/lists/{list_id}/tasks"),
            )?
            .json(&Value::Object(body)),
            "todo",
        )
        .await?;

        self.read_task(list_id, &created.id).await
    }

    async fn update_task(
        &self,
        list_id: &str,
        id: &str,
        request: UpdateTodoTaskRequest,
    ) -> Result<TodoTask, Ms365TodoServiceError> {
        request.validate(list_id, id)?;

        let task_id = id.trim();
        let mut patch = Map::new();

        if let Some(title) = &request.title {
            patch.insert("title".to_string(), json!(title.trim()));
        }

        if let Some(body_preview) = &request.body_preview {
            patch.insert(
                "body".to_string(),
                graph_item_body_value(body_preview.trim()),
            );
        }

        if let Some(due_date) = &request.due_date {
            if due_date.trim().is_empty() {
                patch.insert("dueDateTime".to_string(), Value::Null);
            } else {
                patch.insert("dueDateTime".to_string(), graph_datetime_value(due_date)?);
            }
        }

        if let Some(importance) = request.importance.as_ref() {
            patch.insert("importance".to_string(), json!(importance.trim()));
        }

        if let Some(status) = request.status.as_ref() {
            patch.insert("status".to_string(), json!(status.trim()));
        }

        let current = self.fetch_task(list_id, task_id).await?;
        patch_graph_json(
            &self.client,
            &format!("/me/todo/lists/{list_id}/tasks/{task_id}"),
            current.etag.as_deref(),
            Value::Object(patch),
            "todo",
        )
        .await?;

        self.read_task(list_id, task_id).await
    }

    async fn complete_task(
        &self,
        list_id: &str,
        id: &str,
    ) -> Result<TodoTask, Ms365TodoServiceError> {
        validate_todo_list_id(list_id)?;
        validate_todo_task_id(id)?;

        let task_id = id.trim();
        let current = self.fetch_task(list_id, task_id).await?;
        patch_graph_json(
            &self.client,
            &format!("/me/todo/lists/{list_id}/tasks/{task_id}"),
            current.etag.as_deref(),
            json!({ "status": "completed" }),
            "todo",
        )
        .await?;

        self.read_task(list_id, task_id).await
    }
}

/// Mail mutation service backed by Microsoft Graph.
pub struct GraphMs365MailService {
    client: Client,
}

impl Default for GraphMs365MailService {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphMs365MailService {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Ms365MailService for GraphMs365MailService {
    async fn send_mail(&self, request: SendMailRequest) -> Result<(), Ms365MailServiceError> {
        request.validate()?;

        let mut message = Map::new();
        message.insert("subject".to_string(), json!(request.subject.trim()));
        message.insert(
            "body".to_string(),
            graph_item_body_value(request.body.trim()),
        );
        message.insert(
            "toRecipients".to_string(),
            graph_mail_recipients_value(&request.to)?,
        );

        let cc_recipients = graph_optional_mail_recipients_value(&request.cc)?;
        if let Some(cc_recipients) = cc_recipients {
            message.insert("ccRecipients".to_string(), cc_recipients);
        }

        send_graph_empty(
            graph_request(&self.client, reqwest::Method::POST, "/me/sendMail")?.json(&json!({
                "message": Value::Object(message),
                "saveToSentItems": true,
            })),
            "mail",
        )
        .await
    }

    async fn reply_to_message(
        &self,
        id: &str,
        request: ReplyMailRequest,
    ) -> Result<(), Ms365MailServiceError> {
        request.validate(id)?;

        let message_id = id.trim();
        let action = if request.reply_all {
            "replyAll"
        } else {
            "reply"
        };

        send_graph_empty(
            graph_request(
                &self.client,
                reqwest::Method::POST,
                &format!("/me/messages/{message_id}/{action}"),
            )?
            .json(&json!({
                "comment": request.body.trim(),
            })),
            "mail",
        )
        .await
    }

    async fn forward_message(
        &self,
        id: &str,
        request: ForwardMailRequest,
    ) -> Result<(), Ms365MailServiceError> {
        request.validate(id)?;

        let message_id = id.trim();
        let comment = request
            .comment
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();

        send_graph_empty(
            graph_request(
                &self.client,
                reqwest::Method::POST,
                &format!("/me/messages/{message_id}/forward"),
            )?
            .json(&json!({
                "comment": comment,
                "toRecipients": graph_mail_recipients_value(&request.to)?,
            })),
            "mail",
        )
        .await
    }
}

async fn map_graph_error(response: reqwest::Response, service_name: &str) -> Ms365ServiceError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let message = graph_error_message(&body).unwrap_or_else(|| {
        format!("Microsoft Graph {service_name} request failed with HTTP {status}")
    });

    match status {
        reqwest::StatusCode::BAD_REQUEST => Ms365ServiceError::Validation(message),
        reqwest::StatusCode::UNAUTHORIZED => Ms365ServiceError::Unauthorized,
        reqwest::StatusCode::FORBIDDEN => Ms365ServiceError::Forbidden(message),
        reqwest::StatusCode::NOT_FOUND => Ms365ServiceError::NotFound(message),
        _ => Ms365ServiceError::Upstream(message),
    }
}

fn graph_error_message(body: &str) -> Option<String> {
    let json = serde_json::from_str::<Value>(body).ok()?;
    json.get("error")
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

#[derive(Debug, Deserialize)]
struct GraphPlannerTask {
    id: String,
    title: String,
    #[serde(rename = "planId")]
    plan_id: String,
    #[serde(rename = "bucketId")]
    bucket_id: Option<String>,
    #[serde(rename = "percentComplete", default)]
    percent_complete: u8,
    #[serde(rename = "dueDateTime")]
    due_date: Option<String>,
    #[serde(rename = "createdDateTime")]
    created_at: Option<String>,
    priority: Option<u8>,
    #[serde(default)]
    assignments: HashMap<String, Value>,
    #[serde(rename = "@odata.etag")]
    etag: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphPlannerTaskDetails {
    description: Option<String>,
    #[serde(rename = "@odata.etag")]
    etag: Option<String>,
}

impl PlannerTask {
    fn from_graph(task: GraphPlannerTask, details: GraphPlannerTaskDetails) -> Self {
        let assigned_to = if task.assignments.is_empty() {
            None
        } else {
            let mut assignees = task.assignments.into_keys().collect::<Vec<_>>();
            assignees.sort();
            assignees.into_iter().next()
        };

        Self {
            id: task.id,
            title: task.title,
            plan_id: task.plan_id,
            bucket_id: task.bucket_id,
            percent_complete: task.percent_complete,
            assigned_to,
            due_date: task.due_date,
            created_at: task.created_at,
            priority: task.priority,
            description: details.description,
        }
    }
}

#[derive(Debug, Deserialize)]
struct GraphTodoTask {
    id: String,
    title: String,
    #[serde(default = "default_todo_status")]
    status: String,
    #[serde(default = "default_todo_importance")]
    importance: String,
    #[serde(rename = "isReminderOn", default)]
    is_reminder_on: bool,
    #[serde(rename = "dueDateTime")]
    due_date: Option<GraphDateTimeTimeZone>,
    #[serde(rename = "completedDateTime")]
    completed_at: Option<GraphDateTimeTimeZone>,
    #[serde(rename = "createdDateTime")]
    created_at: Option<String>,
    #[serde(default)]
    body: Option<GraphItemBody>,
    #[serde(rename = "@odata.etag")]
    etag: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphDateTimeTimeZone {
    #[serde(rename = "dateTime")]
    date_time: String,
    #[serde(rename = "timeZone")]
    time_zone: String,
}

#[derive(Debug, Deserialize)]
struct GraphItemBody {
    #[serde(default)]
    content: String,
}

impl TodoTask {
    fn from_graph(list_id: &str, task: GraphTodoTask) -> Self {
        Self {
            id: task.id,
            list_id: list_id.to_string(),
            title: task.title,
            status: task.status,
            importance: task.importance,
            is_reminder_on: task.is_reminder_on,
            due_date: task.due_date.as_ref().map(graph_datetime_string),
            completed_at: task.completed_at.as_ref().map(graph_datetime_string),
            created_at: task.created_at,
            body_preview: task
                .body
                .map(|body| body.content.trim().to_string())
                .filter(|body| !body.is_empty()),
        }
    }
}

fn graph_base_url() -> String {
    std::env::var(GRAPH_BASE_URL_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_GRAPH_BASE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn access_token() -> Result<String, Ms365ServiceError> {
    std::env::var(ACCESS_TOKEN_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .ok_or_else(|| {
            Ms365ServiceError::NotConfigured(format!(
                "MS365 backend requires {ACCESS_TOKEN_ENV} to call Microsoft Graph"
            ))
        })
}

fn graph_request(
    client: &Client,
    method: reqwest::Method,
    path: &str,
) -> Result<reqwest::RequestBuilder, Ms365ServiceError> {
    let token = access_token()?;
    let url = format!("{}{}", graph_base_url(), path);
    Ok(client
        .request(method, url)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/json"))
}

async fn send_graph_json<T: serde::de::DeserializeOwned>(
    builder: reqwest::RequestBuilder,
    service_name: &str,
) -> Result<T, Ms365ServiceError> {
    let response = builder
        .send()
        .await
        .map_err(|error| Ms365ServiceError::Upstream(error.to_string()))?;

    if response.status().is_success() {
        response
            .json::<T>()
            .await
            .map_err(|error| Ms365ServiceError::Upstream(error.to_string()))
    } else {
        Err(map_graph_error(response, service_name).await)
    }
}

async fn send_graph_empty(
    builder: reqwest::RequestBuilder,
    service_name: &str,
) -> Result<(), Ms365ServiceError> {
    let response = builder
        .send()
        .await
        .map_err(|error| Ms365ServiceError::Upstream(error.to_string()))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(map_graph_error(response, service_name).await)
    }
}

async fn patch_graph_json(
    client: &Client,
    path: &str,
    if_match: Option<&str>,
    body: Value,
    service_name: &str,
) -> Result<(), Ms365ServiceError> {
    let mut builder = graph_request(client, reqwest::Method::PATCH, path)?.json(&body);
    if let Some(if_match) = if_match {
        builder = builder.header(reqwest::header::IF_MATCH, if_match);
    }

    let response = builder
        .send()
        .await
        .map_err(|error| Ms365ServiceError::Upstream(error.to_string()))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(map_graph_error(response, service_name).await)
    }
}

fn graph_item_body_value(content: &str) -> Value {
    json!({
        "contentType": "text",
        "content": content,
    })
}

fn graph_datetime_value(value: &str) -> Result<Value, Ms365ServiceError> {
    let parsed = DateTime::parse_from_rfc3339(value.trim()).map_err(|error| {
        Ms365ServiceError::Validation(format!("invalid RFC3339 datetime '{value}': {error}"))
    })?;
    let utc = parsed.with_timezone(&Utc);
    Ok(json!({
        "dateTime": utc.format("%Y-%m-%dT%H:%M:%S").to_string(),
        "timeZone": "UTC",
    }))
}

fn graph_datetime_string(value: &GraphDateTimeTimeZone) -> String {
    let raw = format!("{} {}", value.date_time.trim(), value.time_zone.trim());
    let raw = raw.trim();

    if let Ok(parsed) = DateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f %Z") {
        return parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true);
    }

    if value.time_zone.eq_ignore_ascii_case("UTC") {
        return format!("{}Z", value.date_time.trim_end_matches('Z'));
    }

    value.date_time.clone()
}

fn validate_mail_message_id(id: &str) -> Result<(), Ms365MailServiceError> {
    if id.trim().is_empty() {
        return Err(Ms365MailServiceError::Validation(
            "mail message id is required".to_string(),
        ));
    }
    Ok(())
}

fn validate_mail_recipients(
    recipients: &[String],
    field_name: &str,
) -> Result<(), Ms365MailServiceError> {
    if recipients.is_empty() {
        return Err(Ms365MailServiceError::Validation(format!(
            "{field_name} requires at least one recipient"
        )));
    }

    validate_optional_mail_recipients(recipients, field_name)
}

fn validate_optional_mail_recipients(
    recipients: &[String],
    field_name: &str,
) -> Result<(), Ms365MailServiceError> {
    if recipients
        .iter()
        .any(|recipient| recipient.trim().is_empty())
    {
        return Err(Ms365MailServiceError::Validation(format!(
            "{field_name} recipients cannot be empty"
        )));
    }

    Ok(())
}

fn graph_mail_recipients_value(recipients: &[String]) -> Result<Value, Ms365MailServiceError> {
    validate_mail_recipients(recipients, "mail recipients")?;
    Ok(Value::Array(
        recipients
            .iter()
            .map(|recipient| graph_mail_recipient_value(recipient))
            .collect(),
    ))
}

fn graph_optional_mail_recipients_value(
    recipients: &[String],
) -> Result<Option<Value>, Ms365MailServiceError> {
    validate_optional_mail_recipients(recipients, "mail recipients")?;

    if recipients.is_empty() {
        return Ok(None);
    }

    Ok(Some(Value::Array(
        recipients
            .iter()
            .map(|recipient| graph_mail_recipient_value(recipient))
            .collect(),
    )))
}

fn graph_mail_recipient_value(recipient: &str) -> Value {
    json!({
        "emailAddress": {
            "address": recipient.trim(),
        }
    })
}

fn validate_todo_list_id(list_id: &str) -> Result<(), Ms365TodoServiceError> {
    if list_id.trim().is_empty() {
        return Err(Ms365TodoServiceError::Validation(
            "todo list_id is required".to_string(),
        ));
    }
    Ok(())
}

fn validate_todo_task_id(id: &str) -> Result<(), Ms365TodoServiceError> {
    if id.trim().is_empty() {
        return Err(Ms365TodoServiceError::Validation(
            "todo task id is required".to_string(),
        ));
    }
    Ok(())
}

fn validate_todo_importance(value: Option<&str>) -> Result<(), Ms365TodoServiceError> {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        if !matches!(value, "low" | "normal" | "high") {
            return Err(Ms365TodoServiceError::Validation(
                "todo task importance must be one of low, normal, or high".to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_todo_status(value: Option<&str>) -> Result<(), Ms365TodoServiceError> {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        if !matches!(
            value,
            "notStarted" | "inProgress" | "completed" | "waitingOnOthers" | "deferred"
        ) {
            return Err(Ms365TodoServiceError::Validation(
                "todo task status must be one of notStarted, inProgress, completed, waitingOnOthers, or deferred".to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_todo_datetime(
    value: Option<&str>,
    field_name: &str,
) -> Result<(), Ms365TodoServiceError> {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        DateTime::parse_from_rfc3339(value).map_err(|error| {
            Ms365TodoServiceError::Validation(format!(
                "{field_name} must be valid RFC3339: {error}"
            ))
        })?;
    }
    Ok(())
}

fn default_todo_status() -> String {
    "notStarted".to_string()
}

fn default_todo_importance() -> String {
    "normal".to_string()
}
