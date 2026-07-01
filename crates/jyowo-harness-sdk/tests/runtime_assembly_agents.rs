#![cfg(feature = "testing")]

mod runtime_assembly_support;
use runtime_assembly_support::*;

#[test]
fn plugins_are_activated_before_session_runtime_assembly() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-plugin-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(TestModelProvider::default().with_events(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("done".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]));
        let manifest = plugin_manifest("runtime-plugin");
        let plugin: Arc<dyn Plugin> = Arc::new(RuntimePlugin {
            manifest: manifest.manifest.clone(),
            session_id,
        });
        let runtime = StaticLinkRuntimeLoader::default()
            .with_plugin(plugin_id("runtime-plugin"), plugin);
        let plugin_registry = PluginRegistry::builder()
            .with_config(PluginConfig {
                allow_project_plugins: true,
                ..PluginConfig::default()
            })
            .with_source(DiscoverySource::Project("/workspace".into()))
            .with_manifest_loader(Arc::new(SdkStaticManifestLoader {
                records: vec![manifest],
            }))
            .with_runtime_loader(Arc::new(runtime))
            .build()
            .expect("plugin registry should build");

        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_plugin_registry(plugin_registry)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        session
            .run_turn("assemble plugin runtime")
            .await
            .expect("turn should run");

        let requests = model.requests().await;
        let tool_names: Vec<_> = requests[0]
            .tools
            .as_ref()
            .expect("plugin tool should be exposed")
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert!(tool_names.contains(&"plugin-tool"));

        let request_text = requests[0]
            .messages
            .iter()
            .flat_map(|message| &message.parts)
            .filter_map(|part| match part {
                harness_contracts::MessagePart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(request_text.contains("plugin memory is active"));

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| {
            matches!(event, Event::PluginLoaded(loaded) if loaded.plugin_id == plugin_id("runtime-plugin"))
        }));
        let encoded_events = serde_json::to_string(&events).unwrap();
        assert!(!encoded_events.contains("/plugins/runtime-plugin"));
    });
}

#[test]
fn plugin_mcp_servers_are_injected_into_session_tool_pool() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-plugin-mcp-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(TestModelProvider::default().with_events(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("done".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]));
        let manifest = plugin_mcp_manifest("project-mcp-plugin");
        let plugin: Arc<dyn Plugin> = Arc::new(McpRuntimePlugin {
            manifest: manifest.manifest.clone(),
        });
        let runtime =
            StaticLinkRuntimeLoader::default().with_plugin(plugin_id("project-mcp-plugin"), plugin);
        let plugin_registry = PluginRegistry::builder()
            .with_config(PluginConfig {
                allow_project_plugins: true,
                ..PluginConfig::default()
            })
            .with_source(DiscoverySource::Project("/workspace".into()))
            .with_manifest_loader(Arc::new(SdkStaticManifestLoader {
                records: vec![manifest],
            }))
            .with_runtime_loader(Arc::new(runtime))
            .build()
            .expect("plugin registry should build");

        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(store)
            .with_sandbox(NoopSandbox::new())
            .with_plugin_registry(plugin_registry)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        session
            .run_turn("assemble plugin MCP runtime")
            .await
            .expect("turn should run");

        let requests = model.requests().await;
        let tool_names: Vec<_> = requests[0]
            .tools
            .as_ref()
            .expect("plugin MCP tool should be exposed")
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert!(tool_names.contains(&"mcp__plugin-mcp__echo"));
    });
}

#[test]
fn disabled_plugins_are_discovered_without_session_auto_activation() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-disabled-plugin");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let manifest = plugin_manifest("disabled-plugin");
        let runtime = StaticLinkRuntimeLoader::default().with_plugin(
            plugin_id("disabled-plugin"),
            Arc::new(FailingRuntimePlugin {
                manifest: manifest.manifest.clone(),
                failure: "disabled plugin should not activate".to_owned(),
            }),
        );
        let plugin_registry = PluginRegistry::builder()
            .with_config(PluginConfig {
                allow_project_plugins: true,
                disabled_plugins: BTreeSet::from([PluginName::new("disabled-plugin").unwrap()]),
                ..PluginConfig::default()
            })
            .with_source(DiscoverySource::Project("/workspace".into()))
            .with_manifest_loader(Arc::new(SdkStaticManifestLoader {
                records: vec![manifest],
            }))
            .with_runtime_loader(Arc::new(runtime))
            .build()
            .expect("plugin registry should build");

        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_plugin_registry(plugin_registry)
            .build()
            .await
            .expect("harness should build");

        harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("disabled plugin should not block session creation");

        let registry = harness
            .plugin_registry()
            .expect("plugin registry should remain available");
        assert!(matches!(
            registry.state(&plugin_id("disabled-plugin")),
            Some(harness_plugin::PluginLifecycleState::Deactivated)
        ));
        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(!events.iter().any(|event| matches!(
            event,
            Event::PluginLoaded(loaded) if loaded.plugin_id == plugin_id("disabled-plugin")
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            Event::PluginFailed(failed) if failed.plugin_id == plugin_id("disabled-plugin")
        )));
    });
}

