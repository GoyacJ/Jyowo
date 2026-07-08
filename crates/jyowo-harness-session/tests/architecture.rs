#[test]
fn session_crate_does_not_own_runtime_execution_dependencies() {
    let manifest = include_str!("../Cargo.toml");
    for dependency in [
        "jyowo-harness-execution",
        "jyowo-harness-hook",
        "jyowo-harness-mcp",
        "jyowo-harness-model",
        "jyowo-harness-permission",
        "jyowo-harness-sandbox",
        "jyowo-harness-tool",
    ] {
        assert!(
            !manifest.contains(dependency),
            "session crate must not depend on runtime crate `{dependency}`"
        );
    }
}

#[test]
fn session_crate_does_not_export_turn_runtime() {
    let lib = include_str!("../src/lib.rs");
    assert!(
        !lib.contains("pub mod turn") && !lib.contains("pub use turn::*"),
        "session crate must not export the turn runtime"
    );
}
