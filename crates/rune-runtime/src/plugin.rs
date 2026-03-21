//! Plugin discovery, loading, and lifecycle management.
//!
//! Plugins are discovered by scanning a `plugins_dir` for subdirectories
//! containing a `PLUGIN.md` manifest file with YAML frontmatter. Each plugin
//! is spawned as a subprocess communicating via stdin/stdout JSON-RPC.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::hooks::{HookEvent, HookHandler, HookRegistry};

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// Parsed from PLUGIN.md frontmatter.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Unique plugin name.
    pub name: String,
    /// Semver version string.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Path to the plugin binary (relative to plugin dir or absolute).
    pub binary: PathBuf,
    /// Hook events this plugin subscribes to.
    pub hooks: Vec<String>,
    /// The directory containing the PLUGIN.md file.
    #[serde(skip)]
    pub source_dir: PathBuf,
}

/// YAML frontmatter parsed from a PLUGIN.md file.
#[derive(Clone, Debug, Deserialize)]
pub struct PluginFrontmatter {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub binary: Option<String>,
    pub hooks: Option<String>,
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

/// A running plugin subprocess.
pub struct PluginInstance {
    pub manifest: PluginManifest,
    child: Child,
}

impl PluginInstance {
    /// Spawn the plugin binary as a subprocess.
    pub async fn spawn(manifest: PluginManifest) -> Result<Self, String> {
        let binary_path = if manifest.binary.is_relative() {
            manifest.source_dir.join(&manifest.binary)
        } else {
            manifest.binary.clone()
        };

        if !binary_path.exists() {
            return Err(format!(
                "plugin binary not found: {}",
                binary_path.display()
            ));
        }

        let child = Command::new(&binary_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("failed to spawn plugin {}: {e}", manifest.name))?;

        info!(plugin = %manifest.name, binary = %binary_path.display(), "plugin spawned");

        Ok(Self { manifest, child })
    }

