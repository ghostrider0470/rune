use rune_config::ModelProviderConfig;
use rune_models::{
    provider_from_config, AzureOpenAiProvider, ChatMessage, CompletionRequest, FinishReason,
    ModelError, ModelProvider, OpenAiProvider, Role,
};
use wiremock::matchers::{header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn simple_request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some("Hello".into()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }],
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
    let p = AzureOpenAiProvider::new(
        "https://myres.openai.azure.com/",
        "dep",
        "2025-01-01",
        "k",
    );
    assert!(p.url().starts_with("https://myres.openai.azure.com/openai/"));
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

// --- Provider selection from config ---

#[test]
fn selects_azure_provider() {
    unsafe { std::env::set_var("TEST_AZURE_KEY_SEL", "fake") };
    let cfg = ModelProviderConfig {
        provider_name: "azure".into(),
        endpoint: "https://test.openai.azure.com".into(),
        deployment_name: Some("gpt-4o".into()),
        api_version: Some("2024-06-01".into()),
        api_key_env: Some("TEST_AZURE_KEY_SEL".into()),
        model_alias: None,
    };
    let provider = provider_from_config(&cfg).unwrap();
    // We can't downcast easily, but we can verify it works by checking it's Send+Sync
    let _: Box<dyn ModelProvider> = provider;
    unsafe { std::env::remove_var("TEST_AZURE_KEY_SEL") };
}

#[test]
fn selects_openai_provider() {
    unsafe { std::env::set_var("TEST_OAI_KEY_SEL", "fake") };
    let cfg = ModelProviderConfig {
        provider_name: "openai".into(),
        endpoint: "https://api.openai.com/v1".into(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("TEST_OAI_KEY_SEL".into()),
        model_alias: None,
    };
    let provider = provider_from_config(&cfg).unwrap();
    let _: Box<dyn ModelProvider> = provider;
    unsafe { std::env::remove_var("TEST_OAI_KEY_SEL") };
}

#[test]
fn azure_requires_deployment_name() {
    unsafe { std::env::set_var("TEST_AZURE_KEY_DEP", "fake") };
    let cfg = ModelProviderConfig {
        provider_name: "azure".into(),
        endpoint: "https://test.openai.azure.com".into(),
        deployment_name: None,
        api_version: Some("2024-06-01".into()),
        api_key_env: Some("TEST_AZURE_KEY_DEP".into()),
        model_alias: None,
    };
    let err = provider_from_config(&cfg).unwrap_err();
    assert!(matches!(err, ModelError::Configuration(_)));
    unsafe { std::env::remove_var("TEST_AZURE_KEY_DEP") };
}

#[test]
fn azure_requires_api_version() {
    unsafe { std::env::set_var("TEST_AZURE_KEY_VER", "fake") };
    let cfg = ModelProviderConfig {
        provider_name: "azure".into(),
        endpoint: "https://test.openai.azure.com".into(),
        deployment_name: Some("gpt-4o".into()),
        api_version: None,
        api_key_env: Some("TEST_AZURE_KEY_VER".into()),
        model_alias: None,
    };
    let err = provider_from_config(&cfg).unwrap_err();
    assert!(matches!(err, ModelError::Configuration(_)));
    unsafe { std::env::remove_var("TEST_AZURE_KEY_VER") };
}

#[test]
fn missing_api_key_env_returns_auth_error() {
    let cfg = ModelProviderConfig {
        provider_name: "openai".into(),
        endpoint: "https://api.openai.com/v1".into(),
        deployment_name: None,
        api_version: None,
        api_key_env: Some("DEFINITELY_NOT_SET_12345".into()),
        model_alias: None,
    };
    let err = provider_from_config(&cfg).unwrap_err();
    assert!(matches!(err, ModelError::Auth(_)));
}
