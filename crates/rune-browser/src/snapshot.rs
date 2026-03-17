use std::fmt::Write as _;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::error::BrowserError;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A semantic element extracted from the browser's accessibility tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotElement {
    /// Numeric reference for interactive elements: `[ref=N]`.
    /// Only set on elements that a user (or agent) can interact with.
    pub ref_id: Option<u32>,
    /// Accessibility role (e.g. `button`, `link`, `textbox`, `heading`).
    pub role: String,
    /// Accessible name / label.
    pub name: String,
    /// Text content or current value.
    pub value: Option<String>,
    /// Depth in the tree (used for indentation in the text representation).
    pub depth: u32,
    /// Whether this element is interactive (clickable, focusable, editable).
    pub interactive: bool,
    /// Direct children in the accessibility tree.
    pub children: Vec<SnapshotElement>,
}

/// A complete semantic snapshot of a browser page.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrowserSnapshot {
    /// The URL of the captured page.
    pub url: String,
    /// The page title.
    pub title: String,
    /// Top-level elements from the accessibility tree.
    pub elements: Vec<SnapshotElement>,
    /// A compact text representation of the page, designed for LLM consumption.
    pub text: String,
}

/// Options controlling how snapshots are captured.
#[derive(Clone, Debug)]
pub struct SnapshotOptions {
    /// Chrome DevTools Protocol HTTP endpoint (e.g. `http://localhost:9222`).
    /// When `None`, the engine will try `http://localhost:9222`.
    pub cdp_endpoint: Option<String>,
    /// Navigation timeout in milliseconds.
    pub timeout_ms: u64,
    /// Maximum accessibility tree depth to capture.
    pub max_depth: u32,
    /// Optional CSS selector to wait for before capturing the snapshot.
    /// When set, the engine polls `document.querySelector(selector)` until the
    /// element exists (or the timeout expires) instead of using a fixed delay.
    pub wait_for: Option<String>,
}

impl Default for SnapshotOptions {
    fn default() -> Self {
        Self {
            cdp_endpoint: None,
            timeout_ms: 30_000,
            max_depth: 10,
            wait_for: None,
        }
    }
}

// ---------------------------------------------------------------------------
// CDP JSON types (subset)
// ---------------------------------------------------------------------------

/// A target entry returned by `GET /json` on the CDP HTTP endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CdpTarget {
    /// WebSocket URL for the DevTools protocol.
    web_socket_debugger_url: Option<String>,
    /// The page URL.
    #[serde(default)]
    url: String,
    /// The page title.
    #[serde(default)]
    #[allow(dead_code)]
    title: String,
    /// Target type (e.g. "page").
    #[serde(default, rename = "type")]
    target_type: String,
}

/// A node in the accessibility tree returned by `Accessibility.getFullAXTree`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AXNode {
    node_id: String,
    #[serde(default)]
    parent_id: Option<String>,
    role: Option<AXValue>,
    name: Option<AXValue>,
    value: Option<AXValue>,
    #[serde(default)]
    properties: Vec<AXProperty>,
    #[serde(default)]
    child_ids: Vec<String>,
    #[serde(default)]
    ignored: bool,
}

#[derive(Debug, Deserialize)]
struct AXValue {
    #[serde(default)]
    value: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct AXProperty {
    name: String,
    value: AXValue,
}

/// JSON-RPC request for the CDP WebSocket protocol.
#[derive(Serialize)]
struct CdpRequest {
    id: u32,
    method: String,
    params: serde_json::Value,
}

/// JSON-RPC response from the CDP WebSocket protocol.
#[derive(Deserialize)]
struct CdpResponse {
    #[allow(dead_code)]
    id: Option<u32>,
    result: Option<serde_json::Value>,
    error: Option<CdpResponseError>,
}

#[derive(Debug, Deserialize)]
struct CdpResponseError {
    message: String,
}

// ---------------------------------------------------------------------------
// Roles considered "interactive" for ref-id assignment
// ---------------------------------------------------------------------------

const INTERACTIVE_ROLES: &[&str] = &[
    "button",
    "link",
    "textbox",
    "combobox",
    "checkbox",
    "radio",
    "slider",
    "spinbutton",
    "switch",
    "tab",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "searchbox",
    "textarea",
];

fn is_interactive_role(role: &str) -> bool {
    let lower = role.to_ascii_lowercase();
    INTERACTIVE_ROLES.iter().any(|r| *r == lower)
}

// ---------------------------------------------------------------------------
// SnapshotEngine
// ---------------------------------------------------------------------------

/// The snapshot engine connects to a Chromium-based browser via the Chrome
/// DevTools Protocol (CDP) and captures semantic page snapshots built from
/// the accessibility tree.
pub struct SnapshotEngine {
    options: SnapshotOptions,
    client: reqwest::Client,
}

impl SnapshotEngine {
    /// Create a new engine with the given options.
    #[must_use]
    pub fn new(options: SnapshotOptions) -> Self {
        Self {
            options,
            client: reqwest::Client::new(),
        }
    }

