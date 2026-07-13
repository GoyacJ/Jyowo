#![cfg(feature = "testing")]

mod runtime_assembly_support;
use runtime_assembly_support::*;

#[cfg(feature = "steering-queue")]
#[test]
fn sdk_installs_steering_drain() {
    block_on(async {
        let workspace = unique_workspace("sdk-steering-drain");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(TestModelProvider::default().with_events(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("ok".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]));

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        session
            .push_steering(harness_session::SteeringRequest {
                kind: SteeringKind::Append,
                body: SteeringBody::Text("include release blockers".to_owned()),
                priority: None,
                correlation_id: None,
                source: SteeringSource::User,
            })
            .await
            .expect("steering should queue");

        session
            .run_turn("summarize audit")
            .await
            .expect("turn should run");

        let request_text = model
            .requests()
            .await
            .first()
            .expect("model should receive request")
            .messages
            .iter()
            .flat_map(|message| &message.parts)
            .filter_map(|part| match part {
                harness_contracts::MessagePart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(request_text.contains("summarize audit"));
        assert!(request_text.contains("include release blockers"));

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        let applied_at = events
            .iter()
            .position(|event| matches!(event, Event::SteeringMessageApplied(_)))
            .expect("steering applied event should be emitted");
        let assistant_at = events
            .iter()
            .position(|event| matches!(event, Event::AssistantMessageCompleted(_)))
            .expect("assistant completion should be emitted");
        assert!(applied_at < assistant_at);
    });
}

#[cfg(feature = "programmatic-tool-calling")]
#[test]
fn sdk_ptc_feature_propagates_to_engine() {
    let _builder = harness_engine::Engine::builder()
        .with_code_sandbox(Arc::new(harness_sandbox::MiniLuaCodeSandbox::new()));
}

#[test]
fn sdk_default_feature_profile_matches_architecture() {
    let manifest = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    )
    .expect("SDK manifest should be readable");
    let defaults = sdk_default_features(&manifest);

    for expected in [
        "sqlite-store",
        "jsonl-store",
        "local-sandbox",
        "interactive-permission",
        "mcp-stdio",
        "provider-anthropic",
        "tool-search",
        "steering-queue",
        "observability-redactor",
        "builtin-toolset",
    ] {
        assert!(
            defaults.contains(&expected.to_owned()),
            "SDK default features must include {expected}"
        );
    }

    for excluded in [
        "programmatic-tool-calling",
        "agents-subagent",
        "agents-team",
        "observability-otel",
        "observability-prometheus",
        "plugin-dynamic-load",
        "plugin-manifest-sign",
        "docker-sandbox",
        "ssh-sandbox",
    ] {
        assert!(
            !defaults.contains(&excluded.to_owned()),
            "SDK default features must not include high-risk feature {excluded}"
        );
    }
}

#[test]
fn sdk_default_profile_matches_architecture() {
    block_on(async {
        let workspace = unique_workspace("sdk-default-profile");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let tool_use_id = ToolUseId::new();
        let model = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: tool_use_id,
                        name: "tool_search".to_owned(),
                        input: json!({ "query": "select:FileRead" }),
                    },
                },
                ModelStreamEvent::MessageStop,
            ]),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("profile ready".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
        ]));

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .build()
            .await
            .expect("harness should build");

        let observer = harness
            .observer()
            .expect("default profile should install observer");
        let redacted = observer.redactor.redact(
            "token sk-abcdefghijklmnopqrstuvwxyz",
            &harness_contracts::RedactRules::default(),
        );
        assert!(!redacted.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(
            harness.elicitation_handler().is_some(),
            "default profile should install elicitation handler"
        );

        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_permission_mode(PermissionMode::BypassPermissions),
            )
            .await
            .expect("session should be created");
        #[cfg(feature = "steering-queue")]
        session
            .push_steering(harness_session::SteeringRequest {
                kind: SteeringKind::Append,
                body: SteeringBody::Text("default profile steering".to_owned()),
                priority: None,
                correlation_id: None,
                source: SteeringSource::User,
            })
            .await
            .expect("steering should queue");

        session
            .run_turn("exercise default profile")
            .await
            .expect("turn should run through engine");

        let requests = model.requests().await;
        let first_request = requests.first().expect("model should receive request");
        let tool_names = first_request
            .tools
            .as_ref()
            .expect("default profile should expose tools")
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>();
        for expected in ["FileRead", "ListDir", "Grep", "Bash", "tool_search"] {
            assert!(tool_names.contains(&expected));
        }
        #[cfg(feature = "steering-queue")]
        {
            let request_text = first_request
                .messages
                .iter()
                .flat_map(|message| &message.parts)
                .filter_map(|part| match part {
                    harness_contracts::MessagePart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(request_text.contains("default profile steering"));
        }

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        let (created_snapshot, created_hash) = events
            .iter()
            .find_map(|event| match event {
                Event::SessionCreated(created) => {
                    Some((created.snapshot_id, created.effective_config_hash))
                }
                _ => None,
            })
            .expect("session created event should exist");
        assert_ne!(created_snapshot, SnapshotId::from_u128(0));
        assert_ne!(created_hash.0, [0; 32]);
        let run_started = events
            .iter()
            .find_map(|event| match event {
                Event::RunStarted(run) => Some(run),
                _ => None,
            })
            .expect("run start event should exist");
        assert_ne!(run_started.snapshot_id, SnapshotId::from_u128(0));
        assert_eq!(run_started.effective_config_hash, created_hash);
        assert!(
            events.iter().any(|event| {
                matches!(event, Event::ToolSearchQueried(queried) if queried.tool_use_id == tool_use_id)
            }),
            "tool_search should be queried; events: {events:#?}"
        );
        assert!(events.iter().any(|event| {
            matches!(event, Event::ToolUseCompleted(completed) if completed.tool_use_id == tool_use_id)
        }));
        #[cfg(feature = "steering-queue")]
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::SteeringMessageApplied(_))));

        let compact_workspace = unique_workspace("sdk-default-profile-compact");
        std::fs::create_dir_all(&compact_workspace).unwrap();
        let compact_session_id = SessionId::new();
        let compact_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let compact_model = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Error(ModelError::ContextTooLong {
                tokens: 2_000,
                max: 100,
            }),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("compact ready".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
        ]));
        let compact_harness = Harness::builder()
            .with_workspace_root(&compact_workspace)
            .with_model_arc(compact_model)
            .with_store_arc(compact_store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("compact harness should build");
        let compact_session = compact_harness
            .create_session(
                SessionOptions::new(&compact_workspace).with_session_id(compact_session_id),
            )
            .await
            .expect("compact session should be created");
        compact_session
            .run_turn("force compact")
            .await
            .expect("compact fallback should run");
        let compact_events: Vec<_> = compact_store
            .read(
                TenantId::SINGLE,
                compact_session_id,
                ReplayCursor::FromStart,
            )
            .await
            .expect("compact events should be readable")
            .collect()
            .await;
        let compact_stages = compact_events
            .iter()
            .filter_map(|event| match event {
                Event::ContextStageTransitioned(stage) => Some(stage.stage.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            compact_stages,
            vec![
                ContextStageId::ToolResultBudget,
                ContextStageId::Snip,
                ContextStageId::Microcompact,
                ContextStageId::Collapse,
                ContextStageId::Autocompact,
            ]
        );
    });
}

