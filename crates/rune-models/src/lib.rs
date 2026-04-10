#![doc = "Model provider abstraction for Rune — Azure OpenAI, OpenAI, and extensible providers."]

mod error;
mod provider;
mod types;

pub use error::ModelError;
pub use provider::ModelProvider;
pub use provider::anthropic::AnthropicProvider;
pub use provider::azure::AzureOpenAiProvider;
pub use provider::azure_foundry::AzureFoundryProvider;
pub use provider::bedrock::BedrockProvider;
pub use provider::deepseek::DeepSeekProvider;
pub use provider::google::GoogleProvider;
pub use provider::groq::GroqProvider;
pub use provider::mistral::MistralProvider;
pub use provider::ollama::OllamaProvider;
pub use provider::openai::OpenAiProvider;
pub use types::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, FunctionCall,
    FunctionDefinition, ImageUrlPart, MessagePart, Role, StreamEvent, ToolCallRequest,
    ToolDefinition, Usage,
};

use rune_config::ModelProviderConfig;
use rune_config::{ModelResolutionError, ModelsConfig};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::warn;

fn validate_azure_api_version(version: &str) -> Result<(), ModelError> {
    let (date_part, suffix) = match version.strip_suffix("-preview") {
        Some(date) => (date, true),
        None => (version, false),
    };

    let parts: Vec<&str> = date_part.split('-').collect();
    let valid = parts.len() == 3
        && parts[0].len() == 4
        && parts[0].chars().all(|c| c.is_ascii_digit())
        && parts[1].len() == 2
        && parts[1].chars().all(|c| c.is_ascii_digit())
        && parts[2].len() == 2
        && parts[2].chars().all(|c| c.is_ascii_digit());

    if !valid {
        let hint = if suffix {
            "expected YYYY-MM-DD-preview"
        } else {
            "expected YYYY-MM-DD or YYYY-MM-DD-preview"
        };
        return Err(ModelError::Configuration(format!(
            "invalid Azure API version '{version}': {hint}"
        )));
    }

    Ok(())
}

fn is_azure_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains(".azure.com")
        || lower.contains(".azure-api.net")
        || lower.contains("azure.cognitiveservices")
}

fn validate_azure_endpoint(base_url: &str, provider_kind: &str) -> Result<(), ModelError> {
    if base_url.trim().is_empty() {
        return Err(ModelError::Configuration(format!(
            "{provider_kind} provider requires a non-empty base_url / endpoint"
        )));
    }
    Ok(())
}

fn validate_azure_deployment_name(name: &str) -> Result<(), ModelError> {
    if name.trim().is_empty() {
        return Err(ModelError::Configuration(
            "Azure deployment_name must not be empty or whitespace-only".into(),
        ));
    }
    const FORBIDDEN: &[char] = &['/', '?', '#', '%', ' '];
    if let Some(bad) = name.chars().find(|c| FORBIDDEN.contains(c)) {
        return Err(ModelError::Configuration(format!(
            "Azure deployment_name '{name}' contains invalid character '{bad}' \
             — deployment names are embedded in the URL path and must not contain /, ?, #, %, or spaces"
        )));
    }
    Ok(())
}

