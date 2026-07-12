use std::{fs, path::Path};

use harness_contracts::{
    AgentProfile, AgentProfileContextMode, AgentProfileMemoryScope, AgentProfileSandboxInheritance,
    AgentProfileScope, AgentWorkspaceIsolationMode, CapabilityRouteKind, ExecutionDefaultsRecord,
    ExecutionOverridesRecord, ModelProtocol, PermissionMode, PluginSelectionRecord,
    ProviderCapabilityRoute, ProviderCapabilityRouteSettings,
    ProviderProfileConversationCapability, ProviderProfileDefinition,
    ProviderProfileModelDescriptor, ProviderProfileModelLifecycle, ProviderSecretEntry,
    ProviderSecretsRecord, ProviderSelectionRecord, SkillSelectionRecord, ToolProfile, TrustLevel,
};
use harness_daemon::{RuntimeConfigError, RuntimeConfigResolver};
use harness_plugin::{PluginCapabilities, PluginLifecycleState, PluginManifest, PluginName};
use serde::Serialize;
use tempfile::TempDir;

#[test]
fn project_settings_merge_over_global_runtime_configuration() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global(
        "execution-defaults.json",
        &ExecutionDefaultsRecord {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.7,
            subagents_enabled: true,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
    );
    fixture.write_project(
        "provider-selection.json",
        &ProviderSelectionRecord {
            default_config_id: Some("project-model".into()),
        },
    );
    fixture.write_project(
        "execution-overrides.json",
        &ExecutionOverridesRecord {
            permission_mode: Some(PermissionMode::Plan),
            context_compression_trigger_ratio: Some(0.9),
            ..ExecutionOverridesRecord::default()
        },
    );
    fixture.write_global_raw(
        "mcp-servers.json",
        r#"[
          {"enabled":true,"displayName":"global one","id":"one","scope":"global","transport":{"kind":"stdio","command":"one","args":[]}},
          {"enabled":true,"displayName":"old shared","id":"shared","scope":"global","transport":{"kind":"stdio","command":"old","args":[]}}
        ]"#,
    );
    fixture.write_project_raw(
        "mcp-servers.json",
        r#"[
          {"enabled":false,"displayName":"project shared","id":"shared","scope":"session","transport":{"kind":"stdio","command":"new","args":[]}},
          {"enabled":true,"displayName":"project two","id":"two","scope":"session","transport":{"kind":"stdio","command":"two","args":[]}}
        ]"#,
    );

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("resolve merged snapshot");

    assert_eq!(snapshot.provider.config_id, "project-model");
    assert_eq!(
        snapshot.execution_defaults.permission_mode,
        PermissionMode::Plan
    );
    assert_eq!(snapshot.execution_defaults.tool_profile, ToolProfile::Full);
    assert_eq!(
        snapshot
            .execution_defaults
            .context_compression_trigger_ratio,
        0.9
    );
    assert_eq!(
        snapshot
            .mcp_servers
            .iter()
            .map(|server| (server.id.as_str(), server.enabled))
            .collect::<Vec<_>>(),
        vec![("one", true), ("shared", false), ("two", true)]
    );
}

#[test]
fn project_routes_override_by_operation_and_inherit_other_global_operations() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global(
        "provider-capability-routes.json",
        &routes(&[
            (
                "global",
                "provider-global",
                &["image.generate", "image.edit"],
            ),
            ("global", "provider-global", &["video.generate"]),
        ]),
    );
    fixture.write_project(
        "provider-capability-routes.json",
        &routes(&[("project-model", "provider-project", &["image.generate"])]),
    );

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), Some("global-model"))
        .expect("resolve routes");
    let route_for = |operation: &str| {
        snapshot
            .provider_routes
            .routes
            .iter()
            .find(|route| route.operation_ids.iter().any(|item| item == operation))
            .expect("operation route")
    };

    assert_eq!(route_for("image.generate").config_id, "project-model");
    assert_eq!(route_for("image.edit").config_id, "global");
    assert_eq!(route_for("video.generate").config_id, "global");
}

