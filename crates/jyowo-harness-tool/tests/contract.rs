#![cfg(feature = "builtin-toolset")]

use std::time::Duration;

use harness_contracts::BudgetMetric;
use harness_tool::{BuiltinToolset, ToolJournalAuthority, ToolRegistry};
use serde_json::Value;

#[test]
fn default_builtin_tools_declare_nonzero_result_budgets() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    for tool in snapshot.iter_sorted().map(|(_, tool)| tool) {
        assert!(
            tool.descriptor().budget.limit > 0,
            "{} should declare a nonzero result budget",
            tool.descriptor().name
        );
    }
}

#[test]
fn default_builtin_tools_declare_tool_specific_result_budgets() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    let file_read = snapshot
        .get("FileRead")
        .unwrap()
        .descriptor()
        .budget
        .clone();
    let grep = snapshot.get("Grep").unwrap().descriptor().budget.clone();
    let process_read = snapshot
        .get("ProcessRead")
        .unwrap()
        .descriptor()
        .budget
        .clone();
    let web_fetch = snapshot
        .get("WebFetch")
        .unwrap()
        .descriptor()
        .budget
        .clone();

    assert_eq!(file_read.metric, BudgetMetric::Chars);
    assert_eq!(grep.metric, BudgetMetric::Lines);
    assert_eq!(process_read.metric, BudgetMetric::Bytes);
    assert_eq!(web_fetch.metric, BudgetMetric::Bytes);
    assert_ne!(file_read.limit, grep.limit);
    assert_ne!(file_read.limit, web_fetch.limit);
    assert_ne!(process_read.limit, web_fetch.limit);
}

#[test]
#[cfg(all(
    not(feature = "programmatic-tool-calling"),
    not(feature = "minimax-tools"),
    not(feature = "seedance-tools")
))]
fn default_builtin_toolset_name_snapshot_is_stable() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    let names = snapshot
        .iter_sorted()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();

    let mut expected = vec![
        "AskUserQuestion",
        "Artifact",
        "Automation",
        "Bash",
        "BrowserDevTools",
        "BrowserUse",
        "ComputerUse",
        "Diagnostics",
        "FileEdit",
        "FileRead",
        "FileWrite",
        "GitBranch",
        "GitCommit",
        "GitDiff",
        "GitLog",
        "GitPull",
        "GitPush",
        "GitShow",
        "GitStage",
        "GitStatus",
        "Glob",
        "Grep",
        "ImageGeneration",
        "LSP",
        "ListDir",
        "NotebookEdit",
        "ProcessRead",
        "ProcessStart",
        "ProcessStop",
        "ReadBlob",
        "SendMessage",
        "Session",
        "TaskStop",
        "Todo",
        "WebFetch",
        "WebSearch",
        "Workflow",
        "Worktree",
    ];
    #[cfg(feature = "zhipu-tools")]
    expected.extend([
        "ZhipuImageGeneration",
        "ZhipuImageGenerationAsync",
        "ZhipuImageGenerationQuery",
        "ZhipuSpeechToText",
        "ZhipuTextToSpeech",
        "ZhipuVideoGeneration",
        "ZhipuVideoGenerationQuery",
    ]);
    expected.extend([
        "memory",
        "skills_invoke",
        "skills_list",
        "skills_run_script",
        "skills_view",
    ]);

    assert_eq!(names, expected);
}

#[test]
fn default_builtin_toolset_journal_authority_snapshot_is_stable() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    for (name, _) in snapshot.iter_sorted() {
        let expected = match name.as_str() {
            "Bash" | "Diagnostics" | "ProcessStart" => ToolJournalAuthority::Sandbox,
            "execute_code" => ToolJournalAuthority::ExecuteCode,
            _ => ToolJournalAuthority::None,
        };
        assert_eq!(
            snapshot.journal_authority(name),
            expected,
            "{name} journal authority should stay stable"
        );
    }
}

#[test]
fn shell_and_process_tools_declare_long_running_policies() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    let expected = [
        ("Bash", Duration::from_secs(5), Duration::from_secs(600)),
        (
            "ProcessStart",
            Duration::from_secs(5),
            Duration::from_secs(120),
        ),
        (
            "ProcessRead",
            Duration::from_secs(5),
            Duration::from_secs(30),
        ),
        (
            "ProcessStop",
            Duration::from_secs(5),
            Duration::from_secs(30),
        ),
    ];

    for (name, stall_threshold, hard_timeout) in expected {
        let policy = snapshot
            .get(name)
            .unwrap()
            .descriptor()
            .properties
            .long_running
            .as_ref()
            .unwrap_or_else(|| panic!("{name} should declare a long-running policy"));
        assert_eq!(policy.stall_threshold, stall_threshold, "{name} stall");
        assert_eq!(policy.hard_timeout, hard_timeout, "{name} timeout");
    }
}

