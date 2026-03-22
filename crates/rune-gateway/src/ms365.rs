//! Microsoft 365 gateway services.

use std::collections::HashMap;

use async_trait::async_trait;
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

/// Errors surfaced by the Planner service boundary.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum Ms365PlannerServiceError {
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

    fn graph_base_url(&self) -> String {
        std::env::var(GRAPH_BASE_URL_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_GRAPH_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string()
    }

    fn access_token(&self) -> Result<String, Ms365PlannerServiceError> {
        std::env::var(ACCESS_TOKEN_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.trim().to_string())
            .ok_or_else(|| {
                Ms365PlannerServiceError::NotConfigured(format!(
                    "Planner backend requires {ACCESS_TOKEN_ENV} to call Microsoft Graph"
                ))
            })
    }

    fn request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, Ms365PlannerServiceError> {
        let token = self.access_token()?;
        let url = format!("{}{}", self.graph_base_url(), path);
        Ok(self
            .client
            .request(method, url)
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "application/json"))
    }

    async fn send_json<T: serde::de::DeserializeOwned>(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<T, Ms365PlannerServiceError> {
        let response = builder
            .send()
            .await
            .map_err(|error| Ms365PlannerServiceError::Upstream(error.to_string()))?;

        if response.status().is_success() {
            response
                .json::<T>()
                .await
                .map_err(|error| Ms365PlannerServiceError::Upstream(error.to_string()))
        } else {
            Err(map_graph_error(response).await)
        }
    }

    async fn patch_json(
        &self,
        path: &str,
        if_match: &str,
        body: Value,
    ) -> Result<(), Ms365PlannerServiceError> {
        let response = self
            .request(reqwest::Method::PATCH, path)?
            .header(reqwest::header::IF_MATCH, if_match)
            .json(&body)
            .send()
            .await
            .map_err(|error| Ms365PlannerServiceError::Upstream(error.to_string()))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(map_graph_error(response).await)
        }
    }

    async fn fetch_task(&self, id: &str) -> Result<GraphPlannerTask, Ms365PlannerServiceError> {
        self.send_json(self.request(reqwest::Method::GET, &format!("/planner/tasks/{id}"))?)
            .await
    }

    async fn fetch_task_details(
        &self,
        id: &str,
    ) -> Result<GraphPlannerTaskDetails, Ms365PlannerServiceError> {
        self.send_json(self.request(
            reqwest::Method::GET,
            &format!("/planner/tasks/{id}/details"),
        )?)
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

        let created: GraphPlannerTask = self
            .send_json(
                self.request(reqwest::Method::POST, "/planner/tasks")?
                    .json(&Value::Object(body)),
            )
            .await?;

        if let Some(description) = request.description {
            let trimmed = description.trim().to_string();
            if !trimmed.is_empty() {
                let details = self.fetch_task_details(&created.id).await?;
                self.patch_json(
                    &format!("/planner/tasks/{}/details", created.id),
                    details.etag.as_deref().ok_or_else(|| {
                        Ms365PlannerServiceError::Upstream(
                            "planner task details response missing etag".to_string(),
                        )
                    })?,
                    json!({ "description": trimmed }),
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
            self.patch_json(
                &format!("/planner/tasks/{task_id}"),
                current.etag.as_deref().ok_or_else(|| {
                    Ms365PlannerServiceError::Upstream(
                        "planner task response missing etag".to_string(),
                    )
                })?,
                Value::Object(task_patch),
            )
            .await?;
        }

        if let Some(description) = request.description {
            let details = self.fetch_task_details(task_id).await?;
            self.patch_json(
                &format!("/planner/tasks/{task_id}/details"),
                details.etag.as_deref().ok_or_else(|| {
                    Ms365PlannerServiceError::Upstream(
                        "planner task details response missing etag".to_string(),
                    )
                })?,
                json!({ "description": description.trim() }),
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
        self.patch_json(
            &format!("/planner/tasks/{task_id}"),
            current.etag.as_deref().ok_or_else(|| {
                Ms365PlannerServiceError::Upstream("planner task response missing etag".to_string())
            })?,
            json!({ "percentComplete": 100 }),
        )
        .await?;

        self.read_task(task_id).await
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

async fn map_graph_error(response: reqwest::Response) -> Ms365PlannerServiceError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let message = graph_error_message(&body)
        .unwrap_or_else(|| format!("Microsoft Graph planner request failed with HTTP {status}"));

    match status {
        reqwest::StatusCode::BAD_REQUEST => Ms365PlannerServiceError::Validation(message),
        reqwest::StatusCode::UNAUTHORIZED => Ms365PlannerServiceError::Unauthorized,
        reqwest::StatusCode::FORBIDDEN => Ms365PlannerServiceError::Forbidden(message),
        reqwest::StatusCode::NOT_FOUND => Ms365PlannerServiceError::NotFound(message),
        _ => Ms365PlannerServiceError::Upstream(message),
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
