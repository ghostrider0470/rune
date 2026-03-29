use std::sync::{LazyLock, Mutex, MutexGuard};

use rune_config::{ConfiguredModel, ModelProviderConfig, ModelsConfig};
use rune_models::{
    AnthropicProvider, AzureFoundryProvider, AzureOpenAiProvider, ChatMessage, CompletionRequest,
    FinishReason, FunctionDefinition, GoogleProvider, ImageUrlPart, MessagePart, ModelError,
    ModelProvider, OpenAiProvider, Role, RoutedModelProvider, StreamEvent, ToolDefinition,
    provider_from_config,
};
use wiremock::matchers::{body_partial_json, header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn simple_request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some("Hello".into()),
            content_parts: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }],
        stable_prefix_messages: None,
        stable_prefix_tools: None,
        model: Some("gpt-4o".into()),
        temperature: None,
        max_tokens: None,
        tools: None,
    }
}

fn success_body() -> serde_json::Value {
    serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "Hello! How can I help?"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 8,
            "total_tokens": 18
        }
    })
}

#[tokio::test]
async fn google_complete_stream_passthroughs_openai_compatible_sse() {
    let server = MockServer::start().await;
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n\n",
        "data: [DONE]\n\n"
    );

    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let provider = GoogleProvider::with_base_url(&server.uri(), "test-key");
    let mut request = simple_request();
    request.model = Some("gemini-2.5-pro".into());

    let mut stream = provider.complete_stream(&request).await.unwrap();
    let mut events = Vec::new();
    while let Some(event) = stream.recv().await {
        events.push(event);
    }

    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0], StreamEvent::TextDelta(delta) if delta == "Hello"));
    assert!(matches!(&events[1], StreamEvent::TextDelta(delta) if delta == " world"));
    match &events[2] {
        StreamEvent::Done(response) => {
            assert_eq!(response.content.as_deref(), Some("Hello world"));
            assert_eq!(response.finish_reason, Some(FinishReason::Stop));
            assert_eq!(response.usage.total_tokens, 5);
        }
        other => panic!("expected done event, got {other:?}"),
    }

    let requests = server.received_requests().await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        payload.get("model"),
        Some(&serde_json::json!("gemini-2.5-pro"))
    );
    assert_eq!(payload.get("stream"), Some(&serde_json::json!(true)));
    assert_eq!(
        payload.get("stream_options"),
        Some(&serde_json::json!({"include_usage": true}))
    );
}

// --- Azure URL construction ---

#[test]
fn azure_url_standard() {
    let p = AzureOpenAiProvider::new(
        "https://myres.openai.azure.com",
        "gpt-4o",
        "2024-06-01",
        "k",
    );
    assert_eq!(
        p.url(),
        "https://myres.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-06-01"
    );
}

#[test]
fn azure_url_strips_trailing_slash() {
    let p = AzureOpenAiProvider::new("https://myres.openai.azure.com/", "dep", "2025-01-01", "k");
    assert!(
        p.url()
            .starts_with("https://myres.openai.azure.com/openai/")
    );
}

// --- Azure header handling ---

#[tokio::test]
async fn azure_sends_api_key_header() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(header("api-key", "test-secret-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "dep", "2024-06-01", "test-secret-key");
    let resp = p.complete(&simple_request()).await.unwrap();
    assert_eq!(resp.content.as_deref(), Some("Hello! How can I help?"));
    assert_eq!(resp.finish_reason, Some(FinishReason::Stop));
    assert_eq!(resp.usage.total_tokens, 18);
}

// --- Azure request body golden tests ---

/// Azure OpenAI must NOT send the `model` field — the deployment name in the URL
/// identifies the model.  This contrasts with vanilla OpenAI which always sends `model`.
#[tokio::test]
async fn azure_body_omits_model_field() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "gpt-4o", "2024-06-01", "k");
    let _ = p.complete(&simple_request()).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(
        body.get("model").is_none(),
        "Azure OpenAI body must NOT contain 'model' — deployment is in the URL. Got: {body}"
    );
}

/// Azure OpenAI uses `max_tokens`, not `max_completion_tokens`.
#[tokio::test]
async fn azure_body_uses_max_tokens_not_max_completion_tokens() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "gpt-4o", "2024-06-01", "k");
    let mut request = simple_request();
    request.max_tokens = Some(512);
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        body.get("max_tokens"),
        Some(&serde_json::json!(512)),
        "Azure body should use 'max_tokens'"
    );
    assert!(
        body.get("max_completion_tokens").is_none(),
        "Azure body must NOT contain 'max_completion_tokens'"
    );
}

/// When max_tokens is None, the field should be omitted entirely.
#[tokio::test]
async fn azure_body_omits_max_tokens_when_none() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "gpt-4o", "2024-06-01", "k");
    let mut request = simple_request();
    request.max_tokens = None;
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(
        body.get("max_tokens").is_none(),
        "Azure body should omit 'max_tokens' when not set"
    );
}

/// Azure request body forwards tools correctly.
#[tokio::test]
async fn azure_body_includes_tools_when_provided() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "gpt-4o", "2024-06-01", "k");
    let mut request = simple_request();
    request.tools = Some(vec![ToolDefinition {
        tool_type: "function".into(),
        function: FunctionDefinition {
            name: "get_weather".into(),
            description: "Get current weather".into(),
            parameters: serde_json::json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        },
    }]);
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let tools = body
        .get("tools")
        .expect("Azure body should include 'tools'");
    assert!(tools.is_array());
    assert_eq!(tools.as_array().unwrap().len(), 1);
    assert_eq!(tools[0]["function"]["name"], "get_weather");
}

/// Azure request body omits tools when None.
#[tokio::test]
async fn azure_body_omits_tools_when_none() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "gpt-4o", "2024-06-01", "k");
    let mut request = simple_request();
    request.tools = None;
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(
        body.get("tools").is_none(),
        "Azure body should omit 'tools' when not set"
    );
}

/// Golden test: full Azure request shape with all fields populated.
#[tokio::test]
async fn azure_request_golden_shape_full() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "gpt-4o-prod", "2024-06-01", "secret-key");
    let request = CompletionRequest {
        messages: vec![
            ChatMessage {
                role: Role::System,
                content: Some("You are helpful.".into()),
                content_parts: None,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: Role::User,
                content: Some("Hello".into()),
                content_parts: None,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ],
        stable_prefix_tools: None,
        stable_prefix_messages: Some(vec![ChatMessage {
            role: Role::System,
            content: Some("You are helpful.".into()),
            content_parts: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }]),
        model: Some("gpt-4o".into()), // should NOT appear in Azure body
        temperature: Some(0.7),
        max_tokens: Some(1024),
        tools: None,
    };
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let req = &requests[0];

    // Header assertions
    let api_key_header = req
        .headers
        .get("api-key")
        .expect("Azure must send api-key header");
    assert_eq!(api_key_header.to_str().unwrap(), "secret-key");
    assert!(
        req.headers.get("authorization").is_none(),
        "Azure must NOT send Authorization header"
    );

    // URL path assertion
    assert!(
        req.url.path().contains("/openai/deployments/gpt-4o-prod/"),
        "URL must contain deployment name in path: {}",
        req.url
    );
    assert!(
        req.url
            .query()
            .unwrap_or("")
            .contains("api-version=2024-06-01"),
        "URL must contain api-version query param: {}",
        req.url
    );

    // Body assertions
    let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();

    // Must NOT have model
    assert!(body.get("model").is_none(), "Azure body must omit 'model'");

    // Must have messages
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[2]["role"], "assistant");

    // Must have temperature and max_tokens
    assert_eq!(body["temperature"], serde_json::json!(0.7));
    assert_eq!(body["max_tokens"], serde_json::json!(1024));

    // Only expected keys present
    let keys: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    for key in &keys {
        assert!(
            ["messages", "temperature", "max_tokens", "tools"].contains(key),
            "unexpected key '{key}' in Azure request body"
        );
    }
}

