#![cfg(feature = "testing")]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use futures::{executor::block_on, stream, StreamExt};
use harness_contracts::{
    ContextPatchSource, ContextStageId, Decision, DeferPolicy, DeferredToolHint, EndReason, Event,
    HookEventKind, ManifestValidationFailure as ContractManifestValidationFailure, McpServerId,
    McpServerSource, MemoryError, MemoryId, MemoryKind, MemorySessionCtx, MemorySource,
    MemoryVisibility, MessageId, MessagePart, ModelError, PluginId, ProviderRestriction,
    RedactRules, Redactor, RequestId, SessionSummaryView, SnapshotId, SteeringBody, SteeringKind,
    SteeringSource, TeamId, TenantId, ToolDeferredPoolChangedEvent, ToolDescriptor, ToolGroup,
    ToolOrigin, ToolPoolChangeSource, ToolProperties, ToolResult, ToolSearchMode, ToolUseId,
    TrustLevel, UsageSnapshot,
};
use harness_hook::HookRegistry;
use harness_journal::{EventStore, ReplayCursor};
use harness_mcp::{
    McpConnection, McpConnectionState, McpError, McpRegistry, McpServerScope, McpServerSpec,
    McpToolDescriptor, McpToolResult, SamplingRequest, TransportChoice,
};
#[cfg(feature = "memory-consolidation")]
use harness_memory::{ConsolidationHook, ConsolidationOutcome};
use harness_memory::{MemoryLifecycle, MemoryMetadata, MemoryRecord, MemoryStore};
use harness_model::{
    ContentDelta, HealthStatus, InferContext, ModelCapabilities, ModelDescriptor, ModelProvider,
    ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_observability::{
    AttributeValue, InMemorySpan, Observer, Span, SpanAttributes, TraceCarrier, TraceContext,
    Tracer,
};
use harness_plugin::{
    DiscoverySource, ManifestLoaderError, ManifestOrigin, ManifestRecord, Plugin,
    PluginActivationContext, PluginActivationResult, PluginCapabilities, PluginConfig, PluginError,
    PluginEventSink, PluginManifest, PluginManifestLoader, PluginName, PluginRegistry,
    StaticLinkRuntimeLoader,
};
use harness_session::{ConfigDelta, ReloadMode};
use harness_skill::{
    BundledSkillRecord, SkillLoader, SkillPlatform, SkillRegistration, SkillSource,
    SkillSourceConfig,
};
use harness_tool::{
    default_result_budget, BuiltinToolset, PermissionCheck, SchemaResolverContext, Tool,
    ToolContext, ToolEvent, ToolRegistry, ToolStream, ValidationError,
};
use jyowo_harness_sdk::{prelude::*, testing::*};
use serde_json::json;
use serde_json::Value;
use tokio::sync::Notify;

#[test]
fn knowledge_retrieval_context_patch_source_has_sdk_facing_shape() {
    let source = ContextPatchSource::KnowledgeRetrieval {
        provider_id: "knowledge-runtime".to_owned(),
        knowledge_base_ids: vec!["kb-runtime".to_owned()],
        reference_chunk_count: 2,
    };

    let value = serde_json::to_value(source).expect("context patch source serializes");

    assert_eq!(value["type"], "knowledge_retrieval");
    assert_eq!(value["provider_id"], "knowledge-runtime");
    assert_eq!(value["knowledge_base_ids"][0], "kb-runtime");
    assert_eq!(value["reference_chunk_count"], 2);
}

#[test]
fn create_session_uses_engine_runtime_path() {
    block_on(async {
        let workspace = unique_workspace("sdk-engine-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

        let harness = Harness::builder()
            .with_model(MockProvider::default().with_events(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("engine delta".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]))
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
            .run_turn("prove engine path")
            .await
            .expect("turn should run");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;

        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::AssistantDeltaProduced(delta) if delta.message_id != MessageId::from_u128(0))),
            "SDK-created sessions must emit streaming assistant deltas from the Engine path"
        );
    });
}

#[test]
fn session_lifecycle_hooks_are_triggered() {
    block_on(async {
        let workspace = unique_workspace("sdk-session-lifecycle-hooks");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let hook_registry = HookRegistry::builder()
            .with_hook(Box::new(MockHookHandler::new(
                "session-lifecycle",
                vec![
                    HookEventKind::Setup,
                    HookEventKind::SessionStart,
                    HookEventKind::SessionEnd,
                ],
            )))
            .build()
            .expect("hook registry should build");

        let harness = Harness::builder()
            .with_model(MockProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_hook_registry(hook_registry)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        session
            .end(EndReason::Completed)
            .await
            .expect("session should end");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;

        for expected in [
            HookEventKind::Setup,
            HookEventKind::SessionStart,
            HookEventKind::SessionEnd,
        ] {
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    Event::HookTriggered(triggered)
                        if triggered.hook_event_kind == expected
                            && triggered.handler_id == "session-lifecycle"
                )),
                "missing {expected:?}"
            );
        }
    });
}