#[test]
fn sdk_default_installs_builtin_toolset() {
    block_on(async {
        let workspace = unique_workspace("sdk-default-builtins");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("ready".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ])]));

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![]))
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect("session should be created");
        session
            .run_turn("show default tools")
            .await
            .expect("turn should complete");

        let requests = model.requests().await;
        let tool_names = requests[0]
            .tools
            .as_ref()
            .expect("default session should expose builtins")
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>();
        for expected in ["FileRead", "ListDir", "Grep", "Bash"] {
            assert!(
                tool_names.contains(&expected),
                "SDK default session should install builtin {expected}"
            );
        }
    });
}

#[test]
fn tool_search_uses_conversation_model_capabilities() {
    block_on(async {
        let workspace = unique_workspace("sdk-tool-search-provider-caps");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let tool_use_id = ToolUseId::new();
        let mut caps = ConversationModelCapability::default();
        caps.tool_calling = true;
        let model = Arc::new(CapabilityScriptedProvider::new(
            caps,
            vec![
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::ToolUseComplete {
                            id: tool_use_id,
                            name: "tool_search".to_owned(),
                            input: json!({ "query": "select:deferred_tool" }),
                        },
                    },
                    ModelStreamEvent::MessageStop,
                ],
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::Text("done".to_owned()),
                    },
                    ModelStreamEvent::MessageStop,
                ],
            ],
        ));
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .with_tool(Box::new(SdkPluginTool::new_deferred("deferred_tool")))
            .build()
            .expect("tool registry should build");

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_tool_registry(registry)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_model_id("test-model")
                    .with_tool_search_mode(ToolSearchMode::Always)
                    .with_permission_mode(PermissionMode::BypassPermissions),
            )
            .await
            .expect("session should be created");

        session
            .run_turn("load deferred tool")
            .await
            .expect("tool search should use provider-backed capabilities");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| {
            matches!(
                event,
                Event::ToolSchemaMaterialized(materialized)
                    if materialized.tool_use_id == tool_use_id
                        && materialized.backend == "inline_reinjection"
                        && materialized.names == vec!["deferred_tool".to_owned()]
            )
        }));
        assert!(!events.iter().any(|event| {
            matches!(event, Event::ToolUseFailed(failed) if failed.tool_use_id == tool_use_id)
        }));
    });
}