/// Contrast test: OpenAI sends model + max_completion_tokens while Azure sends neither.
#[tokio::test]
async fn azure_vs_openai_body_shape_contrast() {
    let azure_server = MockServer::start().await;
    let openai_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&azure_server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&openai_server)
        .await;

    let azure = AzureOpenAiProvider::new(&azure_server.uri(), "dep", "2024-06-01", "k");
    let openai = OpenAiProvider::new(&openai_server.uri(), "k");

    let mut request = simple_request();
    request.max_tokens = Some(256);

    let _ = azure.complete(&request).await.unwrap();
    let _ = openai.complete(&request).await.unwrap();

    let azure_reqs = azure_server.received_requests().await.unwrap();
    let openai_reqs = openai_server.received_requests().await.unwrap();

    let azure_body: serde_json::Value = serde_json::from_slice(&azure_reqs[0].body).unwrap();
    let openai_body: serde_json::Value = serde_json::from_slice(&openai_reqs[0].body).unwrap();

    // Azure: no model, max_tokens
    assert!(azure_body.get("model").is_none(), "Azure must omit model");
    assert_eq!(azure_body["max_tokens"], 256);
    assert!(azure_body.get("max_completion_tokens").is_none());

    // OpenAI: has model, max_completion_tokens
    assert!(
        openai_body.get("model").is_some(),
        "OpenAI must include model"
    );
    assert_eq!(openai_body["max_completion_tokens"], 256);
    assert!(openai_body.get("max_tokens").is_none());
}

// --- OpenAI header handling ---

#[tokio::test]
async fn openai_sends_bearer_token() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(header("Authorization", "Bearer my-openai-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(&server.uri(), "my-openai-key");
    let resp = p.complete(&simple_request()).await.unwrap();
    assert_eq!(resp.content.as_deref(), Some("Hello! How can I help?"));
}

#[tokio::test]
async fn openai_azure_mode_sends_api_key_header() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(header("api-key", "azure-openai-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = OpenAiProvider::azure(&server.uri(), "azure-openai-key");
    let resp = p.complete(&simple_request()).await.unwrap();
    assert_eq!(resp.content.as_deref(), Some("Hello! How can I help?"));
}

#[tokio::test]
async fn openai_uses_max_completion_tokens_field() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(&server.uri(), "my-openai-key");
    let mut request = simple_request();
    request.max_tokens = Some(123);

    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        body.get("max_completion_tokens"),
        Some(&serde_json::json!(123))
    );
    assert!(body.get("max_tokens").is_none());
}

// --- Error mapping ---

#[tokio::test]
async fn maps_401_to_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": { "message": "Invalid key", "code": "invalid_api_key" }
        })))
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(&server.uri(), "bad");
    let err = p.complete(&simple_request()).await.unwrap_err();
    assert!(matches!(err, ModelError::Auth(_)));
}

#[tokio::test]
async fn maps_429_to_rate_limited() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "30")
                .set_body_json(serde_json::json!({
                    "error": { "message": "Too many requests", "code": "rate_limit" }
                })),
        )
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(&server.uri(), "k");
    let err = p.complete(&simple_request()).await.unwrap_err();
    match err {
        ModelError::RateLimited {
            retry_after_secs, ..
        } => assert_eq!(retry_after_secs, Some(30)),
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

#[tokio::test]
async fn maps_500_to_transient() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "d", "v", "k");
    let err = p.complete(&simple_request()).await.unwrap_err();
    assert!(matches!(err, ModelError::Transient(_)));
}

#[tokio::test]
async fn maps_context_length_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": { "message": "context_length_exceeded", "code": "context_length_exceeded" }
        })))
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(&server.uri(), "k");
    let err = p.complete(&simple_request()).await.unwrap_err();
    assert!(matches!(err, ModelError::ContextLengthExceeded(_)));
}

// --- Azure-specific error mapping ---

#[tokio::test]
async fn maps_azure_unsupported_api_version_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": {
                "code": "InvalidApiVersionIdentifier",
                "message": "The api-version '9999-01-01' is invalid."
            }
        })))
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "dep", "9999-01-01", "k");
    let err = p.complete(&simple_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::UnsupportedApiVersion(_)),
        "expected UnsupportedApiVersion, got {err:?}"
    );
}

#[tokio::test]
async fn maps_azure_unsupported_api_version_from_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": {
                "code": "BadRequest",
                "message": "The API version '0000-01-01' is not supported by this resource."
            }
        })))
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "dep", "0000-01-01", "k");
    let err = p.complete(&simple_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::UnsupportedApiVersion(_)),
        "expected UnsupportedApiVersion, got {err:?}"
    );
}

#[tokio::test]
async fn maps_azure_deployment_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": {
                "code": "DeploymentNotFound",
                "message": "The API deployment for this resource does not exist."
            }
        })))
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "bad-dep", "2024-06-01", "k");
    let err = p.complete(&simple_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::DeploymentNotFound(_)),
        "expected DeploymentNotFound, got {err:?}"
    );
}

// --- Tool calls in response ---

#[tokio::test]
async fn parses_tool_calls() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\": \"/tmp/test\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8 }
        })))
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(&server.uri(), "k");
    let resp = p.complete(&simple_request()).await.unwrap();
    assert_eq!(resp.finish_reason, Some(FinishReason::ToolCalls));
    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].function.name, "read_file");
}

// --- Cached prompt token extraction ---

#[tokio::test]
async fn openai_response_extracts_cached_prompt_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "Hi"}, "finish_reason": "stop"}],
            "usage": {
                "prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120,
                "prompt_tokens_details": {"cached_tokens": 80}
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(&server.uri(), "k");
    let resp = p.complete(&simple_request()).await.unwrap();
    assert_eq!(resp.usage.cached_prompt_tokens, Some(80));
    assert_eq!(resp.usage.uncached_prompt_tokens, Some(20));
}

#[tokio::test]
async fn openai_response_no_cached_details_returns_none() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(&server.uri(), "k");
    let resp = p.complete(&simple_request()).await.unwrap();
    assert_eq!(resp.usage.cached_prompt_tokens, None);
    assert_eq!(resp.usage.uncached_prompt_tokens, None);
}

// --- Provider selection from config ---