#[test]
fn project_skill_and_plugin_selections_disable_unselected_global_packages() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global(
        "skills.json",
        &SkillSelectionRecord {
            enabled: vec!["global-on".into(), "global-off".into()],
        },
    );
    fixture.write_project(
        "skills.json",
        &SkillSelectionRecord {
            enabled: vec!["global-on".into(), "project-on".into()],
        },
    );
    fixture.write_project(
        "plugins.json",
        &PluginSelectionRecord {
            allow_project_plugins: true,
            enabled: vec!["global-on".into(), "project-on".into()],
        },
    );

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("resolve selections");

    assert_eq!(
        snapshot.enabled_skill_ids.into_iter().collect::<Vec<_>>(),
        vec!["global-on", "project-on"]
    );
    assert_eq!(
        snapshot.enabled_plugin_ids.into_iter().collect::<Vec<_>>(),
        vec!["global-on", "project-on"]
    );
    assert!(snapshot.allow_project_plugins);
}

#[test]
fn project_agent_profile_selection_resolves_global_definition_and_daemon_private_memory() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global("agent-profiles.json", &[agent_profile("custom-reviewer")]);
    fixture.write_project_raw(
        "agent-profile-selection.json",
        r#"{"defaultProfileId":"custom-reviewer"}"#,
    );

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("resolve agent profile");

    assert_eq!(
        snapshot.default_agent_profile_id.as_deref(),
        Some("custom-reviewer")
    );
    assert!(snapshot
        .agent_profiles
        .iter()
        .any(|profile| profile.id == "custom-reviewer"));
    let workspace = fixture
        .workspace()
        .canonicalize()
        .expect("canonical workspace");
    let workspace_key = blake3::hash(workspace.as_os_str().as_encoded_bytes())
        .to_hex()
        .to_string();
    assert_eq!(
        snapshot.memory_database_path,
        fixture
            .home
            .canonicalize()
            .expect("canonical Jyowo home")
            .join("runtime/workspaces")
            .join(workspace_key)
            .join("memory/memory.sqlite3")
    );
    assert!(!snapshot.memory_database_path.starts_with(&workspace));
}

#[test]
fn canonical_workspace_memory_path_is_stable_and_workspace_scoped() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    let second_workspace = fixture.root.path().join("second-workspace");
    fs::create_dir_all(second_workspace.join(".jyowo/config")).expect("second workspace");
    let resolver = RuntimeConfigResolver::new(fixture.config_root());

    let first = resolver
        .resolve(fixture.workspace(), None)
        .expect("first workspace snapshot");
    let same = resolver
        .resolve(&fixture.workspace.join("."), None)
        .expect("same canonical workspace snapshot");
    let second = resolver
        .resolve(&second_workspace, None)
        .expect("second workspace snapshot");

    assert_eq!(first.memory_database_path, same.memory_database_path);
    assert_ne!(first.memory_database_path, second.memory_database_path);
}

#[test]
fn runtime_without_workspace_uses_daemon_global_memory_path() {
    let fixture = RuntimeFixture::new();
    let resolver = RuntimeConfigResolver::new(fixture.config_root());

    let path = resolver
        .resolve_memory_database_path(None)
        .expect("global memory path");

    assert_eq!(
        path,
        fixture
            .home
            .canonicalize()
            .expect("canonical Jyowo home")
            .join("runtime/memory/memory.sqlite3")
    );
}

#[test]
fn malformed_project_configuration_fails_closed_without_secret_leakage() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_project_raw(
        "mcp-servers.json",
        r#"[{"enabled":true,"id":"broken","displayName":"secret-value","scope":"project","transport":{"kind":"stdio","command":17}}]"#,
    );

    let error = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("malformed project config must fail");
    let message = error.to_string();

    assert!(matches!(error, RuntimeConfigError::Decode { .. }));
    assert!(!message.contains("secret-value"));
}

