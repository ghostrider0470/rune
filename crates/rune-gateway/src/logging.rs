//! Structured JSON logging initialization via `tracing-subscriber`.

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use rune_config::LoggingConfig;

/// Initialize the global tracing subscriber based on config.
///
/// Call this once at startup. Subsequent calls are no-ops (tracing guard).
pub fn init_logging(config: &LoggingConfig) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.level));

    let registry = tracing_subscriber::registry().with(filter);

    if config.json {
        registry.with(fmt::layer().json()).init();
    } else {
        registry.with(fmt::layer()).init();
    }
}