#[test]
fn selects_azure_provider() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_AZURE_KEY_SEL", "fake") };
    let cfg = ModelProviderConfig {
        name: "azure".into(),
        kind: "azure-openai".into(),
        base_url: "https://test.openai.azure.com".into(),
        deployment_name: Some("gpt-4o".into()),
        api_version: Some("2024-06-01".into()),
        api_key_env: Some("TEST_AZURE_KEY_SEL".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    // We can't downcast easily, but we can verify it works by checking it's Send+Sync
    let _: Box<dyn ModelProvider> = provider;
    unsafe { std::env::remove_var("TEST_AZURE_KEY_SEL") };
}

#[test]
fn selects_openai_provider() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_OAI_KEY_SEL", "fake") };
    let cfg = ModelProviderConfig {
        name: "openai".into(),
        kind: "openai".into(),
        base_url: "https://api.openai.com/v1".into(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_OAI_KEY_SEL".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
    unsafe { std::env::remove_var("TEST_OAI_KEY_SEL") };
}

#[test]
fn selects_google_provider() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_GOOGLE_KEY_SEL", "fake") };
    let cfg = ModelProviderConfig {
        name: "google".into(),
        kind: "google".into(),
        base_url: String::new(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_GOOGLE_KEY_SEL".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
    unsafe { std::env::remove_var("TEST_GOOGLE_KEY_SEL") };
}

#[test]
fn selects_azure_foundry_provider_via_primary_kind() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_FOUNDRY_KEY_SEL", "fake") };
    let cfg = ModelProviderConfig {
        name: "foundry".into(),
        kind: "azure-foundry".into(),
        base_url: "https://foundry.services.ai.azure.com".into(),
        deployment_name: None,
        api_version: Some("2024-05-01-preview".into()),
        api_key_env: Some("TEST_FOUNDRY_KEY_SEL".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
    unsafe { std::env::remove_var("TEST_FOUNDRY_KEY_SEL") };
}

#[test]
fn selects_azure_foundry_provider_via_alias_kind() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_FOUNDRY_ALIAS_KEY_SEL", "fake") };
    let cfg = ModelProviderConfig {
        name: "azure-ai".into(),
        kind: "azure-ai".into(),
        base_url: "https://foundry.services.ai.azure.com".into(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_FOUNDRY_ALIAS_KEY_SEL".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
    unsafe { std::env::remove_var("TEST_FOUNDRY_ALIAS_KEY_SEL") };
}

#[test]
fn selects_anthropic_provider_via_azure_alias_kind() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_ANTHROPIC_AZURE_KEY_SEL", "fake") };
    let cfg = ModelProviderConfig {
        name: "anthropic-azure".into(),
        kind: "anthropic_azure".into(),
        base_url: "https://my-anthropic.services.ai.azure.com".into(),
        deployment_name: None,
        api_version: Some("2023-06-01".into()),
        api_key_env: Some("TEST_ANTHROPIC_AZURE_KEY_SEL".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
    unsafe { std::env::remove_var("TEST_ANTHROPIC_AZURE_KEY_SEL") };
}

#[test]
fn selects_ollama_provider_without_api_key() {
    let cfg = ModelProviderConfig {
        name: "ollama".into(),
        kind: "ollama".into(),
        base_url: String::new(),
        deployment_name: None,
        api_version: None,
        api_key_env: None,
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
}

#[test]
fn selects_groq_provider() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_GROQ_KEY_SEL", "fake") };
    let cfg = ModelProviderConfig {
        name: "groq".into(),
        kind: "groq".into(),
        base_url: String::new(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_GROQ_KEY_SEL".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
    unsafe { std::env::remove_var("TEST_GROQ_KEY_SEL") };
}

#[test]
fn selects_deepseek_provider() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_DEEPSEEK_KEY_SEL", "fake") };
    let cfg = ModelProviderConfig {
        name: "deepseek".into(),
        kind: "deepseek".into(),
        base_url: String::new(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_DEEPSEEK_KEY_SEL".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
    unsafe { std::env::remove_var("TEST_DEEPSEEK_KEY_SEL") };
}

#[test]
fn selects_mistral_provider() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_MISTRAL_KEY_SEL", "fake") };
    let cfg = ModelProviderConfig {
        name: "mistral".into(),
        kind: "mistral".into(),
        base_url: String::new(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_MISTRAL_KEY_SEL".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
    unsafe { std::env::remove_var("TEST_MISTRAL_KEY_SEL") };
}

#[test]
fn selects_bedrock_provider_with_inline_credentials() {
    let cfg = ModelProviderConfig {
        name: "bedrock".into(),
        kind: "bedrock".into(),
        base_url: String::new(),
        deployment_name: Some("us-east-1".into()),
        api_version: None,
        api_key_env: None,
        api_key: Some("AKIA_TEST:SECRET_TEST".into()),
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
}

#[test]
fn selects_bedrock_provider_with_env_credentials_pair() {
    let _guard = lock_env();
    unsafe {
        std::env::set_var("TEST_BEDROCK_KEY_SEL", "ENV_AKIA:ENV_SECRET");
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_SECRET_ACCESS_KEY");
    }
    let cfg = ModelProviderConfig {
        name: "bedrock".into(),
        kind: "bedrock".into(),
        base_url: String::new(),
        deployment_name: Some("us-west-2".into()),
        api_version: None,
        api_key_env: Some("TEST_BEDROCK_KEY_SEL".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
    unsafe {
        std::env::remove_var("TEST_BEDROCK_KEY_SEL");
    }
}

#[test]
fn selects_bedrock_provider_via_aws_bedrock_alias() {
    let cfg = ModelProviderConfig {
        name: "bedrock".into(),
        kind: "aws-bedrock".into(),
        base_url: String::new(),
        deployment_name: Some("eu-west-1".into()),
        api_version: None,
        api_key_env: None,
        api_key: Some("AKIA_ALIAS:SECRET_ALIAS".into()),
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
}

#[test]
fn selects_bedrock_provider_with_standard_aws_env_vars() {
    let _guard = lock_env();
    unsafe {
        std::env::remove_var("TEST_BEDROCK_STD_ENV");
        std::env::set_var("AWS_ACCESS_KEY_ID", "ENV_AWS_ACCESS");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "ENV_AWS_SECRET");
        std::env::set_var("AWS_REGION", "ap-southeast-2");
    }
    let cfg = ModelProviderConfig {
        name: "bedrock".into(),
        kind: "bedrock".into(),
        base_url: String::new(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_BEDROCK_STD_ENV".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let debug = format!("{provider:?}");
    assert!(debug.contains("BedrockProvider"));
    let concrete = rune_models::BedrockProvider::new("", "ENV_AWS_ACCESS", "ENV_AWS_SECRET", None);
    assert_eq!(concrete.region(), "ap-southeast-2");
    unsafe {
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_SECRET_ACCESS_KEY");
        std::env::remove_var("AWS_REGION");
    }
}

#[test]
fn selects_bedrock_provider_with_aws_default_region_fallback() {
    let _guard = lock_env();
    unsafe {
        std::env::remove_var("TEST_BEDROCK_DEFAULT_REGION_ENV");
        std::env::remove_var("AWS_REGION");
        std::env::set_var("AWS_ACCESS_KEY_ID", "ENV_AWS_ACCESS_DEFAULT");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "ENV_AWS_SECRET_DEFAULT");
        std::env::set_var("AWS_DEFAULT_REGION", "eu-central-1");
    }
    let cfg = ModelProviderConfig {
        name: "bedrock".into(),
        kind: "bedrock".into(),
        base_url: String::new(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_BEDROCK_DEFAULT_REGION_ENV".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let debug = format!("{provider:?}");
    assert!(debug.contains("BedrockProvider"));
    let concrete = rune_models::BedrockProvider::new(
        "",
        "ENV_AWS_ACCESS_DEFAULT",
        "ENV_AWS_SECRET_DEFAULT",
        None,
    );
    assert_eq!(concrete.region(), "eu-central-1");
    unsafe {
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_SECRET_ACCESS_KEY");
        std::env::remove_var("AWS_DEFAULT_REGION");
    }
}

#[test]
fn selects_bedrock_provider_with_default_region_when_no_region_is_configured() {
    let _guard = lock_env();
    unsafe {
        std::env::remove_var("TEST_BEDROCK_DEFAULT_REGION_NONE");
        std::env::remove_var("AWS_REGION");
        std::env::remove_var("AWS_DEFAULT_REGION");
        std::env::set_var("AWS_ACCESS_KEY_ID", "ENV_AWS_ACCESS_FALLBACK");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "ENV_AWS_SECRET_FALLBACK");
    }
    let cfg = ModelProviderConfig {
        name: "bedrock".into(),
        kind: "bedrock".into(),
        base_url: String::new(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_BEDROCK_DEFAULT_REGION_NONE".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let debug = format!("{provider:?}");
    assert!(debug.contains("BedrockProvider"));
    let concrete = rune_models::BedrockProvider::new(
        "",
        "ENV_AWS_ACCESS_FALLBACK",
        "ENV_AWS_SECRET_FALLBACK",
        None,
    );
    assert_eq!(concrete.region(), "us-east-1");
    unsafe {
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_SECRET_ACCESS_KEY");
    }
}

#[test]
fn falls_back_to_openai_for_unknown_provider_kind() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_FALLBACK_OAI_KEY", "fallback-key") };
    let cfg = ModelProviderConfig {
        name: "custom".into(),
        kind: "custom-openai-compatible".into(),
        base_url: "https://api.example.com/v1".into(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_FALLBACK_OAI_KEY".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let debug = format!("{provider:?}");
    assert!(debug.contains("OpenAiProvider"));
    assert!(debug.contains("https://api.example.com/v1"));
    unsafe { std::env::remove_var("TEST_FALLBACK_OAI_KEY") };
}

#[test]
fn empty_kind_uses_provider_name_for_google_selection() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_EMPTY_KIND_GOOGLE", "google-key") };
    let cfg = ModelProviderConfig {
        name: "google".into(),
        kind: String::new(),
        base_url: String::new(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_EMPTY_KIND_GOOGLE".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let provider = provider_from_config(&cfg).unwrap();
    let debug = format!("{provider:?}");
    assert!(debug.contains("GoogleProvider"));
    unsafe { std::env::remove_var("TEST_EMPTY_KIND_GOOGLE") };
}

#[test]
fn missing_api_key_uses_openai_api_key_env_by_default() {
    let _guard = lock_env();
    unsafe {
        std::env::remove_var("OPENAI_API_KEY");
    }
    let cfg = ModelProviderConfig {
        name: "openai".into(),
        kind: "openai".into(),
        base_url: "https://api.openai.com/v1".into(),
        deployment_name: None,
        api_version: None,
        api_key_env: None,
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let err = provider_from_config(&cfg).unwrap_err();
    match err {
        ModelError::Auth(message) => assert!(message.contains("OPENAI_API_KEY"), "{message}"),
        other => panic!("expected auth error, got {other:?}"),
    }
}

#[test]
fn bedrock_requires_credentials() {
    let _guard = lock_env();
    unsafe {
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_SECRET_ACCESS_KEY");
        std::env::remove_var("AWS_REGION");
        std::env::remove_var("AWS_DEFAULT_REGION");
    }
    let cfg = ModelProviderConfig {
        name: "bedrock".into(),
        kind: "bedrock".into(),
        base_url: String::new(),
        deployment_name: Some("us-east-1".into()),
        api_version: None,
        api_key_env: None,
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let err = provider_from_config(&cfg).unwrap_err();
    assert!(matches!(err, ModelError::Auth(_)));
}

#[test]
fn azure_requires_deployment_name() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_AZURE_KEY_DEP", "fake") };
    let cfg = ModelProviderConfig {
        name: "azure".into(),
        kind: "azure-openai".into(),
        base_url: "https://test.openai.azure.com".into(),
        deployment_name: None,
        api_version: Some("2024-06-01".into()),
        api_key_env: Some("TEST_AZURE_KEY_DEP".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let err = provider_from_config(&cfg).unwrap_err();
    assert!(matches!(err, ModelError::Configuration(_)));
    unsafe { std::env::remove_var("TEST_AZURE_KEY_DEP") };
}

#[test]
fn azure_requires_api_version() {
    let _guard = lock_env();
    unsafe { std::env::set_var("TEST_AZURE_KEY_VER", "fake") };
    let cfg = ModelProviderConfig {
        name: "azure".into(),
        kind: "azure-openai".into(),
        base_url: "https://test.openai.azure.com".into(),
        deployment_name: Some("gpt-4o".into()),
        api_version: None,
        api_key_env: Some("TEST_AZURE_KEY_VER".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let err = provider_from_config(&cfg).unwrap_err();
    assert!(matches!(err, ModelError::Configuration(_)));
    unsafe { std::env::remove_var("TEST_AZURE_KEY_VER") };
}

#[test]
fn missing_api_key_env_returns_auth_error() {
    let cfg = ModelProviderConfig {
        name: "openai".into(),
        kind: "openai".into(),
        base_url: "https://api.openai.com/v1".into(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("DEFINITELY_NOT_SET_12345".into()),
        api_key: None,
        model_alias: None,
        models: vec![],
    };
    let err = provider_from_config(&cfg).unwrap_err();
    assert!(matches!(err, ModelError::Auth(_)));
}

#[tokio::test]
async fn routed_provider_dispatches_by_provider_model_id_and_strips_prefix() {
    let openai_server = MockServer::start().await;
    let codex_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "model": "gpt-5.4"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&openai_server)
        .await;

    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "model": "gpt-5.3-codex"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&codex_server)
        .await;

    {
        let _guard = lock_env();
        unsafe {
            std::env::set_var("TEST_ROUTED_OPENAI_KEY", "fake-openai");
            std::env::set_var("TEST_ROUTED_CODEX_KEY", "fake-codex");
        }
    }

    let models = ModelsConfig {
        default_model: Some("oc-01-openai/gpt-5.4".into()),
        default_image_model: None,
        fallbacks: vec![],
        image_fallbacks: vec![],
        auth_orders: vec![],
        providers: vec![
            ModelProviderConfig {
                name: "oc-01-openai".into(),
                kind: "openai".into(),
                base_url: openai_server.uri(),
                deployment_name: None,
                api_version: None,
                api_key_env: Some("TEST_ROUTED_OPENAI_KEY".into()),
                api_key: None,
                model_alias: None,
                models: vec![ConfiguredModel::Id("gpt-5.4".into())],
            },
            ModelProviderConfig {
                name: "openai-codex".into(),
                kind: "openai".into(),
                base_url: codex_server.uri(),
                deployment_name: None,
                api_version: None,
                api_key_env: Some("TEST_ROUTED_CODEX_KEY".into()),
                api_key: None,
                model_alias: None,
                models: vec![ConfiguredModel::Id("gpt-5.3-codex".into())],
            },
        ],
    };

    let provider = RoutedModelProvider::from_models_config(&models).unwrap();

    let codex_request = CompletionRequest {
        model: Some("openai-codex/gpt-5.3-codex".into()),
        ..simple_request()
    };
    provider.complete(&codex_request).await.unwrap();

    let openai_request = CompletionRequest {
        model: Some("oc-01-openai/gpt-5.4".into()),
        ..simple_request()
    };
    provider.complete(&openai_request).await.unwrap();

    {
        let _guard = lock_env();
        unsafe {
            std::env::remove_var("TEST_ROUTED_OPENAI_KEY");
            std::env::remove_var("TEST_ROUTED_CODEX_KEY");
        }
    }
}

// --- Fallback chain routing ---

use rune_config::ModelFallbackChainConfig;

#[tokio::test]
async fn routed_provider_falls_back_on_retriable_error() {
    let primary_server = MockServer::start().await;
    let fallback_server = MockServer::start().await;

    // Primary returns 429 (retriable).
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "60")
                .set_body_json(serde_json::json!({
                    "error": { "message": "rate limited", "code": "rate_limit" }
                })),
        )
        .expect(4)
        .mount(&primary_server)
        .await;

    // Fallback succeeds.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&fallback_server)
        .await;

    {
        let _guard = lock_env();
        unsafe {
            std::env::set_var("TEST_FALLBACK_PRIMARY_KEY", "fake-primary");
            std::env::set_var("TEST_FALLBACK_SECONDARY_KEY", "fake-secondary");
        }
    }

    let models = ModelsConfig {
        default_model: Some("primary/gpt-5.4".into()),
        default_image_model: None,
        fallbacks: vec![ModelFallbackChainConfig {
            name: "default-chat".into(),
            chain: vec!["primary/gpt-5.4".into(), "secondary/claude-opus-4-6".into()],
        }],
        image_fallbacks: vec![],
        auth_orders: vec![],
        providers: vec![
            ModelProviderConfig {
                name: "primary".into(),
                kind: "openai".into(),
                base_url: primary_server.uri(),
                deployment_name: None,
                api_version: None,
                api_key_env: Some("TEST_FALLBACK_PRIMARY_KEY".into()),
                api_key: None,
                model_alias: None,
                models: vec![ConfiguredModel::Id("gpt-5.4".into())],
            },
            ModelProviderConfig {
                name: "secondary".into(),
                kind: "openai".into(),
                base_url: fallback_server.uri(),
                deployment_name: None,
                api_version: None,
                api_key_env: Some("TEST_FALLBACK_SECONDARY_KEY".into()),
                api_key: None,
                model_alias: None,
                models: vec![ConfiguredModel::Id("claude-opus-4-6".into())],
            },
        ],
    };

    let provider = RoutedModelProvider::from_models_config(&models).unwrap();

    let request = CompletionRequest {
        model: Some("primary/gpt-5.4".into()),
        ..simple_request()
    };

    // Should succeed via fallback.
    let resp = provider.complete(&request).await.unwrap();
    assert_eq!(resp.content.as_deref(), Some("Hello! How can I help?"));

    {
        let _guard = lock_env();
        unsafe {
            std::env::remove_var("TEST_FALLBACK_PRIMARY_KEY");
            std::env::remove_var("TEST_FALLBACK_SECONDARY_KEY");
        }
    }
}

#[tokio::test]
async fn routed_provider_does_not_fallback_on_non_retriable_error() {
    let primary_server = MockServer::start().await;
    let fallback_server = MockServer::start().await;

    // Primary returns 401 (non-retriable).
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": { "message": "Invalid key", "code": "invalid_api_key" }
        })))
        .expect(4)
        .mount(&primary_server)
        .await;

    // Fallback should NOT be called.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(0)
        .mount(&fallback_server)
        .await;

    {
        let _guard = lock_env();
        unsafe {
            std::env::set_var("TEST_NOFB_PRIMARY_KEY", "fake-primary");
            std::env::set_var("TEST_NOFB_SECONDARY_KEY", "fake-secondary");
        }
    }

    let models = ModelsConfig {
        default_model: Some("primary/gpt-5.4".into()),
        default_image_model: None,
        fallbacks: vec![ModelFallbackChainConfig {
            name: "default-chat".into(),
            chain: vec!["primary/gpt-5.4".into(), "secondary/claude-opus-4-6".into()],
        }],
        image_fallbacks: vec![],
        auth_orders: vec![],
        providers: vec![
            ModelProviderConfig {
                name: "primary".into(),
                kind: "openai".into(),
                base_url: primary_server.uri(),
                deployment_name: None,
                api_version: None,
                api_key_env: Some("TEST_NOFB_PRIMARY_KEY".into()),
                api_key: None,
                model_alias: None,
                models: vec![ConfiguredModel::Id("gpt-5.4".into())],
            },
            ModelProviderConfig {
                name: "secondary".into(),
                kind: "openai".into(),
                base_url: fallback_server.uri(),
                deployment_name: None,
                api_version: None,
                api_key_env: Some("TEST_NOFB_SECONDARY_KEY".into()),
                api_key: None,
                model_alias: None,
                models: vec![ConfiguredModel::Id("claude-opus-4-6".into())],
            },
        ],
    };

    let provider = RoutedModelProvider::from_models_config(&models).unwrap();

    let request = CompletionRequest {
        model: Some("primary/gpt-5.4".into()),
        ..simple_request()
    };

    // Should fail immediately without trying fallback.
    let err = provider.complete(&request).await.unwrap_err();
    assert!(matches!(err, ModelError::Auth(_)));

    {
        let _guard = lock_env();
        unsafe {
            std::env::remove_var("TEST_NOFB_PRIMARY_KEY");
            std::env::remove_var("TEST_NOFB_SECONDARY_KEY");
        }
    }
}

#[tokio::test]
async fn routed_provider_returns_last_error_when_all_fallbacks_fail() {
    let primary_server = MockServer::start().await;
    let fallback_server = MockServer::start().await;

    // Primary returns 500 (retriable).
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .expect(1)
        .mount(&primary_server)
        .await;

    // Fallback also returns 500 (retriable).
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Also Down"))
        .expect(1)
        .mount(&fallback_server)
        .await;

    {
        let _guard = lock_env();
        unsafe {
            std::env::set_var("TEST_ALLFB_PRIMARY_KEY", "fake-primary");
            std::env::set_var("TEST_ALLFB_SECONDARY_KEY", "fake-secondary");
        }
    }

    let models = ModelsConfig {
        default_model: Some("primary/gpt-5.4".into()),
        default_image_model: None,
        fallbacks: vec![ModelFallbackChainConfig {
            name: "default-chat".into(),
            chain: vec!["primary/gpt-5.4".into(), "secondary/claude-opus-4-6".into()],
        }],
        image_fallbacks: vec![],
        auth_orders: vec![],
        providers: vec![
            ModelProviderConfig {
                name: "primary".into(),
                kind: "openai".into(),
                base_url: primary_server.uri(),
                deployment_name: None,
                api_version: None,
                api_key_env: Some("TEST_ALLFB_PRIMARY_KEY".into()),
                api_key: None,
                model_alias: None,
                models: vec![ConfiguredModel::Id("gpt-5.4".into())],
            },
            ModelProviderConfig {
                name: "secondary".into(),
                kind: "openai".into(),
                base_url: fallback_server.uri(),
                deployment_name: None,
                api_version: None,
                api_key_env: Some("TEST_ALLFB_SECONDARY_KEY".into()),
                api_key: None,
                model_alias: None,
                models: vec![ConfiguredModel::Id("claude-opus-4-6".into())],
            },
        ],
    };

    let provider = RoutedModelProvider::from_models_config(&models).unwrap();

    let request = CompletionRequest {
        model: Some("primary/gpt-5.4".into()),
        ..simple_request()
    };

    // Both fail — should get the last error (from fallback).
    let err = provider.complete(&request).await.unwrap_err();
    assert!(matches!(err, ModelError::Transient(_)));

    {
        let _guard = lock_env();
        unsafe {
            std::env::remove_var("TEST_ALLFB_PRIMARY_KEY");
            std::env::remove_var("TEST_ALLFB_SECONDARY_KEY");
        }
    }
}

#[tokio::test]
async fn routed_provider_skips_fallback_when_no_chain_configured() {
    let primary_server = MockServer::start().await;

    // Primary returns 500 (retriable) but there's no fallback chain.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .expect(1)
        .mount(&primary_server)
        .await;

    {
        let _guard = lock_env();
        unsafe {
            std::env::set_var("TEST_NOCHAIN_PRIMARY_KEY", "fake-primary");
        }
    }

    let models = ModelsConfig {
        default_model: Some("primary/gpt-5.4".into()),
        default_image_model: None,
        fallbacks: vec![], // No fallback chains.
        image_fallbacks: vec![],
        auth_orders: vec![],
        providers: vec![ModelProviderConfig {
            name: "primary".into(),
            kind: "openai".into(),
            base_url: primary_server.uri(),
            deployment_name: None,
            api_version: None,
            api_key_env: Some("TEST_NOCHAIN_PRIMARY_KEY".into()),
            api_key: None,
            model_alias: None,
            models: vec![ConfiguredModel::Id("gpt-5.4".into())],
        }],
    };

    let provider = RoutedModelProvider::from_models_config(&models).unwrap();

    let request = CompletionRequest {
        model: Some("primary/gpt-5.4".into()),
        ..simple_request()
    };

    let err = provider.complete(&request).await.unwrap_err();
    assert!(matches!(err, ModelError::Transient(_)));

    {
        let _guard = lock_env();
        unsafe {
            std::env::remove_var("TEST_NOCHAIN_PRIMARY_KEY");
        }
    }
}

// === Azure Foundry golden tests =============================================
//
// The Foundry provider routes by model family:
//   claude-* → /anthropic/v1/messages  (Anthropic Messages API)
//   *        → /openai/v1/chat/completions (OpenAI Chat Completions)
//
// These tests verify the exact URL, headers, and body shape for each path.

fn anthropic_success_body() -> serde_json::Value {
    serde_json::json!({
        "content": [{"type": "text", "text": "Hello from Claude!"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 12, "output_tokens": 7}
    })
}

// --- Foundry model-family routing ---

/// Non-claude model names must hit the OpenAI endpoint path.
#[tokio::test]
async fn foundry_routes_gpt_to_openai_endpoint() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "foundry-key");
    let request = CompletionRequest {
        model: Some("gpt-5.4".into()),
        ..simple_request()
    };
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .url
            .path()
            .contains("/openai/v1/chat/completions"),
        "GPT model should route to OpenAI path, got: {}",
        requests[0].url
    );
}

/// Claude model names must hit the Anthropic endpoint path.
#[tokio::test]
async fn foundry_routes_claude_to_anthropic_endpoint() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "foundry-key");
    let request = CompletionRequest {
        model: Some("claude-sonnet-4-5-20250514".into()),
        ..simple_request()
    };
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].url.path().contains("/anthropic/v1/messages"),
        "Claude model should route to Anthropic path, got: {}",
        requests[0].url
    );
}

/// Default model (no model specified) should route to OpenAI path.
#[tokio::test]
async fn foundry_default_model_routes_to_openai() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "foundry-key");
    let mut request = simple_request();
    request.model = None;
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    assert!(
        requests[0]
            .url
            .path()
            .contains("/openai/v1/chat/completions"),
        "Default (no model) should route to OpenAI path, got: {}",
        requests[0].url
    );
}

// --- Foundry OpenAI-path wire shape ---

/// Foundry OpenAI path: body INCLUDES model (unlike Azure OpenAI which omits it).
#[tokio::test]
async fn foundry_openai_body_includes_model() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "foundry-key");
    let request = CompletionRequest {
        model: Some("gpt-5.4".into()),
        ..simple_request()
    };
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        body.get("model"),
        Some(&serde_json::json!("gpt-5.4")),
        "Foundry OpenAI path must include 'model' in body (routing is by model field, not URL deployment)"
    );
}

/// Foundry OpenAI path uses `max_completion_tokens` (standard OpenAI convention).
#[tokio::test]
async fn foundry_openai_uses_max_completion_tokens() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "foundry-key");
    let mut request = simple_request();
    request.model = Some("gpt-5.4".into());
    request.max_tokens = Some(1024);
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        body.get("max_completion_tokens"),
        Some(&serde_json::json!(1024)),
        "Foundry OpenAI path should use 'max_completion_tokens'"
    );
    assert!(
        body.get("max_tokens").is_none(),
        "Foundry OpenAI path must NOT use 'max_tokens'"
    );
}

/// Golden: full Foundry OpenAI request shape with all fields populated.
#[tokio::test]
async fn foundry_openai_request_golden_shape() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::with_api_version(&server.uri(), "my-foundry-key", "2024-05-01");
    let request = CompletionRequest {
        stable_prefix_messages: None,
        stable_prefix_tools: None,
        messages: vec![
            ChatMessage {
                role: Role::System,
                content: Some("You are helpful.".into()),
                content_parts: None,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: Role::User,
                content: Some("What is Rust?".into()),
                content_parts: None,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ],
        model: Some("gpt-5.4".into()),
        temperature: Some(0.5),
        max_tokens: Some(2048),
        tools: Some(vec![ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDefinition {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: serde_json::json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            },
        }]),
    };
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let req = &requests[0];

    // URL: /openai/v1/chat/completions (no deployment path, no api-version query)
    assert!(
        req.url.path().contains("/openai/v1/chat/completions"),
        "Foundry OpenAI URL path: {}",
        req.url
    );

    // Auth: api-key header, NOT Bearer
    let api_key_header = req
        .headers
        .get("api-key")
        .expect("Foundry must send api-key header");
    assert_eq!(api_key_header.to_str().unwrap(), "my-foundry-key");
    assert!(
        req.headers.get("authorization").is_none(),
        "Foundry must NOT send Authorization header"
    );

    // Body
    let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();

    assert_eq!(body["model"], "gpt-5.4", "Must include model");
    assert_eq!(body["temperature"], 0.5);
    assert_eq!(body["max_completion_tokens"], 2048);

    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[1]["role"], "user");

    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["function"]["name"], "search");
}