    /// Send a JSON-RPC request to the plugin and read its response.
    pub async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        });

        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or("plugin stdin not available")?;
        let mut line = serde_json::to_string(&request)
            .map_err(|e| format!("serialize error: {e}"))?;
        line.push('\n');

        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("failed to write to plugin stdin: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("failed to flush plugin stdin: {e}"))?;

        let stdout = self
            .child
            .stdout
            .as_mut()
            .ok_or("plugin stdout not available")?;
        let mut reader = BufReader::new(stdout);
        let mut response_line = String::new();

        let read_result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            reader.read_line(&mut response_line),
        )
        .await
        .map_err(|_| "plugin response timed out".to_string())?
        .map_err(|e| format!("failed to read plugin stdout: {e}"))?;

        if read_result == 0 {
            return Err("plugin closed stdout".to_string());
        }

        serde_json::from_str(&response_line)
            .map_err(|e| format!("invalid JSON from plugin: {e}"))
    }

    /// Check if the plugin process is still running.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Kill the plugin process.
    pub async fn kill(&mut self) {
        if let Err(e) = self.child.kill().await {
            warn!(plugin = %self.manifest.name, error = %e, "failed to kill plugin");
        } else {
            info!(plugin = %self.manifest.name, "plugin killed");
        }
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Thread-safe registry for discovered and loaded plugins.
#[derive(Clone)]
pub struct PluginRegistry {
    manifests: Arc<RwLock<HashMap<String, PluginManifest>>>,
    instances: Arc<RwLock<HashMap<String, Arc<tokio::sync::Mutex<PluginInstance>>>>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            manifests: Arc::new(RwLock::new(HashMap::new())),
            instances: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a plugin manifest (discovered but not yet started).
    pub async fn register(&self, manifest: PluginManifest) {
        let name = manifest.name.clone();
        self.manifests.write().await.insert(name.clone(), manifest);
        debug!(plugin = %name, "plugin manifest registered");
    }

    /// Remove a plugin manifest and stop it if running.
    pub async fn remove(&self, name: &str) -> Option<PluginManifest> {
        self.stop(name).await;
        let removed = self.manifests.write().await.remove(name);
        if removed.is_some() {
            debug!(plugin = %name, "plugin removed");
        }
        removed
    }

    /// Start a registered plugin (spawn its subprocess).
    pub async fn start(&self, name: &str) -> Result<(), String> {
        let manifest = self
            .manifests
            .read()
            .await
            .get(name)
            .cloned()
            .ok_or_else(|| format!("plugin not found: {name}"))?;

        let instance = PluginInstance::spawn(manifest).await?;
        self.instances
            .write()
            .await
            .insert(name.to_string(), Arc::new(tokio::sync::Mutex::new(instance)));
        Ok(())
    }

    /// Stop a running plugin.
    pub async fn stop(&self, name: &str) {
        if let Some(instance) = self.instances.write().await.remove(name) {
            instance.lock().await.kill().await;
        }
    }

    /// Stop all running plugins.
    pub async fn stop_all(&self) {
        let names: Vec<String> = self.instances.read().await.keys().cloned().collect();
        for name in names {
            self.stop(&name).await;
        }
    }

    /// List all registered plugin manifests.
    pub async fn list(&self) -> Vec<PluginManifest> {
        self.manifests.read().await.values().cloned().collect()
    }

    /// List names of running plugins.
    pub async fn list_running(&self) -> Vec<String> {
        self.instances.read().await.keys().cloned().collect()
    }

    /// Get a manifest by name.
    pub async fn get(&self, name: &str) -> Option<PluginManifest> {
        self.manifests.read().await.get(name).cloned()
    }

    /// Send a hook event to a specific running plugin.
    pub async fn notify_plugin(
        &self,
        name: &str,
        event: &HookEvent,
        context: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let instances = self.instances.read().await;
        let instance = instances
            .get(name)
            .ok_or_else(|| format!("plugin {name} is not running"))?
            .clone();
        drop(instances);

        let mut inst = instance.lock().await;
        inst.send_request(
            &format!("hook/{}", event.as_str()),
            context.clone(),
        )
        .await
    }

    /// Register all plugin hook handlers into a HookRegistry.
    pub async fn register_hooks(&self, hook_registry: &HookRegistry) {
        let manifests = self.manifests.read().await;
        for manifest in manifests.values() {
            for hook_str in &manifest.hooks {
                if let Some(event) = HookEvent::from_str(hook_str) {
                    let handler = PluginHookHandler {
                        plugin_name: manifest.name.clone(),
                        registry: self.clone(),
                    };
                    hook_registry.register(event, Box::new(handler)).await;
                    debug!(
                        plugin = %manifest.name,
                        hook = %hook_str,
                        "registered plugin hook handler"
                    );
                } else {
                    warn!(
                        plugin = %manifest.name,
                        hook = %hook_str,
                        "unknown hook event, skipping"
                    );
                }
            }
        }
    }

    /// Number of registered plugins.
    pub async fn len(&self) -> usize {
        self.manifests.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.manifests.read().await.is_empty()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Plugin hook handler (bridges HookHandler → PluginRegistry)
// ---------------------------------------------------------------------------

struct PluginHookHandler {
    plugin_name: String,
    registry: PluginRegistry,
}

#[async_trait::async_trait]
impl HookHandler for PluginHookHandler {
    async fn handle(
        &self,
        event: &HookEvent,
        context: &mut serde_json::Value,
    ) -> Result<(), String> {
        match self
            .registry
            .notify_plugin(&self.plugin_name, event, context)
            .await
        {
            Ok(response) => {
                // If the plugin returns a "context" field, merge it back.
                if let Some(new_ctx) = response.get("result") {
                    if let (Some(target), Some(source)) =
                        (context.as_object_mut(), new_ctx.as_object())
                    {
                        for (k, v) in source {
                            target.insert(k.clone(), v.clone());
                        }
                    }
                }
                Ok(())
            }
            Err(e) => {
                error!(
                    plugin = %self.plugin_name,
                    event = %event.as_str(),
                    error = %e,
                    "plugin hook handler failed"
                );
                Err(e)
            }
        }
    }

    fn plugin_name(&self) -> &str {
        &self.plugin_name
    }
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Summary returned by a plugin scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginScanSummary {
    pub discovered: usize,
    pub loaded: usize,
    pub removed: usize,
}

/// Scans a plugins directory for `*/PLUGIN.md` files and populates a [`PluginRegistry`].
pub struct PluginLoader {
    plugins_dir: PathBuf,
    registry: Arc<PluginRegistry>,
}

impl PluginLoader {
    pub fn new(plugins_dir: impl Into<PathBuf>, registry: Arc<PluginRegistry>) -> Self {
        Self {
            plugins_dir: plugins_dir.into(),
            registry,
        }
    }

    /// Scan the plugins directory and register discovered manifests.
    pub async fn scan(&self) -> PluginScanSummary {
        if !self.plugins_dir.exists() {
            debug!(dir = %self.plugins_dir.display(), "plugins directory does not exist, skipping scan");
            return PluginScanSummary {
                discovered: 0,
                loaded: 0,
                removed: 0,
            };
        }

        let mut discovered = 0;
        let mut loaded = 0;
        let mut seen_names = Vec::new();

        let mut entries = match tokio::fs::read_dir(&self.plugins_dir).await {
            Ok(entries) => entries,
            Err(e) => {
                warn!(error = %e, dir = %self.plugins_dir.display(), "failed to read plugins directory");
                return PluginScanSummary {
                    discovered: 0,
                    loaded: 0,
                    removed: 0,
                };
            }
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let plugin_md = path.join("PLUGIN.md");
            if !plugin_md.exists() {
                continue;
            }

            discovered += 1;
            match load_plugin_manifest(&plugin_md).await {
                Ok(manifest) => {
                    seen_names.push(manifest.name.clone());
                    self.registry.register(manifest).await;
                    loaded += 1;
                }
                Err(e) => {
                    warn!(
                        path = %plugin_md.display(),
                        error = %e,
                        "failed to load plugin manifest"
                    );
                }
            }
        }

        // Remove manifests that no longer exist on disk
        let current = self.registry.list().await;
        let mut removed = 0;
        for m in current {
            if !seen_names.contains(&m.name) {
                self.registry.remove(&m.name).await;
                removed += 1;
            }
        }

        info!(
            discovered,
            loaded,
            removed,
            dir = %self.plugins_dir.display(),
            "plugin scan complete"
        );

        PluginScanSummary {
            discovered,
            loaded,
            removed,
        }
    }

    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }
}

// ---------------------------------------------------------------------------
// Manifest parsing
// ---------------------------------------------------------------------------

/// Parse PLUGIN.md frontmatter into a [`PluginManifest`].
pub fn parse_plugin_frontmatter(content: &str) -> Option<PluginFrontmatter> {
    // Reuse the same YAML frontmatter parser from the skill system
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }

    let after_first = &content[3..];
    let end_pos = after_first.find("\n---")?;
    let yaml_block = after_first[..end_pos].trim();

    // Use the same minimal YAML parser
    let value = parse_yaml_to_json(yaml_block)?;
    serde_json::from_value(value).ok()
}

/// Minimal YAML-subset parser (mirrors the one in skill.rs).
fn parse_yaml_to_json(yaml: &str) -> Option<serde_json::Value> {
    let mut map = serde_json::Map::new();

    for line in yaml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (key, value) = line.split_once(':')?;
        let key = key.trim().to_string();
        let value = value.trim();

        if value.is_empty() {
            map.insert(key, serde_json::Value::Null);
        } else if value == "true" || value == "false" {
            map.insert(
                key,
                serde_json::Value::Bool(value.parse::<bool>().unwrap_or(false)),
            );
        } else if value.starts_with('{') || value.starts_with('[') {
            if let Ok(parsed) = serde_json::from_str(value) {
                map.insert(key, parsed);
            } else {
                map.insert(key, serde_json::Value::String(value.to_string()));
            }
        } else {
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value);
            map.insert(key, serde_json::Value::String(value.to_string()));
        }
    }

    Some(serde_json::Value::Object(map))
}

