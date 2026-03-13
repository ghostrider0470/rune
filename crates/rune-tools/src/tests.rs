use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rune_core::{ToolCallId, ToolCategory};
use serde_json::Value;

use crate::definition::{ToolCall, ToolDefinition};
use crate::executor::{AlwaysAllow, ApprovalCheck, ToolExecutor};
use crate::registry::ToolRegistry;
use crate::{register_builtin_stubs, validate_arguments, BuiltinToolExecutor};

#[test]
fn registry_starts_empty() {
    let reg = ToolRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert!(reg.list().is_empty());
}

#[test]
fn registry_register_and_lookup() {
    let mut reg = ToolRegistry::new();
    reg.register(ToolDefinition {
        name: "test_tool".into(),
        description: "A test tool.".into(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::FileRead,
        requires_approval: false,
    });

    assert_eq!(reg.len(), 1);
    let tool = match reg.lookup("test_tool") {
        Ok(tool) => tool,
        Err(err) => panic!("lookup failed: {err}"),
    };
    assert_eq!(tool.name, "test_tool");
    assert!(!tool.requires_approval);
}

#[test]
fn registry_lookup_unknown_returns_error() {
    let reg = ToolRegistry::new();
    let err = match reg.lookup("nonexistent") {
        Ok(_) => panic!("unexpected tool found"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("nonexistent"));
}

#[test]
fn registry_list_returns_sorted() {
    let mut reg = ToolRegistry::new();
    for name in ["zebra", "alpha", "middle"] {
        reg.register(ToolDefinition {
            name: name.into(),
            description: String::new(),
            parameters: serde_json::json!({"type": "object"}),
            category: ToolCategory::FileRead,
            requires_approval: false,
        });
    }

    let names: Vec<_> = reg.list().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn register_builtin_tools_populates_expected_tools() {
    let mut reg = ToolRegistry::new();
    register_builtin_stubs(&mut reg);

    assert_eq!(reg.len(), 6);

    let expected = ["read", "write", "edit", "exec", "process", "list_files"];
    for name in expected {
        assert!(reg.lookup(name).is_ok(), "missing builtin: {name}");
    }

    assert!(match reg.lookup("exec") {
        Ok(tool) => tool.requires_approval,
        Err(_) => false,
    });
    assert!(!match reg.lookup("read") {
        Ok(tool) => tool.requires_approval,
        Err(_) => true,
    });
}

#[test]
fn validate_arguments_passes_with_required_fields() {
    let def = ToolDefinition {
        name: "test".into(),
        description: String::new(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        }),
        category: ToolCategory::FileRead,
        requires_approval: false,
    };

    let args = serde_json::json!({"path": "/tmp/test"});
    assert!(validate_arguments(&def, &args).is_ok());
}

#[test]
fn validate_arguments_fails_on_missing_required() {
    let def = ToolDefinition {
        name: "test".into(),
        description: String::new(),
        parameters: serde_json::json!({
            "type": "object",
            "required": ["path"]
        }),
        category: ToolCategory::FileRead,
        requires_approval: false,
    };

    let args = serde_json::json!({});
    let err = match validate_arguments(&def, &args) {
        Ok(()) => panic!("expected validation error"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("path"));
}

#[test]
fn validate_arguments_passes_when_no_required() {
    let def = ToolDefinition {
        name: "test".into(),
        description: String::new(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::FileRead,
        requires_approval: false,
    };

    assert!(validate_arguments(&def, &serde_json::json!({})).is_ok());
}

#[tokio::test]
async fn always_allow_approval_check_permits() {
    let checker = AlwaysAllow;
    let call = ToolCall {
        tool_call_id: ToolCallId::new(),
        tool_name: "exec".into(),
        arguments: serde_json::json!({"command": "printf ok"}),
    };

    assert!(checker.check(&call, true).await.is_ok());
    assert!(checker.check(&call, false).await.is_ok());
}

#[test]
fn tool_definition_roundtrips_through_serde() {
    let def = ToolDefinition {
        name: "read".into(),
        description: "Read a file.".into(),
        parameters: serde_json::json!({"type": "object", "required": ["path"]}),
        category: ToolCategory::FileRead,
        requires_approval: false,
    };

    let json = match serde_json::to_value(&def) {
        Ok(json) => json,
        Err(err) => panic!("serialization failed: {err}"),
    };
    assert_eq!(json["name"], "read");
    assert_eq!(json["category"], "file_read");

    let restored: ToolDefinition = match serde_json::from_value(json) {
        Ok(value) => value,
        Err(err) => panic!("deserialization failed: {err}"),
    };
    assert_eq!(restored.name, "read");
}

#[tokio::test]
async fn write_and_read_file_executor_work() {
    let exec = BuiltinToolExecutor::new();
    let dir = temp_dir();
    let path = dir.join("a/b/test.txt");

    let write_result = exec
        .execute(ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "write".into(),
            arguments: serde_json::json!({
                "path": path,
                "content": "line1\nline2\nline3"
            }),
        })
        .await;
    match write_result {
        Ok(result) => assert!(!result.is_error),
        Err(err) => panic!("write failed: {err}"),
    }

    let read_result = exec
        .execute(ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "read".into(),
            arguments: serde_json::json!({
                "path": dir.join("a/b/test.txt"),
                "offset": 2,
                "limit": 2
            }),
        })
        .await;

    let read_result = match read_result {
        Ok(result) => result,
        Err(err) => panic!("read failed: {err}"),
    };
    assert_eq!(read_result.output, "line2\nline3");
}

#[tokio::test]
async fn read_reports_binary_files() {
    let exec = BuiltinToolExecutor::new();
    let dir = temp_dir();
    let path = dir.join("bin.dat");
    let write_result = tokio::fs::write(&path, [0_u8, 159, 146, 150]).await;
    if let Err(err) = write_result {
        panic!("failed to seed binary file: {err}");
    }

    let result = exec
        .execute(ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "read".into(),
            arguments: serde_json::json!({ "path": path }),
        })
        .await;

    let result = match result {
        Ok(result) => result,
        Err(err) => panic!("read failed: {err}"),
    };
    assert!(result.output.contains("binary file"));
}

#[tokio::test]
async fn edit_replaces_exact_string_once() {
    let exec = BuiltinToolExecutor::new();
    let dir = temp_dir();
    let path = dir.join("edit.txt");
    let seed_result = tokio::fs::write(&path, "hello world\nhello world").await;
    if let Err(err) = seed_result {
        panic!("failed to seed file: {err}");
    }

    let edit_result = exec
        .execute(ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "edit".into(),
            arguments: serde_json::json!({
                "path": path,
                "oldText": "hello",
                "newText": "hi"
            }),
        })
        .await;
    if let Err(err) = edit_result {
        panic!("edit failed: {err}");
    }

    let content = match tokio::fs::read_to_string(dir.join("edit.txt")).await {
        Ok(content) => content,
        Err(err) => panic!("failed to read edited file: {err}"),
    };
    assert_eq!(content, "hi world\nhello world");
}

