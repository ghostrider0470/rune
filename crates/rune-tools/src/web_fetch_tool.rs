//! HTTP fetch tool for agents to retrieve web content, APIs, and issue trackers.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, COOKIE, HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect::Policy;
use tracing::instrument;

use crate::definition::{ToolCall, ToolDefinition, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Maximum response body size returned to the LLM context (50 KB).
const MAX_BODY_BYTES: usize = 50 * 1024;

/// Default request timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const REDACTED_HEADER_VALUE: &str = "<redacted>";
const DEFAULT_REDIRECT_LIMIT: usize = 10;

/// Executor for the `web_fetch` tool.
///
/// Makes HTTP GET/POST requests and returns the response body, truncated
/// to fit within LLM context limits.
pub struct WebFetchToolExecutor {
    client: reqwest::Client,
    sessions: Arc<Mutex<HashMap<String, StoredSession>>>,
}

#[derive(Clone, Debug, Default)]
struct StoredSession {
    default_headers: HeaderMap,
    cookie_header: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RedirectPolicyMode {
    Follow,
    Manual,
    Error,
}

struct RequestPlan {
    method: String,
    url: String,
    body: Option<String>,
    session_key: Option<String>,
    persist_session: bool,
    redirect_policy: RedirectPolicyMode,
    request_headers: HeaderMap,
    display_headers: Vec<String>,
    request_cookie_header: Option<String>,
}

impl WebFetchToolExecutor {
    /// Create a new web-fetch executor with default settings.
    pub fn new() -> Result<Self, ToolError> {
        let client = build_client(RedirectPolicyMode::Follow)?;
        Ok(Self {
            client,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Create from an existing reqwest client (useful for testing).
    pub fn with_client(client: reqwest::Client) -> Self {
        Self {
            client,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[instrument(skip(self, call), fields(tool = "web_fetch"))]
    async fn handle(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let request_plan = match self.build_request_plan(call) {
            Ok(plan) => plan,
            Err(output) => {
                return Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output,
                    is_error: true,
                    tool_execution_id: None,
                });
            }
        };

        let client = if request_plan.redirect_policy == RedirectPolicyMode::Follow {
            self.client.clone()
        } else {
            build_client(request_plan.redirect_policy)?
        };

        // Build the request
        let mut request = match request_plan.method.as_str() {
            "GET" => client.get(&request_plan.url),
            "POST" => client.post(&request_plan.url),
            other => {
                return Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: format!("Unsupported HTTP method: {other}. Use GET or POST."),
                    is_error: true,
                    tool_execution_id: None,
                });
            }
        };

        // Apply headers
        request = request.headers(request_plan.request_headers.clone());

        // Apply body (POST)
        if let Some(body_content) = &request_plan.body {
            request = request.body(body_content.clone());
        }

        // Execute the request
        let response = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                let msg = if e.is_timeout() {
                    format!("Request timed out after {}s", DEFAULT_TIMEOUT.as_secs())
                } else if e.is_redirect() {
                    format!(
                        "Redirect handling aborted request: {e}. Configure redirect_policy=follow or manual if redirects are expected."
                    )
                } else if e.is_connect() {
                    format!("Connection failed: {e}")
                } else {
                    format!("HTTP request failed: {e}")
                };
                return Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: msg,
                    is_error: true,
                    tool_execution_id: None,
                });
            }
        };

        let session_update =
            build_session_update(response.headers(), &request_plan.request_cookie_header);
        if request_plan.persist_session {
            if let Some(session_key) = request_plan.session_key.as_deref() {
                self.store_session(
                    session_key,
                    &request_plan.request_headers,
                    session_update.as_ref(),
                )?;
            }
        }

        let status = response.status();
        let status_code = status.as_u16();

        // Collect selected response headers
        let response_headers: Vec<String> = response
            .headers()
            .iter()
            .filter(|(name, _)| {
                let n = name.as_str();
                matches!(
                    n,
                    "content-type"
                        | "content-length"
                        | "location"
                        | "set-cookie"
                        | "x-ratelimit-remaining"
                        | "retry-after"
                )
            })
            .map(|(name, value)| format!("{}: {}", name, value.to_str().unwrap_or("<binary>")))
            .collect();

        // Read body text
        let full_body = match response.text().await {
            Ok(text) => text,
            Err(e) => {
                return Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: format!("HTTP {status_code} — failed to read response body: {e}"),
                    is_error: true,
                    tool_execution_id: None,
                });
            }
        };

        // Truncate for LLM context
        let (body_text, truncated) = if full_body.len() > MAX_BODY_BYTES {
            let truncated_body = truncate_utf8(&full_body, MAX_BODY_BYTES);
            (truncated_body.to_string(), true)
        } else {
            (full_body.clone(), false)
        };

        // Format output
        let mut output = format!(
            "HTTP {status_code} {}\n",
            status.canonical_reason().unwrap_or("")
        );
        if !request_plan.display_headers.is_empty() {
            output.push_str("Request headers:\n");
            for header in &request_plan.display_headers {
                output.push_str(header);
                output.push('\n');
            }
        }
        if let Some(session_key) = request_plan.session_key.as_deref() {
            output.push_str(&format!(
                "Session: {} ({})\n",
                session_key,
                if request_plan.persist_session {
                    "persisted"
                } else {
                    "read-only"
                }
            ));
        }
        output.push_str(&format!(
            "Redirect policy: {}\n",
            request_plan.redirect_policy.as_str()
        ));
        if !response_headers.is_empty() {
            for h in &response_headers {
                output.push_str(h);
                output.push('\n');
            }
        }
        output.push('\n');
        output.push_str(&body_text);
        if truncated {
            output.push_str(&format!(
                "\n\n[truncated: showing {MAX_BODY_BYTES} of {} bytes]",
                full_body.len()
            ));
        }

        Ok(ToolResult {
            tool_call_id: call.tool_call_id.clone(),
            output,
            is_error: false,
            tool_execution_id: None,
        })
    }

    fn build_request_plan(&self, call: &ToolCall) -> Result<RequestPlan, String> {
        let url = call
            .arguments
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required field: url".to_string())?;

        let method = call
            .arguments
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET")
            .to_uppercase();

        let session_key = call
            .arguments
            .get("session")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let persist_session = call
            .arguments
            .get("persist_session")
            .and_then(|v| v.as_bool())
            .unwrap_or(session_key.is_some());
        let redirect_policy = parse_redirect_policy(
            call.arguments
                .get("redirect_policy")
                .and_then(|v| v.as_str()),
        )?;

        // Parse optional headers
        let raw_headers: HashMap<String, String> = call
            .arguments
            .get("headers")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let mut request_headers = HeaderMap::new();
        let mut display_headers = Vec::new();

        if let Some(session_key) = session_key.as_deref() {
            if let Some(stored) = self.load_session(session_key)? {
                request_headers.extend(stored.default_headers.clone());
                for (name, value) in stored.default_headers.iter() {
                    let rendered_value = if sensitive_header_names().contains(name.as_str()) {
                        REDACTED_HEADER_VALUE.to_string()
                    } else {
                        value.to_str().unwrap_or("<binary>").to_string()
                    };
                    display_headers.push(format!("{}: {}", name.as_str(), rendered_value));
                }
                if let Some(cookie_header) = stored.cookie_header {
                    let cookie_value = HeaderValue::from_str(&cookie_header).map_err(|e| {
                        format!("Invalid persisted cookie header for session '{session_key}': {e}")
                    })?;
                    request_headers.insert(COOKIE, cookie_value);
                    display_headers.retain(|line| !line.starts_with("cookie:"));
                    display_headers.push(format!("cookie: {REDACTED_HEADER_VALUE}"));
                }
            }
        }

        let (explicit_headers, explicit_display_headers) = build_request_headers(&raw_headers)?;
        for (name, value) in &explicit_headers {
            request_headers.insert(name.clone(), value.clone());
        }
        merge_display_headers(&mut display_headers, explicit_display_headers);

        let request_cookie_header = request_headers
            .get(COOKIE)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);

        let body = call
            .arguments
            .get("body")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(RequestPlan {
            method,
            url: url.to_string(),
            body,
            session_key,
            persist_session,
            redirect_policy,
            request_headers,
            display_headers,
            request_cookie_header,
        })
    }

    fn load_session(&self, session_key: &str) -> Result<Option<StoredSession>, String> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| "web_fetch session store lock poisoned".to_string())?;
        Ok(sessions.get(session_key).cloned())
    }

    fn store_session(
        &self,
        session_key: &str,
        request_headers: &HeaderMap,
        update: Option<&SessionUpdate>,
    ) -> Result<(), ToolError> {
        let mut sessions = self.sessions.lock().map_err(|_| {
            ToolError::ExecutionFailed("web_fetch session store lock poisoned".into())
        })?;
        let entry = sessions.entry(session_key.to_string()).or_default();
        entry.default_headers = request_headers.clone();
        if let Some(update) = update {
            entry.cookie_header = update.cookie_header.clone();
        }
        Ok(())
    }
}

