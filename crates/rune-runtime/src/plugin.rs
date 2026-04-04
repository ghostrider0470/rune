//! Plugin discovery, loading, and lifecycle management.
//!
//! Plugins are discovered by scanning a `plugins_dir` for subdirectories
//! containing a `PLUGIN.md` manifest file with YAML frontmatter. Each plugin
//! is spawned as a subprocess communicating via stdin/stdout JSON-RPC.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
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

pub const PLUGIN_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_PLUGIN_VERSION: &str = "0.0.0";
const DEFAULT_PLUGIN_BINARY: &str = "./plugin";

/// Parsed native plugin manifest contract.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    /// Unique plugin name.
    pub name: String,
    /// Absolute path to the manifest used to construct this contract.
    pub manifest_path: PathBuf,
    /// Versioned manifest schema understood by the runtime.
    pub schema_version: u32,
    /// Semver version string.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Path to the plugin binary (relative to plugin dir or absolute).
    pub binary: PathBuf,
    /// Runtime capabilities declared by the plugin author.
    pub capabilities: Vec<String>,
    /// Canonicalized capabilities for deterministic comparisons.
    pub capability_set: Vec<String>,
    /// Hook events this plugin subscribes to.
    pub hooks: Vec<String>,
    /// Canonicalized hook subscriptions for deterministic comparisons.
    pub hook_set: Vec<String>,
    /// Optional manifest author identifier.
    pub author: Option<String>,
    /// Optional homepage or source URL.
    pub homepage: Option<String>,
    /// The directory containing the PLUGIN.md file.
    #[serde(skip)]
    pub source_dir: PathBuf,
}

/// YAML frontmatter parsed from a PLUGIN.md file.
#[derive(Clone, Debug, Deserialize)]
pub struct PluginFrontmatter {
    pub schema_version: Option<u32>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub binary: Option<String>,
    pub capabilities: Option<String>,
    pub hooks: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginManifestDiagnostic {
    pub field: String,
    pub message: String,
}

impl PluginManifestDiagnostic {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginManifestValidationError {
    pub path: PathBuf,
    pub diagnostics: Vec<PluginManifestDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginManifestReport {
    pub manifest: PluginManifest,
    pub warnings: Vec<PluginManifestDiagnostic>,
}

impl PluginManifestValidationError {
    fn new(path: impl Into<PathBuf>, diagnostics: Vec<PluginManifestDiagnostic>) -> Self {
        Self {
            path: path.into(),
            diagnostics,
        }
    }