#[test]
fn tool_search_inline_reinjection_makes_deferred_schema_visible_to_next_turn_request() {
    block_on(async {
        let workspace = unique_workspace("sdk-tool-search-inline-reinjects");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let tool_use_id = ToolUseId::new();
        let mut caps = ConversationModelCapability::default();
        caps.tool_calling = true;
        let model = Arc::new(CapabilityScriptedProvider::new(
            caps,
            vec![
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::ToolUseComplete {
                            id: tool_use_id,
                            name: "tool_search".to_owned(),
                            input: json!({ "query": "select:deferred_tool" }),
                        },
                    },
                    ModelStreamEvent::MessageStop,
                ],
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::Text("done".to_owned()),
                    },
                    ModelStreamEvent::MessageStop,
                ],
            ],
        ));
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .with_tool(Box::new(SdkPluginTool::new_deferred("deferred_tool")))
            .build()
            .expect("tool registry should build");

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_tool_registry(registry)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_model_id("test-model")
                    .with_tool_search_mode(ToolSearchMode::Always)
                    .with_permission_mode(PermissionMode::BypassPermissions),
            )
            .await
            .expect("session should be created");

        session
            .run_turn("load deferred tool")
            .await
            .expect("inline reinjection should hot reload deferred tools");

        let requests = model.requests().await;
        let second_request_tools = requests
            .get(1)
            .and_then(|request| request.tools.as_ref())
            .expect("tool_search should trigger a follow-up model request with tools");
        assert!(
            second_request_tools
                .iter()
                .any(|tool| tool.name == "deferred_tool"),
            "inline reinjection must expose materialized deferred schema to the next request"
        );

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| {
            matches!(
                event,
                Event::ToolSchemaMaterialized(materialized)
                    if materialized.tool_use_id == tool_use_id
                        && materialized.backend == "inline_reinjection"
                        && materialized.names == vec!["deferred_tool".to_owned()]
                        && materialized.cache_impact.prompt_cache_invalidated
            )
        }));
    });
}