fn build_client(redirect_policy: RedirectPolicyMode) -> Result<reqwest::Client, ToolError> {
    reqwest::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .user_agent("rune-agent/0.1")
        .redirect(match redirect_policy {
            RedirectPolicyMode::Follow => Policy::limited(DEFAULT_REDIRECT_LIMIT),
            RedirectPolicyMode::Manual => Policy::none(),
            RedirectPolicyMode::Error => Policy::custom(|attempt| {
                attempt.error("redirect blocked by web_fetch redirect_policy=error")
            }),
        })
        .build()
        .map_err(|e| ToolError::ExecutionFailed(format!("failed to build HTTP client: {e}")))
}

fn build_request_headers(
    raw_headers: &HashMap<String, String>,
) -> Result<(HeaderMap, Vec<String>), String> {
    let mut request_headers = HeaderMap::new();
    let mut display_headers = Vec::new();
    let sensitive = sensitive_header_names();

    for (key, value) in raw_headers {
        let header_name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|e| format!("Invalid header name '{key}': {e}"))?;
        let header_value = HeaderValue::from_str(value)
            .map_err(|e| format!("Invalid header value for '{key}': {e}"))?;
        let is_sensitive = sensitive.contains(header_name.as_str());
        request_headers.append(header_name.clone(), header_value);
        let rendered_value = if is_sensitive {
            REDACTED_HEADER_VALUE
        } else {
            value.as_str()
        };
        display_headers.push(format!("{}: {}", header_name.as_str(), rendered_value));
    }

    display_headers.sort();
    Ok((request_headers, display_headers))
}

