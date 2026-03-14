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
    FunctionDefinition, Role, ToolCallRequest, ToolDefinition, Usage,
};

use rune_config::ModelProviderConfig;
use rune_config::{ModelResolutionError, ModelsConfig};
use std::collections::HashMap;
use std::sync::Arc;

/// Build a `Box<dyn ModelProvider>` from a [`ModelProviderConfig`].
///
/// Provider selection is driven by `provider_name`:
/// - `"anthropic"` → [`AnthropicProvider`]
/// - `"azure"` or `"azure_openai"` → [`AzureOpenAiProvider`]
/// - `"openai"` → [`OpenAiProvider`]
/// - `"google"` or `"gemini"` → [`GoogleProvider`]
/// - `"ollama"` → [`OllamaProvider`]
/// - `"groq"` → [`GroqProvider`]
/// - `"deepseek"` → [`DeepSeekProvider`]
/// - `"mistral"` → [`MistralProvider`]
/// - `"bedrock"` or `"aws-bedrock"` → [`BedrockProvider`]
/// - Anything else → [`OpenAiProvider`] (generic OpenAI-compatible fallback)
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
                    &api_key,
                    api_version,
                )))
            }
        }
        "anthropic_azure" | "azure_anthropic" => {
            let api_key = resolve_api_key(cfg)?;
            let api_version = cfg.api_version.as_deref().unwrap_or("2023-06-01");
            Ok(Box::new(AnthropicProvider::azure(
                &cfg.base_url,
                &api_key,
                api_version,
            )))
        }
        "azure" | "azure_openai" | "azure-openai" => {
            let api_key = resolve_api_key(cfg)?;
            let deployment = cfg.deployment_name.as_deref().ok_or_else(|| {
                ModelError::Configuration("Azure provider requires deployment_name".into())
            })?;
            let api_version = cfg.api_version.as_deref().ok_or_else(|| {
                ModelError::Configuration("Azure provider requires api_version".into())
            })?;
            Ok(Box::new(AzureOpenAiProvider::new(
                &cfg.base_url,
                deployment,
                api_version,
                &api_key,
            )))
        }
        "azure-foundry" | "azure-ai" => {
            let api_key = resolve_api_key(cfg)?;
            Ok(Box::new(AzureFoundryProvider::with_api_version(
                &cfg.base_url,
                &api_key,
                cfg.api_version.as_deref().unwrap_or("2023-06-01"),
            )))
        }
        "openai" => {
            let api_key = resolve_api_key(cfg)?;
            let is_azure = cfg.base_url.contains(".azure.com") || cfg.base_url.contains("azure");
            if is_azure {
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
        "bedrock" | "aws-bedrock" | "aws_bedrock" => {
            let (access_key_id, secret_access_key) = resolve_aws_credentials(cfg)?;
            let region = cfg
                .deployment_name
                .as_deref()
                .unwrap_or_default();
            let endpoint_override = if cfg.base_url.is_empty() {
                None
            } else {
                Some(cfg.base_url.as_str())
            };
            Ok(Box::new(BedrockProvider::new(
                region,
                &access_key_id,
                &secret_access_key,
                endpoint_override,
            )))
        }
        _ => {
            let api_key = resolve_api_key(cfg)?;
            Ok(Box::new(OpenAiProvider::new(&cfg.base_url, &api_key)))
        }
    }
}

/// A provider router that dispatches requests by configured `provider/model`
/// inventory, while preserving legacy single-provider raw model behavior.
#[derive(Debug)]
pub struct RoutedModelProvider {
    models: ModelsConfig,
    providers: HashMap<String, Arc<dyn ModelProvider>>,
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
        })
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

        let resolved = self
            .models
            .resolve_model(model_ref)
            .map_err(map_resolution_error)?;
        let provider = self.providers.get(&resolved.provider.name).ok_or_else(|| {
            ModelError::Configuration(format!(
                "provider '{}' is configured in the model inventory but not available",
                resolved.provider.name
            ))
        })?;

        let mut routed_request = request.clone();
        routed_request.model = Some(resolved.raw_model.to_string());
        provider.complete(&routed_request).await
    }
}

fn resolve_api_key(cfg: &ModelProviderConfig) -> Result<String, ModelError> {
    // Direct api_key takes precedence
    if let Some(ref key) = cfg.api_key {
        if !key.is_empty() {
            return Ok(key.clone());
        }
    }
    // Fall back to env var
    let env_var = cfg.api_key_env.as_deref().unwrap_or("OPENAI_API_KEY");
    std::env::var(env_var).map_err(|_| {
        ModelError::Auth(format!(
            "API key env var '{env_var}' not set for provider '{}'",
            cfg.name
        ))
    })
}

/// Resolve AWS credentials for Bedrock.
///
/// Checks the `api_key` field (format: `ACCESS_KEY_ID:SECRET_ACCESS_KEY`),
/// then falls back to `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` env vars.
fn resolve_aws_credentials(
    cfg: &ModelProviderConfig,
) -> Result<(String, String), ModelError> {
    // Check direct api_key with colon-separated format
    if let Some(ref key) = cfg.api_key {
        if !key.is_empty() {
            if let Some((access, secret)) = key.split_once(':') {
                if !access.is_empty() && !secret.is_empty() {
                    return Ok((access.to_string(), secret.to_string()));
                }
            }
        }
    }

    // Fall back to env vars specified in config
    if let Some(ref env_var) = cfg.api_key_env {
        if let Ok(combined) = std::env::var(env_var) {
            if let Some((access, secret)) = combined.split_once(':') {
                if !access.is_empty() && !secret.is_empty() {
                    return Ok((access.to_string(), secret.to_string()));
                }
            }
        }
    }

    // Standard AWS env vars
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