#[test]
fn tool_stream_deferred_pool_change_is_not_injected_into_next_sdk_turn() {
    block_on(async {
        let workspace = unique_workspace("sdk-deferred-delta-next-turn");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let tool_use_id = ToolUseId::new();
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::ToolUseComplete {
                            id: tool_use_id,
                            name: "emit_deferred_delta".to_owned(),
                            input: json!({}),
                        },
                    },
                    ModelStreamEvent::MessageStop,
                ],
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::Text("first done".to_owned()),
                    },
                    ModelStreamEvent::MessageStop,
                ],
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::Text("second done".to_owned()),
                    },
                    ModelStreamEvent::MessageStop,
                ],
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::Text("third done".to_owned()),
                    },
                    ModelStreamEvent::MessageStop,
                ],
            ],
        ));
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .with_tool(Box::new(DeferredDeltaEmitterTool::new("deferred_tool")))
            .with_tool(Box::new(SdkPluginTool::new_deferred("deferred_tool")))
            .build()
            .expect("tool registry should build");

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_tool_registry(registry)
            .build()
            .await
            .expect("harness should build");
        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_tool_search_mode(ToolSearchMode::Always),
            )
            .await
            .expect("session should be created");

        session
            .run_turn("discover deferred tools")
            .await
            .expect("first turn should emit deferred delta");
        session
            .run_turn("use deferred hint")
            .await
            .expect("second turn should receive deferred delta");
        session
            .run_turn("after hint consumed")
            .await
            .expect("third turn should not repeat deferred delta");

        let requests = model.requests().await;
        let second_turn_text = request_text(&requests[2]);
        assert!(!second_turn_text.contains("<deferred-tools"));
        assert!(!second_turn_text.contains("deferred_tool"));
        assert!(second_turn_text.contains("use deferred hint"));
        assert!(!request_text(&requests[3]).contains("<deferred-tools"));
    });
}

#[test]
fn tool_search_runtime_uses_conversation_model_capabilities() {
    tool_search_uses_conversation_model_capabilities();
}

#[test]
fn default_session_installs_tool_search_runtime_cap_when_tool_search_is_enabled() {
    block_on(async {
        let workspace = unique_workspace("sdk-tool-search-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let tool_use_id = ToolUseId::new();
        let model = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: tool_use_id,
                        name: "tool_search".to_owned(),
                        input: json!({ "query": "select:FileRead" }),
                    },
                },
                ModelStreamEvent::MessageStop,
            ]),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("done".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
        ]));

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_permission_mode(PermissionMode::BypassPermissions),
            )
            .await
            .expect("session should be created");

        session
            .run_turn("find file tools")
            .await
            .expect("tool search should execute through runtime cap");

        let requests = model.requests().await;
        let tool_names: Vec<_> = requests[0]
            .tools
            .as_ref()
            .expect("default session should expose tools")
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert!(tool_names.contains(&"tool_search"));

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| {
            matches!(event, Event::ToolUseRequested(requested) if requested.tool_name == "tool_search")
        }));
        assert!(events.iter().any(|event| {
            matches!(event, Event::ToolUseCompleted(completed) if completed.tool_use_id == tool_use_id)
        }));
        assert!(events.iter().any(|event| {
            matches!(event, Event::ToolSearchQueried(queried) if queried.tool_use_id == tool_use_id)
        }));
        assert!(!events.iter().any(|event| {
            matches!(event, Event::ToolUseFailed(failed) if failed.tool_use_id == tool_use_id)
        }));
    });
}

#[test]
fn default_session_installs_skill_registry_cap_when_skill_loader_is_configured() {
    block_on(async {
        let workspace = unique_workspace("sdk-skill-registry-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let tool_use_id = ToolUseId::new();
        let model = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: tool_use_id,
                        name: "skills_list".to_owned(),
                        input: json!({}),
                    },
                },
                ModelStreamEvent::MessageStop,
            ]),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("done".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
        ]));
        let loader = SkillLoader::default().with_source(SkillSourceConfig::BundledRecords {
            records: vec![BundledSkillRecord {
                name: "brief".to_owned(),
                description: "Write brief output.".to_owned(),
                body: "Keep the answer short.".to_owned(),
            }],
        });

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_skill_loader(loader)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_permission_mode(PermissionMode::BypassPermissions),
            )
            .await
            .expect("session should be created");

        session
            .run_turn("list skills")
            .await
            .expect("skills_list should execute through SkillRegistryCap");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| {
            matches!(
                event,
                Event::ToolUseCompleted(completed)
                    if completed.tool_use_id == tool_use_id
                        && format!("{:?}", completed.result).contains("brief")
            )
        }));
    });
}