pub fn provider_from_config(
    cfg: &ModelProviderConfig,
) -> Result<Box<dyn ModelProvider>, ModelError> {
    let kind = if cfg.kind.is_empty() {
        cfg.name.as_str()
    } else {
        cfg.kind.as_str()
    };

    match kind.to_lowercase().as_str() {
        "anthropic" => {
            let api_key = resolve_api_key(cfg)?;
            let api_version = cfg.api_version.as_deref().unwrap_or("2023-06-01");
            if cfg.base_url.is_empty() || cfg.base_url.contains("api.anthropic.com") {
                Ok(Box::new(AnthropicProvider::direct(&api_key)))
            } else {
                Ok(Box::new(AnthropicProvider::azure(
                    &cfg.base_url,
                    cfg.deployment_name.as_deref().unwrap_or(""),
                    api_version,
                    &api_key,
                )))
            }
        }
        "anthropic_azure" | "azure_anthropic" => {
            validate_azure_endpoint(&cfg.base_url, "Anthropic Azure")?;
            let api_key = resolve_api_key(cfg)?;
            let api_version = cfg.api_version.as_deref().unwrap_or("2023-06-01");
            validate_azure_api_version(api_version)?;
            Ok(Box::new(AnthropicProvider::azure(
                &cfg.base_url,
                cfg.deployment_name.as_deref().unwrap_or(""),
                api_version,
                &api_key,
            )))
        }
        "azure" | "azure_openai" | "azure-openai" => {
            validate_azure_endpoint(&cfg.base_url, "Azure OpenAI")?;
            let api_key = resolve_api_key(cfg)?;
            let deployment = cfg.deployment_name.as_deref().ok_or_else(|| {
                ModelError::Configuration("Azure provider requires deployment_name".into())
            })?;
            validate_azure_deployment_name(deployment)?;
            let api_version = cfg.api_version.as_deref().ok_or_else(|| {
                ModelError::Configuration("Azure provider requires api_version".into())
            })?;
            validate_azure_api_version(api_version)?;
            Ok(Box::new(AzureOpenAiProvider::new(
                &cfg.base_url,
                deployment,
                api_version,
                &api_key,
            )))
        }
        "azure-foundry" | "azure-ai" => {
            validate_azure_endpoint(&cfg.base_url, "Azure Foundry")?;
            let api_key = resolve_api_key(cfg)?;
            let api_version = cfg.api_version.as_deref().unwrap_or("2023-06-01");
            validate_azure_api_version(api_version)?;
            Ok(Box::new(AzureFoundryProvider::with_api_version(
                &cfg.base_url,
                &api_key,
                api_version,
            )))
        }
        "openai" => {
            let api_key = resolve_api_key(cfg)?;
            if is_azure_url(&cfg.base_url) {
                Ok(Box::new(OpenAiProvider::azure(&cfg.base_url, &api_key)))
            } else {
                Ok(Box::new(OpenAiProvider::new(&cfg.base_url, &api_key)))
            }
        }
        "google" | "gemini" => {
            let api_key = resolve_api_key(cfg)?;
            if cfg.base_url.is_empty() {
                Ok(Box::new(GoogleProvider::new(&api_key)))
            } else {
                Ok(Box::new(GoogleProvider::with_base_url(
                    &cfg.base_url,
                    &api_key,
                )))
            }
        }
        "ollama" => {
            if cfg.base_url.is_empty() {
                Ok(Box::new(OllamaProvider::new()))
            } else {
                Ok(Box::new(OllamaProvider::with_base_url(&cfg.base_url)))
            }
        }
        "groq" => {
            let api_key = resolve_api_key(cfg)?;
            if cfg.base_url.is_empty() {
                Ok(Box::new(GroqProvider::new(&api_key)))
            } else {
                Ok(Box::new(GroqProvider::with_base_url(
                    &cfg.base_url,
                    &api_key,
                )))
            }
        }
        "deepseek" => {
            let api_key = resolve_api_key(cfg)?;
            if cfg.base_url.is_empty() {
                Ok(Box::new(DeepSeekProvider::new(&api_key)))
            } else {
                Ok(Box::new(DeepSeekProvider::with_base_url(
                    &cfg.base_url,
                    &api_key,
                )))
            }
        }
        "mistral" => {
            let api_key = resolve_api_key(cfg)?;
            if cfg.base_url.is_empty() {
                Ok(Box::new(MistralProvider::new(&api_key)))
            } else {
                Ok(Box::new(MistralProvider::with_base_url(
                    &cfg.base_url,
                    &api_key,
                )))
            }
        }
        "openrouter" => {
            let api_key = resolve_api_key(cfg)?;
            let base_url = if cfg.base_url.is_empty() {
                "https://openrouter.ai/api/v1"
            } else {
                cfg.base_url.as_str()
            };
            Ok(Box::new(OpenAiProvider::new(base_url, &api_key)))
        }
        "perplexity" => {
            let api_key = resolve_api_key(cfg)?;
            let base_url = if cfg.base_url.is_empty() {
                "https://api.perplexity.ai"
            } else {
                cfg.base_url.as_str()
            };
            Ok(Box::new(OpenAiProvider::new(base_url, &api_key)))
        }
        "bedrock" | "aws-bedrock" | "aws_bedrock" => {
            let (access_key_id, secret_access_key) = resolve_aws_credentials(cfg)?;
            let region = cfg
                .api_version
                .clone()
                .or_else(|| std::env::var("AWS_REGION").ok())
                .unwrap_or_else(|| "us-east-1".to_string());
            Ok(Box::new(BedrockProvider::new(
                &region,
                &access_key_id,
                &secret_access_key,
                None,
            )))
        }
        _ => {
            let api_key = resolve_api_key(cfg)?;
            if is_azure_url(&cfg.base_url) {
                Ok(Box::new(OpenAiProvider::azure(&cfg.base_url, &api_key)))
            } else {
                Ok(Box::new(OpenAiProvider::new(&cfg.base_url, &api_key)))
            }
        }
    }
}