#[test]
fn default_session_uses_config_snapshot_hashes() {
    block_on(async {
        let workspace = unique_workspace("sdk-config-hashes");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

        let harness = Harness::builder()
            .with_model(MockProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
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

        let created = events
            .iter()
            .find_map(|event| match event {
                Event::SessionCreated(created) => Some(created),
                _ => None,
            })
            .expect("session creation event should be emitted");

        assert_ne!(created.options_hash, [0; 32]);
        assert_ne!(created.effective_config_hash.0, [0; 32]);
    });
}

#[test]
fn run_started_uses_non_zero_config_snapshot() {
    block_on(async {
        let workspace = unique_workspace("sdk-run-started-config-snapshot");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

        let harness = Harness::builder()
            .with_model(MockProvider::default().with_events(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("ok".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]))
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");

        session.run_turn("hello").await.expect("turn should run");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        let created_hash = events
            .iter()
            .find_map(|event| match event {
                Event::SessionCreated(created) => Some(created.effective_config_hash),
                _ => None,
            })
            .expect("session creation event should be emitted");
        let run_started = events
            .iter()
            .find_map(|event| match event {
                Event::RunStarted(started) => Some(started),
                _ => None,
            })
            .expect("run start event should be emitted");

        assert_ne!(run_started.snapshot_id, SnapshotId::from_u128(0));
        assert_ne!(run_started.effective_config_hash.0, [0; 32]);
        assert_eq!(run_started.effective_config_hash, created_hash);
    });
}

#[cfg(feature = "steering-queue")]
#[test]
fn sdk_installs_steering_drain() {
    block_on(async {
        let workspace = unique_workspace("sdk-steering-drain");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(MockProvider::default().with_events(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("ok".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]));

        let harness = Harness::builder()
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
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(MockBroker::new(vec![Decision::AllowOnce]))
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
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
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
        assert!(events.iter().any(|event| {
            matches!(event, Event::ToolSearchQueried(queried) if queried.tool_use_id == tool_use_id)
        }));
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
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(MockBroker::new(vec![]))
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
fn tool_search_uses_provider_capabilities() {
    block_on(async {
        let workspace = unique_workspace("sdk-tool-search-provider-caps");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let tool_use_id = ToolUseId::new();
        let mut caps = ModelCapabilities::default();
        caps.supports_tool_reference = true;
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
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(MockBroker::new(vec![Decision::AllowOnce]))
            .with_tool_registry(registry)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_model_id("mock-model")
                    .with_tool_search_mode(ToolSearchMode::Always),
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
                        && materialized.backend == "anthropic_tool_reference"
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
        let mut caps = ModelCapabilities::default();
        caps.supports_tool_reference = false;
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
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(MockBroker::new(vec![Decision::AllowOnce]))
            .with_tool_registry(registry)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_model_id("mock-model")
                    .with_tool_search_mode(ToolSearchMode::Always),
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
fn deferred_pool_change_is_injected_into_next_sdk_turn_once() {
    block_on(async {
        let workspace = unique_workspace("sdk-deferred-delta-next-turn");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let tool_use_id = ToolUseId::new();
        let model = Arc::new(CapabilityScriptedProvider::new(
            ModelCapabilities::default(),
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
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(MockBroker::new(vec![Decision::AllowOnce]))
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
        assert!(second_turn_text.contains("<deferred-tools initial=\"true\""));
        assert!(second_turn_text.contains("deferred_tool"));
        assert!(second_turn_text.contains("use deferred hint"));
        assert!(!request_text(&requests[3]).contains("<deferred-tools"));
    });
}

#[test]
fn tool_search_runtime_is_provider_backed() {
    tool_search_uses_provider_capabilities();
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
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(MockBroker::new(vec![Decision::AllowOnce]))
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
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
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(MockBroker::new(vec![Decision::AllowOnce]))
            .with_skill_loader(loader)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
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
            .with_model(MockProvider::default())
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
                if triggered.handler_id == "skill:audit:start"
                    && triggered.hook_event_kind == HookEventKind::SessionStart)
        }));
    });
}

#[tokio::test]
async fn reload_rejects_invalid_skill_and_keeps_registry_generation() {
    let workspace = unique_workspace("sdk-skill-reload-validation");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

    let harness = Harness::builder()
        .with_model(MockProvider::default())
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
        .with_model_arc(model.clone())
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .with_permission_broker(MockBroker::new(vec![Decision::AllowOnce]))
        .with_skill_loader(loader)
        .build()
        .await
        .expect("harness should build");
    let session = harness
        .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
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
        .expect("skills_list should complete");

    assert!(completed.contains("old-skill"));
    assert!(!completed.contains("new-skill"));
}

#[test]
fn mcp_tools_are_injected_into_default_session() {
    block_on(async {
        let workspace = unique_workspace("sdk-mcp-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let server_id = McpServerId("fixture".into());
        let mcp_registry = McpRegistry::new();
        mcp_registry
            .add_ready_server(
                McpServerSpec::new(
                    server_id.clone(),
                    "fixture mcp",
                    TransportChoice::InProcess,
                    McpServerSource::Workspace,
                ),
                McpServerScope::Session(session_id),
                Arc::new(MockMcpConnection {
                    tools: vec![mcp_tool("lookup", false), mcp_tool("always", true)],
                }),
            )
            .await
            .expect("mcp server registers");
        let model = Arc::new(MockProvider::default().with_events(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("done".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]));

        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
            .with_sandbox(NoopSandbox::new())
            .with_mcp_config(McpConfig {
                registry: mcp_registry,
                server_ids_to_inject: vec![server_id],
            })
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
        session.run_turn("use mcp").await.expect("turn should run");

        let requests = model.requests().await;
        let tool_names: Vec<_> = requests[0]
            .tools
            .as_ref()
            .expect("default session should expose loaded tools")
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert!(tool_names.contains(&"mcp__fixture__always"));
        assert!(
            !tool_names.contains(&"mcp__fixture__lookup"),
            "MCP tools without AlwaysLoad metadata should stay deferred when tool search is forced"
        );
    });
}

#[test]
fn mcp_metrics_are_forwarded_to_observer() {
    block_on(async {
        let session_id = SessionId::new();
        let server_id = McpServerId("fixture-metrics".into());
        let mcp_registry = McpRegistry::new();
        mcp_registry
            .add_ready_server(
                McpServerSpec::new(
                    server_id.clone(),
                    "fixture metrics mcp",
                    TransportChoice::InProcess,
                    McpServerSource::Workspace,
                ),
                McpServerScope::Session(session_id),
                Arc::new(MockMcpConnection {
                    tools: vec![mcp_tool("lookup", false)],
                }),
            )
            .await
            .expect("mcp server registers");
        let tracer = Arc::new(RecordingAnyTracer::default());
        let observer = Arc::new(
            Observer::builder()
                .with_tracer(tracer.clone())
                .build()
                .expect("observer should build"),
        );

        let harness = Harness::builder()
            .with_model(MockProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_observer(observer)
            .with_mcp_config(McpConfig {
                registry: mcp_registry,
                server_ids_to_inject: vec![server_id.clone()],
            })
            .build()
            .await
            .expect("harness should build");

        harness
            .mcp_config()
            .expect("mcp config")
            .registry
            .handle_resource_updated(
                &server_id,
                "jyowo://sessions/1".to_owned(),
                Arc::new(harness_mcp::NoopMcpEventSink),
            )
            .await
            .expect("resource update");

        let span = tracer
            .spans()
            .into_iter()
            .find(|span| span.name == "mcp.resource.updated")
            .expect("mcp metric span");
        assert_eq!(
            string_attr(&span.attrs, "server_id"),
            Some("fixture-metrics")
        );
    });
}

#[test]
fn mcp_sampling_provider_invokes_model_without_session_turn() {
    block_on(async {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(MockProvider::default().with_events(vec![
            ModelStreamEvent::MessageStart {
                message_id: "sampling".to_owned(),
                usage: UsageSnapshot {
                    input_tokens: 3,
                    ..UsageSnapshot::default()
                },
            },
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("sampled".to_owned()),
            },
            ModelStreamEvent::MessageDelta {
                stop_reason: None,
                usage_delta: UsageSnapshot {
                    output_tokens: 2,
                    ..UsageSnapshot::default()
                },
            },
            ModelStreamEvent::MessageStop,
        ]));
        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let provider =
            harness.mcp_sampling_provider(TenantId::SINGLE, Some(session_id), Some(run_id));
        let response = harness_mcp::SamplingProvider::create_message(
            &provider,
            SamplingRequest {
                session_id,
                run_id: Some(run_id),
                server_id: McpServerId("github".to_owned()),
                request_id: RequestId::new(),
                model_id: Some("mock-model".to_owned()),
                input_tokens: 3,
                max_output_tokens: 8,
                tool_rounds: 0,
                requested_timeout: None,
                permission_mode: harness_contracts::PermissionMode::Default,
                server_trust: TrustLevel::AdminTrusted,
                prompt_cache_namespace: Some("mcp::sampling::github::namespace".to_owned()),
                params: json!({
                    "messages": [
                        {
                            "role": "user",
                            "content": { "type": "text", "text": "hello" }
                        }
                    ]
                }),
            },
        )
        .await
        .expect("sampling should invoke model");

        assert_eq!(response.model_id, "mock-model");
        assert_eq!(
            response.content,
            json!({ "type": "text", "text": "sampled" })
        );
        assert_eq!(response.input_tokens, 3);
        assert_eq!(response.output_tokens, 2);
        let requests = model.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].messages[0].parts,
            vec![MessagePart::Text("hello".to_owned())]
        );
        assert_eq!(
            requests[0].extra["prompt_cache_namespace"],
            "mcp::sampling::github::namespace"
        );
        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(
            events.is_empty(),
            "sampling provider must not append main session history"
        );
    });
}

#[test]
fn tool_search_pending_mcp_servers_reflect_registry_state_and_retains_deferred_descriptors() {
    block_on(async {
        let workspace = unique_workspace("sdk-mcp-pending-projection");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let ready_server_id = McpServerId("ready".into());
        let pending_server_id = McpServerId("pending".into());
        let mcp_registry = McpRegistry::new();
        for server_id in [ready_server_id.clone(), pending_server_id.clone()] {
            mcp_registry
                .add_ready_server(
                    McpServerSpec::new(
                        server_id.clone(),
                        format!("{} mcp", server_id.0),
                        TransportChoice::InProcess,
                        McpServerSource::Workspace,
                    ),
                    McpServerScope::Session(session_id),
                    Arc::new(MockMcpConnection {
                        tools: vec![mcp_tool("lookup", false)],
                    }),
                )
                .await
                .expect("mcp server registers");
        }
        mcp_registry
            .set_connection_state(
                &pending_server_id,
                McpConnectionState::Reconnecting {
                    attempt: 1,
                    last_error: "transport reset".to_owned(),
                },
            )
            .await
            .expect("pending state");
        let tool_use_id = ToolUseId::new();
        let model = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: tool_use_id,
                        name: "tool_search".to_owned(),
                        input: json!({ "query": "pending mcp" }),
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
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(MockBroker::new(vec![Decision::AllowOnce]))
            .with_mcp_config(McpConfig {
                registry: mcp_registry,
                server_ids_to_inject: vec![ready_server_id, pending_server_id],
            })
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

        session.run_turn("find pending mcp").await.expect("turn");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        let tool_search_result = events
            .iter()
            .find_map(|event| match event {
                Event::ToolUseCompleted(completed) if completed.tool_use_id == tool_use_id => {
                    match &completed.result {
                        ToolResult::Structured(value) => Some(value.clone()),
                        _ => None,
                    }
                }
                _ => None,
            })
            .expect("tool_search should complete");

        assert_eq!(
            tool_search_result["pending_mcp_servers"],
            json!(["pending"])
        );
        assert!(
            tool_search_result["total_deferred_tools"]
                .as_u64()
                .expect("total_deferred_tools should be a number")
                >= 2
        );
        assert!(tool_search_result["matches"]
            .as_array()
            .expect("matches should be an array")
            .contains(&json!("mcp__pending__lookup")));
    });
}

#[test]
fn sdk_installs_default_stream_elicitation_handler() {
    tokio_runtime().block_on(async {
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let harness = Harness::builder()
            .with_model(MockProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");
        let handler = harness
            .elicitation_handler()
            .expect("default elicitation handler should be installed");
        let request_id = RequestId::new();

        let pending = tokio::spawn(async move {
            handler
                .handle(harness_mcp::ElicitationRequest {
                    request_id,
                    server_id: McpServerId("fixture".to_owned()),
                    schema: json!({
                        "type": "object",
                        "properties": { "answer": { "type": "string" } },
                        "required": ["answer"]
                    }),
                    subject: "Need input".to_owned(),
                    detail: None,
                    timeout: Some(std::time::Duration::from_secs(1)),
                })
                .await
        });

        tokio::task::yield_now().await;
        harness
            .resolve_elicitation(request_id, json!({ "answer": "ok" }))
            .await
            .expect("default stream resolver should resolve");
        let value = pending
            .await
            .expect("elicitation task should finish")
            .expect("elicitation should resolve");
        assert_eq!(value, json!({ "answer": "ok" }));
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let events: Vec<_> = store
            .query_after(TenantId::SINGLE, None, 10)
            .await
            .expect("events should be readable")
            .into_iter()
            .map(|envelope| envelope.payload)
            .collect();
        assert!(events.iter().any(|event| {
            matches!(event, Event::McpElicitationRequested(requested)
                if requested.request_id == request_id)
        }));
        assert!(events.iter().any(|event| {
            matches!(event, Event::McpElicitationResolved(resolved)
                if resolved.request_id == request_id)
        }));
    });
}

#[test]
fn plugins_are_activated_before_session_runtime_assembly() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-plugin-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(MockProvider::default().with_events(vec![
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
            .with_model(MockProvider::default())
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
            .with_model(MockProvider::default())
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
        assert!(error.to_string().contains("manifest loader"));

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
    });
}

#[test]
fn default_session_installs_memory_manager_into_context_engine() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-memory-context-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let memory = Arc::new(MockMemoryProvider::new("memory-runtime"));
        memory
            .upsert(memory_record(
                session_id,
                "prefers compact architecture notes",
            ))
            .await
            .expect("memory upsert should succeed");
        let model = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("done".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ])]));

        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_memory_provider_arc(memory)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        session
            .run_turn("what should I remember?")
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
        assert!(request_text.contains("prefers compact architecture notes"));
    });
}

