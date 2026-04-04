use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};
use tracing_subscriber::{
    EnvFilter, Layer, fmt, layer::Context, layer::SubscriberExt, registry::LookupSpan,
    util::SubscriberInitExt,
};

use rune_config::LoggingConfig;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Clone)]
pub struct LogStore {
    capacity: usize,
    entries: Arc<RwLock<VecDeque<LogEntry>>>,
    tx: broadcast::Sender<LogEntry>,
}

impl LogStore {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.max(64));
        Self {
            capacity: capacity.max(1),
            entries: Arc::new(RwLock::new(VecDeque::with_capacity(capacity.max(1)))),
            tx,
        }
    }

    pub async fn push(&self, entry: LogEntry) {
        let mut entries = self.entries.write().await;
        entries.push_back(entry.clone());
        while entries.len() > self.capacity {
            entries.pop_front();
        }
        drop(entries);
        let _ = self.tx.send(entry);
    }

    pub async fn snapshot(&self) -> Vec<LogEntry> {
        self.entries.read().await.iter().cloned().collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.tx.subscribe()
    }
}

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

#[derive(Clone)]
struct LogStoreLayer {
    store: LogStore,
}

impl<S> Layer<S> for LogStoreLayer
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        use tracing::field::{Field, Visit};

        struct Visitor {
            message: Option<String>,
            fields: serde_json::Map<String, serde_json::Value>,
        }

        impl Visit for Visitor {
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                let rendered = format!("{value:?}");
                if field.name() == "message" {
                    self.message = Some(rendered.trim_matches('\"').to_string());
                } else {
                    self.fields.insert(
                        field.name().to_string(),
                        serde_json::Value::String(rendered),
                    );
                }
            }

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "message" {
                    self.message = Some(value.to_string());
                } else {
                    self.fields.insert(
                        field.name().to_string(),
                        serde_json::Value::String(value.to_string()),
                    );
                }
            }

            fn record_bool(&mut self, field: &Field, value: bool) {
                self.fields
                    .insert(field.name().to_string(), serde_json::Value::Bool(value));
            }

            fn record_i64(&mut self, field: &Field, value: i64) {
                self.fields.insert(
                    field.name().to_string(),
                    serde_json::Value::Number(value.into()),
                );
            }

            fn record_u64(&mut self, field: &Field, value: u64) {
                self.fields.insert(
                    field.name().to_string(),
                    serde_json::Value::Number(value.into()),
                );
            }

            fn record_f64(&mut self, field: &Field, value: f64) {
                if let Some(number) = serde_json::Number::from_f64(value) {
                    self.fields
                        .insert(field.name().to_string(), serde_json::Value::Number(number));
                }
            }
        }

        let mut visitor = Visitor {
            message: None,
            fields: serde_json::Map::new(),
        };
        event.record(&mut visitor);

        let entry = LogEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: event.metadata().level().to_string(),
            target: event.metadata().target().to_string(),
            message: visitor
                .message
                .unwrap_or_else(|| event.metadata().name().to_string()),
            fields: (!visitor.fields.is_empty()).then_some(visitor.fields),
        };

        let store = self.store.clone();
        tokio::spawn(async move {
            store.push(entry).await;
        });
    }
}

pub fn init_logging_with_store(config: &LoggingConfig, log_store: LogStore) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.level));

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(LogStoreLayer { store: log_store });

    if config.json {
        registry.with(fmt::layer().json()).init();
    } else {
        registry.with(fmt::layer()).init();
    }
}