    pub fn diagnostics(&self) -> &[PluginManifestDiagnostic] {
        &self.diagnostics
    }
}

impl fmt::Display for PluginManifestValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid plugin manifest at {}", self.path.display())?;
        for diagnostic in &self.diagnostics {
            write!(f, "\n- {}: {}", diagnostic.field, diagnostic.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for PluginManifestValidationError {}

impl PluginManifestReport {
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
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
        let mut line =
            serde_json::to_string(&request).map_err(|e| format!("serialize error: {e}"))?;
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

        serde_json::from_str(&response_line).map_err(|e| format!("invalid JSON from plugin: {e}"))
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
        self.instances.write().await.insert(
            name.to_string(),
            Arc::new(tokio::sync::Mutex::new(instance)),
        );
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
        inst.send_request(&format!("hook/{}", event.as_str()), context.clone())
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
                Ok(report) => {
                    if report.has_warnings() {
                        for warning in &report.warnings {
                            info!(
                                path = %plugin_md.display(),
                                field = %warning.field,
                                detail = %warning.message,
                                "plugin manifest default applied"
                            );
                        }
                    }
                    let manifest = report.manifest;
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
    let normalized = content.replace("\r\n", "\n");
    let content = normalized.trim();
    if !content.starts_with("---\n") {
        return None;
    }
    let rest = &content[4..];
    let end = rest.find("\n---")?;
    let yaml_block = &rest[..end];
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
        } else if let Ok(number) = value.parse::<u64>() {
            map.insert(key, serde_json::Value::Number(number.into()));
        } else if let Ok(number) = value.parse::<i64>() {
            map.insert(key, serde_json::Value::Number(number.into()));
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

fn canonicalize_csv_items(items: &[String]) -> Vec<String> {
    let mut deduped = BTreeSet::new();
    for item in items {
        deduped.insert(item.trim().to_string());
    }
    deduped
        .into_iter()
        .filter(|item| !item.is_empty())
        .collect()
}

fn parse_csv_field(
    _field: &str,
    value: Option<String>,
) -> Result<Vec<String>, PluginManifestDiagnostic> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    let items = value
        .split(',')
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect::<Vec<_>>();

    Ok(items)
}

fn validate_plugin_frontmatter(
    path: &Path,
    frontmatter: PluginFrontmatter,
) -> Result<PluginManifestReport, PluginManifestValidationError> {
    let source_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let manifest_path = path.to_path_buf();
    let dir_name = source_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let mut diagnostics = Vec::new();
    let mut warnings = Vec::new();

    let schema_version = frontmatter
        .schema_version
        .unwrap_or(PLUGIN_MANIFEST_SCHEMA_VERSION);
    if schema_version != PLUGIN_MANIFEST_SCHEMA_VERSION {
        diagnostics.push(PluginManifestDiagnostic::new(
            "schema_version",
            format!(
                "unsupported schema version {schema_version}; supported version is {PLUGIN_MANIFEST_SCHEMA_VERSION}"
            ),
        ));
    }

    let name = frontmatter.name.unwrap_or_else(|| {
        warnings.push(PluginManifestDiagnostic::new(
            "name",
            format!("missing name; defaulted to directory name `{dir_name}`"),
        ));
        dir_name.clone()
    });
    if name.trim().is_empty() {
        diagnostics.push(PluginManifestDiagnostic::new("name", "must be non-empty"));
    }

    let version = frontmatter.version.unwrap_or_else(|| {
        warnings.push(PluginManifestDiagnostic::new(
            "version",
            format!("missing version; defaulted to {DEFAULT_PLUGIN_VERSION}"),
        ));
        DEFAULT_PLUGIN_VERSION.to_string()
    });
    if version.trim().is_empty() {
        diagnostics.push(PluginManifestDiagnostic::new(
            "version",
            "must be non-empty; use an explicit semver string or omit it to default to 0.0.0",
        ));
    }

    let description = frontmatter.description.unwrap_or_else(|| {
        let default_description = format!("Plugin: {name}");
        warnings.push(PluginManifestDiagnostic::new(
            "description",
            format!("missing description; defaulted to `{default_description}`"),
        ));
        default_description
    });
    if description.trim().is_empty() {
        diagnostics.push(PluginManifestDiagnostic::new(
            "description",
            "must be non-empty",
        ));
    }

    let binary_value = frontmatter.binary.unwrap_or_else(|| {
        warnings.push(PluginManifestDiagnostic::new(
            "binary",
            format!("missing binary; defaulted to {DEFAULT_PLUGIN_BINARY}"),
        ));
        DEFAULT_PLUGIN_BINARY.to_string()
    });
    if binary_value.trim().is_empty() {
        diagnostics.push(PluginManifestDiagnostic::new(
            "binary",
            "must be non-empty; use a relative executable path or omit it to default to ./plugin",
        ));
    }
    let binary = PathBuf::from(binary_value.trim());

    let capabilities = match parse_csv_field("capabilities", frontmatter.capabilities) {
        Ok(items) => items,
        Err(err) => {
            diagnostics.push(err);
            Vec::new()
        }
    };
    let capability_set = canonicalize_csv_items(&capabilities);

    let hooks = match parse_csv_field("hooks", frontmatter.hooks) {
        Ok(items) => items,
        Err(err) => {
            diagnostics.push(err);
            Vec::new()
        }
    };
    let hook_set = canonicalize_csv_items(&hooks);

    let author = frontmatter.author.map(|value| value.trim().to_string());
    if author.as_deref().is_some_and(|value| value.is_empty()) {
        diagnostics.push(PluginManifestDiagnostic::new(
            "author",
            "must be non-empty when provided",
        ));
    }

    let homepage = frontmatter.homepage.map(|value| value.trim().to_string());
    if homepage.as_deref().is_some_and(|value| value.is_empty()) {
        diagnostics.push(PluginManifestDiagnostic::new(
            "homepage",
            "must be non-empty when provided",
        ));
    }

    let mut duplicate_capabilities = BTreeMap::<String, usize>::new();
    for capability in &capabilities {
        *duplicate_capabilities
            .entry(capability.clone())
            .or_default() += 1;
    }
    let duplicate_capabilities = duplicate_capabilities
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(capability, _)| capability)
        .collect::<Vec<_>>();
    if !duplicate_capabilities.is_empty() {
        diagnostics.push(PluginManifestDiagnostic::new(
            "capabilities",
            format!(
                "contains duplicate entries: {}",
                duplicate_capabilities.join(", ")
            ),
        ));
    }

    for hook in &hooks {
        if HookEvent::from_str(hook).is_none() {
            diagnostics.push(PluginManifestDiagnostic::new(
                "hooks",
                format!("unknown hook event `{hook}`"),
            ));
        }
    }

    if diagnostics.is_empty() {
        Ok(PluginManifestReport {
            manifest: PluginManifest {
            name,
            manifest_path,
            schema_version,
            version,
            description,
            binary,
            capabilities,
            capability_set,
            hooks,
            hook_set,
            author: author.filter(|value| !value.is_empty()),
            homepage: homepage.filter(|value| !value.is_empty()),
            source_dir: source_dir.to_path_buf(),
        },
            warnings,
        })
    } else {
        Err(PluginManifestValidationError::new(path, diagnostics))
    }
}

/// Load a plugin manifest from a PLUGIN.md file path.
pub async fn load_plugin_manifest(path: &Path) -> Result<PluginManifestReport, String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("read error: {e}"))?;

    let frontmatter = parse_plugin_frontmatter(&content)
        .ok_or_else(|| "no valid frontmatter found".to_string())?;

    validate_plugin_frontmatter(path, frontmatter).map_err(|e| e.to_string())
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
schema_version: 1
name: my-plugin
version: 1.0.0
description: A test plugin
binary: ./run.sh
capabilities: hooks, notifications
hooks: pre_tool_call, post_tool_call
---

# My Plugin

Body content here.
"#;
        let fm = parse_plugin_frontmatter(content).unwrap();
        assert_eq!(fm.schema_version, Some(1));
        assert_eq!(fm.name.as_deref(), Some("my-plugin"));
        assert_eq!(fm.version.as_deref(), Some("1.0.0"));
        assert_eq!(fm.description.as_deref(), Some("A test plugin"));
        assert_eq!(fm.binary.as_deref(), Some("./run.sh"));
        assert_eq!(fm.capabilities.as_deref(), Some("hooks, notifications"));
        assert_eq!(fm.hooks.as_deref(), Some("pre_tool_call, post_tool_call"));
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
        assert!(fm.capabilities.is_none());
    }

