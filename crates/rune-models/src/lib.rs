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
    ChatMessage, CompletionRequest, CompletionResponse, ContentPart, FinishReason, FunctionCall,
    FunctionDefinition, Role, StreamEvent, ToolCallRequest, ToolDefinition, Usage,
};

use rune_config::ModelProviderConfig;
use rune_config::{ModelResolutionError, ModelsConfig};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;

/// Validate that an Azure API version follows the `YYYY-MM-DD` or
/// `YYYY-MM-DD-preview` format.  Returns `Ok(())` on success or a
/// [`ModelError::Configuration`] describing the problem.
fn validate_azure_api_version(version: &str) -> Result<(), ModelError> {
    // Accepted formats: "2024-06-01", "2024-02-15-preview"
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

/// Check whether a URL points to an Azure endpoint (AI Foundry, Azure OpenAI,
/// or any `*.azure.com` / `*.azure-api.net` host).  Used by the OpenAI-compatible
/// provider path and the default fallback to switch from `Authorization: Bearer`
/// to the `api-key` header that Azure expects.
fn is_azure_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains(".azure.com")
        || lower.contains(".azure-api.net")
        || lower.contains("azure.cognitiveservices")
}

/// Validate that an Azure provider has a non-empty endpoint URL.
fn validate_azure_endpoint(base_url: &str, provider_kind: &str) -> Result<(), ModelError> {
    if base_url.trim().is_empty() {
        return Err(ModelError::Configuration(format!(
            "{provider_kind} provider requires a non-empty base_url / endpoint"
        )));
    }
    Ok(())
}