// --- Foundry Anthropic-path wire shape ---

/// Foundry Anthropic path sends api-key AND anthropic-version headers.
#[tokio::test]
async fn foundry_anthropic_sends_correct_headers() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::with_api_version(&server.uri(), "my-key", "2023-06-01");
    let request = CompletionRequest {
        model: Some("claude-sonnet-4-5-20250514".into()),
        ..simple_request()
    };
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let req = &requests[0];

    let api_key = req
        .headers
        .get("api-key")
        .expect("Foundry Anthropic must send api-key header");
    assert_eq!(api_key.to_str().unwrap(), "my-key");

    let anthropic_version = req
        .headers
        .get("anthropic-version")
        .expect("Foundry Anthropic must send anthropic-version header");
    assert_eq!(anthropic_version.to_str().unwrap(), "2023-06-01");
}

/// Foundry Anthropic path extracts system message from messages array.
#[tokio::test]
async fn foundry_anthropic_extracts_system_message() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let request = CompletionRequest {
        stable_prefix_messages: None,
        stable_prefix_tools: None,
        messages: vec![
            ChatMessage {
                role: Role::System,
                content: Some("You are a helpful assistant.".into()),
                content_parts: None,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: Role::User,
                content: Some("Hi".into()),
                content_parts: None,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ],
        model: Some("claude-sonnet-4-5-20250514".into()),
        temperature: None,
        max_tokens: None,
        tools: None,
    };
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();

    // System message should be a top-level field, not in the messages array
    assert_eq!(
        body["system"], "You are a helpful assistant.",
        "Anthropic format requires system as top-level field"
    );

    // Messages array should only contain non-system messages
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1, "System message should be extracted");
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "Hi");
}