    #[test]
    fn parse_manifest_rejects_missing_delimiter() {
        let content = "name: broken\ndescription: nope\n";
        assert!(parse_plugin_frontmatter(content).is_none());
    }

    #[test]
    fn validate_manifest_builds_versioned_contract() {
        let frontmatter = PluginFrontmatter {
            schema_version: Some(1),
            name: Some("native-plugin".into()),
            version: Some("1.2.3".into()),
            description: Some("Native plugin".into()),
            binary: Some("./bin/native".into()),
            capabilities: Some("hooks, commands".into()),
            hooks: Some("pre_tool_call, post_tool_call".into()),
            author: Some("Rune Team".into()),
            homepage: Some("https://example.com/native-plugin".into()),
        };

        let report = validate_plugin_frontmatter(Path::new("/tmp/native/PLUGIN.md"), frontmatter)
            .expect("manifest should validate");
        let manifest = report.manifest;

        assert!(report.warnings.is_empty());
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.capabilities, vec!["hooks", "commands"]);
        assert_eq!(manifest.capability_set, vec!["commands", "hooks"]);
        assert_eq!(manifest.hooks, vec!["pre_tool_call", "post_tool_call"]);
        assert_eq!(manifest.hook_set, vec!["post_tool_call", "pre_tool_call"]);
        assert_eq!(manifest.binary, PathBuf::from("./bin/native"));
        assert_eq!(manifest.author.as_deref(), Some("Rune Team"));
        assert_eq!(
            manifest.homepage.as_deref(),
            Some("https://example.com/native-plugin")
        );
    }

    #[test]
    fn validate_manifest_reports_precise_diagnostics() {
        let frontmatter = PluginFrontmatter {
            schema_version: Some(99),
            name: Some(String::new()),
            version: Some(String::new()),
            description: Some(String::new()),
            binary: Some(String::new()),
            capabilities: Some("hooks, hooks".into()),
            hooks: Some("pre_tool_call, unknown_event".into()),
            author: Some(String::new()),
            homepage: Some(String::new()),
        };

        let error = validate_plugin_frontmatter(Path::new("/tmp/bad/PLUGIN.md"), frontmatter)
            .expect_err("manifest should fail validation");
        let rendered = error.to_string();

        assert!(rendered.contains("schema_version"));
        assert!(rendered.contains("name"));
        assert!(rendered.contains("version"));
        assert!(rendered.contains("description"));
        assert!(rendered.contains("binary"));
        assert!(rendered.contains("capabilities"));
        assert!(rendered.contains("hooks"));
        assert!(rendered.contains("author"));
        assert!(rendered.contains("homepage"));
        assert!(rendered.contains("unsupported schema version 99"));
    }