#[cfg(feature = "memory-builtin")]
#[test]
fn default_session_freezes_builtin_memdir_into_system_prompt() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-builtin-memdir-system");
        std::fs::create_dir_all(&workspace).unwrap();
        let memdir_root = workspace.join("memory");
        let session_id = SessionId::new();
        let builtin = harness_memory::BuiltinMemory::at(&memdir_root, TenantId::SINGLE);
        builtin
            .append_section(
                harness_memory::MemdirFile::Memory,
                "profile",
                "first session fact",
            )
            .await
            .expect("seed memory");
        let model = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Stream(vec![ModelStreamEvent::MessageStop]),
            ScriptedResponse::Stream(vec![ModelStreamEvent::MessageStop]),
        ]));

        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_builtin_memory(builtin.clone())
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        builtin
            .append_section(
                harness_memory::MemdirFile::Memory,
                "late",
                "late fact after session creation",
            )
            .await
            .expect("late memory write");
        session
            .run_turn("first turn")
            .await
            .expect("turn should run");

        let second_session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect("second session should be created");
        second_session
            .run_turn("second turn")
            .await
            .expect("second turn should run");

        let requests = model.requests().await;
        let first_system = requests[0].system.as_deref().unwrap_or_default();
        let second_system = requests[1].system.as_deref().unwrap_or_default();
        assert!(first_system.contains("first session fact"));
        assert!(!first_system.contains("late fact after session creation"));
        assert!(second_system.contains("late fact after session creation"));
    });
}

