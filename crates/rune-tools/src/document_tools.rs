//! Document understanding tools: extract text and lightweight structure from PDF, DOCX, XLSX.

use std::io::Read;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use quick_xml::Reader;
use quick_xml::events::Event;
use tracing::instrument;
use zip::ZipArchive;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;
use rune_core::ToolCategory;

pub struct DocumentToolExecutor {
    workspace_root: PathBuf,
}

impl DocumentToolExecutor {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    fn resolve_path(&self, tool: &str, raw: &str) -> Result<PathBuf, ToolError> {
        let candidate = Path::new(raw);
        if candidate.is_absolute() {
            return Err(ToolError::InvalidArguments {
                tool: tool.to_string(),
                reason: "absolute paths are not allowed".into(),
            });
        }
        let workspace_root = self
            .workspace_root
            .canonicalize()
            .map_err(|e| ToolError::ExecutionFailed(format!("workspace root invalid: {e}")))?;
        let joined = workspace_root.join(candidate);
        let canonical = if joined.exists() {
            joined
                .canonicalize()
                .map_err(|e| ToolError::ExecutionFailed(format!("path resolution failed: {e}")))?
        } else {
            return Err(ToolError::InvalidArguments {
                tool: tool.to_string(),
                reason: "path does not exist".into(),
            });
        };
        if !canonical.starts_with(&workspace_root) {
            return Err(ToolError::InvalidArguments {
                tool: tool.to_string(),
                reason: "path escapes workspace boundary".into(),
            });
        }
        Ok(canonical)
    }

    fn required_str<'a>(call: &'a ToolCall, key: &str) -> Result<&'a str, ToolError> {
        call.arguments
            .get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: call.tool_name.clone(),
                reason: format!("missing required parameter: {key}"),
            })
    }

    fn optional_u64(call: &ToolCall, key: &str) -> Option<u64> {
        call.arguments.get(key).and_then(|v| v.as_u64())
    }

    fn page_window(call: &ToolCall) -> (usize, Option<usize>) {
        let from = Self::optional_u64(call, "from_page").unwrap_or(1) as usize;
        let to = Self::optional_u64(call, "to_page").map(|v| v as usize);
        (from.max(1), to)
    }

    async fn extract_pdf(&self, path: &Path) -> Result<String, ToolError> {
        pdf_extract::extract_text(path)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to extract PDF text: {e}")))
    }

    async fn extract_docx(&self, path: &Path) -> Result<String, ToolError> {
        let file = std::fs::File::open(path)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to open DOCX: {e}")))?;
        let mut archive = ZipArchive::new(file)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to read DOCX zip: {e}")))?;
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .map_err(|e| ToolError::ExecutionFailed(format!("missing word/document.xml: {e}")))?
            .read_to_string(&mut xml)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to read DOCX xml: {e}")))?;
        Ok(extract_text_from_wordprocessing_xml(&xml))
    }

    async fn extract_xlsx(&self, path: &Path) -> Result<String, ToolError> {
        let file = std::fs::File::open(path)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to open XLSX: {e}")))?;
        let mut archive = ZipArchive::new(file)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to read XLSX zip: {e}")))?;
        let shared = read_zip_entry(&mut archive, "xl/sharedStrings.xml").ok();
        let shared_strings = shared
            .as_deref()
            .map(parse_shared_strings)
            .unwrap_or_default();
        let mut sheets = Vec::new();
        for i in 1..=32 {
            let name = format!("xl/worksheets/sheet{i}.xml");
            match read_zip_entry(&mut archive, &name) {
                Ok(xml) => sheets.push(format!(
                    "# sheet{i}\n{}",
                    extract_sheet_text(&xml, &shared_strings)
                )),
                Err(_) if i == 1 => continue,
                Err(_) => break,
            }
        }
        if sheets.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "no worksheets found in XLSX".into(),
            ));
        }
        Ok(sheets.join("\n\n"))
    }
}

#[async_trait]
impl ToolExecutor for DocumentToolExecutor {
    #[instrument(skip(self, call), fields(tool = %call.tool_name))]
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let path_str = Self::required_str(&call, "path")?;
        let path = self.resolve_path(&call.tool_name, path_str)?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let (from_page, to_page) = Self::page_window(&call);
        let mut output = match ext.as_str() {
            "pdf" => self.extract_pdf(&path).await?,
            "docx" => self.extract_docx(&path).await?,
            "xlsx" => self.extract_xlsx(&path).await?,
            _ => {
                return Err(ToolError::InvalidArguments {
                    tool: call.tool_name.clone(),
                    reason: format!("unsupported document format: {ext}"),
                });
            }
        };
        if ext == "pdf" {
            output = slice_pdf_pages(&output, from_page, to_page);
        }
        Ok(ToolResult {
            tool_call_id: call.tool_call_id.clone(),
            output,
            is_error: false,
            tool_execution_id: None,
        })
    }
}

