use std::time::Duration;

use tokio::process::{Child, Command};
use tracing::{debug, info, warn};

use crate::error::BrowserError;

/// Options for launching a local Chromium instance.
#[derive(Clone, Debug)]
pub struct LaunchOptions {
    /// Path to the Chromium/Chrome binary. If `None`, [`find_chromium_binary`]
    /// is used to discover one.
    pub binary_path: Option<String>,
    /// Remote debugging port.
    pub port: u16,
    /// Run headless (default `true`).
    pub headless: bool,
    /// How long to wait for the browser to become ready (ms).
    pub timeout_ms: u64,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            binary_path: None,
            port: 9222,
            headless: true,
            timeout_ms: 10_000,
        }
    }
}

/// Manages a locally-spawned Chromium process for CDP-based browsing.
pub struct ChromiumLauncher {
    child: Option<Child>,
    cdp_endpoint: String,
}

impl ChromiumLauncher {
    /// Launch a Chromium instance with the given options.
    pub async fn launch(options: LaunchOptions) -> Result<Self, BrowserError> {
        let binary = match options.binary_path {
            Some(ref p) if !p.is_empty() => p.clone(),
            _ => find_chromium_binary().ok_or_else(|| {
                BrowserError::NotAvailable(
                    "no Chromium binary found on PATH; install chromium or google-chrome"
                        .to_string(),
                )
            })?,
        };

        info!(%binary, port = options.port, headless = options.headless, "launching Chromium");

        let mut cmd = Command::new(&binary);
        cmd.args([
            if options.headless {
                "--headless=new"
            } else {
                "--no-first-run"
            },
            &format!("--remote-debugging-port={}", options.port),
            "--no-sandbox",
            "--disable-gpu",
            "--disable-software-rasterizer",
            "--disable-dev-shm-usage",
            // Start with a blank page so we get a page target immediately.
            "about:blank",
        ]);

        // Suppress noisy Chromium stderr in logs.
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        let child = cmd.spawn().map_err(|e| {
            BrowserError::NotAvailable(format!("failed to spawn Chromium at {binary}: {e}"))
        })?;

        let cdp_endpoint = format!("http://127.0.0.1:{}", options.port);

        let mut launcher = Self {
            child: Some(child),
            cdp_endpoint: cdp_endpoint.clone(),
        };

        // Poll until CDP is ready.
        if let Err(e) = launcher.wait_until_ready(options.timeout_ms).await {
            launcher.shutdown().await;
            return Err(e);
        }

        info!(%cdp_endpoint, "Chromium is ready");
        Ok(launcher)
    }

    /// The CDP HTTP endpoint of the launched browser.
    #[must_use]
    pub fn cdp_endpoint(&self) -> &str {
        &self.cdp_endpoint
    }

    /// Shut down the browser process.
    pub async fn shutdown(&mut self) {
        if let Some(ref mut child) = self.child.take() {
            let _ = child.start_kill();
            match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
                Ok(_) => debug!("Chromium process exited"),
                Err(_) => warn!("Chromium process did not exit after kill signal"),
            }
        }
    }

    /// Poll the CDP `/json/version` endpoint until it responds.
    async fn wait_until_ready(&mut self, timeout_ms: u64) -> Result<(), BrowserError> {
        let client = reqwest::Client::new();
        let version_url = format!("{}/json/version", self.cdp_endpoint);
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);

        loop {
            match client
                .get(&version_url)
                .timeout(Duration::from_millis(500))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                _ => {}
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(BrowserError::Timeout(timeout_ms));
            }

            // Check that the child hasn't exited unexpectedly.
            if let Some(ref mut child) = self.child {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        return Err(BrowserError::NotAvailable(format!(
                            "Chromium exited prematurely with status {status}"
                        )));
                    }
                    Err(e) => {
                        return Err(BrowserError::NotAvailable(format!(
                            "failed to check Chromium process status: {e}"
                        )));
                    }
                    Ok(None) => {} // still running
                }
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

impl Drop for ChromiumLauncher {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

/// Search common locations for a Chromium/Chrome binary.
#[must_use]
pub fn find_chromium_binary() -> Option<String> {
    let candidates = [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
    ];

    for name in &candidates {
        if which_exists(name) {
            return Some((*name).to_string());
        }
    }
    None
}

/// Check whether a binary exists on PATH using `which`.
fn which_exists(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_launch_options() {
        let opts = LaunchOptions::default();
        assert_eq!(opts.port, 9222);
        assert!(opts.headless);
        assert_eq!(opts.timeout_ms, 10_000);
        assert!(opts.binary_path.is_none());
    }

    #[test]
    fn find_chromium_binary_does_not_panic() {
        // Just verifies the function runs without panicking.
        // The result depends on the host environment.
        let _ = find_chromium_binary();
    }
}
