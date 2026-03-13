#![doc = "Model provider abstraction for Rune — Azure OpenAI, OpenAI, and extensible providers."]

mod error;
mod provider;
mod types;

pub use error::ModelError;
pub use provider::ModelProvider;
pub use provider::anthropic::AnthropicProvider;
pub use provider::azure::AzureOpenAiProvider;
pub use provider::azure_foundry::AzureFoundryProvider;
pub use provider::openai::OpenAiProvider;
pub use types::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, FunctionCall,
    FunctionDefinition, Role, ToolCallRequest, ToolDefinition, Usage,
};

use rune_config::ModelProviderConfig;

/// Build a `Box<dyn ModelProvider>` from a [`ModelProviderConfig`].
///
/// Provider selection is driven by `provider_name`:
/// - `"azure"` or `"azure_openai"` → [`AzureOpenAiProvider`]
/// - `"openai"` (or anything else for now) → [`OpenAiProvider`]
pub fn provider_from_config(
    cfg: &ModelProviderConfig,
) -> Result<Box<dyn ModelProvider>, ModelError> {
    let api_key = resolve_api_key(cfg)?;

    match cfg.name.to_lowercase().as_str() {
        "anthropic" => Ok(Box::new(AnthropicProvider::direct(&api_key))),
        "anthropic_azure" | "azure_anthropic" => {
            let api_version = cfg.api_version.as_deref().unwrap_or("2023-06-01");
            Ok(Box::new(AnthropicProvider::azure(
                &cfg.base_url,
                &api_key,
                api_version,
            )))
        }
        "azure" | "azure_openai" => {
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
        _ => Ok(Box::new(OpenAiProvider::new(&cfg.base_url, &api_key))),
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