#[cfg(feature = "memory-builtin")]
#[test]
fn default_session_truncates_oversized_memdir_snapshot_to_latest_sections() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-builtin-memdir-overflow");
        std::fs::create_dir_all(&workspace).unwrap();
        let memdir_root = workspace.join("memory");
        let tenant_dir = memdir_root.join(TenantId::SINGLE.to_string());
        std::fs::create_dir_all(&tenant_dir).unwrap();
        let oversized = (0..220)
            .map(|index| format!("§ section-{index:03}\n{}\n", "x".repeat(96)))
            .collect::<String>();
        std::fs::write(tenant_dir.join("MEMORY.md"), oversized).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::MessageStop,
        ])]));

        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_builtin_memory_root(&memdir_root)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        session
            .run_turn("overflow check")
            .await
            .expect("turn should run");

        let system = model.requests().await[0].system.clone().unwrap_or_default();
        assert!(!system.contains("section-000"));
        assert!(system.contains("section-219"));
        assert!(system.contains("sections truncated"));
        assert!(system.chars().count() <= 24_500);

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(!events
            .iter()
            .any(|event| matches!(event, Event::MemdirOverflow(_))));
    });
}

#[cfg(feature = "memory-builtin")]
#[test]
fn default_session_degrades_extreme_memdir_snapshot_to_head_only_and_emits_overflow() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-builtin-memdir-extreme-overflow");
        std::fs::create_dir_all(&workspace).unwrap();
        let memdir_root = workspace.join("memory");
        let tenant_dir = memdir_root.join(TenantId::SINGLE.to_string());
        std::fs::create_dir_all(&tenant_dir).unwrap();
        let oversized = (0..420)
            .map(|index| format!("§ section-{index:03}\n{}\n", "x".repeat(96)))
            .collect::<String>();
        std::fs::write(tenant_dir.join("MEMORY.md"), oversized).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::MessageStop,
        ])]));

        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_builtin_memory_root(&memdir_root)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        session
            .run_turn("overflow check")
            .await
            .expect("turn should run");

        let system = model.requests().await[0].system.clone().unwrap_or_default();
        assert!(system.contains("section-000"));
        assert!(!system.contains("section-219"));
        assert!(!system.contains("section-419"));
        assert!(system.chars().count() <= 1_500);

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| {
            matches!(event, Event::MemdirOverflow(overflow)
            if overflow.session_id == session_id
                && overflow.file == harness_contracts::MemdirFileTag::Memory
                && overflow.current_chars > overflow.threshold
                && overflow.threshold == 36_000
                && matches!(
                    overflow.strategy_applied,
                    harness_contracts::OverflowStrategy::HeadOnly { kept_chars: 1024 }
                ))
        }));
    });
}

#[test]
fn default_session_initializes_memory_provider() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-memory-initialize");
        std::fs::create_dir_all(&workspace).unwrap();
        let team_id = TeamId::new();
        let memory = Arc::new(InitializingMemoryProvider::default());

        let harness = Harness::builder()
            .with_model(MockProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_memory_provider_arc(memory.clone())
            .build()
            .await
            .expect("harness should build");

        let _session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_user_id("user-1")
                    .with_team_id(team_id),
            )
            .await
            .expect("session should be created");

        assert_eq!(memory.initializes.load(Ordering::SeqCst), 1);
        let initialized = memory.initialized_identity.lock().unwrap();
        assert_eq!(
            initialized.as_ref(),
            Some(&(Some("user-1".to_owned()), Some(team_id)))
        );
    });
}

#[test]
fn default_session_end_passes_identity_and_real_summary_to_memory_provider() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-memory-session-end");
        std::fs::create_dir_all(&workspace).unwrap();
        let team_id = TeamId::new();
        let memory = Arc::new(EndingMemoryProvider::default());

        let harness = Harness::builder()
            .with_model(MockProvider::default().with_events(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("final answer".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]))
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_memory_provider_arc(memory.clone())
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_user_id("user-1")
                    .with_team_id(team_id),
            )
            .await
            .expect("session should be created");
        session.run_turn("remember end").await.unwrap();
        session.end(EndReason::Completed).await.unwrap();

        let ended = memory.ended.lock().unwrap().clone().expect("session end");
        assert_eq!(ended.user_id.as_deref(), Some("user-1"));
        assert_eq!(ended.team_id, Some(team_id));
        assert_eq!(ended.turn_count, 1);
        assert_eq!(ended.tool_use_count, 0);
        assert_eq!(ended.final_assistant_text.as_deref(), Some("final answer"));
        assert_eq!(memory.shutdowns.load(Ordering::SeqCst), 1);
    });
}

