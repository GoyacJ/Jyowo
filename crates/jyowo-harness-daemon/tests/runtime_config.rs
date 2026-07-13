use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

use harness_contracts::{
    AgentProfile, AgentProfileContextMode, AgentProfileMemoryScope, AgentProfileSandboxInheritance,
    AgentProfileScope, AgentWorkspaceIsolationMode, CapabilityRouteKind, ExecutionDefaultsRecord,
    ExecutionOverridesRecord, ModelProtocol, PermissionMode, PluginSelectionRecord,
    ProviderCapabilityRoute, ProviderCapabilityRouteSettings,
    ProviderProfileConversationCapability, ProviderProfileDefinition,
    ProviderProfileModelDescriptor, ProviderProfileModelLifecycle, ProviderSecretEntry,
    ProviderSecretsRecord, ProviderSelectionRecord, SkillId, SkillSelectionRecord, SkillStatus,
    ToolProfile, TrustLevel,
};
use harness_daemon::{RuntimeConfigError, RuntimeConfigResolver};
use harness_plugin::{PluginCapabilities, PluginLifecycleState, PluginManifest, PluginName};
use jyowo_harness_sdk::{skill_config::SecretString, SkillConfigStoreError, SkillSecretStore};
use serde::Serialize;
use tempfile::TempDir;

#[test]
fn skill_config_document_is_loaded_from_the_global_config_root() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global_raw(
        "skill-config.json",
        r#"{
          "version": 1,
          "skills": {
            "workspace:configured": {
              "values": { "github.org": "jyowo" },
              "secrets": { "github.token": { "configured": true } }
            },
            "workspace:other": {
              "values": { "github.org": "other" },
              "secrets": {}
            }
          }
        }"#,
    );

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .with_skill_secret_store(Arc::new(MemorySecretStore::default()))
        .resolve(fixture.workspace(), None)
        .expect("resolve skill config");

    assert_eq!(
        snapshot
            .skill_config
            .value_for("workspace:configured", "github.org"),
        Some(&serde_json::json!("jyowo"))
    );
    assert_eq!(
        snapshot
            .skill_config
            .value_for("workspace:other", "github.org"),
        Some(&serde_json::json!("other"))
    );
    assert!(snapshot
        .skill_config
        .contains_secret_for("workspace:configured", "github.token"));
    assert!(!format!("{snapshot:?}").contains("test-secret-plaintext"));
}

#[test]
fn skill_config_secret_availability_uses_the_injected_secret_store() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global_raw(
        "skill-config.json",
        r#"{
          "version": 1,
          "skills": {
            "workspace:configured": {
              "values": {},
              "secrets": { "github.token": { "configured": true } }
            }
          }
        }"#,
    );
    let secret_store = Arc::new(MemorySecretStore::default());
    let resolver = RuntimeConfigResolver::new(fixture.config_root())
        .with_skill_secret_store(secret_store.clone());

    let missing = resolver
        .resolve(fixture.workspace(), None)
        .expect("resolve missing secret");
    assert!(!missing
        .skill_config
        .secret_is_available_for("workspace:configured", "github.token")
        .unwrap());

    secret_store
        .set(
            "workspace:configured",
            "github.token",
            SecretString::from("daemon-test-secret".to_owned()),
        )
        .unwrap();
    let available = resolver
        .resolve(fixture.workspace(), None)
        .expect("resolve available secret");
    assert!(available
        .skill_config
        .secret_is_available_for("workspace:configured", "github.token")
        .unwrap());
}

#[test]
fn configured_false_public_metadata_does_not_require_secret_store_access() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global_raw(
        "skill-config.json",
        r#"{"version":1,"skills":{"workspace:configured":{"values":{"region":"cn-east"},"secrets":{"region":{"configured":false}}}}}"#,
    );

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .with_skill_secret_store(Arc::new(UnavailableSecretStore))
        .resolve(fixture.workspace(), None)
        .expect("public config snapshot must not read the secret store");

    assert_eq!(
        snapshot
            .skill_config
            .value_for("workspace:configured", "region"),
        Some(&serde_json::json!("cn-east"))
    );
}