/// Foundry Anthropic path uses `max_tokens` (Anthropic convention, required field).
#[tokio::test]
async fn foundry_anthropic_uses_max_tokens() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let request = CompletionRequest {
        model: Some("claude-sonnet-4-5-20250514".into()),
        max_tokens: Some(4096),
        ..simple_request()
    };
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();

    assert_eq!(
        body["max_tokens"], 4096,
        "Anthropic path must use 'max_tokens'"
    );
    assert!(
        body.get("max_completion_tokens").is_none(),
        "Anthropic path must NOT use 'max_completion_tokens'"
    );
}

/// Foundry Anthropic path defaults max_tokens to 8192 when not provided.
#[tokio::test]
async fn foundry_anthropic_defaults_max_tokens() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let request = CompletionRequest {
        model: Some("claude-sonnet-4-5-20250514".into()),
        max_tokens: None,
        ..simple_request()
    };
    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();

    assert_eq!(
        body["max_tokens"], 8192,
        "Anthropic path should default max_tokens to 8192"
    );
}

/// Golden: full Foundry Anthropic request shape.
#[tokio::test]
async fn foundry_anthropic_request_golden_shape() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::with_api_version(&server.uri(), "my-foundry-key", "2023-06-01");
    let request = CompletionRequest {
        stable_prefix_messages: None,
        stable_prefix_tools: None,
        messages: vec![
            ChatMessage {
                role: Role::System,
                content: Some("Be concise.".into()),
                content_parts: None,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: Role::User,
                content: Some("Explain Rust.".into()),
                content_parts: None,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some("Rust is a systems language.".into()),
                content_parts: None,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: Role::User,
                content: Some("More detail.".into()),
                content_parts: None,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ],
        model: Some("claude-sonnet-4-5-20250514".into()),
        temperature: None,
        max_tokens: Some(2048),
        tools: None,
    };
    let resp = p.complete(&request).await.unwrap();

    // Verify response parsing
    assert_eq!(resp.content.as_deref(), Some("Hello from Claude!"));
    assert_eq!(resp.finish_reason, Some(FinishReason::Stop));
    assert_eq!(resp.usage.prompt_tokens, 12);
    assert_eq!(resp.usage.completion_tokens, 7);

    // Verify request shape
    let requests = server.received_requests().await.unwrap();
    let req = &requests[0];

    // URL
    assert!(
        req.url.path().contains("/anthropic/v1/messages"),
        "Anthropic path URL: {}",
        req.url
    );

    // Headers
    assert_eq!(
        req.headers.get("api-key").unwrap().to_str().unwrap(),
        "my-foundry-key"
    );
    assert_eq!(
        req.headers
            .get("anthropic-version")
            .unwrap()
            .to_str()
            .unwrap(),
        "2023-06-01"
    );

    // Body
    let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();

    assert_eq!(body["model"], "claude-sonnet-4-5-20250514");
    assert_eq!(body["max_tokens"], 2048);
    assert_eq!(body["system"], "Be concise.");

    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3, "System excluded, 3 remaining");
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "Explain Rust.");
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[2]["role"], "user");

    // Only expected top-level keys
    let keys: std::collections::HashSet<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    let expected: std::collections::HashSet<&str> =
        ["model", "max_tokens", "system", "messages"].into();
    assert_eq!(
        keys, expected,
        "Unexpected keys in Anthropic request body: {keys:?}"
    );
}