#[test]
#[cfg(feature = "memory-consolidation")]
fn default_session_runs_memory_consolidation_hook_on_session_end() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-memory-consolidation");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let tracer = Arc::new(RecordingAnyTracer::default());
        let observer = Arc::new(
            Observer::builder()
                .with_tracer(tracer.clone())
                .build()
                .expect("observer should build"),
        );
        let hook = Arc::new(RecordingConsolidationHook::default());

        let harness = Harness::builder()
            .with_model(MockProvider::default().with_events(vec![ModelStreamEvent::MessageStop]))
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_memory_consolidation_hook_arc(hook.clone())
            .with_observer(observer)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        session.run_turn("consolidate").await.unwrap();
        session.end(EndReason::Completed).await.unwrap();

        assert_eq!(hook.calls.load(Ordering::SeqCst), 1);
        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| matches!(
            event,
            Event::MemoryConsolidationRan(ran)
                if ran.hook_id == "sdk-consolidation"
                    && ran.promoted == vec![hook.promoted]
        )));
        assert!(tracer
            .spans()
            .iter()
            .any(|span| span.name == "memory.consolidation.ran"));
    });
}

#[test]
fn default_session_records_external_memory_metrics_to_observer() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-memory-observer-external");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let memory = Arc::new(MockMemoryProvider::new("memory-observer"));
        memory
            .upsert(memory_record(session_id, "observer fact"))
            .await
            .expect("memory upsert should succeed");
        let tracer = Arc::new(RecordingAnyTracer::default());
        let observer = Arc::new(
            Observer::builder()
                .with_tracer(tracer.clone())
                .build()
                .expect("observer should build"),
        );

        let harness = Harness::builder()
            .with_model(MockProvider::default().with_events(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("done".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]))
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_memory_provider_arc(memory)
            .with_observer(observer)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        session.run_turn("what do I remember?").await.unwrap();

        let spans = tracer.spans();
        assert!(spans
            .iter()
            .any(|span| span.name == "memory.external.configured"));
        let recall = spans
            .iter()
            .find(|span| span.name == "memory.recall")
            .expect("recall metric should be recorded");
        assert_eq!(
            string_attr(&recall.attrs, "provider_id"),
            Some("memory-observer")
        );
        assert_eq!(string_attr(&recall.attrs, "outcome"), Some("recalled"));
        assert_eq!(int_attr(&recall.attrs, "returned_count"), Some(1));
        let hit_rate = spans
            .iter()
            .find(|span| span.name == "memory.recall.hit_rate")
            .expect("recall hit-rate metric should be recorded");
        assert_eq!(bool_attr(&hit_rate.attrs, "hit"), Some(true));
    });
}

#[cfg(feature = "memory-builtin")]
#[test]
fn default_session_records_memdir_overflow_metric_to_observer() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-memory-observer-overflow");
        std::fs::create_dir_all(&workspace).unwrap();
        let memdir_root = workspace.join("memory");
        let tenant_dir = memdir_root.join(TenantId::SINGLE.to_string());
        std::fs::create_dir_all(&tenant_dir).unwrap();
        let oversized = (0..420)
            .map(|index| {
                format!(
                    "§ section-{index:03}\n{}\n",
                    "secret-section-value".repeat(6)
                )
            })
            .collect::<String>();
        std::fs::write(tenant_dir.join("MEMORY.md"), oversized).unwrap();
        let tracer = Arc::new(RecordingAnyTracer::default());
        let observer = Arc::new(
            Observer::builder()
                .with_tracer(tracer.clone())
                .build()
                .expect("observer should build"),
        );

        let harness = Harness::builder()
            .with_model(MockProvider::default().with_events(vec![ModelStreamEvent::MessageStop]))
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_builtin_memory_root(&memdir_root)
            .with_observer(observer)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect("session should be created");
        session.run_turn("overflow").await.unwrap();

        let spans = tracer.spans();
        let bytes = spans
            .iter()
            .find(|span| span.name == "memory.memdir.bytes")
            .expect("memdir bytes metric should be recorded");
        assert_eq!(string_attr(&bytes.attrs, "file"), Some("memory"));
        assert!(int_attr(&bytes.attrs, "bytes").unwrap_or_default() > 36_000);

        let overflow = spans
            .into_iter()
            .find(|span| span.name == "memory.memdir.overflow")
            .expect("memdir overflow metric should be recorded");
        assert_eq!(string_attr(&overflow.attrs, "file"), Some("memory"));
        assert!(int_attr(&overflow.attrs, "current_chars").unwrap_or_default() > 36_000);
        assert_eq!(int_attr(&overflow.attrs, "threshold"), Some(36_000));
        assert!(
            overflow
                .attrs
                .attrs
                .values()
                .all(|value| !format!("{value:?}").contains("secret-section-value")),
            "memory metrics must not include raw memdir content"
        );
    });
}

#[test]
fn memory_metric_reason_is_redacted_and_bounded() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-memory-observer-redaction");
        std::fs::create_dir_all(&workspace).unwrap();
        let tracer = Arc::new(RecordingAnyTracer::default());
        let observer = Arc::new(
            Observer::builder()
                .with_tracer(tracer.clone())
                .with_redactor(Arc::new(TestRedactor))
                .build()
                .expect("observer should build"),
        );

        let harness = Harness::builder()
            .with_model(MockProvider::default().with_events(vec![ModelStreamEvent::MessageStop]))
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_memory_provider(ErrorMemoryProvider)
            .with_observer(observer)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect("session should be created");
        session.run_turn("what do I remember?").await.unwrap();

        let degraded = tracer
            .spans()
            .into_iter()
            .find(|span| span.name == "memory.recall.degraded")
            .expect("recall degraded metric should be recorded");
        let reason = string_attr(&degraded.attrs, "reason").expect("reason attr");
        assert!(reason.contains("[REDACTED]"));
        assert!(!reason.contains("secret-token"));
        assert!(reason.chars().count() <= 160);
    });
}

#[test]
fn default_session_uses_user_and_team_memory_actor() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-memory-actor");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let team_id = TeamId::new();
        let memory = Arc::new(MockMemoryProvider::new("memory-actor"));
        memory
            .upsert(memory_record_with_visibility(
                MemoryVisibility::User {
                    user_id: "user-1".to_owned(),
                },
                "user scoped fact",
            ))
            .await
            .expect("user memory upsert");
        memory
            .upsert(memory_record_with_visibility(
                MemoryVisibility::Team { team_id },
                "team scoped fact",
            ))
            .await
            .expect("team memory upsert");
        let model = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::MessageStop,
        ])]));

        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_memory_provider_arc(memory)
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_user_id("user-1")
                    .with_team_id(team_id),
            )
            .await
            .expect("session should be created");
        session
            .run_turn("what should I remember?")
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
        assert!(request_text.contains("user scoped fact"));
        assert!(request_text.contains("team scoped fact"));
    });
}