#[tokio::test]
async fn required_secret_status_query_propagates_store_failure() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_project(
        "skills.json",
        &SkillSelectionRecord {
            enabled: vec!["required-secret".into()],
        },
    );
    fixture.write_project_skill_with_frontmatter(
        "required-secret",
        "config:\n  - key: token\n    type: string\n    secret: true\n    required: true",
        "Body.",
    );

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .with_skill_secret_store(Arc::new(UnavailableSecretStore))
        .resolve(fixture.workspace(), None)
        .expect("snapshot construction does not read undeclared runtime secrets");
    let report = snapshot.skill_loader.load_all().await.expect("load skills");
    let registry = jyowo_harness_sdk::ext::SkillRegistry::builder()
        .with_skills(report.loaded)
        .build();
    let mut registry_snapshot = (*registry.snapshot()).clone();

    let error = jyowo_harness_sdk::apply_skill_config_statuses(
        &mut registry_snapshot,
        &snapshot.skill_config,
    )
    .expect_err("required secret status must propagate secure-store failure");

    assert_eq!(error, SkillConfigStoreError::SecretStoreUnavailable);
    assert!(!format!("{error:?} {error}").contains("must-not-leak-secret"));
}

#[tokio::test]
async fn wrong_typed_required_value_from_global_document_marks_skill_missing() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_project(
        "skills.json",
        &SkillSelectionRecord {
            enabled: vec!["typed".into()],
        },
    );
    fixture.write_project_skill_with_frontmatter(
        "typed",
        "config:\n  - key: region\n    type: string\n    required: true",
        "Body without config interpolation.",
    );
    fixture.write_global_raw(
        "skill-config.json",
        r#"{"version":1,"skills":{"workspace:typed":{"values":{"region":42},"secrets":{}}}}"#,
    );

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .with_skill_secret_store(Arc::new(MemorySecretStore::default()))
        .resolve(fixture.workspace(), None)
        .expect("resolve runtime snapshot");
    let report = snapshot.skill_loader.load_all().await.expect("load skills");
    let registry = jyowo_harness_sdk::ext::SkillRegistry::builder()
        .with_skills(report.loaded)
        .build();
    let mut registry_snapshot = (*registry.snapshot()).clone();
    jyowo_harness_sdk::apply_skill_config_statuses(&mut registry_snapshot, &snapshot.skill_config)
        .expect("status assembly");

    assert!(matches!(
        registry_snapshot
            .status
            .get(&SkillId("workspace:typed".to_owned())),
        Some(SkillStatus::PrerequisiteMissing { config_keys, .. })
            if config_keys == &["region".to_owned()]
    ));
}

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
fn explicit_task_provider_overrides_project_selection_and_global_default() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();

    let global = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("global provider selection");
    fixture.write_project(
        "provider-selection.json",
        &ProviderSelectionRecord {
            default_config_id: Some("project-model".into()),
        },
    );

    let project = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("project provider selection");
    let explicit = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), Some("global-model"))
        .expect("explicit task provider selection");

    assert_eq!(global.provider.config_id, "global-model");
    assert_eq!(project.provider.config_id, "project-model");
    assert_eq!(explicit.provider.config_id, "global-model");
}

#[test]
fn project_routes_override_by_operation_and_inherit_other_global_operations() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global(
        "provider-profiles.json",
        &[
            profile("global-model", "anthropic", "claude-global"),
            profile("project-model", "minimax", "MiniMax-M2.1"),
            profile("global", "minimax", "MiniMax-M2.1"),
        ],
    );
    fixture.write_global(
        "provider-capability-routes.json",
        &ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![
                route(
                    CapabilityRouteKind::ImageGeneration,
                    "global",
                    "minimax",
                    &["minimax.image_generation"],
                ),
                route(
                    CapabilityRouteKind::VideoGeneration,
                    "global",
                    "minimax",
                    &["minimax.video_generation"],
                ),
            ],
        },
    );
    fixture.write_project(
        "provider-capability-routes.json",
        &routes(&[("project-model", "minimax", &["minimax.image_generation"])]),
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

    assert_eq!(
        route_for("minimax.image_generation").config_id,
        "project-model"
    );
    assert_eq!(route_for("minimax.video_generation").config_id, "global");
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
fn runtime_configuration_diagnostics_are_bounded_and_do_not_echo_user_input() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    let malicious = format!("api_key=diagnostic-secret-{}", "x".repeat(2_048));
    fixture.write_project_raw(
        "mcp-servers.json",
        &format!(
            r#"[
              {{"enabled":true,"displayName":"one","id":"{malicious}","scope":"session","transport":{{"kind":"stdio","command":"one"}}}},
              {{"enabled":true,"displayName":"two","id":"{malicious}","scope":"session","transport":{{"kind":"stdio","command":"two"}}}}
            ]"#
        ),
    );

    let error = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("duplicate malicious id must fail");
    let message = error.to_string();

    assert!(!message.contains("diagnostic-secret"));
    assert!(message.len() <= 512, "diagnostic length: {}", message.len());
}

