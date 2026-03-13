//! Background service supervisor placeholder.
//!
//! Future waves will add scheduler, channel polling, and other background
//! services here. For now this is a structural placeholder that starts and
//! stops cleanly.

use tracing::info;

/// Manages background services (scheduler, channel adapters, etc.).
///
/// Currently a placeholder — no services are registered yet.
pub struct BackgroundSupervisor {
    _handle: Option<tokio::task::JoinHandle<()>>,
}

impl BackgroundSupervisor {
    /// Create a new supervisor. No background services are started yet.
    #[must_use]
    pub fn new() -> Self {
        Self { _handle: None }
    }

    /// Start all registered background services.
    ///
    /// TODO(wave-5+): register scheduler, channel pollers, health watchers.
    pub fn start(&mut self) {
        info!("background supervisor started (no services registered)");
        self._handle = Some(tokio::spawn(async {
            // Placeholder — future services will be selected/joined here.
            std::future::pending::<()>().await;
        }));
    }

    /// Request graceful shutdown of all background services.
    pub fn shutdown(&mut self) {
        if let Some(handle) = self._handle.take() {
            handle.abort();
            info!("background supervisor shut down");
        }
    }
}

impl Default for BackgroundSupervisor {
    fn default() -> Self {
        Self::new()
    }
}