// --- Foundry vs Azure OpenAI contrast ---

/// Contrast: Foundry OpenAI path includes model + uses max_completion_tokens,
/// while Azure OpenAI omits model + uses max_tokens.
#[tokio::test]
async fn foundry_openai_vs_azure_openai_body_contrast() {
    let foundry_server = MockServer::start().await;
    let azure_server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&foundry_server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&azure_server)
        .await;

    let foundry = AzureFoundryProvider::new(&foundry_server.uri(), "k");
    let azure = AzureOpenAiProvider::new(&azure_server.uri(), "dep", "2024-06-01", "k");

    let mut request = simple_request();
    request.max_tokens = Some(512);

    let _ = foundry.complete(&request).await.unwrap();
    let _ = azure.complete(&request).await.unwrap();

    let foundry_reqs = foundry_server.received_requests().await.unwrap();
    let azure_reqs = azure_server.received_requests().await.unwrap();

    let foundry_body: serde_json::Value = serde_json::from_slice(&foundry_reqs[0].body).unwrap();
    let azure_body: serde_json::Value = serde_json::from_slice(&azure_reqs[0].body).unwrap();

    // Foundry: has model, max_completion_tokens (OpenAI convention)
    assert!(
        foundry_body.get("model").is_some(),
        "Foundry OpenAI must include model"
    );
    assert_eq!(foundry_body["max_completion_tokens"], 512);
    assert!(foundry_body.get("max_tokens").is_none());

    // Azure OpenAI: no model, max_tokens (Azure convention)
    assert!(
        azure_body.get("model").is_none(),
        "Azure OpenAI must omit model"
    );
    assert_eq!(azure_body["max_tokens"], 512);
    assert!(azure_body.get("max_completion_tokens").is_none());

    // URL contrast
    assert!(
        foundry_reqs[0]
            .url
            .path()
            .contains("/openai/v1/chat/completions"),
        "Foundry uses /openai/v1/ path"
    );
    assert!(
        azure_reqs[0].url.path().contains("/openai/deployments/"),
        "Azure OpenAI uses /openai/deployments/ path"
    );
}