#[test]
fn runtime_configuration_paths_and_decode_details_are_redacted() {
    let root = tempfile::tempdir().expect("tempdir");
    let secret_path = root.path().join("path-containing-diagnostic-secret");
    fs::create_dir_all(&secret_path).expect("secret path");
    let error = RuntimeConfigResolver::new(secret_path.join("missing-config"))
        .resolve_memory_database_path(None)
        .expect_err("missing config must fail");
    let message = error.to_string();
    assert!(!message.contains("diagnostic-secret"));
    assert!(message.len() <= 512, "diagnostic length: {}", message.len());

    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_project_raw(
        "mcp-servers.json",
        r#"[{"enabled":true,"displayName":"one","id":"one","scope":"session","diagnostic-secret-field":true,"transport":{"kind":"stdio","command":"one"}}]"#,
    );
    let error = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("unknown field must fail");
    let message = error.to_string();
    assert!(!message.contains("diagnostic-secret"));
    assert!(message.len() <= 512, "diagnostic length: {}", message.len());
}

#[test]
fn persisted_mcp_records_are_validated_before_runtime_snapshot() {
    let invalid_records = [
        r#"{"enabled":true,"displayName":"server","id":"bad/id","scope":"session","transport":{"kind":"stdio","command":"node"}}"#,
        r#"{"enabled":true,"displayName":"server","id":"server","scope":"workspace","transport":{"kind":"stdio","command":"node"}}"#,
        r#"{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"stdio","command":"node","args":[""]}}"#,
        r#"{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"stdio","command":"node","env":[{"key":"BAD-KEY","value":"safe"}]}}"#,
        r#"{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"stdio","command":"node","env":[{"key":"API_KEY","value":"safe"}]}}"#,
        r#"{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"stdio","command":"node","env":[{"key":"MODE","value":"sk-diagnostic-secret"}]}}"#,
        r#"{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"stdio","command":"node","inheritEnv":["SERVICE_TOKEN"]}}"#,
        r#"{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"stdio","command":"node","workingDir":"../escape"}}"#,
        r#"{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"http","url":"ftp://example.com"}}"#,
        r#"{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"http","url":"https://example.com","headers":[{"key":"Authorization","value":"safe"}]}}"#,
        r#"{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"http","url":"https://example.com","headers":[{"key":"bad header","value":"safe"}]}}"#,
        r#"{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"inProcess"}}"#,
    ];

    for record in invalid_records {
        let fixture = RuntimeFixture::new();
        fixture.write_global_provider_files();
        fixture.write_project_raw("mcp-servers.json", &format!("[{record}]"));

        let error = RuntimeConfigResolver::new(fixture.config_root())
            .resolve(fixture.workspace(), None)
            .expect_err("invalid persisted MCP record must fail closed");
        let message = error.to_string();
        assert!(!message.contains("diagnostic-secret"));
        assert!(message.len() <= 512);
    }
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

#[test]
fn provider_route_missing_config_is_rejected_during_resolution() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global(
        "provider-capability-routes.json",
        &routes(&[(
            "missing-route-config",
            "minimax",
            &["minimax.image_generation"],
        )]),
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("route config must exist at resolution time");
}

#[test]
fn provider_route_missing_secret_is_rejected_during_resolution() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global(
        "provider-profiles.json",
        &[
            profile("global-model", "anthropic", "claude-global"),
            profile("route-model", "minimax", "MiniMax-M2.1"),
        ],
    );
    fixture.write_global(
        "provider-capability-routes.json",
        &routes(&[("route-model", "minimax", &["minimax.image_generation"])]),
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("route config must have a secret at resolution time");
}

#[test]
fn provider_route_provider_mismatch_is_rejected_during_resolution() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global(
        "provider-profiles.json",
        &[
            profile("global-model", "anthropic", "claude-global"),
            profile("route-model", "minimax", "MiniMax-M2.1"),
        ],
    );
    fixture.write_global(
        "provider-secrets.json",
        &ProviderSecretsRecord {
            entries: vec![secret("global-model"), secret("route-model")],
        },
    );
    fixture.write_global(
        "provider-capability-routes.json",
        &routes(&[("route-model", "openai", &["minimax.image_generation"])]),
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("route provider must match its config");
}

#[test]
fn provider_route_unknown_operation_is_rejected_during_resolution() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_route_provider();
    fixture.write_global(
        "provider-capability-routes.json",
        &routes(&[("route-model", "minimax", &["unknown.operation"])]),
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("route operation must exist in the provider catalog");
}

#[test]
fn provider_route_kind_mismatch_is_rejected_during_resolution() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_route_provider();
    fixture.write_global(
        "provider-capability-routes.json",
        &routes(&[("route-model", "minimax", &["minimax.video_generation"])]),
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("route kind must match the provider operation");
}

#[test]
fn disabled_provider_route_does_not_require_a_live_config_or_adapter() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    let mut disabled = route(
        CapabilityRouteKind::FileOperation,
        "removed-config",
        "minimax",
        &["minimax.files.upload"],
    );
    disabled.enabled = false;
    fixture.write_global(
        "provider-capability-routes.json",
        &ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![disabled],
        },
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("disabled route must not require runtime dependencies");
}