#[derive(Debug)]
pub struct RoutedModelProvider {
    models: ModelsConfig,
    providers: HashMap<String, Arc<dyn ModelProvider>>,
    circuit_breakers: Mutex<HashMap<String, CircuitBreakerState>>,
    circuit_breaker_threshold: u32,
    circuit_breaker_cooldown: Duration,
}

#[derive(Debug, Clone)]
struct CircuitBreakerState {
    failures: u32,
    opened_at: Option<Instant>,
}

impl CircuitBreakerState {
    fn new() -> Self {
        Self {
            failures: 0,
            opened_at: None,
        }
    }

    fn is_open(&self, now: Instant, cooldown: Duration) -> bool {
        self.opened_at
            .map(|opened_at| now.duration_since(opened_at) < cooldown)
            .unwrap_or(false)
    }
}

impl RoutedModelProvider {
    pub fn from_models_config(models: &ModelsConfig) -> Result<Self, ModelError> {
        let mut providers = HashMap::new();

        for provider_cfg in &models.providers {
            providers.insert(
                provider_cfg.name.clone(),
                Arc::from(provider_from_config(provider_cfg)?),
            );
        }

        Ok(Self {
            models: models.clone(),
            providers,
            circuit_breakers: Mutex::new(HashMap::new()),
            circuit_breaker_threshold: 3,
            circuit_breaker_cooldown: Duration::from_secs(30),
        })
    }

    #[doc(hidden)]
    pub fn register_provider(&mut self, name: &str, provider: Arc<dyn ModelProvider>) {
        self.providers.insert(name.to_string(), provider);
    }

    #[doc(hidden)]
    pub fn with_circuit_breaker_settings(mut self, threshold: u32, cooldown: Duration) -> Self {
        self.circuit_breaker_threshold = threshold.max(1);
        self.circuit_breaker_cooldown = cooldown;
        self
    }

    fn circuit_allows(&self, provider_name: &str, model_ref: &str) -> Result<(), ModelError> {
        let now = Instant::now();
        let mut breakers = self
            .circuit_breakers
            .lock()
            .expect("circuit breakers poisoned");
        let state = breakers
            .entry(provider_name.to_string())
            .or_insert_with(CircuitBreakerState::new);

        if state.is_open(now, self.circuit_breaker_cooldown) {
            return Err(ModelError::Transient(format!(
                "circuit breaker open for provider '{provider_name}' while routing model '{model_ref}' (cooldown {:?})",
                self.circuit_breaker_cooldown
            )));
        }

        if state.opened_at.is_some() {
            state.opened_at = None;
            state.failures = 0;
        }

        Ok(())
    }

    fn record_provider_result(
        &self,
        provider_name: &str,
        model_ref: &str,
        result: Result<(), &ModelError>,
    ) {
        let mut breakers = self
            .circuit_breakers
            .lock()
            .expect("circuit breakers poisoned");
        let state = breakers
            .entry(provider_name.to_string())
            .or_insert_with(CircuitBreakerState::new);

        match &result {
            Ok(()) => {
                state.failures = 0;
                state.opened_at = None;
            }
            Err(error) if error.is_retriable() => {
                state.failures = state.failures.saturating_add(1);
                if state.failures >= self.circuit_breaker_threshold {
                    state.opened_at = Some(Instant::now());
                    warn!(
                        provider = provider_name,
                        model = model_ref,
                        failures = state.failures,
                        threshold = self.circuit_breaker_threshold,
                        cooldown_secs = self.circuit_breaker_cooldown.as_secs(),
                        error = %error,
                        "provider circuit breaker opened; degraded-mode fallback is now active"
                    );
                }
            }
            Err(_) => {
                state.failures = 0;
                state.opened_at = None;
            }
        }
    }

