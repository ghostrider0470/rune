use std::sync::Arc;

use reqwest::Url;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::error::BrowserError;
use crate::snapshot::{BrowserSnapshot, SnapshotEngine, SnapshotOptions};

/// Configuration for the browser pool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPoolConfig {
    /// Optional Chrome DevTools HTTP endpoint.
    pub cdp_endpoint: Option<String>,
    /// Path to the Chromium binary. Reserved for a future launcher-backed implementation.
    pub chromium_path: Option<String>,
    /// Maximum concurrent browse requests.
    pub max_instances: usize,
    /// URL patterns blocked by policy.
    pub blocked_urls: Vec<String>,
}

impl Default for BrowserPoolConfig {
    fn default() -> Self {
        Self {
            cdp_endpoint: None,
            chromium_path: None,
            max_instances: 3,
            blocked_urls: Vec::new(),
        }
    }
}

/// Lightweight concurrency guard around the semantic snapshot engine.
pub struct BrowserPool {
    semaphore: Arc<Semaphore>,
    config: BrowserPoolConfig,
}

impl BrowserPool {
    /// Create a new browser pool from config.
    #[must_use]
    pub fn new(config: BrowserPoolConfig) -> Self {
        let max_instances = config.max_instances.max(1);
        Self {
            semaphore: Arc::new(Semaphore::new(max_instances)),
            config: BrowserPoolConfig {
                max_instances,
                ..config
            },
        }
    }

    /// Browse a URL and return a semantic snapshot.
    pub async fn browse(
        &self,
        url: &str,
        options: &SnapshotOptions,
    ) -> Result<BrowserSnapshot, BrowserError> {
        validate_url(url)?;
        if self.is_blocked(url) {
            return Err(BrowserError::UrlBlocked {
                url: url.to_string(),
            });
        }

        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| BrowserError::NotAvailable("browser pool is closed".to_string()))?;

        let mut effective_options = options.clone();
        if effective_options.cdp_endpoint.is_none() {
            effective_options.cdp_endpoint = self.config.cdp_endpoint.clone();
        }

        let engine = SnapshotEngine::new(effective_options);
        engine.navigate_and_snapshot(url).await
    }

    fn is_blocked(&self, url: &str) -> bool {
        self.config
            .blocked_urls
            .iter()
            .any(|pattern| wildcard_match(pattern, url))
    }
}

fn validate_url(url: &str) -> Result<(), BrowserError> {
    let parsed = Url::parse(url).map_err(|err| BrowserError::InvalidUrl {
        url: url.to_string(),
        reason: err.to_string(),
    })?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(BrowserError::InvalidUrl {
            url: url.to_string(),
            reason: format!("unsupported scheme '{scheme}'"),
        }),
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    if pattern == "*" {
        return true;
    }

    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }

    let mut cursor = 0usize;
    let anchored_start = !pattern.starts_with('*');
    let anchored_end = !pattern.ends_with('*');

    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if index == 0 && anchored_start {
            if !value[cursor..].starts_with(part) {
                return false;
            }
            cursor += part.len();
            continue;
        }

        if index == parts.len() - 1 && anchored_end {
            return value[cursor..].ends_with(part);
        }

        let Some(offset) = value[cursor..].find(part) else {
            return false;
        };
        cursor += offset + part.len();
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_url_scheme() {
        let err = validate_url("ftp://example.com").unwrap_err();
        assert!(matches!(err, BrowserError::InvalidUrl { .. }));
    }

    #[test]
    fn wildcard_patterns_block_expected_urls() {
        assert!(wildcard_match(
            "https://internal.example.com/*",
            "https://internal.example.com/a"
        ));
        assert!(wildcard_match(
            "*example.com*",
            "https://www.example.com/docs"
        ));
        assert!(!wildcard_match(
            "https://internal.example.com/*",
            "https://public.example.com/a"
        ));
    }

    #[tokio::test]
    async fn browser_pool_blocks_configured_urls() {
        let pool = BrowserPool::new(BrowserPoolConfig {
            blocked_urls: vec!["https://blocked.example.com/*".to_string()],
            ..BrowserPoolConfig::default()
        });

        let err = pool
            .browse(
                "https://blocked.example.com/path",
                &SnapshotOptions::default(),
            )
            .await
            .unwrap_err();

        assert!(matches!(err, BrowserError::UrlBlocked { .. }));
    }
}