#[test]
fn plugin_discovery_rejection_records_replay_event() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-plugin-discovery-rejected");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let manifest = plugin_manifest("discovery-rejected-plugin");
        let plugin_registry = PluginRegistry::builder()
            .with_config(PluginConfig {
                allow_project_plugins: true,
                policy: PluginAdmissionPolicy::Allow(BTreeSet::from([PluginName::new(
                    "allowed-plugin",
                )
                .unwrap()])),
                ..PluginConfig::default()
            })
            .with_source(DiscoverySource::Project("/workspace".into()))
            .with_manifest_loader(Arc::new(SdkStaticManifestLoader {
                records: vec![manifest],
            }))
            .build()
            .expect("plugin registry should build");

        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_plugin_registry(plugin_registry)
            .build()
            .await
            .expect("harness should build");

        harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("discovery rejection should not block session creation");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| {
            matches!(event, Event::PluginRejected(rejected)
                if rejected.plugin_id == plugin_id("discovery-rejected-plugin"))
        }));
        let encoded_events = serde_json::to_string(&events).unwrap();
        assert!(!encoded_events.contains("/plugins/discovery-rejected-plugin"));
    });
}

#[test]
fn plugin_activation_failure_records_failed_event_without_raw_error() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-plugin-failed-event");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let manifest = plugin_manifest("failed-plugin");
        let plugin: Arc<dyn Plugin> = Arc::new(FailingRuntimePlugin {
            manifest: manifest.manifest.clone(),
            failure: "sidecar crashed with Authorization=Bearer plugin-secret-token".to_owned(),
        });
        let runtime =
            StaticLinkRuntimeLoader::default().with_plugin(plugin_id("failed-plugin"), plugin);
        let plugin_registry = PluginRegistry::builder()
            .with_config(PluginConfig {
                allow_project_plugins: true,
                ..PluginConfig::default()
            })
            .with_source(DiscoverySource::Project("/workspace".into()))
            .with_manifest_loader(Arc::new(SdkStaticManifestLoader {
                records: vec![manifest],
            }))
            .with_runtime_loader(Arc::new(runtime))
            .build()
            .expect("plugin registry should build");

        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_plugin_registry(plugin_registry)
            .build()
            .await
            .expect("harness should build");

        let error = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect_err("plugin activation failure should stop session creation");
        let error = error.to_string();
        assert!(error.contains("Plugin activation failed."));
        assert!(!error.contains("sidecar crashed"));
        assert!(!error.contains("plugin-secret-token"));
        assert!(!error.contains("Authorization"));

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| {
            matches!(event, Event::PluginFailed(failed)
                if failed.plugin_id == plugin_id("failed-plugin")
                    && failed.failure == "Plugin failure withheld from conversation timeline.")
        }));
        let encoded_events = serde_json::to_string(&events).unwrap();
        assert!(!encoded_events.contains("plugin-secret-token"));
        assert!(!encoded_events.contains("Authorization"));
        assert!(!encoded_events.contains("/plugins/failed-plugin"));
    });
}

#[test]
fn plugin_manifest_validation_records_real_hash() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-plugin-manifest-validation");
        let plugin_dir = workspace.join(".jyowo/plugins/bad-plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        let raw_manifest = r#"{
  "manifest_schema_version": 1,
  "name": "bad-plugin",
  "version": "0.1.0",
  "capabilities": {}
}"#;
        std::fs::write(plugin_dir.join("plugin.json"), raw_manifest).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let sink = Arc::new(RecordingPluginEventSink::default());
        let plugin_registry = PluginRegistry::builder()
            .with_config(PluginConfig {
                allow_project_plugins: true,
                ..PluginConfig::default()
            })
            .with_source(DiscoverySource::Project(workspace.clone()))
            .with_event_sink(sink.clone())
            .build()
            .expect("plugin registry should build");

        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_plugin_registry(plugin_registry)
            .build()
            .await
            .expect("harness should build");

        let _session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("invalid plugin manifest should be skipped after recording validation event");

        let events = sink.events();
        assert!(events.iter().any(|event| matches!(
            event,
            Event::ManifestValidationFailed(failed)
                if failed.partial_name.as_deref() == Some("bad-plugin")
                    && failed.partial_version.as_deref() == Some("0.1.0")
                    && failed.raw_bytes_hash != [0; 32]
        )));
        let replay_events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(replay_events.iter().any(|event| matches!(
            event,
            Event::ManifestValidationFailed(failed)
                if failed.partial_name.as_deref() == Some("bad-plugin")
                    && failed.partial_version.as_deref() == Some("0.1.0")
                    && failed.raw_bytes_hash != [0; 32]
        )));
    });
}

