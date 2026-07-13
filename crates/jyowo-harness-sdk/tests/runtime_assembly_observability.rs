#![cfg(feature = "testing")]

mod runtime_assembly_support;
use runtime_assembly_support::*;

#[test]
fn session_lifecycle_hooks_are_triggered() {
    block_on(async {
        let workspace = unique_workspace("sdk-session-lifecycle-hooks");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let hook_registry = HookRegistry::builder()
            .with_hook(Box::new(TestHookHandler::new(
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
            .with_workspace_root(&workspace)
            .with_model(TestModelProvider::default())
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
            .with_workspace_root(&workspace)
            .with_model(TestModelProvider::default())
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
            .with_workspace_root(&workspace)
            .with_model(TestModelProvider::default().with_events(vec![
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
                Arc::new(TestMcpConnection {
                    tools: vec![mcp_tool("lookup", false), mcp_tool("always", true)],
                }),
            )
            .await
            .expect("mcp server registers");
        let model = Arc::new(TestModelProvider::default().with_events(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("done".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]));

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
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
                    .with_permission_mode(PermissionMode::BypassPermissions)
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
        let workspace = unique_workspace("sdk-mcp-metrics");
        std::fs::create_dir_all(&workspace).unwrap();
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
                Arc::new(TestMcpConnection {
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
            .with_workspace_root(&workspace)
            .with_model(TestModelProvider::default())
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
        let workspace = unique_workspace("sdk-mcp-sampling");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(TestModelProvider::default().with_events(vec![
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
            .with_workspace_root(&workspace)
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
                model_id: Some("test-model".to_owned()),
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

        assert_eq!(response.model_id, "test-model");
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
                    Arc::new(TestMcpConnection {
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
            .with_workspace_root(&workspace)
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_mcp_config(McpConfig {
                registry: mcp_registry,
                server_ids_to_inject: vec![ready_server_id, pending_server_id],
            })
            .build()
            .await
            .expect("harness should build");
        let options = SessionOptions::new(&workspace)
            .with_session_id(session_id)
            .with_tool_search_mode(ToolSearchMode::Always);
        harness
            .create_session(options.clone())
            .await
            .expect("session should be created");

        harness
            .submit_conversation_turn(conversation_turn_request(
                options,
                ConversationTurnInput::ask("find pending mcp"),
                Some(PermissionMode::BypassPermissions),
                None,
                None,
            ))
            .await
            .expect("turn");

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
            .unwrap_or_else(|| panic!("tool_search should complete; events: {events:#?}"));

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
        let workspace = unique_workspace("sdk-stream-elicitation");
        std::fs::create_dir_all(&workspace).unwrap();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model(TestModelProvider::default())
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
                    mode: harness_mcp::ElicitationMode::Form,
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
fn default_session_exposes_tracer_to_runtime() {
    block_on(async {
        let workspace = unique_workspace("sdk-tracer-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let tracer = Arc::new(RecordingTracer::default());

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model(TestModelProvider::default().with_events(vec![
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