#[test]
#[cfg(feature = "programmatic-tool-calling")]
fn execute_code_declares_long_running_policy() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();
    let snapshot = registry.snapshot();
    let policy = snapshot
        .get("execute_code")
        .unwrap()
        .descriptor()
        .properties
        .long_running
        .as_ref()
        .expect("execute_code should declare a long-running policy");

    assert_eq!(policy.stall_threshold, Duration::from_secs(2));
    assert_eq!(policy.hard_timeout, Duration::from_secs(60));
}

#[test]
fn default_builtin_toolset_descriptors_have_baseline_contract_fields() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    for (name, tool) in snapshot.iter_sorted() {
        let descriptor = tool.descriptor();
        assert_eq!(&descriptor.name, name);
        assert!(
            !descriptor.display_name.trim().is_empty(),
            "{name} display name"
        );
        assert!(
            !descriptor.description.trim().is_empty(),
            "{name} description"
        );
        assert!(!descriptor.category.trim().is_empty(), "{name} category");
        assert!(!descriptor.version.trim().is_empty(), "{name} version");
        assert_eq!(
            descriptor
                .input_schema
                .get("type")
                .and_then(|value| value.as_str()),
            Some("object"),
            "{name} input schema should be an object"
        );
    }
}

#[test]
fn default_builtin_toolset_input_schemas_reject_unknown_fields() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    for (name, tool) in snapshot.iter_sorted() {
        assert_eq!(
            tool.descriptor()
                .input_schema
                .get("additionalProperties")
                .and_then(Value::as_bool),
            Some(false),
            "{name} input schema should set additionalProperties=false"
        );
    }
}

#[test]
fn default_builtin_toolset_descriptors_are_searchable() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    for (name, tool) in snapshot.iter_sorted() {
        assert!(
            tool.descriptor()
                .search_hint
                .as_deref()
                .is_some_and(|hint| !hint.trim().is_empty()),
            "{name} should declare a search hint"
        );
    }
}

#[test]
fn base_builtin_toolset_descriptors_declare_output_schemas() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    for (name, tool) in snapshot.iter_sorted() {
        if name.starts_with("MiniMax") || name.starts_with("Seedance") {
            continue;
        }
        assert!(
            tool.descriptor().output_schema.is_some(),
            "{name} should declare an output schema"
        );
    }
}

#[test]
#[cfg(feature = "programmatic-tool-calling")]
fn programmatic_tool_calling_toolset_name_snapshot_is_stable() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    let names = snapshot
        .iter_sorted()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();

    assert!(names.contains(&"execute_code"));
}

#[test]
fn tool_gap_analysis_doc_mentions_current_runtime_and_builtin_tools() {
    let doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/tools/tool-module-gap-analysis.md"
    ))
    .expect("tool gap analysis document should be readable");

    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    for (name, _) in snapshot.iter_sorted() {
        assert!(
            doc.contains(name.as_str()),
            "tool gap analysis doc should mention builtin tool {name}"
        );
    }
    for runtime_tool in [
        "tool_search",
        "background_agent",
        "agent_team",
        "agent",
        "dispatch",
        "message",
        "pause_worker",
        "resume_worker",
        "spawn_worker",
        "stop_team",
        "team_status",
    ] {
        assert!(
            doc.contains(runtime_tool),
            "tool gap analysis doc should mention runtime tool {runtime_tool}"
        );
    }
}

#[test]
fn tool_crate_stays_inside_allowed_dependency_boundary() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();

    #[cfg(not(feature = "minimax-tools"))]
    assert!(!manifest.lines().any(|line| {
        line.trim_start().starts_with("jyowo-harness-model =") && !line.contains("optional = true")
    }));
    assert!(!manifest.contains("jyowo-harness-journal"));
    assert!(!manifest.contains("jyowo-harness-hook"));
    #[cfg(not(feature = "minimax-tools"))]
    assert!(!manifest.contains("reqwest = { workspace = true }"));
}