/// Validate that an Azure deployment name is safe for URL path embedding.
///
/// Deployment names are interpolated directly into the Azure OpenAI URL path
/// (`/openai/deployments/{name}/…`), so they must be non-empty, non-whitespace,
/// and must not contain characters that would break the URL structure.
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
            validate_azure_endpoint(&cfg.base_url, "Anthropic Azure")?;
            let api_key = resolve_api_key(cfg)?;
            let api_version = cfg.api_version.as_deref().unwrap_or("2023-06-01");
            validate_azure_api_version(api_version)?;
            Ok(Box::new(AnthropicProvider::azure(
                &cfg.base_url,
                &api_key,
                api_version,
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
            let region = cfg.deployment_name.as_deref().unwrap_or_default();
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
            let is_azure = is_azure_url(&cfg.base_url);
            if is_azure {
                Ok(Box::new(OpenAiProvider::azure(&cfg.base_url, &api_key)))
            } else {
                Ok(Box::new(OpenAiProvider::new(&cfg.base_url, &api_key)))
            }
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

        // Try the primary model first.
        let primary_result = self.dispatch_single(model_ref, request).await;

        // On retriable failure, walk the fallback chain (if one is configured).
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
                    // Non-retriable error from a fallback — stop the chain.
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

        // For streaming, dispatch to primary only (no fallback chain).
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
        provider.complete_stream(&routed_request).await
    }
}

impl RoutedModelProvider {
    /// Resolve and dispatch a single `model_ref` to its provider.
    async fn dispatch_single(
        &self,
        model_ref: &str,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
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
fn resolve_aws_credentials(cfg: &ModelProviderConfig) -> Result<(String, String), ModelError> {
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn unknown_provider_kind_falls_back_to_openai_compatible_provider() {
        let provider = provider_from_config(&provider_cfg("compat", "compat-openai"))
            .expect("unknown provider kinds should fall back to openai-compatible provider");
        assert!(format!("{provider:?}").contains("OpenAiProvider"));
    }

    #[test]
    fn empty_kind_falls_back_to_provider_name() {
        let mut cfg = provider_cfg("gemini", "");
        cfg.api_key = Some("google-test-key".to_string());

        let provider = provider_from_config(&cfg)
            .expect("empty kind should fall back to provider name for routing");
        assert!(format!("{provider:?}").contains("GoogleProvider"));
    }

    #[test]
    fn bedrock_uses_env_credentials_and_region_fallbacks() {
        let mut cfg = provider_cfg("bedrock", "bedrock");
        cfg.api_key = None;
        cfg.api_key_env = Some("RUNE_TEST_BEDROCK_CREDS".to_string());
        cfg.deployment_name = None;

        unsafe {
            std::env::set_var("RUNE_TEST_BEDROCK_CREDS", "AKIDEXAMPLE:SECRETEXAMPLE");
            std::env::set_var("AWS_REGION", "eu-central-1");
        }

        let provider = provider_from_config(&cfg)
            .expect("bedrock provider should resolve credentials from configured env var");
        let debug = format!("{provider:?}");
        assert!(debug.contains("BedrockProvider"));

        unsafe {
            std::env::remove_var("RUNE_TEST_BEDROCK_CREDS");
            std::env::remove_var("AWS_REGION");
        }
    }

    #[test]
    fn missing_api_key_reports_configured_env_var_name() {
        let mut cfg = provider_cfg("google", "google");
        cfg.api_key = None;
        cfg.api_key_env = Some("RUNE_TEST_MISSING_KEY".to_string());

        unsafe {
            std::env::remove_var("RUNE_TEST_MISSING_KEY");
        }

        let err = provider_from_config(&cfg).expect_err("missing api key should error");
        assert!(err.to_string().contains("RUNE_TEST_MISSING_KEY"));
        assert!(err.to_string().contains("google"));
    }

    // --- Azure API-version validation ---

    #[test]
    fn valid_azure_api_versions() {
        assert!(validate_azure_api_version("2024-06-01").is_ok());
        assert!(validate_azure_api_version("2025-01-01").is_ok());
        assert!(validate_azure_api_version("2023-06-01").is_ok());
        assert!(validate_azure_api_version("2024-02-15-preview").is_ok());
    }

    #[test]
    fn invalid_azure_api_version_format() {
        let err = validate_azure_api_version("v1").unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));
        assert!(err.to_string().contains("invalid Azure API version 'v1'"));

        let err = validate_azure_api_version("2024/06/01").unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));

        let err = validate_azure_api_version("2024-6-1").unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));

        let err = validate_azure_api_version("latest").unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));

        let err = validate_azure_api_version("").unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));
    }

    #[test]
    fn azure_config_rejects_invalid_api_version() {
        let cfg = ModelProviderConfig {
            name: "azure".into(),
            kind: "azure-openai".into(),
            base_url: "https://test.openai.azure.com".into(),
            deployment_name: Some("gpt-4o".into()),
            api_version: Some("v1-bad".into()),
            api_key: Some("test-key".into()),
            api_key_env: None,
            model_alias: None,
            models: vec![],
        };
        let err = provider_from_config(&cfg).unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));
        assert!(err.to_string().contains("invalid Azure API version"));
    }

    #[test]
    fn azure_config_rejects_empty_endpoint() {
        let cfg = ModelProviderConfig {
            name: "azure".into(),
            kind: "azure-openai".into(),
            base_url: String::new(),
            deployment_name: Some("gpt-4o".into()),
            api_version: Some("2024-06-01".into()),
            api_key: Some("test-key".into()),
            api_key_env: None,
            model_alias: None,
            models: vec![],
        };
        let err = provider_from_config(&cfg).unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));
        assert!(err.to_string().contains("non-empty base_url"));
    }

    #[test]
    fn azure_foundry_config_rejects_empty_endpoint() {
        let cfg = ModelProviderConfig {
            name: "foundry".into(),
            kind: "azure-foundry".into(),
            base_url: String::new(),
            deployment_name: None,
            api_version: None,
            api_key: Some("test-key".into()),
            api_key_env: None,
            model_alias: None,
            models: vec![],
        };
        let err = provider_from_config(&cfg).unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));
        assert!(err.to_string().contains("non-empty base_url"));
    }

    // --- Azure deployment-name validation ---

    #[test]
    fn valid_azure_deployment_names() {
        assert!(validate_azure_deployment_name("gpt-4o").is_ok());
        assert!(validate_azure_deployment_name("my-deployment-v2").is_ok());
        assert!(validate_azure_deployment_name("GPT4o_prod").is_ok());
        assert!(validate_azure_deployment_name("a").is_ok());
    }

    #[test]
    fn azure_deployment_name_rejects_empty() {
        let err = validate_azure_deployment_name("").unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn azure_deployment_name_rejects_whitespace_only() {
        let err = validate_azure_deployment_name("   ").unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn azure_deployment_name_rejects_path_unsafe_chars() {
        for (name, bad_char) in [
            ("my/deploy", '/'),
            ("deploy?v=1", '?'),
            ("deploy#frag", '#'),
            ("deploy%20name", '%'),
            ("has space", ' '),
        ] {
            let err = validate_azure_deployment_name(name).unwrap_err();
            assert!(matches!(err, ModelError::Configuration(_)));
            assert!(
                err.to_string().contains(&format!("'{bad_char}'")),
                "expected mention of '{bad_char}' in error: {err}"
            );
        }
    }

    #[test]
    fn azure_config_rejects_path_unsafe_deployment_name() {
        let cfg = ModelProviderConfig {
            name: "azure".into(),
            kind: "azure-openai".into(),
            base_url: "https://test.openai.azure.com".into(),
            deployment_name: Some("my/bad-deploy".into()),
            api_version: Some("2024-06-01".into()),
            api_key: Some("test-key".into()),
            api_key_env: None,
            model_alias: None,
            models: vec![],
        };
        let err = provider_from_config(&cfg).unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));
        assert!(err.to_string().contains("invalid character"));
    }

    #[test]
    fn anthropic_azure_config_rejects_empty_endpoint() {
        let cfg = ModelProviderConfig {
            name: "anthropic-azure".into(),
            kind: "anthropic_azure".into(),
            base_url: String::new(),
            deployment_name: None,
            api_version: Some("2023-06-01".into()),
            api_key: Some("test-key".into()),
            api_key_env: None,
            model_alias: None,
            models: vec![],
        };
        let err = provider_from_config(&cfg).unwrap_err();
        assert!(matches!(err, ModelError::Configuration(_)));
        assert!(err.to_string().contains("non-empty base_url"));
    }
}