#[tokio::test]
async fn edit_fails_when_old_text_missing() {
    let exec = BuiltinToolExecutor::new();
    let dir = temp_dir();
    let path = dir.join("edit-miss.txt");
    let seed_result = tokio::fs::write(&path, "hello world").await;
    if let Err(err) = seed_result {
        panic!("failed to seed file: {err}");
    }

    let err = exec
        .execute(ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "edit".into(),
            arguments: serde_json::json!({
                "path": path,
                "oldText": "missing",
                "newText": "hi"
            }),
        })
        .await;

    match err {
        Ok(_) => panic!("expected edit failure"),
        Err(err) => assert!(err.to_string().contains("exact text not found")),
    }
}

#[tokio::test]
async fn list_files_filters_by_pattern() {
    let exec = BuiltinToolExecutor::new();
    let dir = temp_dir();
    let a = dir.join("a.rs");
    let b = dir.join("b.txt");
    let c = dir.join("c.rs");
    for path in [&a, &b, &c] {
        let result = tokio::fs::write(path, "x").await;
        if let Err(err) = result {
            panic!("failed to seed file {}: {err}", path.display());
        }
    }

    let result = exec
        .execute(ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "list_files".into(),
            arguments: serde_json::json!({
                "path": dir,
                "pattern": "*.rs"
            }),
        })
        .await;

    let result = match result {
        Ok(result) => result,
        Err(err) => panic!("list_files failed: {err}"),
    };
    assert!(result.output.contains("a.rs"));
    assert!(result.output.contains("c.rs"));
    assert!(!result.output.contains("b.txt"));
}

#[tokio::test]
async fn exec_runs_foreground_command() {
    let exec = BuiltinToolExecutor::new();
    let result = exec
        .execute(ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "exec".into(),
            arguments: serde_json::json!({
                "command": "printf hello",
                "env": { "RUNE_TOOLS_TEST": "1" }
            }),
        })
        .await;

    let result = match result {
        Ok(result) => result,
        Err(err) => panic!("exec failed: {err}"),
    };
    let payload = parse_json(&result.output);
    assert_eq!(payload["stdout"], Value::String("hello".to_string()));
    assert_eq!(payload["status"], Value::Number(0.into()));
}

#[tokio::test]
async fn exec_background_and_process_lifecycle_work() {
    let exec = BuiltinToolExecutor::with_approval_checker(Arc::new(AlwaysAllow));
    let start = exec
        .execute(ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "exec".into(),
            arguments: serde_json::json!({
                "command": "printf start; sleep 1; printf end",
                "background": true
            }),
        })
        .await;

    let start = match start {
        Ok(result) => result,
        Err(err) => panic!("background exec failed: {err}"),
    };
    let start_payload = parse_json(&start.output);
    let process_id = match start_payload["processId"].as_str() {
        Some(id) => id.to_string(),
        None => panic!("missing processId in payload: {}", start.output),
    };

    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;

    let log = exec
        .execute(ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "process".into(),
            arguments: serde_json::json!({
                "action": "log",
                "processId": process_id
            }),
        })
        .await;
    let log = match log {
        Ok(result) => result,
        Err(err) => panic!("process log failed: {err}"),
    };
    assert!(log.output.contains("start"));
    assert!(log.output.contains("end"));
}

fn parse_json(value: &str) -> Value {
    match serde_json::from_str(value) {
        Ok(value) => value,
        Err(err) => panic!("invalid json output: {err}; raw={value}"),
    }
}

fn temp_dir() -> std::path::PathBuf {
    let nanos = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos(),
        Err(err) => panic!("system time error: {err}"),
    };
    let dir = std::env::temp_dir().join(format!("rune-tools-test-{nanos}"));
    match std::fs::create_dir_all(&dir) {
        Ok(()) => dir,
        Err(err) => panic!("failed to create temp dir {}: {err}", dir.display()),
    }
}