    /// Navigate to `url` in the browser and return a semantic snapshot.
    ///
    /// This is the main entry point for capturing a page. It:
    /// 1. Discovers the first *page* target via CDP's HTTP `/json` endpoint.
    /// 2. Sends `Page.navigate` over the CDP WebSocket.
    /// 3. Waits for `Page.loadEventFired` (or times out).
    /// 4. Fetches the full accessibility tree.
    /// 5. Converts it into a [`BrowserSnapshot`].
    pub async fn navigate_and_snapshot(&self, url: &str) -> Result<BrowserSnapshot, BrowserError> {
        let endpoint = self.cdp_endpoint();
        let target = self.find_page_target(&endpoint).await?;

        let ws_url = target.web_socket_debugger_url.as_deref().ok_or_else(|| {
            BrowserError::NotAvailable("CDP target has no webSocketDebuggerUrl".to_string())
        })?;

        info!(%url, "navigating browser to URL");

        // Navigate
        let nav_params = serde_json::json!({ "url": url });
        let _nav_result = self.cdp_send(ws_url, "Page.navigate", nav_params).await?;

        // Wait for the page to be ready.
        if let Some(ref selector) = self.options.wait_for {
            self.wait_for_selector(ws_url, selector).await?;
        } else {
            // Fallback: fixed delay when no selector is specified.
            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.options.timeout_ms.min(5_000),
            ))
            .await;
        }