#[test]
fn sdk_installs_default_context_pipeline() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-default-context-pipeline");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Error(ModelError::ContextTooLong {
                tokens: 2_000,
                max: 100,
            }),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("done".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
        ]));

        let harness = Harness::builder()
            .with_model_arc(model)
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
            .run_turn("trigger emergency compact")
            .await
            .expect("turn should compact and retry");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        let stages = events
            .iter()
            .filter_map(|event| match event {
                Event::ContextStageTransitioned(stage) => Some(stage.stage.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            stages,
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

fn tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("tokio runtime")
}

#[test]
fn default_session_exposes_tracer_to_runtime() {
    block_on(async {
        let workspace = unique_workspace("sdk-tracer-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let tracer = Arc::new(RecordingTracer::default());

        let harness = Harness::builder()
            .with_model(MockProvider::default().with_events(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("done".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]))
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_observability(tracer.clone())
            .build()
            .await
            .expect("harness should build");

        let session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect("session should be created");
        session.run_turn("trace").await.expect("turn should run");

        assert!(
            tracer.started.load(Ordering::SeqCst) > 0,
            "Engine runtime should start spans through the configured SDK tracer"
        );
    });
}

#[cfg(feature = "agents-subagent")]
#[test]
fn default_session_installs_agent_tool_when_subagent_runner_is_configured() {
    block_on(async {
        let workspace = unique_workspace("sdk-agent-tool-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(MockProvider::default().with_events(vec![
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

#[cfg(feature = "agents-subagent")]
struct ReadySubagentRunner;

#[cfg(feature = "agents-subagent")]
#[async_trait]
impl harness_subagent::SubagentRunner for ReadySubagentRunner {
    async fn spawn(
        &self,
        spec: harness_subagent::SubagentSpec,
        _input: harness_contracts::TurnInput,
        parent_ctx: harness_subagent::ParentContext,
    ) -> Result<harness_subagent::SubagentHandle, harness_subagent::SubagentError> {
        Ok(harness_subagent::SubagentHandle::ready(
            harness_subagent::SubagentAnnouncement {
                subagent_id: harness_contracts::SubagentId::new(),
                parent_session_id: parent_ctx.parent_session_id,
                status: harness_contracts::SubagentStatus::Completed,
                summary: spec.task,
                result: None,
                usage: harness_contracts::UsageSnapshot::default(),
                transcript_ref: None,
                context_report: None,
            },
        ))
    }
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-{name}-{}-{}",
        std::process::id(),
        harness_contracts::SessionId::new()
    ))
}

fn skill_registration_from(markdown: &str, source: SkillSource) -> SkillRegistration {
    SkillRegistration {
        skill: harness_skill::parse_skill_markdown(markdown, source, None, SkillPlatform::Macos)
            .expect("skill should parse"),
        force_allowlist: None,
    }
}

struct BlockingSkillListProvider {
    tool_use_id: ToolUseId,
    started: Notify,
    release: Notify,
    calls: AtomicUsize,
}

impl BlockingSkillListProvider {
    fn new(tool_use_id: ToolUseId) -> Self {
        Self {
            tool_use_id,
            started: Notify::new(),
            release: Notify::new(),
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ModelProvider for BlockingSkillListProvider {
    fn provider_id(&self) -> &str {
        "mock"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        MockProvider::default().supported_models()
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            self.started.notify_one();
            self.release.notified().await;
            return Ok(Box::pin(stream::iter(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: self.tool_use_id,
                        name: "skills_list".to_owned(),
                        input: json!({}),
                    },
                },
                ModelStreamEvent::MessageStop,
            ])));
        }

        Ok(Box::pin(stream::iter(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("done".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ])))
    }
}

fn sdk_default_features(manifest: &str) -> Vec<String> {
    let mut in_default = false;
    let mut features = Vec::new();
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("default = [") {
            in_default = true;
            continue;
        }
        if in_default && trimmed.starts_with(']') {
            break;
        }
        if in_default {
            if let Some(feature) = trimmed
                .trim_end_matches(',')
                .trim()
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
            {
                features.push(feature.to_owned());
            }
        }
    }
    features
}

fn mcp_tool(name: &str, always_load: bool) -> McpToolDescriptor {
    let mut meta = std::collections::BTreeMap::new();
    if always_load {
        meta.insert("anthropic/alwaysLoad".to_owned(), json!(true));
    }
    McpToolDescriptor {
        name: name.to_owned(),
        description: Some(format!("{name} mcp tool")),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        annotations: None,
        meta,
    }
}

struct MockMcpConnection {
    tools: Vec<McpToolDescriptor>,
}

#[async_trait]
impl McpConnection for MockMcpConnection {
    fn connection_id(&self) -> &'static str {
        "mock-mcp"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Ok(self.tools.clone())
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Ok(McpToolResult::text("ok"))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

fn memory_record(session_id: SessionId, content: &str) -> MemoryRecord {
    MemoryRecord {
        id: MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::UserPreference,
        visibility: MemoryVisibility::Private { session_id },
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: MemorySource::UserInput,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 1.0,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: harness_contracts::now(),
        updated_at: harness_contracts::now(),
    }
}

fn memory_record_with_visibility(visibility: MemoryVisibility, content: &str) -> MemoryRecord {
    MemoryRecord {
        id: MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::UserPreference,
        visibility,
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: MemorySource::UserInput,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 1.0,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: harness_contracts::now(),
        updated_at: harness_contracts::now(),
    }
}

fn request_text(request: &ModelRequest) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            harness_contracts::MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Default)]
struct InitializingMemoryProvider {
    initializes: AtomicUsize,
    initialized_identity: Mutex<Option<(Option<String>, Option<TeamId>)>>,
}

#[async_trait]
impl MemoryStore for InitializingMemoryProvider {
    fn provider_id(&self) -> &'static str {
        "initializing"
    }

    async fn recall(
        &self,
        _query: harness_memory::MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(Vec::new())
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(
        &self,
        _scope: harness_memory::MemoryListScope,
    ) -> Result<Vec<harness_memory::MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryLifecycle for InitializingMemoryProvider {
    async fn initialize(&self, ctx: &MemorySessionCtx<'_>) -> Result<(), MemoryError> {
        assert_eq!(ctx.tenant_id, TenantId::SINGLE);
        assert!(ctx.session_id != SessionId::from_u128(0));
        *self.initialized_identity.lock().unwrap() =
            Some((ctx.user_id.map(str::to_owned), ctx.team_id));
        self.initializes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EndedMemorySnapshot {
    user_id: Option<String>,
    team_id: Option<TeamId>,
    turn_count: u32,
    tool_use_count: u32,
    final_assistant_text: Option<String>,
}

#[derive(Default)]
struct EndingMemoryProvider {
    ended: Mutex<Option<EndedMemorySnapshot>>,
    shutdowns: AtomicUsize,
}

#[async_trait]
impl MemoryStore for EndingMemoryProvider {
    fn provider_id(&self) -> &'static str {
        "ending"
    }

    async fn recall(
        &self,
        _query: harness_memory::MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(Vec::new())
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(
        &self,
        _scope: harness_memory::MemoryListScope,
    ) -> Result<Vec<harness_memory::MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryLifecycle for EndingMemoryProvider {
    async fn on_session_end(
        &self,
        ctx: &MemorySessionCtx<'_>,
        summary: &SessionSummaryView<'_>,
    ) -> Result<(), MemoryError> {
        *self.ended.lock().unwrap() = Some(EndedMemorySnapshot {
            user_id: ctx.user_id.map(str::to_owned),
            team_id: ctx.team_id,
            turn_count: summary.turn_count,
            tool_use_count: summary.tool_use_count,
            final_assistant_text: summary.final_assistant_text.map(str::to_owned),
        });
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), MemoryError> {
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(feature = "memory-consolidation")]
struct RecordingConsolidationHook {
    calls: AtomicUsize,
    promoted: MemoryId,
}

#[cfg(feature = "memory-consolidation")]
impl Default for RecordingConsolidationHook {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            promoted: MemoryId::new(),
        }
    }
}

#[cfg(feature = "memory-consolidation")]
#[async_trait]
impl ConsolidationHook for RecordingConsolidationHook {
    fn hook_id(&self) -> &str {
        "sdk-consolidation"
    }

    async fn on_session_end(
        &self,
        _ctx: &MemorySessionCtx<'_>,
        _summary: &SessionSummaryView<'_>,
    ) -> Result<ConsolidationOutcome, MemoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ConsolidationOutcome {
            promoted: vec![self.promoted],
            demoted: Vec::new(),
            draft_dreams: "sdk dream".to_owned(),
        })
    }
}

fn plugin_manifest(name: &str) -> ManifestRecord {
    ManifestRecord::new(
        PluginManifest {
            manifest_schema_version: 1,
            name: PluginName::new(name).unwrap(),
            version: semver::Version::parse("0.1.0").unwrap(),
            trust_level: TrustLevel::UserControlled,
            description: None,
            authors: Vec::new(),
            repository: None,
            signature: None,
            capabilities: PluginCapabilities {
                tools: vec![harness_plugin::ToolManifestEntry {
                    name: "plugin-tool".to_owned(),
                    destructive: false,
                }],
                memory_provider: Some(harness_plugin::MemoryProviderManifestEntry {
                    name: "plugin-memory".to_owned(),
                }),
                ..PluginCapabilities::default()
            },
            dependencies: Vec::new(),
            min_harness_version: semver::VersionReq::parse(">=0.0.0").unwrap(),
        },
        ManifestOrigin::File {
            path: format!("/plugins/{name}/plugin.json").into(),
        },
        [7; 32],
    )
    .unwrap()
}

fn plugin_id(name: &str) -> PluginId {
    PluginId(format!("{name}@0.1.0"))
}

struct SdkStaticManifestLoader {
    records: Vec<ManifestRecord>,
}

#[async_trait]
impl PluginManifestLoader for SdkStaticManifestLoader {
    async fn enumerate(
        &self,
        _source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        Ok(self.records.clone())
    }
}

struct SdkFailingManifestLoader;

#[async_trait]
impl PluginManifestLoader for SdkFailingManifestLoader {
    async fn enumerate(
        &self,
        _source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        Err(ManifestLoaderError::Validation(
            harness_plugin::ManifestValidationFailure {
                origin: Some(ManifestOrigin::File {
                    path: "/plugins/typed-bad/plugin.json".into(),
                }),
                partial_name: Some("typed-bad".to_owned()),
                partial_version: Some("0.1.0".to_owned()),
                raw_bytes_hash: [8; 32],
                failure: ContractManifestValidationFailure::SchemaViolation {
                    json_pointer: "/capabilities".to_owned(),
                    details: "expected object".to_owned(),
                },
                details: "expected object".to_owned(),
            },
        ))
    }
}

#[derive(Default)]
struct RecordingPluginEventSink {
    events: Mutex<Vec<Event>>,
}

impl RecordingPluginEventSink {
    fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap().clone()
    }
}

impl PluginEventSink for RecordingPluginEventSink {
    fn emit(&self, event: Event) {
        self.events.lock().unwrap().push(event);
    }
}

struct RuntimePlugin {
    manifest: PluginManifest,
    session_id: SessionId,
}

#[async_trait]
impl Plugin for RuntimePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        ctx.tools
            .as_ref()
            .expect("plugin tool handle")
            .register(Box::new(SdkPluginTool::new("plugin-tool")))
            .await?;
        ctx.memory
            .as_ref()
            .expect("plugin memory handle")
            .register(Arc::new(SdkPluginMemoryProvider {
                record: memory_record(self.session_id, "plugin memory is active"),
            }))
            .await?;
        Ok(PluginActivationResult {
            registered_tools: vec!["plugin-tool".to_owned()],
            occupied_slots: vec![harness_plugin::CapabilitySlot::MemoryProvider],
            ..PluginActivationResult::default()
        })
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

struct SdkPluginTool {
    descriptor: ToolDescriptor,
}

impl SdkPluginTool {
    fn new(name: &str) -> Self {
        Self::with_defer_policy(name, DeferPolicy::AlwaysLoad)
    }

    fn new_deferred(name: &str) -> Self {
        Self::with_defer_policy(name, DeferPolicy::ForceDefer)
    }

    fn with_defer_policy(name: &str, defer_policy: DeferPolicy) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: name.to_owned(),
                description: "plugin tool".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.1.0".to_owned(),
                input_schema: json!({ "type": "object" }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy,
                },
                trust_level: TrustLevel::UserControlled,
                required_capabilities: Vec::new(),
                budget: default_result_budget(),
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
            },
        }
    }
}

