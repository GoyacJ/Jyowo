#![cfg(feature = "builtin-toolset")]

use harness_tool::{BuiltinToolset, ToolRegistry};

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
