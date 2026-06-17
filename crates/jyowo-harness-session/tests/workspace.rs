use harness_contracts::TenantId;
use harness_session::{BootstrapFileSpec, SessionOptions, WorkspaceRegistry, WorkspaceSpec};

#[test]
fn workspace_registry_creates_lists_and_gets_by_tenant() {
    let registry = WorkspaceRegistry::new();
    let root = std::env::temp_dir().join(format!(
        "jyowo-session-workspace-{}",
        harness_contracts::WorkspaceId::new()
    ));

    let workspace = registry.create(
        WorkspaceSpec::new(&root, "Workspace")
            .with_bootstrap_files(vec![BootstrapFileSpec::required("AGENTS.md")])
            .with_default_session_options(SessionOptions::default().with_model_id("workspace")),
    );

    assert_eq!(workspace.root_path, root);
    assert_eq!(workspace.display_name, "Workspace");
    assert_eq!(
        registry
            .get(workspace.id)
            .expect("workspace should be stored")
            .bootstrap_files,
        vec![BootstrapFileSpec::required("AGENTS.md")]
    );
    assert_eq!(registry.list(TenantId::SINGLE), vec![workspace]);
}

#[test]
fn workspace_registry_filters_by_tenant() {
    let registry = WorkspaceRegistry::new();
    let tenant = TenantId::new();
    let root = std::env::temp_dir().join(format!(
        "jyowo-session-workspace-{}",
        harness_contracts::WorkspaceId::new()
    ));

    let workspace =
        registry.create(WorkspaceSpec::new(&root, "Tenant Workspace").with_tenant_id(tenant));

    assert!(registry.list(TenantId::SINGLE).is_empty());
    assert_eq!(registry.list(tenant), vec![workspace]);
}