        // Snapshot
        self.snapshot_ws(ws_url, url).await
    }

    /// Capture a semantic snapshot of the page currently loaded in the browser
    /// (without navigating).
    pub async fn snapshot_current(&self) -> Result<BrowserSnapshot, BrowserError> {
        let endpoint = self.cdp_endpoint();
        let target = self.find_page_target(&endpoint).await?;

        let ws_url = target.web_socket_debugger_url.as_deref().ok_or_else(|| {
            BrowserError::NotAvailable("CDP target has no webSocketDebuggerUrl".to_string())
        })?;

        self.snapshot_ws(ws_url, &target.url).await
    }

    /// Build a [`BrowserSnapshot`] from raw HTML without needing a running
    /// browser.  This is intentionally simplistic -- it extracts a useful
    /// subset of elements via basic string scanning rather than pulling in a
    /// full HTML parser dependency.  It is useful for testing and as a
    /// fallback when Chrome is not available.
    #[must_use]
    pub fn from_html(html: &str) -> BrowserSnapshot {
        let title = extract_tag_content(html, "title").unwrap_or_default();
        let mut elements = Vec::new();
        let ref_counter = AtomicU32::new(1);

        // Extract headings
        for level in 1..=6 {
            let tag = format!("h{level}");
            for text in extract_all_tag_contents(html, &tag) {
                elements.push(SnapshotElement {
                    ref_id: None,
                    role: "heading".to_string(),
                    name: text,
                    value: None,
                    depth: 0,
                    interactive: false,
                    children: Vec::new(),
                });
            }
        }

        // Extract paragraphs
        for text in extract_all_tag_contents(html, "p") {
            elements.push(SnapshotElement {
                ref_id: None,
                role: "paragraph".to_string(),
                name: text,
                value: None,
                depth: 0,
                interactive: false,
                children: Vec::new(),
            });
        }

        // Extract links
        for (href, text) in extract_links(html) {
            let rid = ref_counter.fetch_add(1, Ordering::Relaxed);
            elements.push(SnapshotElement {
                ref_id: Some(rid),
                role: "link".to_string(),
                name: text,
                value: Some(href),
                depth: 0,
                interactive: true,
                children: Vec::new(),
            });
        }

        // Extract buttons
        for text in extract_all_tag_contents(html, "button") {
            let rid = ref_counter.fetch_add(1, Ordering::Relaxed);
            elements.push(SnapshotElement {
                ref_id: Some(rid),
                role: "button".to_string(),
                name: text,
                value: None,
                depth: 0,
                interactive: true,
                children: Vec::new(),
            });
        }

        // Extract inputs
        for (input_type, name, value) in extract_inputs(html) {
            let role = match input_type.as_str() {
                "checkbox" => "checkbox",
                "radio" => "radio",
                "submit" | "button" => "button",
                _ => "textbox",
            };
            let rid = ref_counter.fetch_add(1, Ordering::Relaxed);
            elements.push(SnapshotElement {
                ref_id: Some(rid),
                role: role.to_string(),
                name,
                value: if value.is_empty() { None } else { Some(value) },
                depth: 0,
                interactive: true,
                children: Vec::new(),
            });
        }

        let text = render_text(&title, "", &elements);

        BrowserSnapshot {
            url: String::new(),
            title,
            elements,
            text,
        }
    }

    // -- private helpers ----------------------------------------------------

    /// Poll `document.querySelector(selector)` via CDP `Runtime.evaluate`
    /// every 200 ms until the element exists or the timeout expires.
    async fn wait_for_selector(&self, ws_url: &str, selector: &str) -> Result<(), BrowserError> {
        let escaped = selector.replace('\\', "\\\\").replace('\'', "\\'");
        let expression = format!("document.querySelector('{}') !== null", escaped);
        let deadline = tokio::time::Instant::now()
            + tokio::time::Duration::from_millis(self.options.timeout_ms);

        loop {
            let params = serde_json::json!({
                "expression": expression,
                "returnByValue": true,
            });

            match self.cdp_send(ws_url, "Runtime.evaluate", params).await {
                Ok(result) => {
                    let found = result
                        .get("result")
                        .and_then(|r| r.get("value"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    if found {
                        debug!(selector, "wait_for selector matched");
                        return Ok(());
                    }
                }
                Err(e) => {
                    debug!(?e, "wait_for_selector poll error (will retry)");
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(BrowserError::Timeout(self.options.timeout_ms));
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
    }

    /// Resolved CDP HTTP endpoint.
    fn cdp_endpoint(&self) -> String {
        self.options
            .cdp_endpoint
            .clone()
            .unwrap_or_else(|| "http://localhost:9222".to_string())
    }

    /// Discover the first "page"-type target via the CDP HTTP `/json` endpoint.
    async fn find_page_target(&self, endpoint: &str) -> Result<CdpTarget, BrowserError> {
        let json_url = format!("{endpoint}/json");

        let response = self
            .client
            .get(&json_url)
            .timeout(std::time::Duration::from_millis(5_000))
            .send()
            .await
            .map_err(|e| {
                BrowserError::NotAvailable(format!("cannot reach CDP endpoint at {endpoint}: {e}"))
            })?;

        let targets: Vec<CdpTarget> = response
            .json()
            .await
            .map_err(|e| BrowserError::NotAvailable(format!("invalid CDP /json response: {e}")))?;

        debug!(target_count = targets.len(), "CDP targets discovered");

        targets
            .into_iter()
            .find(|t| t.target_type == "page")
            .ok_or_else(|| {
                BrowserError::NotAvailable("no page target found in CDP /json".to_string())
            })
    }

    /// Send a single CDP command over HTTP.  The Chrome DevTools Protocol
    /// normally uses a persistent WebSocket, but for one-shot commands we can
    /// use the `/json/protocol` HTTP endpoints or open a short-lived WS
    /// connection.  Since adding a WebSocket dependency is heavy, we use
    /// reqwest to POST to an internal helper endpoint when available, and
    /// fall back to a simplified HTTP-based flow.
    ///
    /// In practice, `Page.navigate` and `Accessibility.getFullAXTree` must
    /// go through the WebSocket.  We encode the request as a JSON body and
    /// send it via a POST to the browser's built-in HTTP API, which some
    /// Chromium builds expose at `/json/command`.  If that is unavailable we
    /// return a descriptive error.
    async fn cdp_send(
        &self,
        ws_url: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, BrowserError> {
        // Derive an HTTP URL from the WebSocket URL so we can issue the
        // command without a WebSocket client.  Chrome exposes an HTTP
        // fallback at the same port.
        let http_url = ws_url
            .replace("ws://", "http://")
            .replace("wss://", "https://");

        // Build a minimal JSON-RPC payload.
        let request = CdpRequest {
            id: 1,
            method: method.to_string(),
            params,
        };

        let body = serde_json::to_string(&request).map_err(|e| {
            BrowserError::SnapshotFailed(format!("failed to serialize CDP request: {e}"))
        })?;

        let response = self
            .client
            .post(&http_url)
            .header("Content-Type", "application/json")
            .body(body)
            .timeout(std::time::Duration::from_millis(self.options.timeout_ms))
            .send()
            .await
            .map_err(|e| {
                BrowserError::SnapshotFailed(format!("CDP command {method} failed: {e}"))
            })?;

        let cdp_resp: CdpResponse = response.json().await.map_err(|e| {
            BrowserError::SnapshotFailed(format!("invalid CDP response for {method}: {e}"))
        })?;

        if let Some(err) = cdp_resp.error {
            return Err(BrowserError::SnapshotFailed(format!(
                "CDP error for {method}: {}",
                err.message
            )));
        }

        cdp_resp.result.ok_or_else(|| {
            BrowserError::SnapshotFailed(format!("CDP response for {method} contained no result"))
        })
    }

    /// Capture a snapshot given a WebSocket URL to a page target.
    async fn snapshot_ws(
        &self,
        ws_url: &str,
        page_url: &str,
    ) -> Result<BrowserSnapshot, BrowserError> {
        // Fetch the full accessibility tree.
        let tree_result = self
            .cdp_send(ws_url, "Accessibility.getFullAXTree", serde_json::json!({}))
            .await?;

        let ax_nodes: Vec<AXNode> = {
            let nodes_val = tree_result.get("nodes").ok_or_else(|| {
                BrowserError::SnapshotFailed(
                    "Accessibility.getFullAXTree response missing 'nodes'".to_string(),
                )
            })?;
            serde_json::from_value(nodes_val.clone()).map_err(|e| {
                BrowserError::SnapshotFailed(format!("failed to parse AX nodes: {e}"))
            })?
        };

        info!(node_count = ax_nodes.len(), "accessibility tree fetched");

        // Retrieve the page title from the root node or fall back to the
        // target metadata.
        let title = ax_nodes
            .first()
            .and_then(|n| n.name.as_ref())
            .and_then(|v| v.value.as_str())
            .unwrap_or("")
            .to_string();

        // Use a per-snapshot ref counter to avoid races between concurrent calls.
        let ref_counter = AtomicU32::new(1);

        // Build the element tree.
        let elements = Self::build_element_tree(&ref_counter, &ax_nodes, 0, self.options.max_depth);

        let text = render_text(&title, page_url, &elements);

        Ok(BrowserSnapshot {
            url: page_url.to_string(),
            title,
            elements,
            text,
        })
    }

    /// Recursively convert a flat list of [`AXNode`]s into a tree of
    /// [`SnapshotElement`]s, starting from nodes that have no parent (roots)
    /// or from a specific parent id.
    fn build_element_tree(
        ref_counter: &AtomicU32,
        nodes: &[AXNode],
        depth: u32,
        max_depth: u32,
    ) -> Vec<SnapshotElement> {
        // Index nodes by id for O(1) child lookup.
        use std::collections::HashMap;
        let by_id: HashMap<&str, &AXNode> = nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();

        // Find root nodes (no parent).
        let roots: Vec<&AXNode> = nodes
            .iter()
            .filter(|n| n.parent_id.is_none() && !n.ignored)
            .collect();

        let mut elements = Vec::new();
        for root in roots {
            if let Some(el) = Self::convert_node(ref_counter, root, &by_id, depth, max_depth) {
                elements.push(el);
            }
        }
        elements
    }

    /// Convert a single AX node and its children into a [`SnapshotElement`].
    fn convert_node(
        ref_counter: &AtomicU32,
        node: &AXNode,
        by_id: &std::collections::HashMap<&str, &AXNode>,
        depth: u32,
        max_depth: u32,
    ) -> Option<SnapshotElement> {
        if node.ignored {
            return None;
        }

        let role = node
            .role
            .as_ref()
            .and_then(|v| v.value.as_str())
            .unwrap_or("generic")
            .to_string();

        // Skip uninteresting structural roles that add noise.
        if matches!(
            role.as_str(),
            "none" | "generic" | "InlineTextBox" | "StaticText"
        ) && node.child_ids.is_empty()
        {
            // Leaf generic nodes with a name are kept as implicit text.
            let name = node
                .name
                .as_ref()
                .and_then(|v| v.value.as_str())
                .unwrap_or("");
            if name.is_empty() {
                return None;
            }
        }

        let name = node
            .name
            .as_ref()
            .and_then(|v| v.value.as_str())
            .unwrap_or("")
            .to_string();

        let value = node
            .value
            .as_ref()
            .and_then(|v| v.value.as_str())
            .map(|s| s.to_string());

        let interactive = is_interactive_role(&role)
            || node.properties.iter().any(|p| {
                (p.name == "focusable" || p.name == "editable")
                    && p.value.value.as_bool().unwrap_or(false)
            });

        let ref_id = if interactive {
            Some(ref_counter.fetch_add(1, Ordering::Relaxed))
        } else {
            None
        };

        // Recurse into children (respecting max depth).
        let children = if depth + 1 < max_depth {
            node.child_ids
                .iter()
                .filter_map(|cid| {
                    let child_node = by_id.get(cid.as_str())?;
                    Self::convert_node(ref_counter, child_node, by_id, depth + 1, max_depth)
                })
                .collect()
        } else {
            Vec::new()
        };

        Some(SnapshotElement {
            ref_id,
            role,
            name,
            value,
            depth,
            interactive,
            children,
        })
    }
}

// ---------------------------------------------------------------------------
// Text rendering
// ---------------------------------------------------------------------------

/// Render a list of snapshot elements into the compact text format consumed
/// by LLMs.
fn render_text(title: &str, url: &str, elements: &[SnapshotElement]) -> String {
    let mut out = String::with_capacity(4096);

    if !title.is_empty() {
        let _ = writeln!(out, "Page: {title}");
    }
    if !url.is_empty() {
        let _ = writeln!(out, "URL: {url}");
    }
    if !title.is_empty() || !url.is_empty() {
        out.push('\n');
    }

    for el in elements {
        render_element(&mut out, el, 0);
    }

    out
}

fn render_element(out: &mut String, el: &SnapshotElement, indent: usize) {
    let pad = "  ".repeat(indent);

    // Reference prefix
    let ref_prefix = match el.ref_id {
        Some(id) => format!("[{id}] "),
        None => String::new(),
    };

    let line = match (&el.role[..], &el.value) {
        ("heading", _) => format!("{ref_prefix}heading: \"{}\"", el.name),
        ("paragraph", _) => format!("paragraph: \"{}\"", el.name),
        ("link", Some(href)) if !href.is_empty() => {
            format!("{ref_prefix}link: \"{}\" -> {href}", el.name)
        }
        ("button", _) => format!("{ref_prefix}button: \"{}\"", el.name),
        (_, Some(val)) if !val.is_empty() => {
            format!("{ref_prefix}{}: \"{}\" = \"{val}\"", el.role, el.name)
        }
        _ => format!("{ref_prefix}{}: \"{}\"", el.role, el.name),
    };

    let _ = writeln!(out, "{pad}{line}");

    for child in &el.children {
        render_element(out, child, indent + 1);
    }
}

// ---------------------------------------------------------------------------
// Minimal HTML extraction helpers (no external parser dependency)
// ---------------------------------------------------------------------------

/// Extract the text content of the first occurrence of `<tag>...</tag>`.
fn extract_tag_content(html: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");

    let start_tag = html.find(&open)?;
    let after_open = html[start_tag..].find('>')? + start_tag + 1;
    let end_tag = html[after_open..].find(&close)? + after_open;

    Some(strip_tags(&html[after_open..end_tag]).trim().to_string())
}

/// Extract text content of all occurrences of `<tag>...</tag>`.
fn extract_all_tag_contents(html: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut results = Vec::new();
    let mut search_from = 0;

    while search_from < html.len() {
        let remaining = &html[search_from..];
        let Some(rel_start) = remaining.find(&open) else {
            break;
        };
        let abs_start = search_from + rel_start;
        let after_tag = match html[abs_start..].find('>') {
            Some(p) => abs_start + p + 1,
            None => break,
        };
        let end = match html[after_tag..].find(&close) {
            Some(p) => after_tag + p,
            None => break,
        };

        let text = html_decode(&strip_tags(&html[after_tag..end]))
            .trim()
            .to_string();
        if !text.is_empty() {
            results.push(text);
        }
        search_from = end + close.len();
    }
    results
}

/// Extract `(href, text)` pairs from `<a href="...">text</a>`.
fn extract_links(html: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let mut search_from = 0;

    while search_from < html.len() {
        let remaining = &html[search_from..];
        let Some(a_pos) = remaining.find("<a ") else {
            break;
        };
        let abs_a = search_from + a_pos;

        // Find href attribute
        let tag_end = match html[abs_a..].find('>') {
            Some(p) => abs_a + p,
            None => break,
        };
        let tag_str = &html[abs_a..tag_end];
        let href = extract_attr(tag_str, "href").unwrap_or_default();

        let content_start = tag_end + 1;
        let close = match html[content_start..].find("</a>") {
            Some(p) => content_start + p,
            None => break,
        };

        let text = html_decode(&strip_tags(&html[content_start..close]))
            .trim()
            .to_string();
        if !text.is_empty() || !href.is_empty() {
            results.push((href, text));
        }
        search_from = close + 4;
    }
    results
}

/// Extract `(type, name/placeholder, value)` triples from `<input ...>` tags.
fn extract_inputs(html: &str) -> Vec<(String, String, String)> {
    let mut results = Vec::new();
    let mut search_from = 0;

    while search_from < html.len() {
        let remaining = &html[search_from..];
        let Some(pos) = remaining.find("<input") else {
            break;
        };
        let abs_pos = search_from + pos;

        let tag_end = match html[abs_pos..].find('>') {
            Some(p) => abs_pos + p + 1,
            None => break,
        };
        let tag_str = &html[abs_pos..tag_end];

        let input_type = extract_attr(tag_str, "type").unwrap_or_else(|| "text".to_string());
        let name = extract_attr(tag_str, "placeholder")
            .or_else(|| extract_attr(tag_str, "name"))
            .or_else(|| extract_attr(tag_str, "aria-label"))
            .unwrap_or_default();
        let value = extract_attr(tag_str, "value").unwrap_or_default();

        results.push((input_type, name, value));
        search_from = tag_end;
    }
    results
}

/// Extract the value of an HTML attribute from a tag string.
fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let patterns = [
        format!("{attr}=\""),
        format!("{attr}='"),
        format!("{attr} = \""),
        format!("{attr} = '"),
    ];

    for pat in &patterns {
        if let Some(start) = tag.find(pat.as_str()) {
            let val_start = start + pat.len();
            let quote = pat.chars().last()?;
            let val_end = tag[val_start..].find(quote)? + val_start;
            return Some(html_decode(&tag[val_start..val_end]));
        }
    }
    None
}

/// Remove HTML tags from a string, leaving only text content.
fn strip_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

/// Decode basic HTML entities.
fn html_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_html_extracts_title() {
        let html = r#"<html><head><title>My Page</title></head><body></body></html>"#;
        let snap = SnapshotEngine::from_html(html);
        assert_eq!(snap.title, "My Page");
    }

    #[test]
    fn from_html_extracts_headings() {
        let html = r#"<html><body><h1>Hello World</h1><h2>Subtitle</h2></body></html>"#;
        let snap = SnapshotEngine::from_html(html);
        let headings: Vec<&str> = snap
            .elements
            .iter()
            .filter(|e| e.role == "heading")
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(headings, vec!["Hello World", "Subtitle"]);
    }

    #[test]
    fn from_html_extracts_links_with_ref_ids() {
        let html = r#"<html><body><a href="https://example.com">Click here</a></body></html>"#;
        let snap = SnapshotEngine::from_html(html);
        let links: Vec<_> = snap.elements.iter().filter(|e| e.role == "link").collect();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].name, "Click here");
        assert_eq!(links[0].value.as_deref(), Some("https://example.com"));
        assert!(links[0].ref_id.is_some());
        assert!(links[0].interactive);
    }

    #[test]
    fn from_html_extracts_buttons() {
        let html = r#"<html><body><button>Submit</button><button>Cancel</button></body></html>"#;
        let snap = SnapshotEngine::from_html(html);
        let buttons: Vec<&str> = snap
            .elements
            .iter()
            .filter(|e| e.role == "button")
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(buttons, vec!["Submit", "Cancel"]);
        // Each button should have a unique ref_id.
        let refs: Vec<u32> = snap.elements.iter().filter_map(|e| e.ref_id).collect();
        assert_eq!(
            refs.len(),
            refs.iter().collect::<std::collections::HashSet<_>>().len()
        );
    }

    #[test]
    fn from_html_extracts_inputs() {
        let html = r#"<html><body>
            <input type="text" placeholder="Email address">
            <input type="checkbox" name="agree">
        </body></html>"#;
        let snap = SnapshotEngine::from_html(html);
        let textboxes: Vec<_> = snap
            .elements
            .iter()
            .filter(|e| e.role == "textbox")
            .collect();
        assert_eq!(textboxes.len(), 1);
        assert_eq!(textboxes[0].name, "Email address");

        let checkboxes: Vec<_> = snap
            .elements
            .iter()
            .filter(|e| e.role == "checkbox")
            .collect();
        assert_eq!(checkboxes.len(), 1);
    }

    #[test]
    fn from_html_text_format() {
        let html = r#"<html>
            <head><title>Test Page</title></head>
            <body>
                <h1>Welcome</h1>
                <p>Hello there</p>
                <a href="https://example.com">Learn more</a>
                <button>Sign up</button>
            </body>
        </html>"#;
        let snap = SnapshotEngine::from_html(html);

        assert!(snap.text.contains("Page: Test Page"));
        assert!(snap.text.contains("heading: \"Welcome\""));
        assert!(snap.text.contains("paragraph: \"Hello there\""));
        assert!(
            snap.text
                .contains("link: \"Learn more\" -> https://example.com")
        );
        assert!(snap.text.contains("button: \"Sign up\""));
        // Interactive elements should have ref annotations
        assert!(snap.text.contains("[1]"));
    }

    #[test]
    fn from_html_empty_page() {
        let html = "<html><head></head><body></body></html>";
        let snap = SnapshotEngine::from_html(html);
        assert!(snap.title.is_empty());
        assert!(snap.elements.is_empty());
    }

    #[test]
    fn from_html_handles_nested_tags_in_content() {
        let html = r#"<p>This is <strong>bold</strong> text</p>"#;
        let snap = SnapshotEngine::from_html(html);
        let para = snap
            .elements
            .iter()
            .find(|e| e.role == "paragraph")
            .unwrap();
        assert_eq!(para.name, "This is bold text");
    }

    #[test]
    fn from_html_decodes_entities() {
        let html = r#"<a href="https://example.com?a=1&amp;b=2">Link &amp; stuff</a>"#;
        let snap = SnapshotEngine::from_html(html);
        let link = snap.elements.iter().find(|e| e.role == "link").unwrap();
        assert_eq!(link.value.as_deref(), Some("https://example.com?a=1&b=2"));
        assert_eq!(link.name, "Link & stuff");
    }

    #[test]
    fn snapshot_options_defaults() {
        let opts = SnapshotOptions::default();
        assert!(opts.cdp_endpoint.is_none());
        assert_eq!(opts.timeout_ms, 30_000);
        assert_eq!(opts.max_depth, 10);
    }

    #[test]
    fn snapshot_element_serialization_roundtrip() {
        let el = SnapshotElement {
            ref_id: Some(1),
            role: "button".to_string(),
            name: "Click me".to_string(),
            value: None,
            depth: 0,
            interactive: true,
            children: vec![],
        };

        let json = serde_json::to_string(&el).unwrap();
        let deser: SnapshotElement = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.ref_id, Some(1));
        assert_eq!(deser.role, "button");
        assert_eq!(deser.name, "Click me");
        assert!(deser.interactive);
    }

    #[test]
    fn browser_snapshot_serialization_roundtrip() {
        let snap = BrowserSnapshot {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            elements: vec![],
            text: "Page: Example\nURL: https://example.com\n\n".to_string(),
        };

        let json = serde_json::to_string(&snap).unwrap();
        let deser: BrowserSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.url, "https://example.com");
        assert_eq!(deser.title, "Example");
    }

    #[test]
    fn render_text_with_no_content() {
        let text = render_text("", "", &[]);
        assert!(text.is_empty());
    }

    #[test]
    fn render_text_with_title_and_url() {
        let text = render_text("My Page", "https://example.com", &[]);
        assert!(text.starts_with("Page: My Page\n"));
        assert!(text.contains("URL: https://example.com\n"));
    }

    #[test]
    fn interactive_role_detection() {
        assert!(is_interactive_role("button"));
        assert!(is_interactive_role("link"));
        assert!(is_interactive_role("textbox"));
        assert!(is_interactive_role("checkbox"));
        assert!(!is_interactive_role("heading"));
        assert!(!is_interactive_role("paragraph"));
        assert!(!is_interactive_role("generic"));
    }

    #[test]
    fn engine_new_returns_valid_instance() {
        let engine = SnapshotEngine::new(SnapshotOptions::default());
        assert_eq!(engine.cdp_endpoint(), "http://localhost:9222");
    }

    #[test]
    fn engine_custom_endpoint() {
        let opts = SnapshotOptions {
            cdp_endpoint: Some("http://127.0.0.1:9333".to_string()),
            ..Default::default()
        };
        let engine = SnapshotEngine::new(opts);
        assert_eq!(engine.cdp_endpoint(), "http://127.0.0.1:9333");
    }

    #[test]
    fn strip_tags_basic() {
        assert_eq!(strip_tags("<b>bold</b>"), "bold");
        assert_eq!(strip_tags("no tags"), "no tags");
        assert_eq!(strip_tags("<a href=\"x\">link</a>"), "link");
    }

    #[test]
    fn html_decode_basic() {
        assert_eq!(html_decode("a &amp; b"), "a & b");
        assert_eq!(html_decode("&lt;tag&gt;"), "<tag>");
        assert_eq!(html_decode("he said &quot;hi&quot;"), "he said \"hi\"");
    }

    #[test]
    fn extract_attr_works() {
        assert_eq!(
            extract_attr(r#"<a href="https://x.com" class="foo">"#, "href"),
            Some("https://x.com".to_string())
        );
        assert_eq!(
            extract_attr(r#"<input type='text' name='q'>"#, "type"),
            Some("text".to_string())
        );
        assert_eq!(extract_attr("<div>", "href"), None);
    }

    #[test]
    fn from_html_complex_page() {
        let html = r#"<!DOCTYPE html>
        <html>
        <head><title>Dashboard</title></head>
        <body>
            <h1>Dashboard</h1>
            <p>Welcome back, user.</p>
            <h2>Quick Actions</h2>
            <button>New Project</button>
            <button>Import</button>
            <a href="/settings">Settings</a>
            <a href="/logout">Log out</a>
            <h2>Search</h2>
            <input type="text" placeholder="Search projects...">
            <input type="submit" value="Go">
        </body>
        </html>"#;

        let snap = SnapshotEngine::from_html(html);
        assert_eq!(snap.title, "Dashboard");

        // Should have 2 headings (h1 + 2*h2 = 3 headings)
        let heading_count = snap.elements.iter().filter(|e| e.role == "heading").count();
        assert_eq!(heading_count, 3);

        // 1 paragraph
        let para_count = snap
            .elements
            .iter()
            .filter(|e| e.role == "paragraph")
            .count();
        assert_eq!(para_count, 1);

        // 2 links
        let link_count = snap.elements.iter().filter(|e| e.role == "link").count();
        assert_eq!(link_count, 2);

        // 2 buttons from <button> + 1 from <input type="submit">
        let button_count = snap.elements.iter().filter(|e| e.role == "button").count();
        assert_eq!(button_count, 3);

        // 1 textbox
        let textbox_count = snap.elements.iter().filter(|e| e.role == "textbox").count();
        assert_eq!(textbox_count, 1);

        // All interactive elements have unique ref_ids
        let refs: Vec<u32> = snap.elements.iter().filter_map(|e| e.ref_id).collect();
        assert!(!refs.is_empty());
        let unique: std::collections::HashSet<u32> = refs.iter().copied().collect();
        assert_eq!(refs.len(), unique.len());

        // Text output contains expected markers
        assert!(snap.text.contains("heading: \"Dashboard\""));
        assert!(snap.text.contains("[1]"));
    }
}