#[test]
fn enabled_provider_route_without_runtime_adapter_is_rejected() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_route_provider();
    fixture.write_global(
        "provider-capability-routes.json",
        &ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![route(
                CapabilityRouteKind::FileOperation,
                "route-model",
                "minimax",
                &["minimax.files.upload"],
            )],
        },
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("catalog-only route must fail before runtime execution");
}

#[test]
fn enabled_routes_of_one_kind_cannot_target_multiple_configs() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global(
        "provider-profiles.json",
        &[
            profile("global-model", "anthropic", "claude-global"),
            profile("route-one", "minimax", "MiniMax-M2.1"),
            profile("route-two", "minimax", "MiniMax-M2.1"),
        ],
    );
    fixture.write_global(
        "provider-secrets.json",
        &ProviderSecretsRecord {
            entries: vec![
                secret("global-model"),
                secret("route-one"),
                secret("route-two"),
            ],
        },
    );
    fixture.write_global(
        "provider-capability-routes.json",
        &ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![
                route(
                    CapabilityRouteKind::ImageGeneration,
                    "route-one",
                    "minimax",
                    &["minimax.image_generation"],
                ),
                route(
                    CapabilityRouteKind::ImageGeneration,
                    "route-two",
                    "minimax",
                    &["minimax.image_generation"],
                ),
            ],
        },
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("one route kind cannot fan out across provider configs");
}

#[test]
fn mcp_working_directory_must_be_a_directory_and_is_frozen_canonically() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fs::write(fixture.workspace.join("not-a-directory"), "fixture").expect("fixture file");
    fixture.write_project_raw(
        "mcp-servers.json",
        r#"[{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"stdio","command":"node","working_dir":"not-a-directory"}}]"#,
    );
    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("MCP working directory cannot be a regular file");

    fs::create_dir(fixture.workspace.join("working-dir")).expect("working directory");
    fixture.write_project_raw(
        "mcp-servers.json",
        r#"[{"enabled":true,"displayName":"server","id":"server","scope":"session","transport":{"kind":"stdio","command":"node","working_dir":"working-dir"}}]"#,
    );
    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("valid working directory");
    let harness_contracts::McpServerTransportConfig::Stdio { working_dir, .. } =
        &snapshot.mcp_servers[0].transport
    else {
        panic!("stdio transport expected");
    };
    assert_eq!(
        working_dir.as_deref(),
        Some(
            fixture
                .workspace
                .join("working-dir")
                .canonicalize()
                .expect("canonical working directory")
                .to_str()
                .expect("utf-8 fixture path")
        )
    );
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

#[test]
fn disabled_missing_plugin_package_does_not_block_runtime_resolution() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global_plugin_index("missing-disabled", false);

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("disabled missing package must not be read");
}

#[test]
fn disabled_corrupt_plugin_package_does_not_block_runtime_resolution() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global_plugin_index("corrupt-disabled", false);
    let package = fixture
        .home
        .join("plugins/packages")
        .join("corrupt-disabled");
    fs::create_dir_all(&package).expect("corrupt disabled package");
    fs::write(package.join("plugin.json"), b"not-json").expect("corrupt manifest");

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("disabled corrupt package must not be read");
}