#[test]
fn empty_project_provider_selection_is_rejected_instead_of_inheriting_global() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_project(
        "provider-selection.json",
        &ProviderSelectionRecord {
            default_config_id: Some("   ".into()),
        },
    );

    let error = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("present invalid provider selection must fail closed");

    assert!(matches!(error, RuntimeConfigError::Invalid { .. }));
    assert!(error.to_string().contains("project provider selection"));
}

#[test]
fn missing_project_provider_selection_id_is_rejected() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_project(
        "provider-selection.json",
        &ProviderSelectionRecord {
            default_config_id: None,
        },
    );

    let error = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("missing project provider id must fail closed");

    assert!(matches!(error, RuntimeConfigError::Invalid { .. }));
}

#[test]
fn empty_project_agent_profile_selection_is_rejected_instead_of_inheriting_default() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_project_raw(
        "agent-profile-selection.json",
        r#"{"defaultProfileId":"   "}"#,
    );

    let error = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("present invalid agent profile selection must fail closed");

    assert!(matches!(error, RuntimeConfigError::Invalid { .. }));
    assert!(error
        .to_string()
        .contains("project agent profile selection"));
}

#[test]
fn missing_project_agent_profile_selection_id_is_rejected() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_project_raw("agent-profile-selection.json", r#"{}"#);

    let error = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("missing project agent profile id must fail closed");

    assert!(matches!(error, RuntimeConfigError::Invalid { .. }));
}

#[test]
fn invalid_provider_capability_route_is_rejected_without_secret_leakage() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_project(
        "provider-capability-routes.json",
        &routes(&[("", "secret-provider-value", &["image.generate"])]),
    );

    let error = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("invalid route must fail closed");
    let message = error.to_string();

    assert!(matches!(error, RuntimeConfigError::Invalid { .. }));
    assert!(message.contains("project provider capability routes"));
    assert!(!message.contains("secret-provider-value"));
}

#[tokio::test]
async fn skill_content_is_frozen_when_runtime_snapshot_is_resolved() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_project(
        "skills.json",
        &SkillSelectionRecord {
            enabled: vec!["frozen-skill".into()],
        },
    );
    fixture.write_project_skill("frozen-skill", "original skill body");

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("resolve runtime snapshot");
    fixture.write_project_skill("frozen-skill", "mutated skill body");

    let report = snapshot.skill_loader.load_all().await.expect("load skills");
    assert_eq!(report.loaded.len(), 1);
    assert_eq!(report.loaded[0].body.trim(), "original skill body");
}

#[tokio::test]
async fn plugin_manifest_is_frozen_when_runtime_snapshot_is_resolved() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global_plugin("frozen-plugin", "original description");

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("resolve runtime snapshot");
    fixture.write_global_plugin_manifest("frozen-plugin", "mutated description");

    let registry = snapshot
        .materialize_plugin_registry()
        .expect("materialize plugin registry");
    let discovered = registry.discover().await.expect("discover plugins");
    assert_eq!(discovered.len(), 1);
    assert_eq!(
        discovered[0].record.manifest.description.as_deref(),
        Some("original description")
    );
    assert_eq!(registry.snapshot().discovered.len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn global_sidecar_plugin_uses_the_production_runtime_loader() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global_sidecar_plugin("global-sidecar");
    let plugin_id = harness_contracts::PluginId("global-sidecar@0.1.0".to_owned());

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("resolve runtime snapshot");
    let registry = snapshot
        .materialize_plugin_registry()
        .expect("materialize plugin registry");
    registry.discover().await.expect("discover global sidecar");
    registry
        .activate(&plugin_id)
        .await
        .expect("activate global sidecar");

    assert_eq!(
        registry.state(&plugin_id),
        Some(PluginLifecycleState::Activated)
    );
}

#[cfg(unix)]
#[test]
fn workspace_symlink_is_rejected_instead_of_reading_replaced_project_config() {
    use std::os::unix::fs::symlink;

    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    let link = fixture.root.path().join("workspace-link");
    symlink(fixture.workspace(), &link).expect("workspace symlink");

    let error = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(&link, None)
        .expect_err("workspace symlink must fail closed");

    assert!(matches!(error, RuntimeConfigError::WorkspaceSymlink { .. }));
}

