use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

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