    async fn dispatch_single_stream(
        &self,
        model_ref: &str,
        request: &CompletionRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamEvent>, ModelError> {
        let resolved = self
            .models
            .resolve_model(model_ref)
            .map_err(map_resolution_error)?;
        self.circuit_allows(&resolved.provider.name, model_ref)?;
        let provider = self.providers.get(&resolved.provider.name).ok_or_else(|| {
            ModelError::Configuration(format!(
                "provider '{}' is configured in the model inventory but not available",
                resolved.provider.name
            ))
        })?;

        let mut routed_request = request.clone();
        routed_request.model = Some(resolved.raw_model.to_string());
        let result = provider.complete_stream(&routed_request).await;
        self.record_provider_result(
            &resolved.provider.name,
            model_ref,
            result.as_ref().map(|_| ()),
        );
        result
    }
}

#[async_trait::async_trait]
impl ModelProvider for RoutedModelProvider {
    async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        let Some(model_ref) = request.model.as_deref() else {
            if self.providers.len() == 1 {
                let provider = self
                    .providers
                    .values()
                    .next()
                    .expect("single provider should exist");
                return provider.complete(request).await;
            }
            return Err(ModelError::Configuration(
                "model routing requires a selected model".into(),
            ));
        };

        let primary_result = self.dispatch_single(model_ref, request).await;

        let primary_err = match primary_result {
            Ok(resp) => return Ok(resp),
            Err(e) if !e.is_retriable() => return Err(e),
            Err(e) => e,
        };

        let Some(fallbacks) = self.models.fallback_chain_for(model_ref) else {
            return Err(primary_err);
        };

        warn!(
            primary = model_ref,
            error = %primary_err,
            fallback_count = fallbacks.len(),
            "primary provider failed with retriable error, trying fallback chain"
        );