fn merge_display_headers(existing: &mut Vec<String>, replacements: Vec<String>) {
    for replacement in replacements {
        if let Some((name, _)) = replacement.split_once(':') {
            let prefix = format!("{}:", name.trim().to_ascii_lowercase());
            existing.retain(|line| !line.to_ascii_lowercase().starts_with(&prefix));
        }
        existing.push(replacement);
    }
    existing.sort();
}

fn sensitive_header_names() -> HashSet<&'static str> {
    HashSet::from([
        AUTHORIZATION.as_str(),
        COOKIE.as_str(),
        "proxy-authorization",
        "x-api-key",
        "x-auth-token",
        "x-csrf-token",
    ])
}

fn parse_redirect_policy(value: Option<&str>) -> Result<RedirectPolicyMode, String> {
    match value.unwrap_or("follow") {
        "follow" => Ok(RedirectPolicyMode::Follow),
        "manual" => Ok(RedirectPolicyMode::Manual),
        "error" => Ok(RedirectPolicyMode::Error),
        other => Err(format!(
            "Unsupported redirect_policy: {other}. Use follow, manual, or error."
        )),
    }
}

impl RedirectPolicyMode {
    fn as_str(self) -> &'static str {
        match self {
            RedirectPolicyMode::Follow => "follow",
            RedirectPolicyMode::Manual => "manual",
            RedirectPolicyMode::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct SessionUpdate {
    cookie_header: Option<String>,
}

fn build_session_update(
    headers: &HeaderMap,
    existing_cookie_header: &Option<String>,
) -> Option<SessionUpdate> {
    let mut jar = CookieJar::from_cookie_header(existing_cookie_header.as_deref());
    let mut changed = false;

    for value in headers.get_all("set-cookie") {
        if let Ok(set_cookie) = value.to_str() {
            if jar.apply_set_cookie(set_cookie) {
                changed = true;
            }
        }
    }

    if changed || existing_cookie_header.is_some() {
        Some(SessionUpdate {
            cookie_header: jar.to_cookie_header(),
        })
    } else {
        None
    }
}

#[derive(Clone, Debug, Default)]
struct CookieJar {
    cookies: HashMap<String, String>,
}

impl CookieJar {
    fn from_cookie_header(header: Option<&str>) -> Self {
        let mut jar = Self::default();
        if let Some(value) = header {
            for chunk in value.split(';') {
                let trimmed = chunk.trim();
                if let Some((name, value)) = trimmed.split_once('=') {
                    let name = name.trim();
                    if !name.is_empty() {
                        jar.cookies
                            .insert(name.to_string(), value.trim().to_string());
                    }
                }
            }
        }
        jar
    }

    fn apply_set_cookie(&mut self, set_cookie: &str) -> bool {
        let Some(first) = set_cookie.split(';').next() else {
            return false;
        };
        let Some((name, value)) = first.split_once('=') else {
            return false;
        };
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let value = value.trim();
        if value.is_empty() {
            self.cookies.remove(name).is_some()
        } else {
            self.cookies.insert(name.to_string(), value.to_string());
            true
        }
    }

    fn to_cookie_header(&self) -> Option<String> {
        if self.cookies.is_empty() {
            return None;
        }
        let mut pairs: Vec<_> = self
            .cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        pairs.sort();
        Some(pairs.join("; "))
    }
}

/// Truncate a string to at most `max_bytes` without splitting a UTF-8 codepoint.
fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[async_trait]
impl ToolExecutor for WebFetchToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "web_fetch" => self.handle(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

/// Return the `ToolDefinition` for registration in the tool registry.
pub fn web_fetch_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "web_fetch".into(),
        description: "Fetch content from a URL via HTTP GET or POST. Supports optional session reuse, cookie persistence, and configurable redirect handling. Returns status code, selected headers, and response body (truncated to 50KB for LLM context).".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "method": {
                    "type": "string",
                    "description": "HTTP method: GET or POST (default: GET)",
                    "enum": ["GET", "POST"]
                },
                "headers": {
                    "type": "object",
                    "description": "Optional HTTP headers as key-value pairs",
                    "additionalProperties": { "type": "string" }
                },
                "body": {
                    "type": "string",
                    "description": "Optional request body (for POST requests)"
                },
                "session": {
                    "type": "string",
                    "description": "Optional session key. When provided, request headers and cookies can be reused across related fetches."
                },
                "persist_session": {
                    "type": "boolean",
                    "description": "Whether this request updates stored session state. Defaults to true when session is set, otherwise false."
                },
                "redirect_policy": {
                    "type": "string",
                    "description": "Redirect handling mode: follow (default), manual (return redirect response), or error (fail if redirect occurs).",
                    "enum": ["follow", "manual", "error"]
                }
            },
            "required": ["url"]
        }),
        category: rune_core::ToolCategory::External,
        requires_approval: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::LOCATION;
    use rune_core::ToolCallId;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_call(args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "web_fetch".into(),
            arguments: args,
        }
    }

    #[test]
    fn truncate_utf8_ascii() {
        assert_eq!(truncate_utf8("hello world", 5), "hello");
    }

    #[test]
    fn truncate_utf8_multibyte() {
        // '€' is 3 bytes (E2 82 AC)
        let s = "a€b";
        // at max_bytes=2, we can't fit '€' so we get just "a"
        assert_eq!(truncate_utf8(s, 2), "a");
        // at max_bytes=4, we get "a€"
        assert_eq!(truncate_utf8(s, 4), "a€");
    }

    #[test]
    fn truncate_utf8_no_truncation() {
        assert_eq!(truncate_utf8("short", 100), "short");
    }

    #[test]
    fn definition_schema_has_required_url() {
        let def = web_fetch_tool_definition();
        assert_eq!(def.name, "web_fetch");
        let required = def.parameters["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("url")));
        assert!(def.parameters["properties"].get("session").is_some());
        assert!(
            def.parameters["properties"]
                .get("redirect_policy")
                .is_some()
        );
    }

    #[tokio::test]
    async fn missing_url_returns_error() {
        let exec = WebFetchToolExecutor::new().unwrap();
        let call = make_call(serde_json::json!({}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("missing required field: url"));
    }

    #[tokio::test]
    async fn unsupported_method_returns_error_result() {
        let exec = WebFetchToolExecutor::new().unwrap();
        let call = make_call(serde_json::json!({"url": "http://example.com", "method": "DELETE"}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("Unsupported HTTP method"));
    }

    #[tokio::test]
    async fn unknown_tool_name_rejected() {
        let exec = WebFetchToolExecutor::new().unwrap();
        let call = ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "not_web_fetch".into(),
            arguments: serde_json::json!({}),
        };
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[test]
    fn build_request_headers_redacts_sensitive_values() {
        let raw_headers = HashMap::from([
            ("authorization".to_string(), "Bearer secret".to_string()),
            ("x-trace-id".to_string(), "trace-123".to_string()),
        ]);

        let (request_headers, display_headers) = build_request_headers(&raw_headers).unwrap();

        assert_eq!(
            request_headers.get("authorization").unwrap(),
            &HeaderValue::from_static("Bearer secret")
        );
        assert!(display_headers.contains(&"authorization: <redacted>".to_string()));
        assert!(display_headers.contains(&"x-trace-id: trace-123".to_string()));
    }

    #[test]
    fn build_request_headers_rejects_invalid_header_name() {
        let raw_headers = HashMap::from([("bad header".to_string(), "value".to_string())]);

        let error = build_request_headers(&raw_headers).unwrap_err();

        assert!(error.contains("Invalid header name"));
    }

    #[test]
    fn parse_redirect_policy_rejects_invalid_value() {
        let error = parse_redirect_policy(Some("sideways")).unwrap_err();
        assert!(error.contains("Unsupported redirect_policy"));
    }

    #[test]
    fn cookie_jar_merges_and_serializes_cookie_header() {
        let mut jar = CookieJar::from_cookie_header(Some("a=1; b=2"));
        assert!(jar.apply_set_cookie("b=3; Path=/; HttpOnly"));
        assert!(jar.apply_set_cookie("c=4; Secure"));
        assert_eq!(jar.to_cookie_header().as_deref(), Some("a=1; b=3; c=4"));
    }

    #[tokio::test]
    async fn manual_redirect_returns_location_without_following() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header(LOCATION.as_str(), "/final")
                    .set_body_string("redirecting"),
            )
            .mount(&server)
            .await;

        let exec = WebFetchToolExecutor::new().unwrap();
        let call = make_call(serde_json::json!({
            "url": format!("{}/redirect", server.uri()),
            "redirect_policy": "manual"
        }));
        let result = exec.execute(call).await.unwrap();

        assert!(!result.is_error);
        assert!(result.output.contains("HTTP 302"));
        assert!(result.output.contains("Redirect policy: manual"));
        assert!(result.output.contains("location: /final"));
    }

    #[tokio::test]
    async fn session_persists_cookie_across_requests() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/login"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("set-cookie", "session_id=abc123; Path=/; HttpOnly")
                    .set_body_string("logged in"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/me"))
            .and(header("cookie", "session_id=abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        let exec = WebFetchToolExecutor::new().unwrap();
        let login = make_call(serde_json::json!({
            "url": format!("{}/login", server.uri()),
            "session": "acct"
        }));
        let login_result = exec.execute(login).await.unwrap();
        assert!(!login_result.is_error);
        assert!(login_result.output.contains("Session: acct (persisted)"));

        let me = make_call(serde_json::json!({
            "url": format!("{}/me", server.uri()),
            "session": "acct"
        }));
        let me_result = exec.execute(me).await.unwrap();
        assert!(!me_result.is_error);
        assert!(me_result.output.contains("HTTP 200"));
    }
}