#[test]
fn conversation_session_created_event_precedes_skill_loader_events() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-skill-event-order");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let loader = SkillLoader::default().with_source(SkillSourceConfig::BundledRecords {
            records: vec![BundledSkillRecord {
                name: "brief".to_owned(),
                description: "Write brief output.".to_owned(),
                body: "Keep the answer short.".to_owned(),
            }],
        });

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_skill_loader(loader)
            .build()
            .await
            .expect("harness should build");

        harness
            .open_or_create_conversation_session(
                SessionOptions::new(&workspace).with_session_id(session_id),
            )
            .await
            .expect("conversation session should be created");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(matches!(events.first(), Some(Event::SessionCreated(_))));
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::SkillLoaded(_))));

        let sessions = harness
            .list_conversation_sessions(TenantId::SINGLE, 50)
            .await
            .expect("conversation sessions should list");
        assert!(sessions
            .iter()
            .any(|session| session.session_id == session_id));
    });
}

#[test]
fn skill_hooks_register_into_hook_registry() {
    block_on(async {
        let workspace = unique_workspace("sdk-skill-hook-registry");
        std::fs::create_dir_all(&workspace).unwrap();
        let skill_dir = workspace.join("skills");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("audit.md"),
            r#"---
name: audit
description: Audited skill.
hooks:
  - id: start
    events: [SessionStart]
    transport:
      type: builtin
      kind: AuditLog
---
unused body
"#,
        )
        .unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let loader = SkillLoader::default().with_source(SkillSourceConfig::Directory {
            path: skill_dir,
            source_kind: harness_skill::DirectorySourceKind::Workspace,
        });

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_skill_loader(loader)
            .build()
            .await
            .expect("harness should build");

        let _session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;

        assert!(events.iter().any(|event| {
            matches!(event, Event::HookTriggered(triggered)
                if triggered.handler_id.starts_with("skill:audit:start:")
                    && triggered.hook_event_kind == HookEventKind::SessionStart)
        }));
    });
}

#[tokio::test]
async fn workspace_hook_reload_replaces_old_handler_after_new_handler_registers() {
    let workspace = unique_workspace("sdk-skill-hook-replacement");
    std::fs::create_dir_all(&workspace).unwrap();
    let enabled = workspace.join("enabled");
    let package = enabled.join("audit-package");
    std::fs::create_dir_all(&package).unwrap();
    write_package_hook(&package, "events: [SessionStart]");
    let hook_registry = HookRegistry::builder().build().unwrap();
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model(TestModelProvider::default())
        .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_sandbox(NoopSandbox::new())
        .with_hook_registry(hook_registry.clone())
        .build()
        .await
        .expect("harness should build");

    harness
        .reload_workspace_managed_skills(&enabled)
        .await
        .expect("initial hook should load");
    let old_id = hook_registry
        .snapshot()
        .handlers_for(HookEventKind::SessionStart)[0]
        .handler_id()
        .to_owned();

    write_package_hook(&package, "events: [PostToolUse]");
    harness
        .reload_workspace_managed_skills(&enabled)
        .await
        .expect("changed hook should load");

    assert!(hook_registry
        .snapshot()
        .handlers_for(HookEventKind::SessionStart)
        .is_empty());
    let new_id = hook_registry
        .snapshot()
        .handlers_for(HookEventKind::PostToolUse)[0]
        .handler_id()
        .to_owned();
    assert_ne!(new_id, old_id);
}