#[cfg(unix)]
#[test]
fn preexisting_workspace_runtime_symlink_cannot_redirect_daemon_memory() {
    use std::os::unix::fs::symlink;

    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    let external = fixture.root.path().join("external-runtime");
    fs::create_dir_all(&external).expect("external runtime");
    symlink(&external, fixture.workspace.join(".jyowo/runtime")).expect("runtime symlink");

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("workspace runtime path is not daemon storage");
    snapshot
        .ensure_memory_parent()
        .expect("create daemon-private memory parent");
    rusqlite::Connection::open(&snapshot.memory_database_path)
        .expect("open daemon-private workspace memory database")
        .execute_batch("CREATE TABLE proof (value INTEGER);")
        .expect("write daemon-private workspace memory database");

    assert!(snapshot.memory_database_path.exists());
    assert!(!external.join("memory/memory.sqlite3").exists());
}

#[cfg(unix)]
#[test]
fn workspace_runtime_symlink_swap_does_not_redirect_daemon_memory_creation() {
    use std::os::unix::fs::symlink;

    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("resolve runtime snapshot");
    let external = fixture.root.path().join("external-runtime-after-resolve");
    fs::create_dir_all(&external).expect("external runtime");
    symlink(&external, fixture.workspace.join(".jyowo/runtime")).expect("runtime symlink");

    snapshot
        .ensure_memory_parent()
        .expect("daemon memory parent must not traverse the workspace runtime path");
    rusqlite::Connection::open(&snapshot.memory_database_path)
        .expect("open daemon-private workspace memory database")
        .execute_batch("CREATE TABLE proof (value INTEGER);")
        .expect("write daemon-private workspace memory database");

    assert!(snapshot.memory_database_path.exists());
    assert!(!external.join("memory").exists());
}

#[cfg(unix)]
#[test]
fn workspace_runtime_swap_after_memory_parent_creation_cannot_redirect_sqlite_open() {
    use std::os::unix::fs::symlink;

    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("resolve runtime snapshot");
    snapshot
        .ensure_memory_parent()
        .expect("create daemon-private memory parent");
    let external = fixture.root.path().join("external-after-create");
    fs::create_dir_all(&external).expect("external runtime");
    symlink(&external, fixture.workspace.join(".jyowo/runtime")).expect("runtime symlink");

    rusqlite::Connection::open(&snapshot.memory_database_path)
        .expect("open daemon-private workspace memory database")
        .execute_batch("CREATE TABLE proof (value INTEGER);")
        .expect("write daemon-private workspace memory database");

    assert!(snapshot.memory_database_path.exists());
    assert!(!external.join("memory.sqlite3").exists());
    assert!(!external.join("memory/memory.sqlite3").exists());
}

#[cfg(unix)]
#[test]
fn plugin_directory_symlink_escape_is_rejected() {
    use std::os::unix::fs::symlink;

    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    let external = fixture.root.path().join("external-plugins");
    fs::create_dir_all(&external).expect("external plugins");
    symlink(&external, fixture.workspace.join(".jyowo/plugins")).expect("plugins symlink");

    let error = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("plugin symlink escape must fail closed");

    assert!(matches!(error, RuntimeConfigError::ConfigSymlink { .. }));
}

#[cfg(unix)]
#[test]
fn selected_skill_package_symlink_escape_is_rejected() {
    use std::os::unix::fs::symlink;

    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_project(
        "skills.json",
        &SkillSelectionRecord {
            enabled: vec!["escaped-skill".into()],
        },
    );
    let external = fixture.root.path().join("external-skill-package");
    fs::create_dir_all(&external).expect("external skill package");
    fs::write(
        external.join("SKILL.md"),
        "---\nname: escaped-skill\ndescription: escaped\n---\noutside\n",
    )
    .expect("external skill");
    let packages = fixture.workspace.join(".jyowo/skills/packages");
    fs::create_dir_all(&packages).expect("skill packages");
    symlink(&external, packages.join("escaped-skill")).expect("skill package symlink");

    let error = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("selected skill package symlink must fail closed");

    assert!(matches!(error, RuntimeConfigError::Invalid { .. }));
}

