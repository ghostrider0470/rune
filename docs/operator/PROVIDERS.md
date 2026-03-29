# Providers

This is the operator-facing entry doc for Rune model/provider setup.

## Current provider direction

Rune is explicitly Azure-oriented while still supporting broader provider abstraction.

Current provider-related reference surfaces:
- Azure AI Foundry / Azure OpenAI are first-class requirements
- OpenAI and Anthropic provider paths are part of the active runtime shape

## Current canonical references

Use these docs for the current contract picture:
- [`../AZURE-COMPATIBILITY.md`](../AZURE-COMPATIBILITY.md)
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md)
- [`../INDEX.md`](../INDEX.md)
- [`../../rune-plan.md`](../../rune-plan.md)

## Current operator use

Use this doc as the provider entrypoint for:
- where provider setup and Azure-oriented expectations live
- how to navigate from high-level provider questions into the deeper compatibility/runtime docs

---

## Implemented providers

Rune ships 10 model providers, all config-driven through `config.toml`:

| Provider kind | Config value | Auth method | Notes |
|---|---|---|---|
| OpenAI | `openai` | API key via `api_key` or `OPENAI_API_KEY` | Default fallback for unknown provider kinds |
| Anthropic | `anthropic` | API key via `api_key` or `ANTHROPIC_API_KEY` | Also supports Azure-hosted Anthropic (`azure-anthropic`) |
| Azure OpenAI | `azure_openai` / `azure` / `azure-openai` | API key | Requires `deployment_name` and `api_version` in config |
| Azure AI Foundry | `azure_foundry` | API key | Azure AI Foundry-hosted models |
| Google (Gemini) | `google` / `gemini` | API key | Google Gemini API |
| Ollama | `ollama` | None (local) | Default base: `http://localhost:11434` |
| Groq | `groq` | API key | Groq inference API |
| DeepSeek | `deepseek` | API key | DeepSeek API |
| Mistral | `mistral` | API key | Mistral API |
| AWS Bedrock | `bedrock` / `aws-bedrock` / `aws_bedrock` | AWS credentials | Region from `deployment_name` or `AWS_DEFAULT_REGION`; credentials as `ACCESS_KEY_ID:SECRET_ACCESS_KEY` or separate env vars |

### Provider configuration example

```toml
[[models.providers]]
name = "primary"
kind = "openai"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
models = ["gpt-4o"]

[[models.providers]]
name = "azure"
kind = "azure_openai"
base_url = "https://my-resource.openai.azure.com"
deployment_name = "gpt-4o-deployment"
api_version = "2024-06-01"
api_key_env = "AZURE_OPENAI_API_KEY"
models = ["gpt-4o"]

[[models.providers]]
name = "local"
kind = "ollama"
base_url = "http://localhost:11434"
models = ["llama3"]
```

### API key resolution order

For each provider, credentials are resolved in this order:

1. Direct `api_key` field in the provider config block
2. Environment variable named in `api_key_env`
3. Standard provider default (e.g., `OPENAI_API_KEY` for OpenAI-compatible providers)

AWS Bedrock uses a separate flow: `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` env vars, or a colon-separated credential string in the standard key field.

---

## Fallback chains

Fallback chains allow automatic failover when a primary model encounters a retriable error.

### Configuration

```toml
[models]
default_model = "gpt-4o"
image_model = "gpt-4o"

[[models.fallbacks]]
name = "primary-chain"
chain = ["gpt-4o", "claude-sonnet-4-20250514", "llama3"]

[[models.image_fallbacks]]
name = "image-chain"
chain = ["gpt-4o", "claude-sonnet-4-20250514"]
```

### Runtime behavior

`RoutedModelProvider` in `rune-models` handles fallback execution:
- Dispatches to the primary provider for the requested model
- On **retriable errors only** (rate-limit, transient 5xx, quota exhaustion, HTTP transport failure), walks the configured fallback chain sequentially
- **Non-retriable errors** (auth failure, model not found, invalid request) surface immediately without fallback
- Returns the last error if the entire chain is exhausted

### CLI inspection

- `rune models fallbacks` — show configured text fallback chains
- `rune models image-fallbacks` — show configured image fallback chains

---

## Provider scanning

`rune models scan` probes locally reachable providers and reports available models.

**Current scope:** Ollama only. Calls the native `/api/tags` endpoint and returns model name, size, and modification timestamp.

Non-Ollama providers are skipped. Broader cloud provider probing will follow when safe probe semantics are defined.

---

## Azure-specific setup

Azure OpenAI and Azure AI Foundry are first-class providers. Key config differences from generic OpenAI:

- **`deployment_name`** — required; maps to the Azure deployment, not the model name
- **`api_version`** — required; Azure API version string (e.g., `2024-06-01`)
- **`base_url`** — the Azure resource endpoint (e.g., `https://my-resource.openai.azure.com`)

For detailed Azure compatibility requirements, see [`../AZURE-COMPATIBILITY.md`](../AZURE-COMPATIBILITY.md).

---

## Remaining gaps (issue #72)

| Gap | Description |
|---|---|
| `models auth` CLI | Auth status/inspection command exists; secret mutation still uses `rune config set` or direct `config.toml` editing |
| Per-agent auth order | Config structure exists in `rune-config` (`auth_orders`) but no CLI surface to inspect or mutate |
| Azure setup wizard | Azure providers work but require manual `config.toml` editing; no guided setup flow |
| Scan breadth | `models scan` probes Ollama only; cloud provider probing not yet implemented |

---

## Read next

- use [`../AZURE-COMPATIBILITY.md`](../AZURE-COMPATIBILITY.md) when you need provider/platform compatibility detail
- use [`../../rune-plan.md`](../../rune-plan.md) when the question is really about strategic provider direction
- use [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) when you need runtime semantics behind model/provider behavior
- use [`../reference/CLI.md`](../reference/CLI.md) for the `models` CLI command reference