    #[test]
    fn validate_manifest_defaults_metadata_and_canonicalizes_duplicates() {
        let frontmatter = PluginFrontmatter {
            schema_version: None,
            name: Some("native-plugin".into()),
            version: None,
            description: Some("Native plugin".into()),
            binary: None,
            capabilities: Some("hooks, commands, hooks".into()),
            hooks: Some("post_tool_call, pre_tool_call, post_tool_call".into()),
            author: None,
            homepage: None,
        };

        let manifest = validate_plugin_frontmatter(Path::new("/tmp/native/PLUGIN.md"), frontmatter)
            .expect_err("duplicates should fail validation");
        let rendered = manifest.to_string();
        assert!(rendered.contains("capabilities"));
    }


    #[test]
    fn validate_manifest_reports_defaults_as_warnings() {
        let frontmatter = PluginFrontmatter {
            schema_version: None,
            name: None,
            version: None,
            description: None,
            binary: None,
            capabilities: None,
            hooks: None,
            author: None,
            homepage: None,
        };

        let report = validate_plugin_frontmatter(Path::new("/tmp/example-plugin/PLUGIN.md"), frontmatter)
            .expect("defaults should still produce a valid manifest");

        assert_eq!(report.manifest.name, "example-plugin");
        assert_eq!(report.manifest.manifest_path, PathBuf::from("/tmp/example-plugin/PLUGIN.md"));
        assert_eq!(report.manifest.version, DEFAULT_PLUGIN_VERSION);
        assert_eq!(report.manifest.description, "Plugin: example-plugin");
        assert_eq!(report.manifest.binary, PathBuf::from(DEFAULT_PLUGIN_BINARY));
        assert_eq!(report.warnings.len(), 4);
        assert!(report.warnings.iter().any(|w| w.field == "name"));
        assert!(report.warnings.iter().any(|w| w.field == "version"));
        assert!(report.warnings.iter().any(|w| w.field == "description"));
        assert!(report.warnings.iter().any(|w| w.field == "binary"));
    }

    #[tokio::test]
    async fn registry_crud() {
        let reg = PluginRegistry::new();

        let manifest = PluginManifest {
            name: "test-plugin".into(),
            manifest_path: PathBuf::from("/tmp/test-plugin/PLUGIN.md"),
            schema_version: 1,
            version: "1.0.0".into(),
            description: "Test".into(),
            binary: PathBuf::from("./test"),
            capabilities: vec!["hooks".into()],
            capability_set: vec!["hooks".into()],
            hooks: vec!["pre_tool_call".into()],
            hook_set: vec!["pre_tool_call".into()],
            author: None,
            homepage: None,
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

        let plugin_dir = tmp.path().join("my-plugin");
        tokio::fs::create_dir_all(&plugin_dir).await.unwrap();
        tokio::fs::write(
            plugin_dir.join("PLUGIN.md"),
            r#"---
schema_version: 1
name: my-plugin
version: 1.0.0
description: A test plugin
binary: ./run.sh
capabilities: hooks, notifications
hooks: pre_tool_call, post_tool_call
---

# My Plugin
"#,
        )
        .await
        .unwrap();

        let no_plugin = tmp.path().join("not-a-plugin");
        tokio::fs::create_dir_all(&no_plugin).await.unwrap();

        let registry = Arc::new(PluginRegistry::new());
        let loader = PluginLoader::new(tmp.path(), registry.clone());

        let summary = loader.scan().await;
        assert_eq!(summary.discovered, 1);
        assert_eq!(summary.loaded, 1);
        assert_eq!(summary.removed, 0);

        let manifest = registry.get("my-plugin").await.unwrap();
        assert_eq!(manifest.manifest_path, plugin_dir.join("PLUGIN.md"));
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.capabilities, vec!["hooks", "notifications"]);
        assert_eq!(manifest.capability_set, vec!["hooks", "notifications"]);
        assert_eq!(manifest.hooks, vec!["pre_tool_call", "post_tool_call"]);
        assert_eq!(manifest.hook_set, vec!["post_tool_call", "pre_tool_call"]);
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
schema_version: 1
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