#[test]
fn disabled_global_plugin_with_invalid_identity_is_absent_from_runtime_resolution() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fs::create_dir_all(fixture.home.join("plugins")).expect("global plugin index parent");
    write_json(
        &fixture.home.join("plugins/index.json"),
        &plugin_index_with_invalid_record(false, false),
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("disabled global plugin must be filtered before identity or path parsing");
}

#[test]
fn disabled_project_plugin_with_invalid_identity_is_absent_from_runtime_resolution() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fs::create_dir_all(fixture.workspace.join(".jyowo/plugins"))
        .expect("project plugin index parent");
    write_json(
        &fixture.workspace.join(".jyowo/plugins/index.json"),
        &plugin_index_with_invalid_record(true, false),
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("disabled project plugin must be filtered before identity or path parsing");
}

#[test]
fn disallowed_project_plugin_with_invalid_identity_is_absent_from_runtime_resolution() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fs::create_dir_all(fixture.workspace.join(".jyowo/plugins"))
        .expect("project plugin index parent");
    write_json(
        &fixture.workspace.join(".jyowo/plugins/index.json"),
        &plugin_index_with_invalid_record(false, true),
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("disallowed project plugin must be filtered before identity or path parsing");
}

#[test]
fn enabled_global_plugin_with_invalid_identity_fails_closed() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fs::create_dir_all(fixture.home.join("plugins")).expect("global plugin index parent");
    write_json(
        &fixture.home.join("plugins/index.json"),
        &plugin_index_with_invalid_record(false, true),
    );

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("enabled invalid plugin identity must fail closed");
}

#[test]
fn enabled_corrupt_plugin_package_fails_closed() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global_plugin_index("corrupt-enabled", true);
    let package = fixture
        .home
        .join("plugins/packages")
        .join("corrupt-enabled");
    fs::create_dir_all(&package).expect("corrupt enabled package");
    fs::write(package.join("plugin.json"), b"not-json").expect("corrupt manifest");

    RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect_err("enabled corrupt package must fail closed");
}

fn plugin_index_with_invalid_record(
    allow_project_plugins: bool,
    enabled: bool,
) -> serde_json::Value {
    serde_json::json!({
        "allowProjectPlugins": allow_project_plugins,
        "records": [{
            "pluginId": "invalid-record@0.1.0",
            "name": "invalid/name",
            "version": "0.1.0",
            "enabled": enabled,
            "packageDir": "../must-not-be-read",
            "sourcePath": "fixture",
            "contentHash": "fixture",
            "importedAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "config": { "mustNotAppear": true }
        }]
    })
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
#[tokio::test]
async fn foreground_and_child_registries_use_frozen_sidecar_executable_bytes() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global_sidecar_plugin("frozen-sidecar");
    let plugin_id = harness_contracts::PluginId("frozen-sidecar@0.1.0".to_owned());

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("resolve runtime snapshot");
    let child_snapshot = snapshot.clone();
    fixture.write_failing_global_sidecar_binary("frozen-sidecar");

    for (label, snapshot) in [("foreground", snapshot), ("child", child_snapshot)] {
        let registry = snapshot
            .materialize_plugin_registry()
            .expect("materialize plugin registry");
        registry.discover().await.expect("discover frozen sidecar");
        registry
            .activate(&plugin_id)
            .await
            .unwrap_or_else(|error| panic!("{label} must execute frozen bytes: {error}"));
    }
}