#[async_trait]
impl Tool for SdkPluginTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn resolve_schema(
        &self,
        _ctx: &SchemaResolverContext,
    ) -> Result<Value, harness_contracts::ToolError> {
        Ok(self.descriptor.input_schema.clone())
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn check_permission(&self, _input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        PermissionCheck::Allowed
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolContext,
    ) -> Result<ToolStream, harness_contracts::ToolError> {
        Ok(Box::pin(futures::stream::empty()))
    }
}

struct DeferredDeltaEmitterTool {
    descriptor: ToolDescriptor,
    deferred_name: String,
}

impl DeferredDeltaEmitterTool {
    fn new(deferred_name: &str) -> Self {
        Self {
            descriptor: SdkPluginTool::new("emit_deferred_delta").descriptor,
            deferred_name: deferred_name.to_owned(),
        }
    }
}

#[async_trait]
impl Tool for DeferredDeltaEmitterTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn check_permission(&self, _input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        PermissionCheck::Allowed
    }

    async fn execute(
        &self,
        _input: Value,
        ctx: ToolContext,
    ) -> Result<ToolStream, harness_contracts::ToolError> {
        let event = Event::ToolDeferredPoolChanged(ToolDeferredPoolChangedEvent {
            session_id: ctx.session_id,
            added: vec![DeferredToolHint {
                name: self.deferred_name.clone(),
                hint: None,
            }],
            removed: Vec::new(),
            source: ToolPoolChangeSource::InitialClassification,
            deferred_total: 1,
            at: harness_contracts::now(),
        });
        Ok(Box::pin(futures::stream::iter([
            ToolEvent::Journal(event),
            ToolEvent::Final(ToolResult::Text("delta emitted".to_owned())),
        ])))
    }
}