// === Azure Foundry Anthropic-path error mapping =============================
//
// The Anthropic Messages API returns errors in a different format than OpenAI:
//   { "type": "error", "error": { "type": "<error_type>", "message": "..." } }
//
// These tests verify that Foundry correctly classifies each Anthropic error type
// into the right ModelError variant.

fn anthropic_error_body(error_type: &str, message: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "error",
        "error": {
            "type": error_type,
            "message": message
        }
    })
}

fn claude_request() -> CompletionRequest {
    CompletionRequest {
        stable_prefix_messages: None,
        stable_prefix_tools: None,
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some("Hello".into()),
            content_parts: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }],
        model: Some("claude-sonnet-4-5-20250514".into()),
        temperature: None,
        max_tokens: None,
        tools: None,
    }
}

/// Anthropic 401 authentication_error → ModelError::Auth
#[tokio::test]
async fn foundry_anthropic_maps_401_authentication_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(anthropic_error_body(
                "authentication_error",
                "Invalid API key",
            )),
        )
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "bad-key");
    let err = p.complete(&claude_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::Auth(ref msg) if msg.contains("Invalid API key")),
        "expected Auth, got {err:?}"
    );
}

/// Anthropic 403 permission_error → ModelError::Auth
#[tokio::test]
async fn foundry_anthropic_maps_403_permission_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(anthropic_error_body("permission_error", "Not allowed")),
        )
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let err = p.complete(&claude_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::Auth(_)),
        "expected Auth, got {err:?}"
    );
}

/// Anthropic 404 not_found_error → ModelError::Provider
#[tokio::test]
async fn foundry_anthropic_maps_404_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(anthropic_error_body("not_found_error", "Model not found")),
        )
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let err = p.complete(&claude_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::Provider(_)),
        "expected Provider, got {err:?}"
    );
}

/// Anthropic 429 rate_limit_error → ModelError::RateLimited with retry-after
#[tokio::test]
async fn foundry_anthropic_maps_429_rate_limit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "45")
                .set_body_json(anthropic_error_body("rate_limit_error", "Rate limited")),
        )
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let err = p.complete(&claude_request()).await.unwrap_err();
    match err {
        ModelError::RateLimited {
            retry_after_secs,
            ref message,
        } => {
            assert_eq!(retry_after_secs, Some(45));
            assert!(message.contains("Rate limited"));
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

/// Anthropic 529 overloaded_error → ModelError::Transient (retriable)
#[tokio::test]
async fn foundry_anthropic_maps_529_overloaded() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(529)
                .set_body_json(anthropic_error_body("overloaded_error", "Overloaded")),
        )
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let err = p.complete(&claude_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::Transient(ref msg) if msg.contains("Overloaded")),
        "expected Transient for 529, got {err:?}"
    );
    assert!(err.is_retriable(), "529 overloaded must be retriable");
}

/// Anthropic 500 api_error → ModelError::Transient
#[tokio::test]
async fn foundry_anthropic_maps_500_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_json(anthropic_error_body("api_error", "Internal server error")),
        )
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let err = p.complete(&claude_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::Transient(_)),
        "expected Transient for 500, got {err:?}"
    );
}

/// Anthropic 400 with context length indicator → ModelError::ContextLengthExceeded
#[tokio::test]
async fn foundry_anthropic_maps_400_context_length() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(anthropic_error_body(
                "invalid_request_error",
                "prompt has too many tokens: 200000 tokens > 100000 maximum",
            )),
        )
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let err = p.complete(&claude_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::ContextLengthExceeded(_)),
        "expected ContextLengthExceeded, got {err:?}"
    );
}

/// Anthropic 400 with content filter indicator → ModelError::ContentFiltered
#[tokio::test]
async fn foundry_anthropic_maps_400_content_filter() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(anthropic_error_body(
                "invalid_request_error",
                "content filter triggered: blocked by content_filter policy",
            )),
        )
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let err = p.complete(&claude_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::ContentFiltered(_)),
        "expected ContentFiltered, got {err:?}"
    );
}

/// Anthropic 400 generic invalid_request_error → ModelError::Provider
#[tokio::test]
async fn foundry_anthropic_maps_400_generic_request_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(anthropic_error_body(
                "invalid_request_error",
                "messages: at least one message is required",
            )),
        )
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let err = p.complete(&claude_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::Provider(_)),
        "expected Provider for generic 400, got {err:?}"
    );
}

/// Anthropic error with non-JSON body still maps correctly by status code
#[tokio::test]
async fn foundry_anthropic_maps_non_json_error_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502).set_body_string("Bad Gateway"))
        .mount(&server)
        .await;

    let p = AzureFoundryProvider::new(&server.uri(), "key");
    let err = p.complete(&claude_request()).await.unwrap_err();
    assert!(
        matches!(err, ModelError::Transient(ref msg) if msg.contains("Bad Gateway")),
        "expected Transient for 502 with plain text body, got {err:?}"
    );
}

