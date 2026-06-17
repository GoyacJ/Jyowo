use harness_contracts::{SessionId, TenantId};
use harness_session::{SessionOptions, SessionPaths};

#[test]
fn session_paths_are_derived_from_workspace_root() {
    let root = std::env::temp_dir().join("jyowo-session-paths-test");
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();

    let paths = SessionPaths::from_workspace(&root, tenant, session);

    assert_eq!(paths.workspace_root, root);
    assert!(paths
        .events
        .ends_with(format!("runtime/events/{tenant}/{session}.jsonl")));
    assert!(paths.blobs.ends_with("data/blobs"));
    assert!(paths.db.ends_with("data/main.db"));
    assert!(paths.memdir.ends_with("data/memdir"));
    assert!(paths.runtime_sessions.ends_with("runtime/sessions"));
}

#[test]
fn session_options_require_explicit_workspace_root() {
    let options = SessionOptions::new(std::path::PathBuf::from("/tmp/jyowo-explicit-root"));

    assert_eq!(
        options.workspace_root,
        std::path::PathBuf::from("/tmp/jyowo-explicit-root")
    );
    assert_eq!(options.tenant_id, TenantId::SINGLE);
}