#[cfg(unix)]
#[test]
fn frozen_sidecar_snapshot_directory_is_removed_after_last_snapshot_drop() {
    let fixture = RuntimeFixture::new();
    fixture.write_global_provider_files();
    fixture.write_global_sidecar_plugin("cleanup-sidecar");

    let snapshot = RuntimeConfigResolver::new(fixture.config_root())
        .resolve(fixture.workspace(), None)
        .expect("resolve runtime snapshot");
    let clone = snapshot.clone();
    let snapshots_root = fixture.home.join("runtime/plugin-snapshots");
    let roots = fs::read_dir(&snapshots_root)
        .expect("snapshot root")
        .collect::<Result<Vec<_>, _>>()
        .expect("snapshot entries");
    assert_eq!(roots.len(), 1);
    let frozen_root = roots[0].path();

    drop(snapshot);
    assert!(frozen_root.exists(), "clone must keep frozen bytes alive");
    drop(clone);
    assert!(!frozen_root.exists(), "last drop must clean frozen bytes");
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

#[derive(Default)]
struct MemorySecretStore {
    secrets: Mutex<BTreeMap<(String, String), SecretString>>,
}

impl SkillSecretStore for MemorySecretStore {
    fn get(
        &self,
        skill_id: &str,
        key: &str,
    ) -> Result<Option<SecretString>, SkillConfigStoreError> {
        Ok(self
            .secrets
            .lock()
            .unwrap()
            .get(&(skill_id.to_owned(), key.to_owned()))
            .cloned())
    }

    fn set(
        &self,
        skill_id: &str,
        key: &str,
        value: SecretString,
    ) -> Result<(), SkillConfigStoreError> {
        self.secrets
            .lock()
            .unwrap()
            .insert((skill_id.to_owned(), key.to_owned()), value);
        Ok(())
    }

    fn delete(&self, skill_id: &str, key: &str) -> Result<(), SkillConfigStoreError> {
        self.secrets
            .lock()
            .unwrap()
            .remove(&(skill_id.to_owned(), key.to_owned()));
        Ok(())
    }
}

struct UnavailableSecretStore;

impl SkillSecretStore for UnavailableSecretStore {
    fn get(
        &self,
        _skill_id: &str,
        _key: &str,
    ) -> Result<Option<SecretString>, SkillConfigStoreError> {
        Err(SkillConfigStoreError::SecretStoreUnavailable)
    }

    fn set(
        &self,
        _skill_id: &str,
        _key: &str,
        _value: SecretString,
    ) -> Result<(), SkillConfigStoreError> {
        Err(SkillConfigStoreError::SecretStoreUnavailable)
    }

    fn delete(&self, _skill_id: &str, _key: &str) -> Result<(), SkillConfigStoreError> {
        Err(SkillConfigStoreError::SecretStoreUnavailable)
    }
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

    fn write_route_provider(&self) {
        self.write_global(
            "provider-profiles.json",
            &[
                profile("global-model", "anthropic", "claude-global"),
                profile("route-model", "minimax", "MiniMax-M2.1"),
            ],
        );
        self.write_global(
            "provider-secrets.json",
            &ProviderSecretsRecord {
                entries: vec![secret("global-model"), secret("route-model")],
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
        self.write_project_skill_with_frontmatter(package_id, "", body);
    }

    fn write_project_skill_with_frontmatter(
        &self,
        package_id: &str,
        frontmatter: &str,
        body: &str,
    ) {
        let package = self
            .workspace
            .join(".jyowo/skills/packages")
            .join(package_id);
        fs::create_dir_all(&package).expect("project skill package");
        fs::write(
            package.join("SKILL.md"),
            format!(
                "---\nname: {package_id}\ndescription: frozen skill\n{frontmatter}\n---\n{body}\n"
            ),
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

    fn write_global_plugin_index(&self, name: &str, enabled: bool) {
        fs::create_dir_all(self.home.join("plugins")).expect("global plugin settings");
        write_json(
            &self.home.join("plugins/index.json"),
            &serde_json::json!({
                "allowProjectPlugins": false,
                "records": [{
                    "pluginId": format!("{name}@0.1.0"),
                    "name": name,
                    "version": "0.1.0",
                    "enabled": enabled,
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

    #[cfg(unix)]
    fn write_failing_global_sidecar_binary(&self, name: &str) {
        use std::os::unix::fs::PermissionsExt;

        let binary = self
            .home
            .join("plugins/packages")
            .join(name)
            .join(format!("jyowo-plugin-{name}"));
        fs::write(&binary, "#!/bin/sh\nexit 91\n").expect("replace source sidecar");
        let mut permissions = fs::metadata(&binary)
            .expect("source sidecar metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(binary, permissions).expect("source sidecar executable");
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

fn route(
    kind: CapabilityRouteKind,
    config_id: &str,
    provider_id: &str,
    operations: &[&str],
) -> ProviderCapabilityRoute {
    ProviderCapabilityRoute {
        kind,
        config_id: config_id.into(),
        provider_id: provider_id.into(),
        operation_ids: operations
            .iter()
            .map(|operation| (*operation).into())
            .collect(),
        enabled: true,
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
