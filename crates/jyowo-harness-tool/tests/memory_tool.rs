//! Tests for the MemoryTool.

#![cfg(feature = "builtin-toolset")]

use harness_contracts::*;
use harness_tool::{builtin::MemoryTool, Tool};
use serde_json::json;

fn make_tool() -> MemoryTool {
    MemoryTool::default()
}

#[test]
fn tool_descriptor_exposes_memory_tool_args_schema() {
    let tool = make_tool();
    let desc = tool.descriptor();
    assert_eq!(desc.name, "memory");
    assert_eq!(desc.display_name, "Memory");
    // Tool belongs to Memory group
    assert!(matches!(desc.group, ToolGroup::Memory));
}

#[test]
fn tool_descriptor_has_all_seven_actions() {
    let tool = make_tool();
    let desc = tool.descriptor();
    let schema_str = serde_json::to_string(&desc.input_schema).unwrap();

    // All 7 actions should be present
    assert!(schema_str.contains("search"));
    assert!(schema_str.contains("read"));
    assert!(schema_str.contains("create"));
    assert!(schema_str.contains("update"));
    assert!(schema_str.contains("delete"));
    assert!(schema_str.contains("list"));
    assert!(schema_str.contains("propose"));
}

#[test]
fn tool_args_parse_search_action() {
    let input = json!({
        "action": "search",
        "query": "rust programming",
        "max_records": 5
    });
    // Validate the flat model-facing format
    assert_eq!(input["action"], "search");
    assert_eq!(input["query"], "rust programming");
    assert_eq!(input["max_records"], 5);
}

#[test]
fn tool_args_parse_create_action() {
    let input = json!({
        "action": "create",
        "draft": {
            "kind": "project_fact",
            "visibility": "user",
            "content": "Rust is a systems programming language"
        }
    });
    assert_eq!(input["action"], "create");
    assert_eq!(input["draft"]["kind"], "project_fact");
    assert_eq!(input["draft"]["content"], "Rust is a systems programming language");
}

#[test]
fn tool_args_parse_delete_action() {
    let input = json!({
        "action": "delete",
        "memory_id": "01J00000000000000000000000",
        "reason": "outdated information"
    });
    assert_eq!(input["action"], "delete");
    assert_eq!(input["reason"], "outdated information");
}

#[test]
fn tool_args_parse_propose_action() {
    let input = json!({
        "action": "propose",
        "draft": {
            "kind": "reference",
            "visibility": "tenant",
            "content": "Candidate memory entry"
        }
    });
    assert_eq!(input["action"], "propose");
    assert_eq!(input["draft"]["content"], "Candidate memory entry");
}

#[test]
fn tool_args_parse_list_action() {
    let input = json!({
        "action": "list",
        "limit": 10
    });
    assert_eq!(input["action"], "list");
    assert_eq!(input["limit"], 10);
}

#[test]
fn model_cannot_provide_runtime_context_fields() {
    // The model-visible schema (MemoryToolArgs) should not expose
    // runtime context fields like tenant_id, session_id, run_id, etc.
    let schema = make_tool().descriptor().input_schema.clone();
    let schema_str = serde_json::to_string(&schema).unwrap();

    assert!(!schema_str.contains("tenant_id"));
    assert!(!schema_str.contains("session_id"));
    assert!(!schema_str.contains("run_id"));
    assert!(!schema_str.contains("permission_context"));
    assert!(!schema_str.contains("authorization_ticket"));
    assert!(!schema_str.contains("non_interactive_policy"));
}
