# Phases 8–14 — Implementation Specifications

> Generated 2026-03-15. Every type, endpoint, migration, and test signature
> is specified so that an LLM can implement each phase without guessing.

---

## Table of Contents

1. [Phase 8 — TTS Backend + UI](#phase-8--tts-backend--ui)
2. [Phase 9 — STT Backend + UI](#phase-9--stt-backend--ui)
3. [Phase 10 — Hybrid Memory Search (Backend)](#phase-10--hybrid-memory-search-backend)
4. [Phase 11 — Device Pairing (Backend)](#phase-11--device-pairing-backend)
5. [Phase 12 — Session Enhancements (Backend + UI)](#phase-12--session-enhancements-backend--ui)
6. [Phase 13 — Usage Analytics (Backend + UI)](#phase-13--usage-analytics-backend--ui)
7. [Phase 14 — Config Editor (Backend + UI)](#phase-14--config-editor-backend--ui)

---

## Phase 8 — TTS Backend + UI

### 8.1 New Crate: `crates/rune-tts/`

**Cargo.toml dependencies:**

```toml
[package]
name = "rune-tts"
version = "0.1.0"
edition = "2021"

[dependencies]
async-trait = "0.1"
bytes = "1"
reqwest = { version = "0.12", features = ["json", "stream"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["fs"] }
tracing = "0.1"
uuid = { version = "1", features = ["v7"] }
```

### 8.2 Rust Types

```rust
// crates/rune-tts/src/lib.rs

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// When TTS is automatically applied.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TtsAutoMode {
    /// TTS is disabled.
    #[default]
    Off,
    /// Every outbound assistant message is converted to audio.
    Always,
    /// Only messages replying to an inbound audio/voice message.
    Inbound,
    /// Only messages where the model includes a `[tts]` tag.
    Tagged,
}

/// TTS voice selection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TtsVoice {
    /// Provider-specific voice identifier (e.g. "alloy", "Rachel").
    pub id: String,
    /// Human-readable label for the UI.
    pub label: String,
}

/// Immutable request to synthesize speech.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TtsRequest {
    /// The text to convert to speech.
    pub text: String,
    /// Optional voice override. Falls back to config default.
    pub voice: Option<String>,
    /// Optional model override. Falls back to config default.
    pub model: Option<String>,
    /// Desired output format. Default: "mp3".
    pub format: Option<String>,
    /// Playback speed multiplier. Default: 1.0. Range: 0.25–4.0.
    pub speed: Option<f64>,
}

/// Successful TTS conversion result.
#[derive(Clone, Debug)]
pub struct TtsResult {
    /// Raw audio bytes.
    pub audio: Bytes,
    /// MIME type of the audio (e.g. "audio/mpeg").
    pub content_type: String,
    /// Duration estimate in seconds (if available from provider).
    pub duration_seconds: Option<f64>,
    /// Number of input characters billed.
    pub input_characters: usize,
}

/// Errors from TTS operations.
#[derive(Debug, Error)]
pub enum TtsError {
    #[error("TTS is disabled")]
    Disabled,

    #[error("provider not configured: {0}")]
    ProviderNotConfigured(String),

    #[error("invalid voice: {0}")]
    InvalidVoice(String),

    #[error("text is empty")]
    EmptyText,

    #[error("text exceeds maximum length ({length} > {max})")]
    TextTooLong { length: usize, max: usize },

    #[error("speed out of range ({speed}); must be 0.25–4.0")]
    SpeedOutOfRange { speed: f64 },

    #[error("provider API error ({status}): {body}")]
    ProviderApi { status: u16, body: String },

    #[error("rate limited; retry after {retry_after_seconds}s")]
    RateLimited { retry_after_seconds: u64 },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Pluggable TTS provider.
#[async_trait]
pub trait TtsProvider: Send + Sync {
    /// Convert text to speech audio.
    async fn synthesize(&self, request: &TtsRequest) -> Result<TtsResult, TtsError>;

    /// List available voices for this provider.
    fn available_voices(&self) -> Vec<TtsVoice>;

    /// Provider name for status reporting.
    fn provider_name(&self) -> &str;

    /// Maximum input character count.
    fn max_input_length(&self) -> usize;
}
```

```rust
// crates/rune-tts/src/openai.rs

use crate::{TtsError, TtsProvider, TtsRequest, TtsResult, TtsVoice};

/// OpenAI TTS provider using the `/v1/audio/speech` endpoint.
pub struct OpenAiTts {
    /// API key for authentication.
    api_key: String,
    /// Model to use (e.g. "tts-1", "tts-1-hd").
    model: String,
    /// Default voice (e.g. "alloy", "echo", "fable", "onyx", "nova", "shimmer").
    default_voice: String,
    /// Base URL. Default: "https://api.openai.com".
    base_url: String,
    /// HTTP client.
    client: reqwest::Client,
}

impl OpenAiTts {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        default_voice: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            default_voice: default_voice.into(),
            base_url: "https://api.openai.com".into(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
}
```

```rust
// crates/rune-tts/src/elevenlabs.rs

/// ElevenLabs TTS provider using the `/v1/text-to-speech/{voice_id}` endpoint.
pub struct ElevenLabsTts {
    /// API key for authentication.
    api_key: String,
    /// Default voice ID.
    default_voice_id: String,
    /// Model ID (e.g. "eleven_multilingual_v2").
    model_id: String,
    /// Base URL. Default: "https://api.elevenlabs.io".
    base_url: String,
    /// HTTP client.
    client: reqwest::Client,
}

impl ElevenLabsTts {
    pub fn new(
        api_key: impl Into<String>,
        default_voice_id: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            default_voice_id: default_voice_id.into(),
            model_id: model_id.into(),
            base_url: "https://api.elevenlabs.io".into(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
}
```

```rust
// crates/rune-tts/src/config.rs

use serde::{Deserialize, Serialize};
use crate::TtsAutoMode;

/// TTS configuration section for `AppConfig`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TtsConfig {
    /// Which provider to use: "openai" or "elevenlabs".
    pub provider: String,
    /// API key (or use api_key_env).
    #[serde(default)]
    pub api_key: Option<String>,
    /// Environment variable holding the API key.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Provider-specific model identifier.
    #[serde(default = "default_tts_model")]
    pub model: String,
    /// Default voice identifier.
    #[serde(default = "default_tts_voice")]
    pub voice: String,
    /// When to auto-apply TTS.
    #[serde(default)]
    pub auto_mode: TtsAutoMode,
    /// Maximum input character count. Default: 4096.
    #[serde(default = "default_max_input_length")]
    pub max_input_length: usize,
    /// Output audio format. Default: "mp3".
    #[serde(default = "default_format")]
    pub format: String,
    /// Custom base URL for the provider API.
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_tts_model() -> String { "tts-1".into() }
fn default_tts_voice() -> String { "alloy".into() }
fn default_max_input_length() -> usize { 4096 }
fn default_format() -> String { "mp3".into() }

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            provider: "openai".into(),
            api_key: None,
            api_key_env: None,
            model: default_tts_model(),
            voice: default_tts_voice(),
            auto_mode: TtsAutoMode::Off,
            max_input_length: default_max_input_length(),
            format: default_format(),
            base_url: None,
        }
    }
}
```

### 8.3 AppConfig Change

Add to `AppConfig` in `crates/rune-config/src/lib.rs`:

```rust
// Add field to AppConfig struct:
#[serde(default)]
pub tts: TtsConfig,
```

Where `TtsConfig` is re-exported from `rune-tts` or defined in `rune-config` with the same shape.

### 8.4 Gateway Routes

#### `GET /tts/status`

**Response 200:**

```json
{
  "enabled": true,
  "provider": "openai",
  "model": "tts-1",
  "voice": "alloy",
  "auto_mode": "off",
  "max_input_length": 4096,
  "format": "mp3",
  "available_voices": [
    { "id": "alloy", "label": "Alloy" },
    { "id": "echo", "label": "Echo" },
    { "id": "fable", "label": "Fable" },
    { "id": "onyx", "label": "Onyx" },
    { "id": "nova", "label": "Nova" },
    { "id": "shimmer", "label": "Shimmer" }
  ]
}
```

**Response when TTS not configured (200, not an error):**

```json
{
  "enabled": false,
  "provider": null,
  "model": null,
  "voice": null,
  "auto_mode": "off",
  "max_input_length": 4096,
  "format": "mp3",
  "available_voices": []
}
```

#### `POST /tts/enable`

**Request body:**

```json
{
  "auto_mode": "always"
}
```

`auto_mode` is optional; defaults to `"always"` if omitted.

**Response 200:**

```json
{
  "enabled": true,
  "auto_mode": "always"
}
```

**Error 400 — provider not configured:**

```json
{
  "code": "bad_request",
  "message": "TTS provider not configured; set [tts] section in config",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532a1-..."
}
```

#### `POST /tts/disable`

No request body.

**Response 200:**

```json
{
  "enabled": false,
  "auto_mode": "off"
}
```

#### `POST /tts/convert`

**Request body:**

```json
{
  "text": "Hello, this is a test of text to speech.",
  "voice": "nova",
  "model": "tts-1-hd",
  "format": "mp3",
  "speed": 1.0
}
```

Only `text` is required. All other fields are optional and fall back to config.

**Response 200:** Binary audio body with headers:

```
Content-Type: audio/mpeg
Content-Disposition: attachment; filename="tts-019532a1.mp3"
X-Tts-Duration-Seconds: 2.4
X-Tts-Input-Characters: 40
```

**Error 400 — empty text:**

```json
{
  "code": "bad_request",
  "message": "text is empty",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532a1-..."
}
```

**Error 400 — text too long:**

```json
{
  "code": "bad_request",
  "message": "text exceeds maximum length (5000 > 4096)",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532a1-..."
}
```

**Error 400 — speed out of range:**

```json
{
  "code": "bad_request",
  "message": "speed out of range (5.0); must be 0.25–4.0",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532a1-..."
}
```

**Error 400 — invalid voice:**

```json
{
  "code": "bad_request",
  "message": "invalid voice: unknown_voice",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532a1-..."
}
```

**Error 400 — TTS disabled:**

```json
{
  "code": "bad_request",
  "message": "TTS is disabled",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532a1-..."
}
```

**Error 502 — upstream provider failure:**

```json
{
  "code": "internal_error",
  "message": "provider API error (429): rate limit exceeded",
  "retriable": true,
  "approval_required": false,
  "request_id": "019532a1-..."
}
```

### 8.5 Route Handler Signatures

```rust
// crates/rune-gateway/src/routes.rs

/// Response for `GET /tts/status`.
#[derive(Serialize)]
pub struct TtsStatusResponse {
    pub enabled: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub voice: Option<String>,
    pub auto_mode: String,
    pub max_input_length: usize,
    pub format: String,
    pub available_voices: Vec<TtsVoiceInfo>,
}

#[derive(Serialize)]
pub struct TtsVoiceInfo {
    pub id: String,
    pub label: String,
}

/// Request body for `POST /tts/enable`.
#[derive(Deserialize)]
pub struct TtsEnableRequest {
    #[serde(default = "default_enable_mode")]
    pub auto_mode: String,
}
fn default_enable_mode() -> String { "always".into() }

/// Response for `POST /tts/enable` and `POST /tts/disable`.
#[derive(Serialize)]
pub struct TtsToggleResponse {
    pub enabled: bool,
    pub auto_mode: String,
}

/// Request body for `POST /tts/convert`.
#[derive(Deserialize)]
pub struct TtsConvertRequest {
    pub text: String,
    pub voice: Option<String>,
    pub model: Option<String>,
    pub format: Option<String>,
    pub speed: Option<f64>,
}

pub async fn tts_status(
    State(state): State<AppState>,
) -> Result<Json<TtsStatusResponse>, GatewayError> { todo!() }

pub async fn tts_enable(
    State(state): State<AppState>,
    Json(body): Json<TtsEnableRequest>,
) -> Result<Json<TtsToggleResponse>, GatewayError> { todo!() }

pub async fn tts_disable(
    State(state): State<AppState>,
) -> Result<Json<TtsToggleResponse>, GatewayError> { todo!() }

pub async fn tts_convert(
    State(state): State<AppState>,
    Json(body): Json<TtsConvertRequest>,
) -> Result<Response, GatewayError> { todo!() }
```

### 8.6 AppState Addition

```rust
// Add to AppState in crates/rune-gateway/src/state.rs:
/// TTS provider for speech synthesis. `None` when TTS is not configured.
pub tts_provider: Option<Arc<dyn rune_tts::TtsProvider>>,
/// Runtime TTS enable flag and auto_mode (mutable via enable/disable endpoints).
pub tts_state: Arc<tokio::sync::RwLock<TtsRuntimeState>>,
```

```rust
/// Mutable TTS runtime state toggled via API.
#[derive(Clone, Debug)]
pub struct TtsRuntimeState {
    pub enabled: bool,
    pub auto_mode: rune_tts::TtsAutoMode,
}
```

### 8.7 Edge Cases

- **Concurrent enable/disable**: `tts_state` is behind `RwLock`; enable/disable take a write lock. Convert takes a read lock. No race.
- **Provider not configured**: `tts_provider` is `None`. `tts_status` returns `enabled: false`. `tts_enable` returns 400. `tts_convert` returns 400.
- **Empty text after trim**: Trim whitespace before length check; reject if empty.
- **Unicode text length**: Count `text.chars().count()` not `text.len()` for the max-length check.
- **Upstream 429**: Map to `TtsError::RateLimited`. Gateway returns 502 with `retriable: true`.
- **Large response body**: Stream the provider response; do not buffer more than 50 MB (reject with 502 if exceeded).
- **Malformed JSON body**: Axum's `Json` extractor returns 422 automatically. No custom handling needed.

### 8.8 SQL Migrations

No new database tables for Phase 8. TTS is stateless (runtime toggle stored in memory; config in TOML/env).

### 8.9 Integration Test Scenarios

```rust
// crates/rune-gateway/tests/tts_tests.rs

/// GET /tts/status returns disabled state when no provider configured.
#[tokio::test]
async fn tts_status_returns_disabled_when_no_provider() { }

/// POST /tts/enable returns 400 when no provider configured.
#[tokio::test]
async fn tts_enable_rejects_when_no_provider() { }

/// POST /tts/convert returns 400 when TTS disabled.
#[tokio::test]
async fn tts_convert_rejects_when_disabled() { }

/// POST /tts/convert returns 400 on empty text.
#[tokio::test]
async fn tts_convert_rejects_empty_text() { }

/// POST /tts/convert returns 400 on text exceeding max length.
#[tokio::test]
async fn tts_convert_rejects_text_too_long() { }

/// POST /tts/convert returns 400 on speed out of range.
#[tokio::test]
async fn tts_convert_rejects_speed_out_of_range() { }

/// Full flow: enable -> convert -> verify audio bytes returned.
/// Uses a mock TtsProvider that returns deterministic bytes.
#[tokio::test]
async fn tts_full_flow_with_mock_provider() { }

/// POST /tts/disable sets auto_mode to off.
#[tokio::test]
async fn tts_disable_sets_off() { }

/// Concurrent enable and convert do not deadlock.
#[tokio::test]
async fn tts_concurrent_enable_convert() { }
```

```rust
// crates/rune-tts/src/lib.rs — unit tests

/// OpenAiTts rejects empty text before making HTTP call.
#[tokio::test]
async fn openai_rejects_empty_text() { }

/// OpenAiTts rejects text exceeding max_input_length.
#[tokio::test]
async fn openai_rejects_text_too_long() { }

/// OpenAiTts rejects speed outside 0.25–4.0.
#[tokio::test]
async fn openai_rejects_invalid_speed() { }

/// ElevenLabsTts rejects empty text.
#[tokio::test]
async fn elevenlabs_rejects_empty_text() { }
```

### 8.10 Acceptance Criteria

- [ ] `cargo build -p rune-tts` succeeds with zero warnings
- [ ] `cargo test -p rune-tts` passes all unit tests
- [ ] `GET /tts/status` returns correct JSON when provider is/is not configured
- [ ] `POST /tts/enable` and `POST /tts/disable` toggle runtime state
- [ ] `POST /tts/convert` with a mock provider returns audio bytes with correct `Content-Type`
- [ ] Empty text, oversized text, and invalid speed all return 400
- [ ] `TtsConfig` round-trips through TOML serialization
- [ ] `RUNE_TTS__PROVIDER`, `RUNE_TTS__API_KEY` env vars override file config
- [ ] Gateway integration tests pass with mock provider

---

## Phase 9 — STT Backend + UI

### 9.1 New Crate: `crates/rune-stt/`

**Cargo.toml dependencies:**

```toml
[package]
name = "rune-stt"
version = "0.1.0"
edition = "2021"

[dependencies]
async-trait = "0.1"
bytes = "1"
reqwest = { version = "0.12", features = ["json", "multipart", "stream"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["fs"] }
tracing = "0.1"
uuid = { version = "1", features = ["v7"] }
```

### 9.2 Rust Types

```rust
// crates/rune-stt/src/lib.rs

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Supported audio input formats.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    Mp3,
    Mp4,
    Mpeg,
    Mpga,
    M4a,
    Wav,
    Webm,
    Ogg,
    Flac,
}

impl AudioFormat {
    /// Infer format from file extension. Returns `None` for unknown extensions.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "mp3" => Some(Self::Mp3),
            "mp4" => Some(Self::Mp4),
            "mpeg" => Some(Self::Mpeg),
            "mpga" => Some(Self::Mpga),
            "m4a" => Some(Self::M4a),
            "wav" => Some(Self::Wav),
            "webm" => Some(Self::Webm),
            "ogg" => Some(Self::Ogg),
            "flac" => Some(Self::Flac),
            _ => None,
        }
    }

    /// MIME type string for this format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Mp4 => "audio/mp4",
            Self::Mpeg => "audio/mpeg",
            Self::Mpga => "audio/mpeg",
            Self::M4a => "audio/mp4",
            Self::Wav => "audio/wav",
            Self::Webm => "audio/webm",
            Self::Ogg => "audio/ogg",
            Self::Flac => "audio/flac",
        }
    }
}

/// Request to transcribe audio.
#[derive(Clone, Debug)]
pub struct SttRequest {
    /// Raw audio bytes.
    pub audio: Bytes,
    /// Audio format (used for Content-Type in multipart upload).
    pub format: AudioFormat,
    /// Filename hint for the multipart form.
    pub filename: String,
    /// Optional language hint (ISO 639-1, e.g. "en").
    pub language: Option<String>,
    /// Optional prompt/context to guide transcription.
    pub prompt: Option<String>,
}

/// Successful transcription result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SttResult {
    /// Transcribed text.
    pub text: String,
    /// Detected or specified language.
    pub language: Option<String>,
    /// Duration of the audio in seconds (if available).
    pub duration_seconds: Option<f64>,
}

/// Errors from STT operations.
#[derive(Debug, Error)]
pub enum SttError {
    #[error("STT is disabled")]
    Disabled,

    #[error("provider not configured: {0}")]
    ProviderNotConfigured(String),

    #[error("unsupported audio format: {0}")]
    UnsupportedFormat(String),

    #[error("audio is empty (0 bytes)")]
    EmptyAudio,

    #[error("audio exceeds maximum size ({size_mb:.1} MB > {max_mb} MB)")]
    AudioTooLarge { size_mb: f64, max_mb: usize },

    #[error("provider API error ({status}): {body}")]
    ProviderApi { status: u16, body: String },

    #[error("rate limited; retry after {retry_after_seconds}s")]
    RateLimited { retry_after_seconds: u64 },

    #[error("transcription returned empty text")]
    EmptyTranscription,

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Pluggable speech-to-text provider.
#[async_trait]
pub trait SttProvider: Send + Sync {
    /// Transcribe audio to text.
    async fn transcribe(&self, request: &SttRequest) -> Result<SttResult, SttError>;

    /// Provider name for status reporting.
    fn provider_name(&self) -> &str;

    /// Maximum audio file size in bytes.
    fn max_audio_size(&self) -> usize;
}
```

```rust
// crates/rune-stt/src/openai.rs

/// OpenAI Whisper STT provider using `/v1/audio/transcriptions`.
pub struct OpenAiStt {
    /// API key.
    api_key: String,
    /// Model name (e.g. "whisper-1").
    model: String,
    /// Base URL. Default: "https://api.openai.com".
    base_url: String,
    /// HTTP client.
    client: reqwest::Client,
    /// Max file size in bytes. Default: 25 MB (OpenAI limit).
    max_audio_size: usize,
}

impl OpenAiStt {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.openai.com".into(),
            client: reqwest::Client::new(),
            max_audio_size: 25 * 1024 * 1024,
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
}
```

```rust
// crates/rune-stt/src/config.rs

use serde::{Deserialize, Serialize};

/// STT configuration section for `AppConfig`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SttConfig {
    /// Provider: "openai".
    pub provider: String,
    /// API key (or use api_key_env).
    #[serde(default)]
    pub api_key: Option<String>,
    /// Env var holding the API key.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Model identifier. Default: "whisper-1".
    #[serde(default = "default_stt_model")]
    pub model: String,
    /// Max upload size in MB. Default: 25.
    #[serde(default = "default_max_upload_mb")]
    pub max_upload_mb: usize,
    /// Auto-transcribe audio attachments before processing. Default: true.
    #[serde(default = "default_auto_transcribe")]
    pub auto_transcribe: bool,
    /// Custom base URL.
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_stt_model() -> String { "whisper-1".into() }
fn default_max_upload_mb() -> usize { 25 }
fn default_auto_transcribe() -> bool { true }

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            provider: "openai".into(),
            api_key: None,
            api_key_env: None,
            model: default_stt_model(),
            max_upload_mb: default_max_upload_mb(),
            auto_transcribe: default_auto_transcribe(),
            base_url: None,
        }
    }
}
```

### 9.3 AppConfig Change

```rust
// Add to AppConfig:
#[serde(default)]
pub stt: SttConfig,
```

### 9.4 Gateway Routes

#### `GET /stt/status`

**Response 200:**

```json
{
  "enabled": true,
  "provider": "openai",
  "model": "whisper-1",
  "max_upload_mb": 25,
  "auto_transcribe": true
}
```

**Response when STT not configured:**

```json
{
  "enabled": false,
  "provider": null,
  "model": null,
  "max_upload_mb": 25,
  "auto_transcribe": false
}
```

#### `POST /stt/transcribe`

**Request:** `multipart/form-data` with fields:

| Field      | Type   | Required | Description                              |
|------------|--------|----------|------------------------------------------|
| `file`     | binary | yes      | Audio file                               |
| `language` | string | no       | ISO 639-1 language hint (e.g. "en")      |
| `prompt`   | string | no       | Context/prompt to guide transcription     |

**Response 200:**

```json
{
  "text": "Hello, this is a transcription test.",
  "language": "en",
  "duration_seconds": 3.2
}
```

**Error 400 — STT disabled:**

```json
{
  "code": "bad_request",
  "message": "STT is disabled",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532b2-..."
}
```

**Error 400 — empty audio:**

```json
{
  "code": "bad_request",
  "message": "audio is empty (0 bytes)",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532b2-..."
}
```

**Error 400 — audio too large:**

```json
{
  "code": "bad_request",
  "message": "audio exceeds maximum size (30.0 MB > 25 MB)",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532b2-..."
}
```

**Error 400 — unsupported format:**

```json
{
  "code": "bad_request",
  "message": "unsupported audio format: .avi",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532b2-..."
}
```

**Error 502 — upstream provider failure:**

```json
{
  "code": "internal_error",
  "message": "provider API error (500): internal server error",
  "retriable": true,
  "approval_required": false,
  "request_id": "019532b2-..."
}
```

### 9.5 Route Handler Signatures

```rust
// crates/rune-gateway/src/routes.rs

use axum::extract::Multipart;

#[derive(Serialize)]
pub struct SttStatusResponse {
    pub enabled: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub max_upload_mb: usize,
    pub auto_transcribe: bool,
}

#[derive(Serialize)]
pub struct SttTranscribeResponse {
    pub text: String,
    pub language: Option<String>,
    pub duration_seconds: Option<f64>,
}

pub async fn stt_status(
    State(state): State<AppState>,
) -> Result<Json<SttStatusResponse>, GatewayError> { todo!() }

pub async fn stt_transcribe(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<SttTranscribeResponse>, GatewayError> { todo!() }
```

### 9.6 AppState Addition

```rust
// Add to AppState:
/// STT provider for transcription. `None` when not configured.
pub stt_provider: Option<Arc<dyn rune_stt::SttProvider>>,
```

### 9.7 Auto-Transcribe Integration

In the `TurnExecutor` (or the channel ingest path before `TurnExecutor` is invoked):

1. When an inbound message contains an audio attachment, check `stt_config.auto_transcribe`.
2. If true, call `stt_provider.transcribe(...)` on the attachment bytes.
3. Prepend the transcription text to the user message: `"[Audio transcription]: {text}\n\n{original_message}"`.
4. If transcription fails, log a warning and pass the original message unchanged.

### 9.8 Edge Cases

- **No file in multipart**: Return 400 `"no audio file provided in multipart form"`.
- **Multiple files**: Only the first `file` field is used; additional fields are ignored.
- **Missing filename extension**: Try to infer from Content-Type header of the multipart part; if both fail, return 400 unsupported format.
- **Concurrent transcriptions**: Each call is independent; no shared mutable state. The reqwest client is clone-safe.
- **Auto-transcribe with no STT provider**: Skip silently (log debug message).
- **Zero-length audio produces empty text**: Return `SttError::EmptyTranscription`.

### 9.9 SQL Migrations

No new database tables for Phase 9. STT is stateless.

### 9.10 Integration Test Scenarios

```rust
// crates/rune-gateway/tests/stt_tests.rs

/// GET /stt/status returns disabled when no provider configured.
#[tokio::test]
async fn stt_status_disabled_when_no_provider() { }

/// POST /stt/transcribe returns 400 when STT disabled.
#[tokio::test]
async fn stt_transcribe_rejects_when_disabled() { }

/// POST /stt/transcribe returns 400 when no file provided.
#[tokio::test]
async fn stt_transcribe_rejects_missing_file() { }

/// POST /stt/transcribe returns 400 for oversized audio.
#[tokio::test]
async fn stt_transcribe_rejects_oversized_audio() { }

/// POST /stt/transcribe returns 400 for unsupported format.
#[tokio::test]
async fn stt_transcribe_rejects_unsupported_format() { }

/// Full flow with mock provider returns transcription text.
#[tokio::test]
async fn stt_transcribe_full_flow_with_mock() { }
```

```rust
// crates/rune-stt/src/lib.rs — unit tests

/// OpenAiStt rejects empty audio.
#[tokio::test]
async fn openai_stt_rejects_empty_audio() { }

/// OpenAiStt rejects oversized audio.
#[tokio::test]
async fn openai_stt_rejects_oversized_audio() { }

/// AudioFormat::from_extension handles all known formats.
#[test]
fn audio_format_from_extension_all_known() { }

/// AudioFormat::from_extension returns None for unknown.
#[test]
fn audio_format_from_extension_unknown() { }

/// AudioFormat::mime_type returns correct MIME strings.
#[test]
fn audio_format_mime_types() { }
```

### 9.11 Acceptance Criteria

- [ ] `cargo build -p rune-stt` succeeds with zero warnings
- [ ] `cargo test -p rune-stt` passes all unit tests
- [ ] `GET /stt/status` returns correct JSON
- [ ] `POST /stt/transcribe` with mock provider returns JSON transcription
- [ ] Empty audio, oversized audio, unsupported format all return 400
- [ ] Auto-transcribe injects transcription text into inbound messages
- [ ] `SttConfig` round-trips through TOML serialization
- [ ] `RUNE_STT__PROVIDER`, `RUNE_STT__API_KEY` env vars override file config

---

## Phase 10 — Hybrid Memory Search (Backend)

### 10.1 SQL Migration

**File:** `crates/rune-store/migrations/2026-03-15-000003_add_memory_embeddings/up.sql`

```sql
-- Enable pgvector extension (requires superuser on first install).
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE memory_embeddings (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    file_path   TEXT NOT NULL,
    chunk_index INT  NOT NULL,
    chunk_text  TEXT NOT NULL,
    embedding   vector(1536),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (file_path, chunk_index)
);

-- IVFFlat index for approximate nearest-neighbour search.
-- `lists = 100` is a good default for up to ~100K rows.
CREATE INDEX idx_memory_embedding ON memory_embeddings
    USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);

-- GIN index for full-text keyword search.
CREATE INDEX idx_memory_tsv ON memory_embeddings
    USING gin (to_tsvector('english', chunk_text));

-- B-tree index for file_path lookups during re-indexing.
CREATE INDEX idx_memory_embeddings_file_path ON memory_embeddings (file_path);
```

**File:** `crates/rune-store/migrations/2026-03-15-000003_add_memory_embeddings/down.sql`

```sql
DROP TABLE IF EXISTS memory_embeddings;
-- Do NOT drop the vector extension — other tables may depend on it.
```

### 10.2 Diesel Schema Addition

```rust
// Add to crates/rune-store/src/schema.rs:

// Note: Diesel does not natively support the `vector` type. The
// memory_embeddings queries use raw SQL via `diesel::sql_query` rather than
// the typed DSL. This table definition covers the non-vector columns so
// that Diesel can map the result struct.
table! {
    /// Memory embedding chunks for hybrid search.
    memory_embeddings (id) {
        id -> Uuid,
        file_path -> Text,
        chunk_index -> Int4,
        chunk_text -> Text,
        // `embedding` is vector(1536) — handled via raw SQL.
        created_at -> Timestamptz,
    }
}

// Add to the allow_tables_to_appear_in_same_query! macro:
// memory_embeddings,
```

### 10.3 Store Models

```rust
// Add to crates/rune-store/src/models.rs:

/// A memory embedding row (without the vector column, which is handled
/// via raw SQL).
#[derive(Debug, Clone, QueryableByName, Serialize, Deserialize)]
pub struct MemoryEmbeddingRow {
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    pub id: Uuid,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub file_path: String,
    #[diesel(sql_type = diesel::sql_types::Int4)]
    pub chunk_index: i32,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub chunk_text: String,
    #[diesel(sql_type = diesel::sql_types::Timestamptz)]
    pub created_at: DateTime<Utc>,
}

/// Result row from keyword search (raw SQL).
#[derive(Debug, Clone, QueryableByName, Serialize)]
pub struct KeywordSearchRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub file_path: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub chunk_text: String,
    #[diesel(sql_type = diesel::sql_types::Float8)]
    pub score: f64,
}

/// Result row from vector search (raw SQL).
#[derive(Debug, Clone, QueryableByName, Serialize)]
pub struct VectorSearchRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub file_path: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub chunk_text: String,
    #[diesel(sql_type = diesel::sql_types::Float8)]
    pub score: f64,
}
```

### 10.4 Repository Trait

```rust
// Add to crates/rune-store/src/repos.rs:

/// Persistence contract for memory embedding chunks.
#[async_trait]
pub trait MemoryEmbeddingRepo: Send + Sync {
    /// Upsert a single embedded chunk (file_path + chunk_index is the natural key).
    async fn upsert_chunk(
        &self,
        file_path: &str,
        chunk_index: i32,
        chunk_text: &str,
        embedding: &[f32],
    ) -> Result<(), StoreError>;

    /// Delete all chunks for a given file.
    async fn delete_by_file(&self, file_path: &str) -> Result<usize, StoreError>;

    /// Keyword search leg: returns rows ordered by ts_rank descending.
    async fn keyword_search(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<crate::models::KeywordSearchRow>, StoreError>;

    /// Vector search leg: returns rows ordered by cosine similarity descending.
    async fn vector_search(
        &self,
        embedding: &[f32],
        limit: i64,
    ) -> Result<Vec<crate::models::VectorSearchRow>, StoreError>;

    /// Count total indexed chunks.
    async fn count(&self) -> Result<i64, StoreError>;

    /// List distinct indexed file paths.
    async fn list_indexed_files(&self) -> Result<Vec<String>, StoreError>;
}
```

### 10.5 PgMemoryEmbeddingRepo Implementation Pattern

```rust
// crates/rune-store/src/pg.rs

use diesel::sql_query;
use diesel::sql_types::{Text, Int4, Float8, Array, BigInt};

/// PostgreSQL-backed memory embedding repository.
#[derive(Clone)]
pub struct PgMemoryEmbeddingRepo {
    pool: PgPool,
}

impl PgMemoryEmbeddingRepo {
    pub fn new(pool: PgPool) -> Self { Self { pool } }
}

#[async_trait]
impl MemoryEmbeddingRepo for PgMemoryEmbeddingRepo {
    async fn upsert_chunk(
        &self,
        file_path: &str,
        chunk_index: i32,
        chunk_text: &str,
        embedding: &[f32],
    ) -> Result<(), StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        // Format embedding as pgvector literal: "[0.1,0.2,...]"
        let embedding_literal = format!(
            "[{}]",
            embedding.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
        );
        sql_query(
            "INSERT INTO memory_embeddings (file_path, chunk_index, chunk_text, embedding)
             VALUES ($1, $2, $3, $4::vector)
             ON CONFLICT (file_path, chunk_index)
             DO UPDATE SET chunk_text = EXCLUDED.chunk_text,
                           embedding  = EXCLUDED.embedding,
                           created_at = now()"
        )
        .bind::<Text, _>(file_path)
        .bind::<Int4, _>(chunk_index)
        .bind::<Text, _>(chunk_text)
        .bind::<Text, _>(&embedding_literal)
        .execute(&mut conn)
        .await
        .map_err(StoreError::from)?;
        Ok(())
    }

    // ... other methods follow the same raw-SQL pattern using
    //     MemoryIndex::keyword_search_sql() and MemoryIndex::vector_search_sql()
}
```

### 10.6 Dependencies

```toml
# Add to crates/rune-store/Cargo.toml:
# pgvector is not needed as a Rust dependency — we use raw SQL with the
# pgvector extension installed in PostgreSQL. No additional Cargo crate.
```

### 10.7 Background Re-Index

```rust
// crates/rune-tools/src/memory_index.rs — add method to MemoryIndex:

/// Re-index a single file and persist via the provided repo.
///
/// 1. Delete existing chunks for the file.
/// 2. Chunk the file content.
/// 3. Embed all chunks.
/// 4. Upsert each embedded chunk.
///
/// Called by the file watcher on `Create`/`Modify` events.
pub async fn reindex_file_to_repo(
    &self,
    path: &Path,
    content: &str,
    repo: &dyn MemoryEmbeddingRepo,
) -> Result<usize, MemoryIndexError> { todo!() }

/// Remove all chunks for a deleted file.
///
/// Called by the file watcher on `Remove` events.
pub async fn remove_file_from_repo(
    &self,
    path: &Path,
    repo: &dyn MemoryEmbeddingRepo,
) -> Result<usize, MemoryIndexError> { todo!() }
```

### 10.8 Edge Cases

- **pgvector not installed**: Migration fails with clear SQL error. Document that `CREATE EXTENSION vector` requires superuser; suggest running it manually if migrations run as a non-superuser role.
- **Empty embedding vector**: Provider returns `[]` — reject with `MemoryIndexError::Embedding`.
- **Dimension mismatch**: Config says 1536 but provider returns 768 — detect in `index_file` by checking `embedding.len() != config.embedding_dimension` and return error.
- **Concurrent re-index of same file**: The `UNIQUE (file_path, chunk_index)` constraint with `ON CONFLICT DO UPDATE` makes concurrent upserts safe. Last writer wins.
- **File deleted mid-index**: `delete_by_file` is called first; if the file is deleted between chunk and embed, the embed fails but the old chunks are already gone. This is acceptable (the file is gone anyway).
- **Very large memory directory**: Batch embedding calls to 100 texts per request (already handled by `OpenAiEmbedding`). Process files sequentially to avoid OOM.
- **No memory files**: `reindex_directory` returns empty vec. Search returns empty results. No error.

### 10.9 Integration Test Scenarios

```rust
// crates/rune-tools/tests/memory_index_integration.rs

/// Chunk + embed + upsert + keyword search returns matching chunks.
/// Requires a running PostgreSQL with pgvector. Use #[ignore] for CI.
#[tokio::test]
#[ignore]
async fn hybrid_search_end_to_end() { }

/// Upsert is idempotent: re-indexing the same file does not create duplicates.
#[tokio::test]
#[ignore]
async fn upsert_idempotent() { }

/// Deleting a file removes all its chunks.
#[tokio::test]
#[ignore]
async fn delete_by_file_removes_chunks() { }

/// RRF correctly boosts documents appearing in both keyword and vector results.
/// (This test uses the pure-Rust RRF function and does not need a database.)
#[test]
fn rrf_boost_both_lists() { }

/// count() returns correct total after indexing.
#[tokio::test]
#[ignore]
async fn count_after_indexing() { }

/// list_indexed_files() returns distinct file paths.
#[tokio::test]
#[ignore]
async fn list_indexed_files_distinct() { }
```

### 10.10 Acceptance Criteria

- [ ] Migration applies cleanly on PostgreSQL 15+ with pgvector 0.7+
- [ ] `MemoryEmbeddingRepo` trait compiles and is implemented for `PgMemoryEmbeddingRepo`
- [ ] `upsert_chunk` correctly round-trips embedding vectors through `vector(1536)`
- [ ] Keyword search returns results ranked by `ts_rank`
- [ ] Vector search returns results ranked by cosine similarity
- [ ] `MemoryIndex.search()` merges both legs via RRF and returns correct order
- [ ] Re-index deletes stale chunks before inserting new ones
- [ ] `cargo test -p rune-tools` passes all non-`#[ignore]` tests
- [ ] Background file watcher triggers re-index on file change

---

## Phase 11 — Device Pairing (Backend)

> Note: The `DeviceRegistry` in `crates/rune-gateway/src/pairing.rs` is already
> implemented with Ed25519 challenge-response, expiring tokens, and in-memory
> storage. Phase 11 adds **durable persistence** to PostgreSQL, role/scope
> management endpoints, and the remaining route handlers not yet wired.

### 11.1 SQL Migration

**File:** `crates/rune-store/migrations/2026-03-15-000004_add_paired_devices/up.sql`

```sql
CREATE TABLE paired_devices (
    id                UUID PRIMARY KEY,
    name              TEXT NOT NULL,
    public_key        TEXT NOT NULL UNIQUE,
    role              TEXT NOT NULL DEFAULT 'operator',
    scopes            JSONB NOT NULL DEFAULT '[]',
    token_hash        TEXT NOT NULL,
    token_expires_at  TIMESTAMPTZ NOT NULL,
    paired_at         TIMESTAMPTZ NOT NULL,
    last_seen_at      TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_paired_devices_token_hash ON paired_devices (token_hash);
CREATE INDEX idx_paired_devices_public_key ON paired_devices (public_key);

CREATE TABLE pairing_requests (
    id            UUID PRIMARY KEY,
    device_name   TEXT NOT NULL,
    public_key    TEXT NOT NULL,
    challenge     TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL,
    expires_at    TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_pairing_requests_expires ON pairing_requests (expires_at);
```

**File:** `crates/rune-store/migrations/2026-03-15-000004_add_paired_devices/down.sql`

```sql
DROP TABLE IF EXISTS pairing_requests;
DROP TABLE IF EXISTS paired_devices;
```

### 11.2 Store Models

```rust
// Add to crates/rune-store/src/models.rs:

/// A paired device row.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = paired_devices)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PairedDeviceRow {
    pub id: Uuid,
    pub name: String,
    pub public_key: String,
    pub role: String,
    pub scopes: serde_json::Value,
    pub token_hash: String,
    pub token_expires_at: DateTime<Utc>,
    pub paired_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Insert payload for a new paired device.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = paired_devices)]
pub struct NewPairedDevice {
    pub id: Uuid,
    pub name: String,
    pub public_key: String,
    pub role: String,
    pub scopes: serde_json::Value,
    pub token_hash: String,
    pub token_expires_at: DateTime<Utc>,
    pub paired_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// A pairing request row.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = pairing_requests)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PairingRequestRow {
    pub id: Uuid,
    pub device_name: String,
    pub public_key: String,
    pub challenge: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Insert payload for a new pairing request.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = pairing_requests)]
pub struct NewPairingRequest {
    pub id: Uuid,
    pub device_name: String,
    pub public_key: String,
    pub challenge: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
```

### 11.3 Diesel Schema Addition

```rust
// Add to crates/rune-store/src/schema.rs:

table! {
    /// Paired devices with Ed25519 authentication.
    paired_devices (id) {
        id -> Uuid,
        name -> Text,
        public_key -> Text,
        role -> Text,
        scopes -> Jsonb,
        token_hash -> Text,
        token_expires_at -> Timestamptz,
        paired_at -> Timestamptz,
        last_seen_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
    }
}

table! {
    /// Pending pairing requests.
    pairing_requests (id) {
        id -> Uuid,
        device_name -> Text,
        public_key -> Text,
        challenge -> Text,
        created_at -> Timestamptz,
        expires_at -> Timestamptz,
    }
}
```

### 11.4 Repository Trait

```rust
// Add to crates/rune-store/src/repos.rs:

/// Persistence contract for device pairing.
#[async_trait]
pub trait DeviceRepo: Send + Sync {
    /// Insert a paired device.
    async fn create_device(&self, device: NewPairedDevice) -> Result<PairedDeviceRow, StoreError>;

    /// Find a device by ID.
    async fn find_device_by_id(&self, id: Uuid) -> Result<PairedDeviceRow, StoreError>;

    /// Find a device by token hash. Used for bearer-token auth.
    async fn find_device_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError>;

    /// List all paired devices.
    async fn list_devices(&self) -> Result<Vec<PairedDeviceRow>, StoreError>;

    /// Update token hash and expiry (for rotation).
    async fn update_token(
        &self,
        id: Uuid,
        token_hash: &str,
        token_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<PairedDeviceRow, StoreError>;

    /// Update role and scopes.
    async fn update_role(
        &self,
        id: Uuid,
        role: &str,
        scopes: serde_json::Value,
    ) -> Result<PairedDeviceRow, StoreError>;

    /// Update last_seen_at timestamp.
    async fn touch_last_seen(
        &self,
        id: Uuid,
        last_seen_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError>;

    /// Delete a device. Returns true if removed.
    async fn delete_device(&self, id: Uuid) -> Result<bool, StoreError>;

    /// Insert a pairing request.
    async fn create_pairing_request(
        &self,
        request: NewPairingRequest,
    ) -> Result<PairingRequestRow, StoreError>;

    /// Find and remove a pairing request (consumed on use).
    async fn take_pairing_request(
        &self,
        id: Uuid,
    ) -> Result<Option<PairingRequestRow>, StoreError>;

    /// List pending (non-expired) pairing requests.
    async fn list_pending_requests(&self) -> Result<Vec<PairingRequestRow>, StoreError>;

    /// Delete expired pairing requests. Returns count removed.
    async fn prune_expired_requests(&self) -> Result<usize, StoreError>;
}
```

### 11.5 Token Hashing

Store tokens as SHA-256 hashes. Never store raw bearer tokens in the database.

```rust
use sha2::{Sha256, Digest};

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}
```

**Dependency:** Add `sha2 = "0.10"` to `crates/rune-gateway/Cargo.toml`.

### 11.6 Gateway Routes — Wire Protocol

#### `POST /devices/pair/request`

**Request:**

```json
{
  "device_name": "my-phone",
  "public_key": "a1b2c3...64 hex chars..."
}
```

**Response 200:**

```json
{
  "request_id": "019532c0-...",
  "challenge": "d4e5f6...64 hex chars...",
  "expires_at": "2026-03-15T12:05:00Z"
}
```

**Error 400 — invalid public key:**

```json
{
  "code": "bad_request",
  "message": "invalid public key: expected 32 bytes",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532c0-..."
}
```

**Error 400 — empty device name:**

```json
{
  "code": "bad_request",
  "message": "device_name must not be empty",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532c0-..."
}
```

#### `POST /devices/pair/approve`

**Request:**

```json
{
  "request_id": "019532c0-...",
  "challenge_response": "abcdef...128 hex chars (64-byte Ed25519 signature)...",
  "role": "operator",
  "scopes": ["sessions:read", "sessions:write", "status:read"]
}
```

`role` and `scopes` are optional; default to `"operator"` and the standard scope set.

**Response 200:**

```json
{
  "device_id": "019532c1-...",
  "name": "my-phone",
  "role": "operator",
  "scopes": ["sessions:read", "sessions:write", "status:read"],
  "token": "full-bearer-token-96-hex-chars...",
  "token_expires_at": "2026-04-14T12:00:00Z"
}
```

**Error 404 — request not found:**

```json
{
  "code": "bad_request",
  "message": "pairing request not found: 019532c0-...",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532c1-..."
}
```

**Error 400 — request expired:**

```json
{
  "code": "bad_request",
  "message": "pairing request expired: 019532c0-...",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532c1-..."
}
```

**Error 400 — verification failed:**

```json
{
  "code": "bad_request",
  "message": "challenge response verification failed",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532c1-..."
}
```

#### `POST /devices/pair/reject`

**Request:**

```json
{
  "request_id": "019532c0-..."
}
```

**Response 200:**

```json
{
  "rejected": true
}
```

#### `GET /devices`

**Response 200:**

```json
{
  "devices": [
    {
      "id": "019532c1-...",
      "name": "my-phone",
      "public_key": "a1b2c3...64 hex...",
      "role": "operator",
      "scopes": ["sessions:read", "sessions:write", "status:read"],
      "token_masked": "a1b2c3...****",
      "token_expires_at": "2026-04-14T12:00:00Z",
      "paired_at": "2026-03-15T12:00:00Z",
      "last_seen_at": "2026-03-15T12:30:00Z"
    }
  ],
  "pending_requests": [
    {
      "id": "019532d0-...",
      "device_name": "new-tablet",
      "created_at": "2026-03-15T12:35:00Z",
      "expires_at": "2026-03-15T12:40:00Z"
    }
  ]
}
```

Note: Tokens are **never** returned in list responses. Only `token_masked` (first 6 chars + `****`) is shown.

#### `DELETE /devices/{id}`

**Response 200:**

```json
{
  "deleted": true
}
```

**Error 404:**

```json
{
  "code": "bad_request",
  "message": "device not found: 019532c1-...",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532d1-..."
}
```

#### `POST /devices/{id}/rotate-token`

**Response 200:**

```json
{
  "device_id": "019532c1-...",
  "token": "new-bearer-token-96-hex-chars...",
  "token_expires_at": "2026-04-14T12:00:00Z"
}
```

### 11.7 Route Handler Signatures

```rust
// crates/rune-gateway/src/routes.rs

#[derive(Deserialize)]
pub struct PairRequestBody {
    pub device_name: String,
    pub public_key: String,
}

#[derive(Serialize)]
pub struct PairRequestResponse {
    pub request_id: Uuid,
    pub challenge: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct PairApproveBody {
    pub request_id: Uuid,
    pub challenge_response: String,
    #[serde(default = "default_device_role")]
    pub role: String,
    #[serde(default = "default_device_scopes")]
    pub scopes: Vec<String>,
}
fn default_device_role() -> String { "operator".into() }
fn default_device_scopes() -> Vec<String> {
    vec!["sessions:read".into(), "sessions:write".into(), "status:read".into()]
}

#[derive(Serialize)]
pub struct PairApproveResponse {
    pub device_id: Uuid,
    pub name: String,
    pub role: String,
    pub scopes: Vec<String>,
    pub token: String,
    pub token_expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct PairRejectBody {
    pub request_id: Uuid,
}

#[derive(Serialize)]
pub struct PairRejectResponse {
    pub rejected: bool,
}

#[derive(Serialize)]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceListEntry>,
    pub pending_requests: Vec<PendingRequestEntry>,
}

#[derive(Serialize)]
pub struct DeviceListEntry {
    pub id: Uuid,
    pub name: String,
    pub public_key: String,
    pub role: String,
    pub scopes: Vec<String>,
    pub token_masked: String,
    pub token_expires_at: DateTime<Utc>,
    pub paired_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct PendingRequestEntry {
    pub id: Uuid,
    pub device_name: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct DeviceDeleteResponse {
    pub deleted: bool,
}

#[derive(Serialize)]
pub struct TokenRotateResponse {
    pub device_id: Uuid,
    pub token: String,
    pub token_expires_at: DateTime<Utc>,
}

pub async fn pair_request(
    State(state): State<AppState>,
    Json(body): Json<PairRequestBody>,
) -> Result<Json<PairRequestResponse>, GatewayError> { todo!() }

pub async fn pair_approve(
    State(state): State<AppState>,
    Json(body): Json<PairApproveBody>,
) -> Result<Json<PairApproveResponse>, GatewayError> { todo!() }

pub async fn pair_reject(
    State(state): State<AppState>,
    Json(body): Json<PairRejectBody>,
) -> Result<Json<PairRejectResponse>, GatewayError> { todo!() }

pub async fn device_list(
    State(state): State<AppState>,
) -> Result<Json<DeviceListResponse>, GatewayError> { todo!() }

pub async fn device_delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<DeviceDeleteResponse>, GatewayError> { todo!() }

pub async fn device_rotate_token(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TokenRotateResponse>, GatewayError> { todo!() }
```

### 11.8 Edge Cases

- **Duplicate public key**: The `UNIQUE (public_key)` constraint prevents pairing two devices with the same key. Return 400 `"a device with this public key is already paired"`.
- **Expired request consumed but not pruned**: `take_pairing_request` removes the row atomically. A background task calls `prune_expired_requests` every 10 minutes.
- **Token hash collision**: SHA-256 collision is astronomically unlikely. No mitigation needed.
- **Race between approve and reject**: Both call `take_pairing_request` which does `DELETE ... RETURNING`. Only the first to execute gets the row; the other gets `None` and returns 404.
- **Clock skew**: Token expiry uses server UTC time. Clients should rely on the returned `token_expires_at` rather than local clocks.
- **Revoked device with in-flight requests**: The auth middleware checks `find_device_by_token_hash` on every request. Once deleted, subsequent requests fail immediately.

### 11.9 Dependencies

```toml
# crates/rune-gateway/Cargo.toml additions:
ed25519-dalek = { version = "2", features = ["std"] }
sha2 = "0.10"
hex = "0.4"
```

### 11.10 Integration Test Scenarios

```rust
// crates/rune-gateway/tests/pairing_tests.rs

/// Full pairing flow: request -> sign -> approve -> use token.
#[tokio::test]
async fn full_pairing_flow_e2e() { }

/// Reject removes request; second reject returns 404.
#[tokio::test]
async fn reject_is_idempotent() { }

/// Approve with wrong signature returns 400.
#[tokio::test]
async fn approve_wrong_signature() { }

/// Approve expired request returns 400.
#[tokio::test]
async fn approve_expired_request() { }

/// Token rotation invalidates old token.
#[tokio::test]
async fn token_rotation_invalidates_old() { }

/// Delete device revokes token.
#[tokio::test]
async fn delete_revokes_token() { }

/// Duplicate public key returns 400.
#[tokio::test]
async fn duplicate_public_key_rejected() { }

/// GET /devices masks tokens and lists pending requests.
#[tokio::test]
async fn device_list_masks_tokens() { }

/// prune_expired_requests removes stale rows.
#[tokio::test]
async fn prune_removes_expired() { }
```

### 11.11 Acceptance Criteria

- [ ] Migration applies cleanly
- [ ] `DeviceRepo` trait compiles and is implemented for `PgDeviceRepo`
- [ ] Full pairing flow works end-to-end with Ed25519 signatures
- [ ] Tokens are stored as SHA-256 hashes, never plaintext
- [ ] `GET /devices` never returns raw tokens
- [ ] Token rotation invalidates the old token immediately
- [ ] Duplicate public keys are rejected
- [ ] Expired requests are pruned
- [ ] All integration tests pass

---

## Phase 12 — Session Enhancements (Backend + UI)

### 12.1 Existing Infrastructure

The `SessionRepo` already has `delete` and `update_metadata` methods (see `crates/rune-store/src/repos.rs` lines 43–51). The `TranscriptRepo` already has `delete_by_session`. This phase wires them into gateway routes and adds cascading cleanup.

### 12.2 Gateway Routes

#### `DELETE /sessions/{id}`

Deletes a session and all associated data (turns, transcript items, tool executions cascade via `ON DELETE CASCADE`).

**Response 200:**

```json
{
  "deleted": true,
  "session_id": "019532e0-...",
  "transcript_items_removed": 42,
  "turns_removed": 5
}
```

**Error 404:**

```json
{
  "code": "session_not_found",
  "message": "session not found: 019532e0-...",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532e1-..."
}
```

#### `PATCH /sessions/{id}`

**Request body (all fields optional):**

```json
{
  "label": "My refactoring session",
  "thinking_level": "high",
  "verbose": true,
  "reasoning": true,
  "custom_metadata": {
    "project": "rune",
    "priority": "high"
  }
}
```

The handler merges the provided fields into the session's existing `metadata` JSONB column. Fields set to `null` are removed from metadata.

**Response 200:**

```json
{
  "id": "019532e0-...",
  "kind": "direct",
  "status": "active",
  "metadata": {
    "label": "My refactoring session",
    "thinking_level": "high",
    "verbose": true,
    "reasoning": true,
    "custom_metadata": {
      "project": "rune",
      "priority": "high"
    }
  },
  "updated_at": "2026-03-15T12:45:00Z"
}
```

**Error 404:**

```json
{
  "code": "session_not_found",
  "message": "session not found: 019532e0-...",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532e2-..."
}
```

**Error 400 — invalid thinking_level:**

```json
{
  "code": "bad_request",
  "message": "thinking_level must be one of: off, low, medium, high",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532e2-..."
}
```

### 12.3 Route Handler Signatures

```rust
// crates/rune-gateway/src/routes.rs

#[derive(Serialize)]
pub struct SessionDeleteResponse {
    pub deleted: bool,
    pub session_id: Uuid,
    pub transcript_items_removed: usize,
    pub turns_removed: usize,
}

#[derive(Deserialize)]
pub struct SessionPatchBody {
    pub label: Option<String>,
    pub thinking_level: Option<String>,
    pub verbose: Option<bool>,
    pub reasoning: Option<bool>,
    pub custom_metadata: Option<serde_json::Value>,
}

/// Allowed values for the `thinking_level` metadata field.
const VALID_THINKING_LEVELS: &[&str] = &["off", "low", "medium", "high"];

pub async fn session_delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SessionDeleteResponse>, GatewayError> { todo!() }

pub async fn session_patch(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SessionPatchBody>,
) -> Result<Json<SessionRow>, GatewayError> { todo!() }
```

### 12.4 Implementation Notes

The `session_delete` handler should:

1. Call `transcript_repo.delete_by_session(id)` to get the removed count.
2. Count turns via `turn_repo.list_by_session(id)` before deletion.
3. Call `session_repo.delete(id)` which cascades to turns, transcript_items, and tool_executions.
4. Emit a `SessionEvent` with kind `"session_deleted"` on the broadcast channel.

The `session_patch` handler should:

1. Load existing session via `session_repo.find_by_id(id)`.
2. Parse existing `metadata` as a `serde_json::Map`.
3. Merge provided fields (overwrite existing keys, remove keys set to `null`).
4. Validate `thinking_level` if provided.
5. Call `session_repo.update_metadata(id, merged, Utc::now())`.
6. Emit a `SessionEvent` with kind `"session_updated"`.

### 12.5 Edge Cases

- **Delete active session with in-flight turn**: The `TurnExecutor` holds `session_id` by value. Deleting the session row while a turn is executing causes subsequent DB writes to fail with foreign-key errors. The executor catches these errors and terminates the turn gracefully.
- **PATCH with empty body**: All fields are optional. An empty body is a valid no-op that updates `last_activity_at` only.
- **PATCH with unknown fields**: Serde's `#[serde(deny_unknown_fields)]` is NOT used; unknown fields are silently ignored for forward compatibility.
- **Concurrent PATCH**: Two concurrent PATCHes to the same session. The `update_metadata` call sets the full metadata blob, so the last writer wins. This is acceptable for metadata updates.
- **Delete non-existent session**: `session_repo.delete(id)` returns `false`. Handler returns 404.
- **Delete already-deleted session**: Same as above — idempotent 404.

### 12.6 SQL Migrations

No new migrations. The `sessions.metadata` JSONB column and `ON DELETE CASCADE` constraints already exist.

### 12.7 Integration Test Scenarios

```rust
// crates/rune-gateway/tests/session_tests.rs

/// DELETE /sessions/{id} removes session and cascades to transcript + turns.
#[tokio::test]
async fn delete_session_cascades() { }

/// DELETE /sessions/{id} returns 404 for non-existent session.
#[tokio::test]
async fn delete_session_not_found() { }

/// PATCH /sessions/{id} merges metadata correctly.
#[tokio::test]
async fn patch_session_merges_metadata() { }

/// PATCH /sessions/{id} rejects invalid thinking_level.
#[tokio::test]
async fn patch_session_rejects_invalid_thinking_level() { }

/// PATCH /sessions/{id} with empty body is a no-op (updates timestamp only).
#[tokio::test]
async fn patch_session_empty_body_noop() { }

/// PATCH /sessions/{id} returns 404 for non-existent session.
#[tokio::test]
async fn patch_session_not_found() { }

/// DELETE emits session_deleted event on broadcast channel.
#[tokio::test]
async fn delete_emits_event() { }

/// PATCH emits session_updated event on broadcast channel.
#[tokio::test]
async fn patch_emits_event() { }
```

### 12.8 Acceptance Criteria

- [ ] `DELETE /sessions/{id}` removes session, turns, transcript items, and tool executions
- [ ] `DELETE /sessions/{id}` returns 404 for non-existent sessions
- [ ] `PATCH /sessions/{id}` correctly merges metadata fields
- [ ] `PATCH /sessions/{id}` validates `thinking_level` values
- [ ] Both endpoints emit appropriate `SessionEvent` on the broadcast channel
- [ ] Deleting an active session does not crash the turn executor
- [ ] All integration tests pass

---

## Phase 13 — Usage Analytics (Backend + UI)

### 13.1 No New Migration

Usage data is derived from existing `turns` table columns: `model_ref`, `usage_prompt_tokens`, `usage_completion_tokens`, `started_at`.

### 13.2 Gateway Route

#### `GET /api/dashboard/usage`

**Query parameters:**

| Parameter    | Type   | Required | Default     | Description                                  |
|-------------|--------|----------|-------------|----------------------------------------------|
| `from`      | string | no       | 30 days ago | ISO 8601 date or datetime                    |
| `to`        | string | no       | now         | ISO 8601 date or datetime                    |
| `group_by`  | string | no       | `"day"`     | `"day"`, `"week"`, `"month"`                 |
| `model`     | string | no       | all         | Filter to a specific `model_ref`             |

**Response 200:**

```json
{
  "from": "2026-02-15T00:00:00Z",
  "to": "2026-03-15T23:59:59Z",
  "group_by": "day",
  "model_filter": null,
  "buckets": [
    {
      "period": "2026-03-14",
      "total_turns": 47,
      "total_prompt_tokens": 125000,
      "total_completion_tokens": 42000,
      "total_tokens": 167000,
      "estimated_cost_usd": 1.25,
      "by_model": [
        {
          "model_ref": "oc-01-anthropic/claude-opus-4-6",
          "turns": 30,
          "prompt_tokens": 100000,
          "completion_tokens": 35000,
          "total_tokens": 135000,
          "estimated_cost_usd": 1.05
        },
        {
          "model_ref": "oc-01-openai/gpt-5.4",
          "turns": 17,
          "prompt_tokens": 25000,
          "completion_tokens": 7000,
          "total_tokens": 32000,
          "estimated_cost_usd": 0.20
        }
      ]
    }
  ],
  "totals": {
    "total_turns": 1420,
    "total_prompt_tokens": 3800000,
    "total_completion_tokens": 1200000,
    "total_tokens": 5000000,
    "estimated_cost_usd": 38.50
  }
}
```

**Response 200 — no data:**

```json
{
  "from": "2026-03-15T00:00:00Z",
  "to": "2026-03-15T23:59:59Z",
  "group_by": "day",
  "model_filter": null,
  "buckets": [],
  "totals": {
    "total_turns": 0,
    "total_prompt_tokens": 0,
    "total_completion_tokens": 0,
    "total_tokens": 0,
    "estimated_cost_usd": 0.0
  }
}
```

**Error 400 — invalid date range:**

```json
{
  "code": "bad_request",
  "message": "\"from\" must be before \"to\"",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532f0-..."
}
```

**Error 400 — invalid group_by:**

```json
{
  "code": "bad_request",
  "message": "group_by must be one of: day, week, month",
  "retriable": false,
  "approval_required": false,
  "request_id": "019532f0-..."
}
```

### 13.3 Rust Types

```rust
// crates/rune-gateway/src/routes.rs

#[derive(Deserialize)]
pub struct UsageQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(default = "default_group_by")]
    pub group_by: String,
    pub model: Option<String>,
}
fn default_group_by() -> String { "day".into() }

const VALID_GROUP_BY: &[&str] = &["day", "week", "month"];

#[derive(Serialize)]
pub struct UsageResponse {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub group_by: String,
    pub model_filter: Option<String>,
    pub buckets: Vec<UsageBucket>,
    pub totals: UsageTotals,
}

#[derive(Serialize)]
pub struct UsageBucket {
    pub period: String,
    pub total_turns: i64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
    pub by_model: Vec<ModelUsage>,
}

#[derive(Serialize)]
pub struct ModelUsage {
    pub model_ref: String,
    pub turns: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Serialize)]
pub struct UsageTotals {
    pub total_turns: i64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
}

pub async fn dashboard_usage(
    State(state): State<AppState>,
    Query(params): Query<UsageQuery>,
) -> Result<Json<UsageResponse>, GatewayError> { todo!() }
```

### 13.4 Cost Estimation

```rust
/// Estimated cost per 1M tokens by model family.
/// These are approximate and should be configurable in future phases.
fn estimate_cost(model_ref: &str, prompt_tokens: i64, completion_tokens: i64) -> f64 {
    let (prompt_rate, completion_rate) = match model_ref {
        m if m.contains("claude-opus") => (15.0, 75.0),   // per 1M tokens
        m if m.contains("claude-sonnet") => (3.0, 15.0),
        m if m.contains("claude-haiku") => (0.25, 1.25),
        m if m.contains("gpt-5.4") => (2.50, 10.0),
        m if m.contains("gpt-4o") => (2.50, 10.0),
        m if m.contains("gpt-4") => (10.0, 30.0),
        m if m.contains("gemini") => (0.50, 1.50),
        m if m.contains("deepseek") => (0.14, 0.28),
        _ => (1.0, 3.0), // conservative default
    };
    (prompt_tokens as f64 * prompt_rate + completion_tokens as f64 * completion_rate) / 1_000_000.0
}
```

### 13.5 SQL Query Pattern

The handler executes a single aggregation query against the `turns` table:

```sql
SELECT
    date_trunc($1, started_at) AS period,
    model_ref,
    COUNT(*) AS turns,
    COALESCE(SUM(usage_prompt_tokens), 0) AS prompt_tokens,
    COALESCE(SUM(usage_completion_tokens), 0) AS completion_tokens
FROM turns
WHERE started_at >= $2
  AND started_at <= $3
  AND ($4::TEXT IS NULL OR model_ref = $4)
  AND usage_prompt_tokens IS NOT NULL
GROUP BY period, model_ref
ORDER BY period ASC, model_ref ASC;
```

Parameters: `$1` = `'day'`/`'week'`/`'month'`, `$2` = from, `$3` = to, `$4` = model filter (nullable).

### 13.6 TurnRepo Extension

```rust
// Add to TurnRepo trait in crates/rune-store/src/repos.rs:

/// Aggregate token usage grouped by time period and model.
async fn usage_aggregate(
    &self,
    group_by: &str,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    model_filter: Option<&str>,
) -> Result<Vec<UsageAggregateRow>, StoreError>;
```

```rust
// Add to crates/rune-store/src/models.rs:

/// Result row from usage aggregation query.
#[derive(Debug, Clone, QueryableByName, Serialize)]
pub struct UsageAggregateRow {
    #[diesel(sql_type = diesel::sql_types::Timestamptz)]
    pub period: DateTime<Utc>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    pub model_ref: Option<String>,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub turns: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub prompt_tokens: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub completion_tokens: i64,
}
```

### 13.7 Edge Cases

- **No turns in range**: Return empty `buckets` with zero `totals`.
- **Turns with NULL usage tokens**: Filtered out by `usage_prompt_tokens IS NOT NULL` in the WHERE clause.
- **Turns with NULL model_ref**: Aggregated under `model_ref: null` — the response renders this as `"model_ref": "unknown"`.
- **Very large date range**: The query is index-friendly (`started_at` has an index). For ranges > 1 year, the response may contain hundreds of buckets; no pagination — the UI handles virtual scrolling.
- **from > to**: Return 400.
- **Invalid date format**: Axum query param parsing fails with 422. The handler also tries `chrono::NaiveDate::parse_from_str` and `DateTime::parse_from_rfc3339` as fallbacks.
- **Concurrent reads**: Read-only query; no contention.
- **Cost accuracy**: Costs are estimates. The `estimate_cost` function uses hardcoded rates. A future phase will make these configurable via `AppConfig`.

### 13.8 Integration Test Scenarios

```rust
// crates/rune-gateway/tests/usage_tests.rs

/// GET /api/dashboard/usage with no data returns empty buckets.
#[tokio::test]
async fn usage_empty_returns_zero_totals() { }

/// GET /api/dashboard/usage aggregates by day correctly.
#[tokio::test]
async fn usage_aggregate_by_day() { }

/// GET /api/dashboard/usage aggregates by week correctly.
#[tokio::test]
async fn usage_aggregate_by_week() { }

/// GET /api/dashboard/usage aggregates by month correctly.
#[tokio::test]
async fn usage_aggregate_by_month() { }

/// GET /api/dashboard/usage filters by model.
#[tokio::test]
async fn usage_filter_by_model() { }

/// GET /api/dashboard/usage rejects from > to.
#[tokio::test]
async fn usage_rejects_invalid_range() { }

/// GET /api/dashboard/usage rejects invalid group_by.
#[tokio::test]
async fn usage_rejects_invalid_group_by() { }

/// Cost estimation returns non-zero for known models.
#[test]
fn cost_estimation_known_models() { }

/// Cost estimation uses default rate for unknown models.
#[test]
fn cost_estimation_unknown_model_uses_default() { }
```

### 13.9 Acceptance Criteria

- [ ] `GET /api/dashboard/usage` returns correct aggregation for day/week/month
- [ ] Model filter works correctly
- [ ] Empty date ranges return zero totals (not errors)
- [ ] `from > to` returns 400
- [ ] Invalid `group_by` returns 400
- [ ] Cost estimation returns reasonable values for known model families
- [ ] NULL usage tokens are excluded from aggregation
- [ ] All integration tests pass

---

## Phase 14 — Config Editor (Backend + UI)

### 14.1 Gateway Routes

#### `GET /config`

Returns the currently active `AppConfig` as JSON. Sensitive fields (API keys, tokens) are redacted.

**Response 200:**

```json
{
  "gateway": {
    "host": "0.0.0.0",
    "port": 8787,
    "auth_token": "****"
  },
  "database": {
    "database_url": "****",
    "max_connections": 10,
    "run_migrations": true
  },
  "models": {
    "default_model": "oc-01-anthropic/claude-opus-4-6",
    "providers": [
      {
        "name": "oc-01-anthropic",
        "kind": "anthropic",
        "base_url": "https://api.anthropic.com",
        "api_key": "****",
        "api_key_env": "ANTHROPIC_API_KEY",
        "models": ["claude-opus-4-6"]
      }
    ]
  },
  "channels": {
    "enabled": ["telegram"],
    "telegram_token": "****"
  },
  "memory": {
    "semantic_search_enabled": true
  },
  "media": {
    "transcription_enabled": true,
    "tts_enabled": true
  },
  "logging": {
    "level": "info",
    "json": true
  },
  "paths": {
    "db_dir": "/data/db",
    "sessions_dir": "/data/sessions",
    "memory_dir": "/data/memory",
    "media_dir": "/data/media",
    "spells_dir": "/data/spells",
    "skills_dir": "/data/skills",
    "logs_dir": "/data/logs",
    "backups_dir": "/data/backups",
    "config_dir": "/config",
    "secrets_dir": "/secrets"
  },
  "runtime": {
    "lanes": {
      "main_capacity": 4,
      "subagent_capacity": 8,
      "cron_capacity": 1024
    }
  },
  "agents": {
    "defaults": {},
    "list": []
  },
  "tts": {
    "provider": "openai",
    "model": "tts-1",
    "voice": "alloy",
    "auto_mode": "off"
  },
  "stt": {
    "provider": "openai",
    "model": "whisper-1",
    "auto_transcribe": true
  }
}
```

#### `PUT /config`

Accepts a full or partial JSON config and applies it as a runtime override. The override is persisted to `{config_dir}/runtime-overrides.json` so it survives restarts.

**Request body:**

```json
{
  "logging": {
    "level": "debug"
  },
  "runtime": {
    "lanes": {
      "main_capacity": 8
    }
  }
}
```

**Response 200:**

```json
{
  "applied": true,
  "changed_keys": ["logging.level", "runtime.lanes.main_capacity"],
  "restart_required": false,
  "warnings": []
}
```

**Response 200 — changes require restart:**

```json
{
  "applied": true,
  "changed_keys": ["database.max_connections"],
  "restart_required": true,
  "warnings": ["database.max_connections: change takes effect after restart"]
}
```

**Error 400 — validation failure:**

```json
{
  "code": "bad_request",
  "message": "config validation failed",
  "retriable": false,
  "approval_required": false,
  "request_id": "01953300-...",
  "details": [
    "gateway.port: must be 1–65535",
    "logging.level: must be one of trace, debug, info, warn, error"
  ]
}
```

**Error 400 — attempting to change immutable fields:**

```json
{
  "code": "bad_request",
  "message": "cannot modify immutable field: database.database_url",
  "retriable": false,
  "approval_required": false,
  "request_id": "01953300-..."
}
```

#### `GET /config/schema`

Returns a JSON Schema (draft 2020-12) describing the full `AppConfig` structure so the UI can render a type-safe form editor.

**Response 200:**

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "AppConfig",
  "type": "object",
  "properties": {
    "gateway": {
      "type": "object",
      "properties": {
        "host": { "type": "string", "default": "0.0.0.0" },
        "port": { "type": "integer", "minimum": 1, "maximum": 65535, "default": 8787 },
        "auth_token": { "type": "string", "sensitive": true }
      }
    },
    "logging": {
      "type": "object",
      "properties": {
        "level": {
          "type": "string",
          "enum": ["trace", "debug", "info", "warn", "error"],
          "default": "info"
        },
        "json": { "type": "boolean", "default": true }
      }
    },
    "runtime": {
      "type": "object",
      "properties": {
        "lanes": {
          "type": "object",
          "properties": {
            "main_capacity": { "type": "integer", "minimum": 1, "maximum": 128, "default": 4 },
            "subagent_capacity": { "type": "integer", "minimum": 1, "maximum": 256, "default": 8 },
            "cron_capacity": { "type": "integer", "minimum": 1, "maximum": 4096, "default": 1024 }
          }
        }
      }
    }
  },
  "immutable": ["database.database_url", "paths"],
  "restart_required": ["database.max_connections", "gateway.host", "gateway.port"]
}
```

Note: The schema includes two custom annotations (`immutable` and `restart_required`) that are not part of JSON Schema but are consumed by the UI.

### 14.2 Rust Types

```rust
// crates/rune-gateway/src/routes.rs

/// Fields that cannot be changed via PUT /config.
const IMMUTABLE_FIELDS: &[&str] = &[
    "database.database_url",
    "paths.db_dir",
    "paths.sessions_dir",
    "paths.memory_dir",
    "paths.media_dir",
    "paths.skills_dir",
    "paths.logs_dir",
    "paths.backups_dir",
    "paths.config_dir",
    "paths.secrets_dir",
];

/// Fields that require a process restart to take effect.
const RESTART_REQUIRED_FIELDS: &[&str] = &[
    "database.max_connections",
    "gateway.host",
    "gateway.port",
];

/// Fields containing secrets that must be redacted in GET responses.
const SENSITIVE_FIELDS: &[&str] = &[
    "gateway.auth_token",
    "database.database_url",
    "models.providers.*.api_key",
    "channels.telegram_token",
    "channels.discord_token",
    "channels.slack_bot_token",
    "channels.slack_app_token",
    "channels.whatsapp_access_token",
    "channels.signal_number",
    "tts.api_key",
    "stt.api_key",
];

#[derive(Serialize)]
pub struct ConfigGetResponse(serde_json::Value);

#[derive(Serialize)]
pub struct ConfigPutResponse {
    pub applied: bool,
    pub changed_keys: Vec<String>,
    pub restart_required: bool,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct ConfigSchemaResponse(serde_json::Value);

pub async fn config_get(
    State(state): State<AppState>,
) -> Result<Json<ConfigGetResponse>, GatewayError> { todo!() }

pub async fn config_put(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<ConfigPutResponse>, GatewayError> { todo!() }

pub async fn config_schema(
    State(state): State<AppState>,
) -> Result<Json<ConfigSchemaResponse>, GatewayError> { todo!() }
```

### 14.3 Redaction Logic

```rust
/// Redact sensitive values in a config JSON value.
/// Replaces any string value at a sensitive path with "****".
fn redact_sensitive(value: &mut serde_json::Value, sensitive_fields: &[&str]) {
    for field_path in sensitive_fields {
        if field_path.contains(".*") {
            // Wildcard: e.g. "models.providers.*.api_key"
            // Walk the path up to the wildcard, iterate array elements,
            // and redact the remaining path in each.
            redact_wildcard(value, field_path);
        } else {
            redact_path(value, field_path);
        }
    }
}

fn redact_path(value: &mut serde_json::Value, dotted_path: &str) {
    let parts: Vec<&str> = dotted_path.split('.').collect();
    let mut current = value;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let Some(obj) = current.as_object_mut() {
                if let Some(v) = obj.get_mut(*part) {
                    if v.is_string() && !v.as_str().unwrap_or("").is_empty() {
                        *v = serde_json::Value::String("****".into());
                    }
                }
            }
        } else {
            current = match current.get_mut(*part) {
                Some(v) => v,
                None => return,
            };
        }
    }
}
```

### 14.4 Override Persistence

Runtime overrides are stored at `{config.paths.config_dir}/runtime-overrides.json`. On startup:

1. Load base config from TOML + env vars (existing flow).
2. If `runtime-overrides.json` exists, deserialize and merge on top.
3. On `PUT /config`, write the merged override to disk atomically (write to `.tmp`, then rename).

```rust
/// Persist runtime overrides atomically.
async fn persist_overrides(
    config_dir: &std::path::Path,
    overrides: &serde_json::Value,
) -> Result<(), GatewayError> {
    let path = config_dir.join("runtime-overrides.json");
    let tmp_path = config_dir.join("runtime-overrides.json.tmp");
    let content = serde_json::to_string_pretty(overrides)
        .map_err(|e| GatewayError::Internal(format!("serialize overrides: {e}")))?;
    tokio::fs::write(&tmp_path, content)
        .await
        .map_err(|e| GatewayError::Internal(format!("write overrides tmp: {e}")))?;
    tokio::fs::rename(&tmp_path, &path)
        .await
        .map_err(|e| GatewayError::Internal(format!("rename overrides: {e}")))?;
    Ok(())
}
```

### 14.5 AppState Addition

```rust
// Add to AppState:
/// Mutable runtime config overrides. The inner Value is merged on top of
/// the base AppConfig on each request that needs the effective config.
pub config_overrides: Arc<tokio::sync::RwLock<serde_json::Value>>,
```

### 14.6 Diff Computation

The `PUT /config` response includes `changed_keys` — a flat list of dotted paths that differ between old and new config. This is computed by recursive comparison:

```rust
/// Compute the list of dotted-path keys that differ between two JSON values.
fn diff_keys(
    old: &serde_json::Value,
    new: &serde_json::Value,
    prefix: &str,
) -> Vec<String> {
    let mut changes = Vec::new();
    match (old, new) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for key in b.keys() {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                match a.get(key) {
                    Some(old_val) => {
                        changes.extend(diff_keys(old_val, &b[key], &path));
                    }
                    None => changes.push(path),
                }
            }
        }
        (a, b) if a != b => changes.push(prefix.to_string()),
        _ => {}
    }
    changes
}
```

### 14.7 Validation

Before applying overrides, validate:

1. **Immutable fields**: If any key in `changed_keys` is in `IMMUTABLE_FIELDS`, reject with 400.
2. **Type validation**: Deserialize the merged config into `AppConfig`. If deserialization fails, return 400 with the Figment error messages.
3. **Range validation**: Port must be 1-65535. Capacity fields must be >= 1. Log level must be valid.

### 14.8 Edge Cases

- **Concurrent PUT /config**: `config_overrides` is behind `RwLock`. Writes are serialized. Last writer wins.
- **Invalid JSON body**: Axum returns 422 before the handler runs.
- **Partial config**: Only provided keys are merged. Missing keys retain their current values.
- **Sensitive field in PUT body**: API keys can be set via PUT (they are not redacted in the request body, only in GET responses). This is intentional — the config editor needs to be able to set keys.
- **config_dir not writable**: `persist_overrides` returns 500. The in-memory override is still applied for the current process lifetime.
- **runtime-overrides.json corrupted on disk**: On startup, if JSON parse fails, log a warning and ignore the file (use base config only).
- **Empty PUT body (`{}`)**: Valid no-op. Returns `changed_keys: []`.
- **Nested merge depth**: The merge is recursive with no depth limit. Deeply nested objects are handled correctly.

### 14.9 SQL Migrations

No new database tables. Config is stored on the filesystem.

### 14.10 Integration Test Scenarios

```rust
// crates/rune-gateway/tests/config_tests.rs

/// GET /config returns redacted sensitive fields.
#[tokio::test]
async fn config_get_redacts_secrets() { }

/// GET /config returns all top-level sections.
#[tokio::test]
async fn config_get_returns_all_sections() { }

/// PUT /config applies logging.level change immediately.
#[tokio::test]
async fn config_put_applies_logging_level() { }

/// PUT /config rejects immutable field changes.
#[tokio::test]
async fn config_put_rejects_immutable_fields() { }

/// PUT /config returns restart_required for gateway.port change.
#[tokio::test]
async fn config_put_flags_restart_required() { }

/// PUT /config with invalid port returns 400.
#[tokio::test]
async fn config_put_validates_port_range() { }

/// PUT /config with empty body is a no-op.
#[tokio::test]
async fn config_put_empty_body_noop() { }

/// PUT /config persists overrides to disk.
#[tokio::test]
async fn config_put_persists_to_disk() { }

/// GET /config/schema returns valid JSON Schema.
#[tokio::test]
async fn config_schema_returns_json_schema() { }

/// Concurrent PUT /config requests do not corrupt state.
#[tokio::test]
async fn config_put_concurrent_safety() { }

/// diff_keys correctly identifies changed paths.
#[test]
fn diff_keys_identifies_changes() { }

/// diff_keys returns empty for identical values.
#[test]
fn diff_keys_empty_for_identical() { }

/// redact_sensitive replaces secrets with "****".
#[test]
fn redact_sensitive_replaces_secrets() { }

/// redact_sensitive handles wildcard paths.
#[test]
fn redact_sensitive_handles_wildcards() { }
```

### 14.11 Acceptance Criteria

- [ ] `GET /config` returns full config with sensitive fields redacted as `"****"`
- [ ] `GET /config` never leaks API keys, tokens, or database URLs
- [ ] `PUT /config` merges partial overrides correctly
- [ ] `PUT /config` rejects changes to immutable fields with 400
- [ ] `PUT /config` flags `restart_required` for appropriate fields
- [ ] `PUT /config` validates the merged config by deserializing into `AppConfig`
- [ ] Overrides are persisted to `runtime-overrides.json` atomically
- [ ] On restart, persisted overrides are loaded and applied
- [ ] Corrupted `runtime-overrides.json` is ignored with a warning
- [ ] `GET /config/schema` returns a JSON Schema with correct types and defaults
- [ ] Empty `PUT /config` body is a valid no-op
- [ ] All integration tests pass

---

## Cross-Phase Dependency Summary

| Phase | Depends On | New Crates | New DB Tables | New Config Sections |
|-------|-----------|------------|---------------|---------------------|
| 8     | —         | `rune-tts` | —             | `[tts]`             |
| 9     | —         | `rune-stt` | —             | `[stt]`             |
| 10    | —         | —          | `memory_embeddings` | —             |
| 11    | —         | —          | `paired_devices`, `pairing_requests` | — |
| 12    | —         | —          | —             | —                   |
| 13    | —         | —          | —             | —                   |
| 14    | 8, 9      | —          | —             | —                   |

Phases 8–13 are independent and can be implemented in parallel. Phase 14 depends on 8 and 9 only because the config schema and GET response must include the `[tts]` and `[stt]` sections.

---

## Full Dependency Manifest (all phases)

| Crate             | Version | Used By         |
|-------------------|---------|-----------------|
| `async-trait`     | 0.1     | 8, 9, 10, 11    |
| `bytes`           | 1       | 8, 9            |
| `reqwest`         | 0.12    | 8, 9            |
| `serde`           | 1       | all             |
| `serde_json`      | 1       | all             |
| `thiserror`       | 2       | 8, 9, 10        |
| `tokio`           | 1       | all             |
| `tracing`         | 0.1     | all             |
| `uuid`            | 1       | all             |
| `ed25519-dalek`   | 2       | 11              |
| `sha2`            | 0.10    | 11              |
| `hex`             | 0.4     | 11              |
| `chrono`          | 0.4     | all             |
| `diesel`          | 2       | 10, 11, 13      |
| `diesel-async`    | 0.5     | 10, 11, 13      |