struct CapabilityScriptedProvider {
    capabilities: ModelCapabilities,
    responses: tokio::sync::Mutex<Vec<Vec<ModelStreamEvent>>>,
    requests: tokio::sync::Mutex<Vec<ModelRequest>>,
}

impl CapabilityScriptedProvider {
    fn new(capabilities: ModelCapabilities, responses: Vec<Vec<ModelStreamEvent>>) -> Self {
        Self {
            capabilities,
            responses: tokio::sync::Mutex::new(responses),
            requests: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for CapabilityScriptedProvider {
    fn provider_id(&self) -> &str {
        "mock"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            provider_id: "mock".to_owned(),
            model_id: "mock-model".to_owned(),
            display_name: "Mock model".to_owned(),
            context_window: 128_000,
            max_output_tokens: 8_192,
            capabilities: self.capabilities.clone(),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);
        let events = {
            let mut responses = self.responses.lock().await;
            if responses.is_empty() {
                vec![ModelStreamEvent::MessageStop]
            } else {
                responses.remove(0)
            }
        };
        Ok(Box::pin(futures::stream::iter(events)))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct SdkPluginMemoryProvider {
    record: MemoryRecord,
}

#[async_trait]
impl harness_memory::MemoryStore for SdkPluginMemoryProvider {
    fn provider_id(&self) -> &str {
        "sdk-plugin-memory"
    }

    async fn recall(
        &self,
        _query: harness_memory::MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(vec![self.record.clone()])
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(
        &self,
        _scope: harness_memory::MemoryListScope,
    ) -> Result<Vec<harness_memory::MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

impl harness_memory::MemoryLifecycle for SdkPluginMemoryProvider {}

#[derive(Default)]
struct RecordingTracer {
    started: AtomicUsize,
}

impl Tracer for RecordingTracer {
    fn start_span(&self, name: &str, attrs: SpanAttributes) -> Box<dyn Span> {
        assert_eq!(name, "engine.run_turn");
        self.started.fetch_add(1, Ordering::SeqCst);
        Box::new(InMemorySpan::new(name, attrs))
    }

    fn inject_context(&self, _carrier: &mut dyn TraceCarrier) {}

    fn extract_context(&self, carrier: &dyn TraceCarrier) -> Option<TraceContext> {
        TraceContext::extract(carrier)
    }
}

#[derive(Default)]
struct RecordingAnyTracer {
    spans: Mutex<Vec<RecordedSpan>>,
}

#[derive(Clone)]
struct RecordedSpan {
    name: String,
    attrs: SpanAttributes,
}

impl RecordingAnyTracer {
    fn spans(&self) -> Vec<RecordedSpan> {
        self.spans.lock().unwrap().clone()
    }
}

impl Tracer for RecordingAnyTracer {
    fn start_span(&self, name: &str, attrs: SpanAttributes) -> Box<dyn Span> {
        self.spans.lock().unwrap().push(RecordedSpan {
            name: name.to_owned(),
            attrs: attrs.clone(),
        });
        Box::new(InMemorySpan::new(name, attrs))
    }

    fn inject_context(&self, _carrier: &mut dyn TraceCarrier) {}

    fn extract_context(&self, carrier: &dyn TraceCarrier) -> Option<TraceContext> {
        TraceContext::extract(carrier)
    }
}

struct ErrorMemoryProvider;

#[async_trait]
impl MemoryStore for ErrorMemoryProvider {
    fn provider_id(&self) -> &str {
        "error-memory"
    }

    async fn recall(
        &self,
        _query: harness_memory::MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        Err(MemoryError::Message(format!(
            "provider failed with secret-token {}",
            "x".repeat(240)
        )))
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(
        &self,
        _scope: harness_memory::MemoryListScope,
    ) -> Result<Vec<harness_memory::MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

impl MemoryLifecycle for ErrorMemoryProvider {}

struct TestRedactor;

impl Redactor for TestRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("secret-token", "[REDACTED]")
    }
}

fn string_attr<'a>(attrs: &'a SpanAttributes, key: &str) -> Option<&'a str> {
    match attrs.attrs.get(key) {
        Some(AttributeValue::String(value)) => Some(value.as_str()),
        _ => None,
    }
}

fn int_attr(attrs: &SpanAttributes, key: &str) -> Option<i64> {
    match attrs.attrs.get(key) {
        Some(AttributeValue::Int(value)) => Some(*value),
        _ => None,
    }
}

fn bool_attr(attrs: &SpanAttributes, key: &str) -> Option<bool> {
    match attrs.attrs.get(key) {
        Some(AttributeValue::Bool(value)) => Some(*value),
        _ => None,
    }
}