        let mut last_err = primary_err;
        for fallback_ref in fallbacks {
            match self.dispatch_single(fallback_ref, request).await {
                Ok(resp) => return Ok(resp),
                Err(e) if e.is_retriable() => {
                    warn!(
                        fallback = fallback_ref.as_str(),
                        error = %e,
                        "fallback provider also failed, continuing chain"
                    );
                    last_err = e;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Err(last_err)
    }

    async fn complete_stream(
        &self,
        request: &CompletionRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamEvent>, ModelError> {
        let Some(model_ref) = request.model.as_deref() else {
            if self.providers.len() == 1 {
                let provider = self
                    .providers
                    .values()
                    .next()
                    .expect("single provider should exist");
                return provider.complete_stream(request).await;
            }
            return Err(ModelError::Configuration(
                "model routing requires a selected model".into(),
            ));
        };

        let primary_result = self.dispatch_single_stream(model_ref, request).await;

        let primary_err = match primary_result {
            Ok(stream) => return Ok(stream),
            Err(e) if !e.is_retriable() => return Err(e),
            Err(e) => e,
        };

        let Some(fallbacks) = self.models.fallback_chain_for(model_ref) else {
            return Err(primary_err);
        };

        warn!(
            primary = model_ref,
            error = %primary_err,
            fallback_count = fallbacks.len(),
            "primary streaming provider failed with retriable error, trying fallback chain"
        );

        let mut last_err = primary_err;
        for fallback_ref in fallbacks {
            match self.dispatch_single_stream(fallback_ref, request).await {
                Ok(stream) => return Ok(stream),
                Err(e) if e.is_retriable() => {
                    warn!(
                        fallback = fallback_ref.as_str(),
                        error = %e,
                        "streaming fallback provider also failed, continuing chain"
                    );
                    last_err = e;
                }
                Err(e) => return Err(e),
            }
        }

        Err(last_err)
    }
}

impl RoutedModelProvider {
    async fn dispatch_single(
        &self,
        model_ref: &str,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        let resolved = self
            .models
            .resolve_model(model_ref)
            .map_err(map_resolution_error)?;
        self.circuit_allows(&resolved.provider.name, model_ref)?;
        let provider = self.providers.get(&resolved.provider.name).ok_or_else(|| {
            ModelError::Configuration(format!(
                "provider '{}' is configured in the model inventory but not available",
                resolved.provider.name
            ))
        })?;

        let mut routed_request = request.clone();
        routed_request.model = Some(resolved.raw_model.to_string());
        let result = provider.complete(&routed_request).await;
        self.record_provider_result(
            &resolved.provider.name,
            model_ref,
            result.as_ref().map(|_| ()),
        );
        result
    }
}

fn resolve_api_key(cfg: &ModelProviderConfig) -> Result<String, ModelError> {
    if let Some(ref key) = cfg.api_key {
        if !key.is_empty() {
            return Ok(key.clone());
        }
    }
    let env_var = cfg.api_key_env.as_deref().unwrap_or("OPENAI_API_KEY");
    std::env::var(env_var).map_err(|_| {
        ModelError::Auth(format!(
            "API key env var '{env_var}' not set for provider '{}'",
            cfg.name
        ))
    })
}

fn resolve_aws_credentials(cfg: &ModelProviderConfig) -> Result<(String, String), ModelError> {
    if let Some(ref key) = cfg.api_key {
        if !key.is_empty() {
            if let Some((access, secret)) = key.split_once(':') {
                if !access.is_empty() && !secret.is_empty() {
                    return Ok((access.to_string(), secret.to_string()));
                }
            }
        }
    }

    if let Some(ref env_var) = cfg.api_key_env {
        if let Ok(combined) = std::env::var(env_var) {
            if let Some((access, secret)) = combined.split_once(':') {
                if !access.is_empty() && !secret.is_empty() {
                    return Ok((access.to_string(), secret.to_string()));
                }
            }
        }
    }

    let access_key_id = std::env::var("AWS_ACCESS_KEY_ID").map_err(|_| {
        ModelError::Auth(format!(
            "AWS credentials not configured for provider '{}': \
             set api_key (ACCESS_KEY_ID:SECRET_ACCESS_KEY) or \
             AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY env vars",
            cfg.name
        ))
    })?;
    let secret_access_key = std::env::var("AWS_SECRET_ACCESS_KEY").map_err(|_| {
        ModelError::Auth(format!(
            "AWS_SECRET_ACCESS_KEY not set for provider '{}'",
            cfg.name
        ))
    })?;

    Ok((access_key_id, secret_access_key))
}

fn map_resolution_error(error: ModelResolutionError) -> ModelError {
    ModelError::Configuration(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_config::ConfiguredModel;
    use std::sync::Mutex as StdMutex;
    use tokio::sync::mpsc;

    fn provider_cfg(name: &str, kind: &str) -> ModelProviderConfig {
        ModelProviderConfig {
            name: name.to_string(),
            kind: kind.to_string(),
            base_url: String::new(),
            api_key: Some("test-key".to_string()),
            deployment_name: None,
            api_version: None,
            api_key_env: None,
            model_alias: None,
            models: vec![],
        }
    }

    #[test]
    fn provider_aliases_construct_expected_implementations() {
        let google = provider_from_config(&provider_cfg("google", "gemini"))
            .expect("gemini alias should construct google provider");
        assert!(format!("{google:?}").contains("GoogleProvider"));

        let ollama = provider_from_config(&provider_cfg("ollama", "ollama"))
            .expect("ollama kind should construct ollama provider");
        assert!(format!("{ollama:?}").contains("OllamaProvider"));

        let groq = provider_from_config(&provider_cfg("groq", "groq"))
            .expect("groq kind should construct groq provider");
        assert!(format!("{groq:?}").contains("GroqProvider"));

        let deepseek = provider_from_config(&provider_cfg("deepseek", "deepseek"))
            .expect("deepseek kind should construct deepseek provider");
        assert!(format!("{deepseek:?}").contains("DeepSeekProvider"));

        let mistral = provider_from_config(&provider_cfg("mistral", "mistral"))
            .expect("mistral kind should construct mistral provider");
        assert!(format!("{mistral:?}").contains("MistralProvider"));

        let openrouter = provider_from_config(&provider_cfg("openrouter", "openrouter"))
            .expect("openrouter kind should construct openai-compatible provider");
        assert!(format!("{openrouter:?}").contains("OpenAiProvider"));

        let perplexity = provider_from_config(&provider_cfg("perplexity", "perplexity"))
            .expect("perplexity kind should construct openai-compatible provider");
        assert!(format!("{perplexity:?}").contains("OpenAiProvider"));
    }

    #[derive(Debug)]
    struct SequenceProvider {
        completions: StdMutex<Vec<Result<CompletionResponse, ModelError>>>,
        stream_results: StdMutex<Vec<Result<String, ModelError>>>,
    }

    impl SequenceProvider {
        fn from_completion_results(results: Vec<Result<CompletionResponse, ModelError>>) -> Self {
            Self {
                completions: StdMutex::new(results.into_iter().rev().collect()),
                stream_results: StdMutex::new(Vec::new()),
            }
        }

        fn from_stream_results(results: Vec<Result<String, ModelError>>) -> Self {
            Self {
                completions: StdMutex::new(Vec::new()),
                stream_results: StdMutex::new(results.into_iter().rev().collect()),
            }
        }
    }

    #[async_trait::async_trait]
    impl ModelProvider for SequenceProvider {
        async fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> Result<CompletionResponse, ModelError> {
            self.completions
                .lock()
                .expect("completions lock poisoned")
                .pop()
                .expect("completion result should exist")
        }

        async fn complete_stream(
            &self,
            _request: &CompletionRequest,
        ) -> Result<mpsc::Receiver<StreamEvent>, ModelError> {
            let next = self
                .stream_results
                .lock()
                .expect("stream lock poisoned")
                .pop()
                .expect("stream result should exist");
            match next {
                Ok(text) => {
                    let (tx, rx) = mpsc::channel(2);
                    let response = CompletionResponse {
                        content: Some(text.clone()),
                        finish_reason: Some(FinishReason::Stop),
                        usage: Usage::default(),
                        tool_calls: vec![],
                    };
                    tx.send(StreamEvent::TextDelta(text))
                        .await
                        .expect("delta send should succeed");
                    tx.send(StreamEvent::Done(response))
                        .await
                        .expect("done send should succeed");
                    Ok(rx)
                }
                Err(err) => Err(err),
            }
        }
    }

    fn routed_provider_with_fallback() -> RoutedModelProvider {
        let models = ModelsConfig {
            fallbacks: vec![rune_config::ModelFallbackChainConfig {
                name: "default-chat".into(),
                chain: vec!["primary".into(), "secondary".into()],
            }],
            providers: vec![
                ModelProviderConfig {
                    name: "primary".into(),
                    kind: "openai".into(),
                    base_url: String::new(),
                    api_key: Some("test-key".into()),
                    deployment_name: None,
                    api_version: None,
                    api_key_env: None,
                    model_alias: None,
                    models: vec![ConfiguredModel::Id("primary".into())],
                },
                ModelProviderConfig {
                    name: "secondary".into(),
                    kind: "openai".into(),
                    base_url: String::new(),
                    api_key: Some("test-key".into()),
                    deployment_name: None,
                    api_version: None,
                    api_key_env: None,
                    model_alias: None,
                    models: vec![ConfiguredModel::Id("secondary".into())],
                },
            ],
            ..Default::default()
        };

        RoutedModelProvider::from_models_config(&models)
            .expect("routed provider should build")
            .with_circuit_breaker_settings(2, Duration::from_secs(60))
    }

    fn request_for(model: &str) -> CompletionRequest {
        CompletionRequest {
            stable_prefix_messages: None,
            stable_prefix_tools: None,
            model: Some(model.to_string()),
            messages: vec![ChatMessage {
                role: Role::User,
                content: Some("ping".to_string()),
                content_parts: None,
                name: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            temperature: None,
            max_tokens: None,
            tools: None,
        }
    }

    #[tokio::test]
    async fn retriable_failures_open_circuit_and_fallback_succeeds() {
        let mut routed = routed_provider_with_fallback();
        routed.register_provider(
            "primary",
            Arc::new(SequenceProvider::from_completion_results(vec![
                Err(ModelError::Transient("first".into())),
                Err(ModelError::Transient("second".into())),
            ])),
        );
        routed.register_provider(
            "secondary",
            Arc::new(SequenceProvider::from_completion_results(vec![
                Ok(CompletionResponse {
                    content: Some("fallback-ok".into()),
                    finish_reason: Some(FinishReason::Stop),
                    usage: Usage::default(),
                    tool_calls: vec![],
                }),
                Ok(CompletionResponse {
                    content: Some("fallback-open-circuit".into()),
                    finish_reason: Some(FinishReason::Stop),
                    usage: Usage::default(),
                    tool_calls: vec![],
                }),
            ])),
        );

        let first = routed
            .complete(&request_for("primary"))
            .await
            .expect("fallback should succeed after first failure");
        assert_eq!(first.content.as_deref(), Some("fallback-ok"));

        let second = routed
            .complete(&request_for("primary"))
            .await
            .expect("fallback should succeed after circuit opens");
        assert_eq!(second.content.as_deref(), Some("fallback-open-circuit"));

        let state = routed
            .circuit_breakers
            .lock()
            .expect("circuit breaker lock poisoned");
        let primary = state.get("primary").expect("primary breaker should exist");
        assert_eq!(primary.failures, 2);
        assert!(primary.opened_at.is_some());
    }

    #[tokio::test]
    async fn stream_failures_fallback_then_open_circuit() {
        let mut routed = routed_provider_with_fallback();
        routed.register_provider(
            "primary",
            Arc::new(SequenceProvider::from_stream_results(vec![
                Err(ModelError::Transient("stream fail".into())),
                Err(ModelError::Transient("stream fail again".into())),
            ])),
        );
        routed.register_provider(
            "secondary",
            Arc::new(SequenceProvider::from_stream_results(vec![
                Ok("secondary-stream-one".into()),
                Ok("secondary-stream-two".into()),
            ])),
        );

        let first = routed
            .complete_stream(&request_for("primary"))
            .await
            .expect("fallback stream should succeed after first failure");
        let events: Vec<_> = collect_stream_events(first).await;
        assert_eq!(
            text_deltas(&events),
            vec!["secondary-stream-one".to_string()]
        );

        let second = routed
            .complete_stream(&request_for("primary"))
            .await
            .expect("fallback stream should succeed after circuit opens");
        let events: Vec<_> = collect_stream_events(second).await;
        assert_eq!(
            text_deltas(&events),
            vec!["secondary-stream-two".to_string()]
        );

        let state = routed
            .circuit_breakers
            .lock()
            .expect("circuit breaker lock poisoned");
        let primary = state.get("primary").expect("primary breaker should exist");
        assert_eq!(primary.failures, 2);
        assert!(primary.opened_at.is_some());
    }

    #[tokio::test]
    async fn stream_open_circuit_blocks_without_fallbacks() {
        let models = ModelsConfig {
            providers: vec![ModelProviderConfig {
                name: "primary".into(),
                kind: "openai".into(),
                base_url: String::new(),
                api_key: Some("test-key".into()),
                deployment_name: None,
                api_version: None,
                api_key_env: None,
                model_alias: None,
                models: vec![ConfiguredModel::Id("primary".into())],
            }],
            ..Default::default()
        };

        let mut routed = RoutedModelProvider::from_models_config(&models)
            .expect("routed provider should build")
            .with_circuit_breaker_settings(1, Duration::from_secs(60));
        routed.register_provider(
            "primary",
            Arc::new(SequenceProvider::from_stream_results(vec![Err(
                ModelError::Transient("stream fail".into()),
            )])),
        );

        let err = routed
            .complete_stream(&request_for("primary"))
            .await
            .expect_err("first stream call should fail and open circuit");
        assert!(err.is_retriable());

        let err = routed
            .complete_stream(&request_for("primary"))
            .await
            .expect_err("second stream call should be blocked by open circuit");
        match err {
            ModelError::Transient(message) => {
                assert!(message.contains("circuit breaker open"));
                assert!(message.contains("primary"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    async fn collect_stream_events(mut rx: mpsc::Receiver<StreamEvent>) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        events
    }

    fn text_deltas(events: &[StreamEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::TextDelta(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}