pub fn document_extract_tool_definition() -> crate::ToolDefinition {
    crate::ToolDefinition {
        name: "extract_document".into(),
        description: "Extract text from PDF, DOCX, and XLSX documents with lightweight structure preservation and optional PDF page ranges.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Workspace-relative path to the document" },
                "from_page": { "type": "integer", "description": "Start page for PDF extraction (1-indexed)" },
                "to_page": { "type": "integer", "description": "End page for PDF extraction (inclusive)" }
            },
            "required": ["path"]
        }),
        category: ToolCategory::FileRead,
        requires_approval: false,
    }
}

fn read_zip_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<String, ToolError> {
    let mut s = String::new();
    archive
        .by_name(name)
        .map_err(|e| ToolError::ExecutionFailed(format!("missing {name}: {e}")))?
        .read_to_string(&mut s)
        .map_err(|e| ToolError::ExecutionFailed(format!("failed reading {name}: {e}")))?;
    Ok(s)
}

fn extract_text_from_wordprocessing_xml(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = String::new();
    let mut in_paragraph = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"w:p" => {
                if !out.is_empty() {
                    out.push('\n');
                }
                in_paragraph = true;
            }
            Ok(Event::Text(t)) if in_paragraph => {
                if let Ok(text) = std::str::from_utf8(t.as_ref()) {
                    out.push_str(text);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }
    out
}

fn parse_shared_strings(xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"t" => in_text = true,
            Ok(Event::End(e)) if e.name().as_ref() == b"si" => {
                out.push(current.clone());
                current.clear();
            }
            Ok(Event::Text(t)) if in_text => {
                if let Ok(text) = std::str::from_utf8(t.as_ref()) {
                    current.push_str(text);
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"t" => in_text = false,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }
    out
}

fn extract_sheet_text(xml: &str, shared_strings: &[String]) -> String {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = String::new();
    let mut current_type: Option<String> = None;
    let mut in_value = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"c" => {
                current_type = e
                    .attributes()
                    .flatten()
                    .find(|a| a.key.as_ref() == b"t")
                    .and_then(|a| String::from_utf8(a.value.into_owned()).ok());
            }
            Ok(Event::Start(e)) if e.name().as_ref() == b"v" => in_value = true,
            Ok(Event::Text(t)) if in_value => {
                if let Ok(v) = std::str::from_utf8(t.as_ref()) {
                    let text = if current_type.as_deref() == Some("s") {
                        v.parse::<usize>()
                            .ok()
                            .and_then(|i| shared_strings.get(i).cloned())
                            .unwrap_or_else(|| v.to_string())
                    } else {
                        v.to_string()
                    };
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&text);
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"v" => in_value = false,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }
    out
}

fn slice_pdf_pages(text: &str, from_page: usize, to_page: Option<usize>) -> String {
    let pages: Vec<&str> = text.split("\x0C").collect();
    if pages.len() <= 1 {
        return text.to_string();
    }
    let start = from_page.saturating_sub(1).min(pages.len());
    let end = to_page.unwrap_or(pages.len()).min(pages.len());
    if start >= end {
        return String::new();
    }
    pages[start..end].join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_pdf_formfeed_pages() {
        let text = "page1\x0Cpage2\x0Cpage3";
        assert_eq!(slice_pdf_pages(text, 2, Some(3)), "page2");
    }

    #[test]
    fn parses_docx_xml_text() {
        let xml = r#"<w:document xmlns:w=\"w\"><w:body><w:p><w:r><w:t>Hello</w:t></w:r></w:p><w:p><w:r><w:t>World</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(extract_text_from_wordprocessing_xml(xml), "Hello\nWorld");
    }

    #[test]
    fn parses_shared_strings() {
        let xml = r#"<sst><si><t>Alpha</t></si><si><t>Beta</t></si></sst>"#;
        assert_eq!(parse_shared_strings(xml), vec!["Alpha", "Beta"]);
    }
}