/// Contrast: same 401 status but OpenAI vs Anthropic error format both map to Auth.
/// Verifies the Foundry routes each path through the correct error mapper.
#[tokio::test]
async fn foundry_error_format_contrast_openai_vs_anthropic() {
    let openai_server = MockServer::start().await;
    let anthropic_server = MockServer::start().await;

    // OpenAI error format
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": { "message": "Invalid key (openai)", "code": "invalid_api_key" }
        })))
        .mount(&openai_server)
        .await;

    // Anthropic error format
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(anthropic_error_body(
                "authentication_error",
                "Invalid key (anthropic)",
            )),
        )
        .mount(&anthropic_server)
        .await;

    // OpenAI path (gpt model)
    let openai_provider = AzureFoundryProvider::new(&openai_server.uri(), "bad");
    let openai_err = openai_provider
        .complete(&CompletionRequest {
            model: Some("gpt-5.4".into()),
            ..simple_request()
        })
        .await
        .unwrap_err();

    // Anthropic path (claude model)
    let anthropic_provider = AzureFoundryProvider::new(&anthropic_server.uri(), "bad");
    let anthropic_err = anthropic_provider
        .complete(&claude_request())
        .await
        .unwrap_err();

    // Both should map to Auth
    assert!(
        matches!(openai_err, ModelError::Auth(ref msg) if msg.contains("openai")),
        "OpenAI path should map to Auth, got {openai_err:?}"
    );
    assert!(
        matches!(anthropic_err, ModelError::Auth(ref msg) if msg.contains("anthropic")),
        "Anthropic path should map to Auth, got {anthropic_err:?}"
    );
}

#[tokio::test]
async fn azure_maps_retry_after_http_date() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "Wed, 21 Oct 2099 07:28:00 GMT")
                .set_body_json(serde_json::json!({
                    "error": {
                        "message": "Rate limited",
                        "code": "429"
                    }
                })),
        )
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "dep", "2024-06-01", "k");
    let err = p.complete(&simple_request()).await.unwrap_err();
    match err {
        ModelError::RateLimited {
            retry_after_secs, ..
        } => {
            assert!(retry_after_secs.is_some());
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

#[tokio::test]
async fn azure_request_prepends_stable_prefix_messages() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = AzureOpenAiProvider::new(&server.uri(), "gpt-4o", "2024-06-01", "k");
    let request = CompletionRequest {
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some("Hello".into()),
            content_parts: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }],
        stable_prefix_tools: None,
        stable_prefix_messages: Some(vec![ChatMessage {
            role: Role::System,
            content: Some("Stable prefix".into()),
            content_parts: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }]),
        model: Some("gpt-4o".into()),
        temperature: None,
        max_tokens: None,
        tools: None,
    };

    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "Stable prefix");
    assert_eq!(msgs[1]["role"], "user");
}

#[tokio::test]
async fn openai_request_prepends_stable_prefix_messages() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = OpenAiProvider::new(&server.uri(), "k");
    let request = CompletionRequest {
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some("Hello".into()),
            content_parts: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }],
        stable_prefix_tools: None,
        stable_prefix_messages: Some(vec![ChatMessage {
            role: Role::System,
            content: Some("Stable prefix".into()),
            content_parts: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }]),
        model: Some("gpt-4o".into()),
        temperature: None,
        max_tokens: None,
        tools: None,
    };

    let _ = p.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "Stable prefix");
    assert_eq!(msgs[1]["role"], "user");
}

#[tokio::test]
async fn google_provider_streams_openai_compatible_sse() {
    let server = MockServer::start().await;

    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}\n\n",
        "data: [DONE]\n\n"
    );

    Mock::given(method("POST"))
        .and(header("authorization", "Bearer test-google-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let provider = rune_models::GoogleProvider::with_base_url(&server.uri(), "test-google-key");
    let mut request = simple_request();
    request.model = Some("gemini-2.0-flash".into());

    let mut rx = provider.complete_stream(&request).await.unwrap();
    let mut deltas = Vec::new();
    let mut final_response = None;

    while let Some(event) = rx.recv().await {
        match event {
            rune_models::StreamEvent::TextDelta(delta) => deltas.push(delta),
            rune_models::StreamEvent::Done(resp) => {
                final_response = Some(resp);
                break;
            }
        }
    }

    assert_eq!(deltas, vec!["Hello", " world"]);
    let resp = final_response.expect("stream should yield final response");
    assert_eq!(resp.content.as_deref(), Some("Hello world"));
    assert_eq!(resp.finish_reason, Some(FinishReason::Stop));
    assert_eq!(resp.usage.total_tokens, 6);
}

#[tokio::test]
async fn openai_unauthenticated_mode_sends_no_auth_headers() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let p = OpenAiProvider::unauthenticated(&server.uri());
    let resp = p.complete(&simple_request()).await.unwrap();
    assert_eq!(resp.content.as_deref(), Some("Hello! How can I help?"));

    let requests = server.received_requests().await.unwrap();
    let req = &requests[0];
    assert!(
        req.headers.get("authorization").is_none(),
        "unauthenticated mode must not send Authorization header"
    );
    assert!(
        req.headers.get("api-key").is_none(),
        "unauthenticated mode must not send api-key header"
    );
}

#[tokio::test]
async fn openai_serializes_multimodal_user_content_parts() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let provider = OpenAiProvider::new(&server.uri(), "k");
    let request = CompletionRequest {
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some(
                "Describe this image

[Attachments]
- photo.jpg (image/jpeg, url=https://example.test/photo.jpg)"
                    .into(),
            ),
            content_parts: Some(vec![
                MessagePart::Text {
                    text: "Describe this image".into(),
                },
                MessagePart::ImageUrl {
                    image_url: ImageUrlPart {
                        url: "https://example.test/photo.jpg".into(),
                    },
                },
            ]),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }],
        stable_prefix_messages: None,
        stable_prefix_tools: None,
        model: Some("gpt-4o".into()),
        temperature: None,
        max_tokens: None,
        tools: None,
    };

    let _ = provider.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "Describe this image");
    assert_eq!(content[1]["type"], "image_url");
    assert_eq!(
        content[1]["image_url"]["url"],
        "https://example.test/photo.jpg"
    );
}

#[tokio::test]
async fn azure_serializes_multimodal_user_content_parts() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let provider = AzureOpenAiProvider::new(&server.uri(), "gpt-4o", "2024-06-01", "k");
    let request = CompletionRequest {
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some(
                "Describe this image

[Attachments]
- photo.jpg (image/jpeg, url=https://example.test/photo.jpg)"
                    .into(),
            ),
            content_parts: Some(vec![
                MessagePart::Text {
                    text: "Describe this image".into(),
                },
                MessagePart::ImageUrl {
                    image_url: ImageUrlPart {
                        url: "https://example.test/photo.jpg".into(),
                    },
                },
            ]),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }],
        stable_prefix_messages: None,
        stable_prefix_tools: None,
        model: Some("gpt-4o".into()),
        temperature: None,
        max_tokens: None,
        tools: None,
    };

    let _ = provider.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image_url");
    assert_eq!(
        content[1]["image_url"]["url"],
        "https://example.test/photo.jpg"
    );
}

#[tokio::test]
async fn anthropic_serializes_multimodal_user_content_parts_as_content_blocks() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [{ "type": "text", "text": "I see a diagram" }],
            "usage": { "input_tokens": 10, "output_tokens": 8 },
            "stop_reason": "end_turn"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = AnthropicProvider::azure(&server.uri(), "", "2023-06-01", "k");
    let request = CompletionRequest {
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some("Describe this image".into()),
            content_parts: Some(vec![
                MessagePart::Text {
                    text: "Describe this image".into(),
                },
                MessagePart::ImageUrl {
                    image_url: ImageUrlPart {
                        url: "data:image/png;base64,QUJDRA==".into(),
                    },
                },
            ]),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }],
        stable_prefix_messages: None,
        stable_prefix_tools: None,
        model: Some("claude-sonnet-4-20250514".into()),
        temperature: None,
        max_tokens: None,
        tools: None,
    };

    let _ = provider.complete(&request).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "Describe this image");
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["source"]["type"], "base64");
    assert_eq!(content[1]["source"]["media_type"], "image/png");
    assert_eq!(content[1]["source"]["data"], "QUJDRA==");
}