#[test]
fn plugin_manifest_validation_preserves_typed_failure() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-plugin-typed-validation");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let plugin_registry = PluginRegistry::builder()
            .with_manifest_loader(Arc::new(SdkFailingManifestLoader))
            .build()
            .expect("plugin registry should build");

        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_plugin_registry(plugin_registry)
            .build()
            .await
            .expect("harness should build");

        let error = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect_err("discovery validation error should stop session creation");
        let error = error.to_string();
        assert!(error.contains("Plugin discovery failed."));
        assert!(!error.contains("manifest loader"));
        assert!(!error.contains("/plugins/typed-bad"));
        assert!(!error.contains("expected object"));

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| matches!(
            event,
            Event::ManifestValidationFailed(failed)
                if failed.partial_name.as_deref() == Some("typed-bad")
                    && matches!(
                        failed.failure,
                        ContractManifestValidationFailure::SchemaViolation { .. }
                    )
        )));
        let encoded_events = serde_json::to_string(&events).unwrap();
        assert!(!encoded_events.contains("/plugins/typed-bad"));
        assert!(!encoded_events.contains("expected object"));
    });
}

#[cfg(feature = "agents-subagent")]
#[test]
fn default_session_installs_agent_tool_when_subagent_runner_is_configured() {
    block_on(async {
        let workspace = unique_workspace("sdk-agent-tool-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(TestModelProvider::default().with_events(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("ready".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]));

        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_subagent_runner(Arc::new(ReadySubagentRunner))
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect("session should be created");
        session
            .run_turn("delegate later")
            .await
            .expect("turn should run");

        let requests = model.requests().await;
        let tool_names: Vec<_> = requests[0]
            .tools
            .as_ref()
            .expect("default session should expose tools")
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert!(tool_names.contains(&"agent"));
    });
}

#[cfg(all(feature = "testing", feature = "agents-team"))]
mod team_prompt_addendum {
    use std::sync::Arc;

    use futures::executor::block_on;
    use harness_contracts::{
        AgentId, CorrelationId, Decision, Message, MessageId, MessagePart, MessageRole, RunId,
        SessionId, TeamId, TenantId, TurnInput,
    };
    use harness_engine::{Engine, EngineId};
    use harness_hook::{HookDispatcher, HookRegistry};
    use harness_model::{ContentDelta, ModelStreamEvent};
    use harness_team::{TeamMemberEngineConfig, TeamMemberRunRequest, TeamMemberRunner};
    use harness_tool::ToolPool;
    use jyowo_harness_sdk::{testing::*, EngineTeamMemberRunner};

    #[test]
    fn team_member_system_prompt_addendum_renders_as_session_addendum() {
        block_on(async {
            let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
            let model = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("member answer".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ])]));
            let base_prompt = "<jyowo-system>\nBase team parent prompt.\n</jyowo-system>";
            let engine = Arc::new(
                Engine::builder()
                    .with_engine_id(EngineId::new("team-addendum-test"))
                    .with_event_store(store)
                    .with_context(harness_context::ContextEngine::builder().build().unwrap())
                    .with_hooks(HookDispatcher::new(
                        HookRegistry::builder().build().unwrap().snapshot(),
                    ))
                    .with_model(model.clone())
                    .with_tools(ToolPool::default())
                    .with_permission_broker(Arc::new(TestBroker::new(vec![Decision::AllowOnce])))
                    .with_workspace_root(std::env::temp_dir())
                    .with_model_id("test-model")
                    .with_system_prompt(Some(base_prompt.to_owned()))
                    .build()
                    .unwrap(),
            );
            let runner = EngineTeamMemberRunner::new(engine);
            let session_id = SessionId::new();
            let mut config = TeamMemberEngineConfig::default();
            config.system_prompt_addendum = Some("Team member constraint.".to_owned());
            let request = TeamMemberRunRequest::synthetic(
                TenantId::SINGLE,
                TeamId::new(),
                AgentId::new(),
                "researcher",
                session_id,
                RunId::new(),
                None,
                TurnInput {
                    message: Message {
                        id: MessageId::new(),
                        role: MessageRole::User,
                        parts: vec![MessagePart::Text("dispatch goal".to_owned())],
                        created_at: harness_contracts::now(),
                    },
                    metadata: serde_json::Value::Null,
                },
                "dispatch goal",
                CorrelationId::new(),
                config,
            );
            runner.run_member(request).await.expect("member run");
            let system = model.requests().await[0].system.clone().unwrap_or_default();
            assert!(system.starts_with(base_prompt));
            assert!(system.contains("<session-addendum>"));
            assert!(system.contains("Team member constraint."));
            assert!(!system.contains("AI 编程伙伴"));
        });
    }
}