#[tokio::test]
async fn failed_hook_replacement_keeps_old_handler_and_registry_snapshot() {
    let workspace = unique_workspace("sdk-skill-hook-replacement-rollback");
    std::fs::create_dir_all(&workspace).unwrap();
    let hook_registry = HookRegistry::builder().build().unwrap();
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model(TestModelProvider::default())
        .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_sandbox(NoopSandbox::new())
        .with_hook_registry(hook_registry.clone())
        .build()
        .await
        .expect("harness should build");
    let session = harness
        .create_session(SessionOptions::new(&workspace))
        .await
        .expect("session should build");
    let source = SkillSource::Plugin {
        plugin_id: PluginId("trusted-plugin".to_owned()),
        trust: TrustLevel::AdminTrusted,
    };
    let initial = skill_registration_from(
        r"---
name: audit
description: Audit skill
hooks:
  - id: start
    events: [SessionStart]
    transport:
      type: builtin
      kind: AuditLog
---
Body
",
        source.clone(),
    );
    let outcome = session
        .reload_with(ConfigDelta::for_tenant(TenantId::SINGLE).add_skill(initial))
        .await
        .expect("initial reload should finish");
    assert_eq!(outcome.mode, ReloadMode::AppliedInPlace);
    let before = harness.skill_registry().snapshot();
    let old_id = hook_registry
        .snapshot()
        .handlers_for(HookEventKind::SessionStart)[0]
        .handler_id()
        .to_owned();

    let mut invalid = skill_registration_from(
        r"---
name: audit
description: Changed audit skill
hooks:
  - id: start
    events: [PostToolUse]
    transport:
      type: builtin
      kind: AuditLog
---
Body
",
        source,
    );
    invalid.skill.frontmatter.hooks[0].events.clear();
    let outcome = session
        .reload_with(ConfigDelta::for_tenant(TenantId::SINGLE).add_skill(invalid))
        .await
        .expect("invalid reload should return outcome");

    assert!(matches!(outcome.mode, ReloadMode::Rejected { .. }));
    assert_eq!(
        harness.skill_registry().snapshot().generation,
        before.generation
    );
    assert!(hook_registry.origin_for(&old_id).is_some());
    assert_eq!(
        harness.skill_registry().get("audit").unwrap().description,
        "Audit skill"
    );
}

#[tokio::test]
async fn sdk_hook_binding_explicitly_rejects_mtls_when_loader_is_bypassed() {
    let workspace = unique_workspace("sdk-skill-mtls-bypass");
    std::fs::create_dir_all(&workspace).unwrap();
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model(TestModelProvider::default())
        .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");
    let skill = harness_skill::parse_skill_markdown(
        r##"---
name: mtls
description: mTLS hook
hooks:
  - id: webhook
    events: [PostToolUse]
    transport:
      type: http
      url: https://hooks.example.test/audit
      security:
        allowlist: ["hooks.example.test"]
        mtls_required: true
---
Body
"##,
        SkillSource::Plugin {
            plugin_id: PluginId("trusted-plugin".to_owned()),
            trust: TrustLevel::AdminTrusted,
        },
        None,
        SkillPlatform::Macos,
    )
    .unwrap();
    harness.skill_registry().register(skill).unwrap();

    let error = harness
        .create_session(SessionOptions::new(&workspace))
        .await
        .expect_err("SDK binding must reject mTLS without a certificate source");

    assert!(error.to_string().contains("mTLS"));
}

#[test]
fn turn_renderer_uses_skill_loader_render_policy() {
    block_on(async {
        let workspace = unique_workspace("sdk-skill-render-policy");
        std::fs::create_dir_all(&workspace).unwrap();
        let tool_use_id = ToolUseId::new();
        let model = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: tool_use_id,
                        name: "skills_invoke".to_owned(),
                        input: json!({ "name": "policy", "params": {} }),
                    },
                },
                ModelStreamEvent::MessageStop,
            ]),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("done".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
        ]));
        let loader = SkillLoader::default()
            .with_source(SkillSourceConfig::BundledRecords {
                records: vec![BundledSkillRecord {
                    name: "policy".to_owned(),
                    description: "Render policy".to_owned(),
                    body: "Value: !`printf policy`.".to_owned(),
                }],
            })
            .with_shell_allowlist(["printf".to_owned()]);
        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model.clone())
            .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_skill_loader(loader)
            .build()
            .await
            .expect("harness should build");
        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_permission_mode(PermissionMode::BypassPermissions),
            )
            .await
            .expect("session should build");

        session
            .run_turn("invoke policy")
            .await
            .expect("turn should run");

        let requests = model.requests().await;
        assert!(request_text(&requests[1]).contains("Value: policy."));
        assert!(!request_text(&requests[1]).contains("[SHELL_NOT_ALLOWED]"));
    });
}