#[cfg(unix)]
#[test]
fn unindexed_plugin_package_symlink_is_not_scanned() {
    use std::os::unix::fs::symlink;

    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    let packages = fixture.workspace.join(".jyowo/plugins/packages");
    fs::create_dir_all(&packages).expect("plugin packages");
    let external = fixture.root.path().join("external-unindexed-plugin");
    fs::create_dir_all(&external).expect("external plugin");
    symlink(&external, packages.join("unindexed")).expect("unindexed plugin symlink");

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("unindexed plugin package must not be scanned");
}

struct RuntimeFixture {
    root: TempDir,
    home: std::path::PathBuf,
    workspace: std::path::PathBuf,
}

impl RuntimeFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join(".jyowo");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(home.join("config")).expect("global config");
        fs::create_dir_all(workspace.join(".jyowo/config")).expect("project config");
        Self {
            root,
            home,
            workspace,
        }
    }

    fn config_root(&self) -> std::path::PathBuf {
        self.home.join("config")
    }

    fn workspace(&self) -> &Path {
        &self.workspace
    }

    fn write_global_provider_files(&self) {
        self.write_global(
            "provider-profiles.json",
            &[
                profile("global-model", "anthropic", "claude-global"),
                profile("project-model", "anthropic", "claude-project"),
                profile("global", "anthropic", "claude-route-global"),
            ],
        );
        self.write_global(
            "provider-secrets.json",
            &ProviderSecretsRecord {
                entries: vec![
                    secret("global-model"),
                    secret("project-model"),
                    secret("global"),
                ],
            },
        );
        self.write_global(
            "provider-selection.json",
            &ProviderSelectionRecord {
                default_config_id: Some("global-model".into()),
            },
        );
    }

    fn write_global(&self, file: &str, value: &(impl Serialize + ?Sized)) {
        write_json(&self.home.join("config").join(file), value);
    }

    fn write_project(&self, file: &str, value: &(impl Serialize + ?Sized)) {
        write_json(&self.workspace.join(".jyowo/config").join(file), value);
    }

    fn write_global_raw(&self, file: &str, value: &str) {
        fs::write(self.home.join("config").join(file), value).expect("write global raw");
    }

    fn write_project_raw(&self, file: &str, value: &str) {
        fs::write(self.workspace.join(".jyowo/config").join(file), value)
            .expect("write project raw");
    }

    fn write_project_skill(&self, package_id: &str, body: &str) {
        let package = self
            .workspace
            .join(".jyowo/skills/packages")
            .join(package_id);
        fs::create_dir_all(&package).expect("project skill package");
        fs::write(
            package.join("SKILL.md"),
            format!("---\nname: {package_id}\ndescription: frozen skill\n---\n{body}\n"),
        )
        .expect("write project skill");
    }

    fn write_global_plugin(&self, name: &str, description: &str) {
        fs::create_dir_all(self.home.join("plugins/packages").join(name))
            .expect("global plugin package");
        self.write_global_plugin_manifest(name, description);
        write_json(
            &self.home.join("plugins/index.json"),
            &serde_json::json!({
                "allowProjectPlugins": false,
                "records": [{
                    "pluginId": format!("{name}@0.1.0"),
                    "name": name,
                    "version": "0.1.0",
                    "enabled": true,
                    "packageDir": name,
                    "sourcePath": "fixture",
                    "contentHash": "fixture",
                    "importedAt": "2026-01-01T00:00:00Z",
                    "updatedAt": "2026-01-01T00:00:00Z",
                    "config": {}
                }]
            }),
        );
    }

    #[cfg(unix)]
    fn write_global_sidecar_plugin(&self, name: &str) {
        use std::os::unix::fs::PermissionsExt;

        self.write_global_plugin(name, "global sidecar plugin");
        let binary = self
            .home
            .join("plugins/packages")
            .join(name)
            .join(format!("jyowo-plugin-{name}"));
        fs::write(
            &binary,
            r#"#!/bin/sh
if [ "$1" = "--harness-runtime" ]; then
request=$(cat)
case "$request" in
  *\"method\":\"activate\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":{"registered_tools":[],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}'
    exit 0
    ;;
  *\"method\":\"deactivate\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":null}'
    exit 0
    ;;
esac
fi
exit 2
"#,
        )
        .expect("global plugin sidecar");
        let mut permissions = fs::metadata(&binary)
            .expect("global plugin sidecar metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(binary, permissions).expect("global plugin sidecar executable");
    }

    fn write_global_plugin_manifest(&self, name: &str, description: &str) {
        let package = self.home.join("plugins/packages").join(name);
        fs::create_dir_all(&package).expect("global plugin package");
        write_json(
            &package.join("plugin.json"),
            &PluginManifest {
                name: PluginName::new(name).expect("plugin name"),
                version: semver::Version::parse("0.1.0").expect("plugin version"),
                trust_level: TrustLevel::UserControlled,
                description: Some(description.into()),
                authors: Vec::new(),
                repository: None,
                signature: None,
                capabilities: PluginCapabilities::default(),
                dependencies: Vec::new(),
                min_harness_version: semver::VersionReq::parse(">=0.0.0")
                    .expect("version requirement"),
            },
        );
    }
}

