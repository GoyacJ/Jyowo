use std::{fs, path::Path};

use harness_contracts::{
    AgentProfile, AgentProfileContextMode, AgentProfileMemoryScope, AgentProfileSandboxInheritance,
    AgentProfileScope, AgentWorkspaceIsolationMode, CapabilityRouteKind, ExecutionDefaultsRecord,
    ExecutionOverridesRecord, ModelProtocol, PermissionMode, PluginSelectionRecord,
    ProviderCapabilityRoute, ProviderCapabilityRouteSettings,
    ProviderProfileConversationCapability, ProviderProfileDefinition,
    ProviderProfileModelDescriptor, ProviderProfileModelLifecycle, ProviderSecretEntry,
    ProviderSecretsRecord, ProviderSelectionRecord, SkillSelectionRecord, ToolProfile,
};
use harness_daemon::{RuntimeConfigError, RuntimeConfigResolver};
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
fn project_agent_profile_selection_resolves_global_definition_and_workspace_memory() {
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
    assert_eq!(
        snapshot.memory_database_path,
        fixture
            .workspace()
            .canonicalize()
            .expect("canonical workspace")
            .join(".jyowo/runtime/memory/memory.sqlite3")
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