fn write_package_hook(package: &std::path::Path, events: &str) {
    std::fs::write(
        package.join("SKILL.md"),
        format!(
            r"---
name: audit
description: Audited skill
hooks:
  - id: start
    {events}
    transport:
      type: builtin
      kind: AuditLog
---
Body
"
        ),
    )
    .unwrap();
}

#[tokio::test]
async fn reload_rejects_invalid_skill_and_keeps_registry_generation() {
    let workspace = unique_workspace("sdk-skill-reload-validation");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model(TestModelProvider::default())
        .with_store_arc(store)
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");
    let session = harness
        .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
        .await
        .expect("session should be created");
    let before = harness.skill_registry().snapshot().generation;

    let outcome = session
        .reload_with(
            ConfigDelta::for_tenant(TenantId::SINGLE).add_skill(skill_registration_from(
                r"---
name: unsafe-reload
description: Unsafe reload
hooks:
  - id: audit
    events: [SessionStart]
    transport:
      type: exec
      command: /usr/local/bin/audit
---
Body
",
                SkillSource::User("home/skills".into()),
            )),
        )
        .await
        .expect("reload should return outcome");

    assert!(matches!(outcome.mode, ReloadMode::Rejected { .. }));
    assert_eq!(harness.skill_registry().snapshot().generation, before);
}

#[tokio::test]
async fn running_turn_uses_snapshot_captured_before_skill_reload() {
    let workspace = unique_workspace("sdk-skill-turn-snapshot");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let tool_use_id = ToolUseId::new();
    let model = Arc::new(BlockingSkillListProvider::new(tool_use_id));
    let loader = SkillLoader::default().with_source(SkillSourceConfig::BundledRecords {
        records: vec![BundledSkillRecord {
            name: "old-skill".to_owned(),
            description: "Old skill.".to_owned(),
            body: "Old body.".to_owned(),
        }],
    });

    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model_arc(model.clone())
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
        .with_skill_loader(loader)
        .build()
        .await
        .expect("harness should build");
    let session = harness
        .create_session(
            SessionOptions::new(&workspace)
                .with_session_id(session_id)
                .with_permission_mode(PermissionMode::BypassPermissions),
        )
        .await
        .expect("session should be created");

    let run_turn = session.run_turn("list skills");
    let reload = async {
        model.started.notified().await;
        let outcome = session
            .reload_with(ConfigDelta::for_tenant(TenantId::SINGLE).add_skill(
                skill_registration_from(
                    r"---
name: new-skill
description: New skill.
---
New body.
",
                    SkillSource::Workspace("data/skills".into()),
                ),
            ))
            .await
            .expect("reload should return outcome");
        model.release.notify_waiters();
        outcome
    };
    let (turn_result, reload_outcome) = tokio::join!(run_turn, reload);
    turn_result.expect("turn should run");
    assert_eq!(reload_outcome.mode, ReloadMode::AppliedInPlace);

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("events should be readable")
        .collect()
        .await;
    let completed = events
        .iter()
        .find_map(|event| match event {
            Event::ToolUseCompleted(completed) if completed.tool_use_id == tool_use_id => {
                Some(format!("{:?}", completed.result))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("skills_list should complete; events: {events:#?}"));

    assert!(completed.contains("old-skill"));
    assert!(!completed.contains("new-skill"));
}
