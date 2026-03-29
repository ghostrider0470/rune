use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rune_config::{ConfiguredModel, ModelFallbackChainConfig, ModelProviderConfig, ModelsConfig};
use rune_models::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, ModelError, ModelProvider,
    Role, RoutedModelProvider, Usage,
};

#[derive(Debug)]
enum StubResult {
    Ok(CompletionResponse),
    Err(ModelError),
}

#[derive(Debug)]
struct StubProvider {
    result: StubResult,
    seen_models: Arc<Mutex<Vec<Option<String>>>>,
}

#[async_trait]
impl ModelProvider for StubProvider {
    async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        self.seen_models
            .lock()
            .unwrap()
            .push(request.model.clone());
        match &self.result {
            StubResult::Ok(response) => Ok(response.clone()),
            StubResult::Err(ModelError::Transient(message)) => {
                Err(ModelError::Transient(message.clone()))
            }
            StubResult::Err(ModelError::Auth(message)) => Err(ModelError::Auth(message.clone())),
            StubResult::Err(other) => panic!("unsupported stub error variant: {other:?}"),
        }
    }
}

fn request(model: &str) -> CompletionRequest {
    CompletionRequest {
        stable_prefix_messages: None,
        stable_prefix_tools: None,
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some("hello".into()),
            content_parts: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }],
        model: Some(model.into()),
        temperature: None,
        max_tokens: None,
        tools: None,
    }
}

fn ok_response(content: &str) -> CompletionResponse {
    CompletionResponse {
        content: Some(content.into()),
        usage: Usage::default(),
        finish_reason: Some(FinishReason::Stop),
        tool_calls: vec![],
    }
}

fn provider_config(name: &str, kind: &str, model: &str) -> ModelProviderConfig {
    ModelProviderConfig {
        name: name.into(),
        kind: kind.into(),
        base_url: if kind == "ollama" {
            "http://localhost:11434".into()
        } else {
            "https://example.invalid/v1".into()
        },
        api_key: if kind == "ollama" {
            None
        } else {
            Some("test-key".into())
        },
        deployment_name: None,
        api_version: None,
        api_key_env: None,
        model_alias: None,
        models: vec![ConfiguredModel::Id(model.into())],
    }
}

#[tokio::test]
async fn routed_provider_uses_ollama_fallback_on_retriable_error() {
    let primary_seen = Arc::new(Mutex::new(Vec::new()));
    let fallback_seen = Arc::new(Mutex::new(Vec::new()));

    let models = ModelsConfig {
        default_model: None,
        default_image_model: None,
        fallbacks: vec![ModelFallbackChainConfig {
            name: "chat".into(),
            chain: vec!["cloud/gpt-4o-mini".into(), "local/llama3.2".into()],
        }],
        image_fallbacks: vec![],
        auth_orders: vec![],
        providers: vec![
            provider_config("cloud", "openai", "gpt-4o-mini"),
            provider_config("local", "ollama", "llama3.2"),
        ],
    };

    let mut provider = RoutedModelProvider::from_models_config(&models).unwrap();
    provider.register_provider(
        "cloud",
        Arc::new(StubProvider {
            result: StubResult::Err(ModelError::Transient("upstream 500".into())),
            seen_models: Arc::clone(&primary_seen),
        }),
    );
    provider.register_provider(
        "local",
        Arc::new(StubProvider {
            result: StubResult::Ok(ok_response("fallback ok")),
            seen_models: Arc::clone(&fallback_seen),
        }),
    );

    let response = provider.complete(&request("cloud/gpt-4o-mini")).await.unwrap();
    assert_eq!(response.content.as_deref(), Some("fallback ok"));
    assert_eq!(
        primary_seen.lock().unwrap().as_slice(),
        &[Some("gpt-4o-mini".into())]
    );
    assert_eq!(
        fallback_seen.lock().unwrap().as_slice(),
        &[Some("llama3.2".into())]
    );
}

#[tokio::test]
async fn routed_provider_stops_on_non_retriable_primary_error() {
    let primary_seen = Arc::new(Mutex::new(Vec::new()));
    let fallback_seen = Arc::new(Mutex::new(Vec::new()));

    let models = ModelsConfig {
        default_model: None,
        default_image_model: None,
        fallbacks: vec![ModelFallbackChainConfig {
            name: "chat".into(),
            chain: vec!["cloud/gpt-4o-mini".into(), "local/llama3.2".into()],
        }],
        image_fallbacks: vec![],
        auth_orders: vec![],
        providers: vec![
            provider_config("cloud", "openai", "gpt-4o-mini"),
            provider_config("local", "ollama", "llama3.2"),
        ],
    };

    let mut provider = RoutedModelProvider::from_models_config(&models).unwrap();
    provider.register_provider(
        "cloud",
        Arc::new(StubProvider {
            result: StubResult::Err(ModelError::Auth("bad key".into())),
            seen_models: Arc::clone(&primary_seen),
        }),
    );
    provider.register_provider(
        "local",
        Arc::new(StubProvider {
            result: StubResult::Ok(ok_response("should not be used")),
            seen_models: Arc::clone(&fallback_seen),
        }),
    );

    let err = provider
        .complete(&request("cloud/gpt-4o-mini"))
        .await
        .expect_err("non-retriable errors should stop fallback chain");
    assert!(matches!(err, ModelError::Auth(_)));
    assert_eq!(
        primary_seen.lock().unwrap().as_slice(),
        &[Some("gpt-4o-mini".into())]
    );
    assert!(fallback_seen.lock().unwrap().is_empty());
}

#[test]
fn ollama_provider_kind_is_supported_in_config_inventory() {
    let cfg = provider_config("local", "ollama", "llama3.2");

    assert_eq!(cfg.kind, "ollama");
    assert!(cfg.supports_model("llama3.2"));
    assert!(!cfg.supports_model("qwen2.5"));
}
