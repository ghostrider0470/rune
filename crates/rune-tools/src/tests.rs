use rune_core::{ToolCallId, ToolCategory};

use crate::approval::PolicyBasedApproval;
use crate::circuit_breaker::CircuitBreakerRegistry;
use crate::definition::{ToolCall, ToolDefinition};
use crate::executor::{AlwaysAllow, ApprovalCheck, ToolExecutor};
use crate::registry::ToolRegistry;
use crate::stubs::{StubExecutor, register_builtin_stubs, validate_arguments};

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
    let tool = reg.lookup("test_tool").unwrap();
    assert_eq!(tool.name, "test_tool");
    assert!(!tool.requires_approval);
}

#[test]
fn registry_lookup_unknown_returns_error() {
    let reg = ToolRegistry::new();
    let err = reg.lookup("nonexistent").unwrap_err();
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
fn registry_contains_unregister_and_register_many_work() {
    let mut reg = ToolRegistry::new();
    reg.register_many([
        ToolDefinition {
            name: "beta".into(),
            description: "beta tool".into(),
            parameters: serde_json::json!({"type": "object"}),
            category: ToolCategory::FileRead,
            requires_approval: false,
        },
        ToolDefinition {
            name: "alpha".into(),
            description: "alpha tool".into(),
            parameters: serde_json::json!({"type": "object"}),
            category: ToolCategory::FileWrite,
            requires_approval: true,
        },
    ]);

    assert_eq!(reg.len(), 2);
    assert!(reg.contains("alpha"));
    assert!(reg.contains("beta"));
    assert!(!reg.contains("gamma"));

    let removed = reg.unregister("alpha").expect("alpha should exist");
    assert_eq!(removed.name, "alpha");
    assert!(removed.requires_approval);
    assert!(!reg.contains("alpha"));
    assert_eq!(reg.len(), 1);

    assert!(reg.unregister("gamma").is_none());
}

#[test]
fn register_builtin_stubs_populates_expected_tools() {
    let mut reg = ToolRegistry::new();
    register_builtin_stubs(&mut reg);

    assert_eq!(reg.len(), 19);

    let expected = [
        "read_file",
        "extract_document",
        "write_file",
        "edit_file",
        "list_files",
        "search_files",
        "execute_command",
        "list_sessions",
        "get_session_status",
        "web_fetch",
        "git",
        "image_generation",
        "context_budget",
        "context_checkpoint",
        "context_gc",
        "comms_send",
        "comms_read",
        "memory_bank_list",
        "memory_bank_get",
    ];
    for name in expected {
        assert!(reg.lookup(name).is_ok(), "missing builtin: {name}");
    }

    // execute_command should require approval
    assert!(reg.lookup("execute_command").unwrap().requires_approval);
    // read_file should not
    assert!(!reg.lookup("read_file").unwrap().requires_approval);
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
    let err = validate_arguments(&def, &args).unwrap_err();
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
async fn stub_executor_returns_output() {
    let exec = StubExecutor;
    let call = ToolCall {
        tool_call_id: ToolCallId::new(),
        tool_name: "read_file".into(),
        arguments: serde_json::json!({"path": "/tmp/x"}),
    };

    let result = exec.execute(call).await.unwrap();
    assert!(!result.is_error);
    assert!(result.output.contains("read_file"));
    assert!(result.output.contains("/tmp/x"));
}

#[tokio::test]
async fn always_allow_approval_check_permits() {
    let checker = AlwaysAllow;
    let call = ToolCall {
        tool_call_id: ToolCallId::new(),
        tool_name: "execute_command".into(),
        arguments: serde_json::json!({"command": "ls"}),
    };

    assert!(checker.check(&call, true).await.is_ok());
    assert!(checker.check(&call, false).await.is_ok());
}

#[tokio::test]
async fn policy_approval_error_contains_details_payload() {
    let checker = PolicyBasedApproval::new(std::collections::HashSet::new());
    let call = ToolCall {
        tool_call_id: ToolCallId::new(),
        tool_name: "exec".into(),
        arguments: serde_json::json!({"command": "echo hi", "workdir": "/tmp"}),
    };

    match checker.check(&call, true).await {
        Err(crate::ToolError::ApprovalRequired { tool, details }) => {
            assert_eq!(tool, "exec");
            assert!(details.contains("echo hi"));
            assert!(details.contains("medium") || details.contains("Medium"));
        }
        other => panic!("expected approval-required error, got {other:?}"),
    }
}

#[test]
fn tool_definition_roundtrips_through_serde() {
    let def = ToolDefinition {
        name: "read_file".into(),
        description: "Read a file.".into(),
        parameters: serde_json::json!({"type": "object", "required": ["path"]}),
        category: ToolCategory::FileRead,
        requires_approval: false,
    };

    let json = serde_json::to_value(&def).unwrap();
    assert_eq!(json["name"], "read_file");
    assert_eq!(json["category"], "file_read");

    let restored: ToolDefinition = serde_json::from_value(json).unwrap();
    assert_eq!(restored.name, "read_file");
}

#[test]
fn context_budget_tool_definitions_are_registered() {
    let mut reg = ToolRegistry::new();
    register_builtin_stubs(&mut reg);

    assert!(reg.lookup("context_budget").is_ok());
    assert!(reg.lookup("context_checkpoint").is_ok());
    assert!(reg.lookup("context_gc").is_ok());
}

#[test]
fn memory_bank_tool_definitions_are_registered() {
    let mut reg = ToolRegistry::new();
    register_builtin_stubs(&mut reg);

    assert!(reg.lookup("memory_bank_list").is_ok());
    assert!(reg.lookup("memory_bank_get").is_ok());
}

#[test]
fn circuit_breaker_opens_and_resets_after_non_retriable_failure() {
    let registry = CircuitBreakerRegistry::new(2, std::time::Duration::from_secs(60));

    assert!(registry.allow("exec").is_ok());
    assert_eq!(registry.record_retriable_failure("exec"), None);
    assert_eq!(registry.record_retriable_failure("exec"), Some(2));

    let snap = registry.snapshot("exec").expect("snapshot should exist");
    assert_eq!(snap.failures, 2);
    assert!(snap.is_open);
    assert!(registry.allow("exec").is_err());

    registry.record_non_retriable_failure("exec");
    let snap = registry.snapshot("exec").expect("snapshot should exist");
    assert_eq!(snap.failures, 0);
    assert!(!snap.is_open);
    assert!(registry.allow("exec").is_ok());
}

#[test]
fn tool_circuit_open_error_formats_tool_name() {
    let err = crate::ToolError::CircuitOpen {
        tool: "exec".into(),
        message: "cooldown 30s remaining".into(),
    };

    let rendered = err.to_string();
    assert!(rendered.contains("exec"));
    assert!(rendered.contains("cooldown 30s remaining"));
}