fn write_json(path: &Path, value: &(impl Serialize + ?Sized)) {
    fs::write(path, serde_json::to_vec_pretty(value).expect("serialize")).expect("write json");
}

fn secret(config_id: &str) -> ProviderSecretEntry {
    ProviderSecretEntry {
        config_id: config_id.into(),
        api_key: format!("{config_id}-secret"),
        official_quota_api_key: None,
    }
}

fn profile(config_id: &str, provider_id: &str, model_id: &str) -> ProviderProfileDefinition {
    let protocol = ModelProtocol::Messages;
    ProviderProfileDefinition {
        id: config_id.into(),
        display_name: config_id.into(),
        provider_id: provider_id.into(),
        model_id: model_id.into(),
        protocol,
        model_options: Default::default(),
        base_url: None,
        provider_defaults: None,
        model_descriptor: ProviderProfileModelDescriptor {
            protocol,
            context_window: 32_000,
            display_name: model_id.into(),
            lifecycle: ProviderProfileModelLifecycle::Stable,
            max_output_tokens: 4_096,
            model_id: model_id.into(),
            provider_id: provider_id.into(),
            conversation_capability: ProviderProfileConversationCapability {
                input_modalities: vec!["text".into()],
                output_modalities: vec!["text".into()],
                context_window: 32_000,
                max_output_tokens: 4_096,
                streaming: true,
                tool_calling: true,
                reasoning: false,
                prompt_cache: false,
                structured_output: false,
            },
            runtime_semantics: None,
        },
    }
}

fn routes(items: &[(&str, &str, &[&str])]) -> ProviderCapabilityRouteSettings {
    ProviderCapabilityRouteSettings {
        version: 1,
        routes: items
            .iter()
            .map(
                |(config_id, provider_id, operations)| ProviderCapabilityRoute {
                    kind: CapabilityRouteKind::ImageGeneration,
                    config_id: (*config_id).into(),
                    provider_id: (*provider_id).into(),
                    operation_ids: operations
                        .iter()
                        .map(|operation| (*operation).into())
                        .collect(),
                    enabled: true,
                },
            )
            .collect(),
    }
}

fn agent_profile(id: &str) -> AgentProfile {
    AgentProfile {
        id: id.into(),
        scope: AgentProfileScope::User,
        role: "Reviewer".into(),
        description: "review code".into(),
        model_config_override: None,
        tool_allowlist: None,
        tool_blocklist: vec![],
        sandbox_inheritance: AgentProfileSandboxInheritance::InheritParent,
        memory_scope: AgentProfileMemoryScope::ReadOnly,
        context_mode: AgentProfileContextMode::Focused,
        max_turns: 8,
        max_depth: 1,
        default_workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
    }
}