/// Load a plugin manifest from a PLUGIN.md file path.
async fn load_plugin_manifest(path: &Path) -> Result<PluginManifest, String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("read error: {e}"))?;

    let frontmatter = parse_plugin_frontmatter(&content)
        .ok_or_else(|| "no valid frontmatter found".to_string())?;

    let source_dir = path
        .parent()
        .ok_or_else(|| "no parent directory".to_string())?;

    let dir_name = source_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let name = frontmatter
        .name
        .unwrap_or_else(|| dir_name.to_string());
    let version = frontmatter.version.unwrap_or_else(|| "0.0.0".to_string());
    let description = frontmatter
        .description
        .unwrap_or_else(|| format!("Plugin: {name}"));

    let binary = frontmatter
        .binary
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./plugin"));

    // Parse hooks as comma-separated list
    let hooks = frontmatter
        .hooks
        .map(|h| {
            h.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    Ok(PluginManifest {
        name,
        version,
        description,
        binary,
        hooks,
        source_dir: source_dir.to_path_buf(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_manifest_basic() {
        let content = r#"---
name: my-plugin
version: 1.0.0
description: A test plugin
binary: ./run.sh
hooks: pre_tool_call, post_tool_call
---

# My Plugin

Body content here.
"#;
        let fm = parse_plugin_frontmatter(content).unwrap();
        assert_eq!(fm.name.as_deref(), Some("my-plugin"));
        assert_eq!(fm.version.as_deref(), Some("1.0.0"));
        assert_eq!(fm.description.as_deref(), Some("A test plugin"));
        assert_eq!(fm.binary.as_deref(), Some("./run.sh"));
        assert_eq!(
            fm.hooks.as_deref(),
            Some("pre_tool_call, post_tool_call")
        );
    }

    #[test]
    fn parse_manifest_minimal() {
        let content = r#"---
name: minimal
---
"#;
        let fm = parse_plugin_frontmatter(content).unwrap();
        assert_eq!(fm.name.as_deref(), Some("minimal"));
        assert!(fm.version.is_none());
        assert!(fm.hooks.is_none());
    }

    #[test]
    fn parse_manifest_rejects_missing_delimiter() {
        let content = "name: broken\ndescription: nope\n";
        assert!(parse_plugin_frontmatter(content).is_none());
    }

    #[tokio::test]
    async fn registry_crud() {
        let reg = PluginRegistry::new();

        let manifest = PluginManifest {
            name: "test-plugin".into(),
            version: "1.0.0".into(),
            description: "Test".into(),
            binary: PathBuf::from("./test"),
            hooks: vec!["pre_tool_call".into()],
            source_dir: PathBuf::from("/tmp"),
        };

        reg.register(manifest).await;
        assert_eq!(reg.len().await, 1);
        assert!(reg.get("test-plugin").await.is_some());

        let list = reg.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-plugin");

        reg.remove("test-plugin").await;
        assert_eq!(reg.len().await, 0);
        assert!(reg.get("test-plugin").await.is_none());
    }

    #[tokio::test]
    async fn loader_scan_discovers_plugins() {
        let tmp = TempDir::new().unwrap();

        // Create a plugin directory with PLUGIN.md
        let plugin_dir = tmp.path().join("my-plugin");
        tokio::fs::create_dir_all(&plugin_dir).await.unwrap();
        tokio::fs::write(
            plugin_dir.join("PLUGIN.md"),
            r#"---
name: my-plugin
version: 1.0.0
description: A test plugin
binary: ./run.sh
hooks: pre_tool_call, post_tool_call
---

# My Plugin
"#,
        )
        .await
        .unwrap();

        // Create directory without PLUGIN.md (should be skipped)
        let no_plugin = tmp.path().join("not-a-plugin");
        tokio::fs::create_dir_all(&no_plugin).await.unwrap();

        let registry = Arc::new(PluginRegistry::new());
        let loader = PluginLoader::new(tmp.path(), registry.clone());

        let summary = loader.scan().await;
        assert_eq!(summary.discovered, 1);
        assert_eq!(summary.loaded, 1);
        assert_eq!(summary.removed, 0);

        let manifest = registry.get("my-plugin").await.unwrap();
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.hooks, vec!["pre_tool_call", "post_tool_call"]);
    }

    #[tokio::test]
    async fn loader_scan_removes_missing_plugins() {
        let tmp = TempDir::new().unwrap();
        let registry = Arc::new(PluginRegistry::new());
        let loader = PluginLoader::new(tmp.path(), registry.clone());

        let plugin_dir = tmp.path().join("ephemeral");
        tokio::fs::create_dir_all(&plugin_dir).await.unwrap();
        tokio::fs::write(
            plugin_dir.join("PLUGIN.md"),
            r#"---
name: ephemeral
version: 0.1.0
description: Will be removed
---
"#,
        )
        .await
        .unwrap();

        let first = loader.scan().await;
        assert_eq!(first.loaded, 1);
        assert!(registry.get("ephemeral").await.is_some());

        tokio::fs::remove_dir_all(&plugin_dir).await.unwrap();

        let second = loader.scan().await;
        assert_eq!(second.removed, 1);
        assert!(registry.get("ephemeral").await.is_none());
    }

    #[tokio::test]
    async fn loader_handles_nonexistent_dir() {
        let registry = Arc::new(PluginRegistry::new());
        let loader = PluginLoader::new("/nonexistent/path", registry.clone());
        let summary = loader.scan().await;
        assert_eq!(summary.discovered, 0);
        assert_eq!(summary.loaded, 0);
    }

    #[test]
    fn manifest_hooks_parsing() {
        let content = r#"---
name: multi-hook
hooks: pre_tool_call, post_tool_call, session_created, session_completed
---
"#;
        let fm = parse_plugin_frontmatter(content).unwrap();
        let hooks_str = fm.hooks.unwrap();
        let hooks: Vec<String> = hooks_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        assert_eq!(
            hooks,
            vec![
                "pre_tool_call",
                "post_tool_call",
                "session_created",
                "session_completed"
            ]
        );
    }
}
