#![cfg(feature = "testing")]

mod runtime_assembly_support;
use runtime_assembly_support::*;

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
fn conversation_turn_input_ask_mode_preserves_prompt_text() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-turn-input-ask");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("answer".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]],
        ));
        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        harness
            .open_or_create_conversation_session(
                SessionOptions::new(&workspace).with_session_id(session_id),
            )
            .await
            .expect("session should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                SessionOptions::new(&workspace).with_session_id(session_id),
                ConversationTurnInput::ask("plain user question"),
                None,
                None,
                None,
            ))
            .await
            .expect("turn should run");

        let requests = model.requests().await;
        assert_eq!(request_text(&requests[0]), "plain user question");
    });
}

#[test]
fn conversation_turn_request_includes_prior_session_messages() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-turn-context-seed");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::Text("first assistant answer".to_owned()),
                    },
                    ModelStreamEvent::MessageStop,
                ],
                vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::Text("second assistant answer".to_owned()),
                    },
                    ModelStreamEvent::MessageStop,
                ],
            ],
        ));
        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        harness
            .open_or_create_conversation_session(
                SessionOptions::new(&workspace).with_session_id(session_id),
            )
            .await
            .expect("session should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                SessionOptions::new(&workspace).with_session_id(session_id),
                ConversationTurnInput::ask("first user question"),
                None,
                None,
                None,
            ))
            .await
            .expect("first turn should run");
        harness
            .submit_conversation_turn(conversation_turn_request(
                SessionOptions::new(&workspace).with_session_id(session_id),
                ConversationTurnInput::ask("second user question"),
                None,
                None,
                None,
            ))
            .await
            .expect("second turn should run");

        let requests = model.requests().await;
        let second_request_text = request_text(&requests[1]);
        assert!(second_request_text.contains("first user question"));
        assert!(second_request_text.contains("first assistant answer"));
        assert!(second_request_text.contains("second user question"));
        assert_eq!(
            second_request_text.matches("second user question").count(),
            1
        );
    });
}

#[test]
fn conversation_session_budget_uses_model_window_and_trigger_ratio() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-context-budget");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(
            CapabilityScriptedProvider::new(
                ConversationModelCapability::default(),
                vec![vec![ModelStreamEvent::MessageStop]],
            )
            .with_context_limits(40, 10),
        );
        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");
        let options = SessionOptions::new(&workspace)
            .with_session_id(session_id)
            .with_context_compression_trigger_ratio(0.5);

        harness
            .open_or_create_conversation_session(options.clone())
            .await
            .expect("session should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                options,
                ConversationTurnInput::ask(
                    "this message is intentionally long enough to cross the configured soft budget",
                ),
                None,
                None,
                None,
            ))
            .await
            .expect("turn should run");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| matches!(
            event,
            Event::ContextBudgetExceeded(exceeded)
                if exceeded.source == BudgetExceedanceSource::LocalEstimate
                    && exceeded.max == 15
        )));
    });
}

#[test]
fn default_conversation_system_prompt_uses_agent_runtime_identity() {
    block_on(async {
        let workspace = unique_workspace("sdk-default-jyowo-system-prompt");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![vec![ModelStreamEvent::MessageStop]],
        ));
        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let options = SessionOptions::new(&workspace)
            .with_session_id(session_id)
            .with_system_prompt_addendum("保留用户提供的附加约束。");
        harness
            .open_or_create_conversation_session(options.clone())
            .await
            .expect("session should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                options,
                ConversationTurnInput::ask("hello"),
                None,
                None,
                None,
            ))
            .await
            .expect("turn should run");

        let system = model.requests().await[0].system.clone().unwrap_or_default();
        assert_agent_runtime_identity(&system);
        assert_runtime_context_contract(&system);
        assert!(system.contains("<session-addendum>"));
        assert!(system.contains("保留用户提供的附加约束。"));
    });
}

#[test]
fn runtime_context_is_included_before_workspace_instructions() {
    block_on(async {
        let workspace = unique_workspace("sdk-runtime-context-order");
        std::fs::create_dir_all(&workspace).unwrap();
        let bootstrap = workspace_bootstrap_fixture(&workspace, "Root workspace rule.", None, None);
        let system = conversation_system_prompt_with_bootstrap(
            workspace,
            bootstrap,
            Some("Session-level constraint."),
        )
        .await;

        assert_runtime_context_contract(&system);
        let runtime = system.find("<runtime-context>").expect("runtime-context");
        let workspace = system
            .find(r#"<workspace-instructions source="AGENTS.md">"#)
            .expect("workspace instructions");
        assert!(runtime < workspace);
    });
}

#[test]
fn runtime_context_does_not_include_provider_credentials() {
    block_on(async {
        let workspace = unique_workspace("sdk-runtime-context-no-credentials");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![vec![ModelStreamEvent::MessageStop]],
        ));
        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let options = SessionOptions::new(&workspace)
            .with_session_id(session_id)
            .with_model_extra(json!({
                "api_key": "sk-test-secret",
                "credential": "provider-credential"
            }));
        harness
            .open_or_create_conversation_session(options.clone())
            .await
            .expect("session should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                options,
                ConversationTurnInput::ask("hello"),
                None,
                None,
                None,
            ))
            .await
            .expect("turn should run");

        let system = model.requests().await[0].system.clone().unwrap_or_default();
        assert_runtime_context_contract(&system);
        assert!(!system.contains("sk-test-secret"));
        assert!(!system.contains("provider-credential"));
    });
}

#[test]
fn default_system_prompt_excludes_coding_partner_language() {
    block_on(async {
        let workspace = unique_workspace("sdk-no-coding-partner-language");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![vec![ModelStreamEvent::MessageStop]],
        ));
        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        harness
            .open_or_create_conversation_session(options.clone())
            .await
            .expect("session should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                options,
                ConversationTurnInput::ask("hello"),
                None,
                None,
                None,
            ))
            .await
            .expect("turn should run");

        let system = model.requests().await[0].system.clone().unwrap_or_default();
        assert!(!system.contains("AI 编程伙伴"));
        assert!(!system.contains("本地项目工作空间里的 AI 编程伙伴"));
    });
}

#[test]
fn workspace_bootstrap_files_render_as_workspace_instruction_sections() {
    block_on(async {
        let workspace = unique_workspace("sdk-workspace-bootstrap-files");
        std::fs::create_dir_all(&workspace).unwrap();
        let bootstrap = workspace_bootstrap_fixture(
            &workspace,
            "Root workspace rule.",
            Some("Jyowo workspace rule."),
            None,
        );
        let system = conversation_system_prompt_with_bootstrap(
            workspace,
            bootstrap,
            Some("Session-level constraint."),
        )
        .await;

        assert!(system.contains(r#"<workspace-instructions source="AGENTS.md">"#));
        assert!(system.contains("Root workspace rule."));
        assert!(system.contains(r#"<workspace-instructions source=".jyowo/AGENTS.md">"#));
        assert!(system.contains("Jyowo workspace rule."));
    });
}

#[test]
fn workspace_bootstrap_addendum_renders_as_workspace_addendum() {
    block_on(async {
        let workspace = unique_workspace("sdk-workspace-bootstrap-addendum");
        std::fs::create_dir_all(&workspace).unwrap();
        let bootstrap = workspace_bootstrap_fixture(
            &workspace,
            "Root workspace rule.",
            Some("Jyowo workspace rule."),
            Some("Workspace bootstrap constraint."),
        );
        let system = conversation_system_prompt_with_bootstrap(
            workspace,
            bootstrap,
            Some("Session-level constraint."),
        )
        .await;

        assert!(system.contains(r#"<workspace-addendum source="workspace-bootstrap">"#));
        assert!(system.contains("Workspace bootstrap constraint."));
    });
}

#[test]
fn session_addendum_renders_after_workspace_sections() {
    block_on(async {
        let workspace = unique_workspace("sdk-workspace-session-addendum-order");
        std::fs::create_dir_all(&workspace).unwrap();
        let bootstrap = workspace_bootstrap_fixture(
            &workspace,
            "Root workspace rule.",
            Some("Jyowo workspace rule."),
            Some("Workspace bootstrap constraint."),
        );
        let system = conversation_system_prompt_with_bootstrap(
            workspace,
            bootstrap,
            Some("Session-level constraint."),
        )
        .await;

        assert_workspace_bootstrap_prompt_order(&system);
        assert!(system.contains("Session-level constraint."));
    });
}

#[test]
fn missing_optional_bootstrap_file_is_omitted() {
    block_on(async {
        let workspace = unique_workspace("sdk-workspace-optional-bootstrap-missing");
        std::fs::create_dir_all(&workspace).unwrap();
        let bootstrap = workspace_bootstrap_fixture(&workspace, "Root workspace rule.", None, None);
        let system = conversation_system_prompt_with_bootstrap(workspace, bootstrap, None).await;

        assert!(system.contains(r#"<workspace-instructions source="AGENTS.md">"#));
        assert!(!system.contains(r#"<workspace-instructions source=".jyowo/AGENTS.md">"#));
    });
}

#[test]
fn required_missing_bootstrap_file_fails_session_creation() {
    block_on(async {
        let workspace = unique_workspace("sdk-workspace-required-bootstrap-missing");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![vec![ModelStreamEvent::MessageStop]],
        ));
        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let bootstrap = WorkspaceBootstrap::new(&workspace).with_files(vec![
            BootstrapFileSpec::required("AGENTS.md"),
            BootstrapFileSpec::optional(".jyowo/AGENTS.md"),
        ]);
        let mut options = SessionOptions::new(&workspace);
        options.workspace_bootstrap = Some(bootstrap);

        let error = harness
            .create_session(options)
            .await
            .expect_err("missing required bootstrap file should fail");

        assert!(error.to_string().contains("AGENTS.md"));
        assert!(model.requests().await.is_empty());
    });
}

#[test]
fn workspace_bootstrap_content_changes_session_hash_input() {
    block_on(async {
        let workspace = unique_workspace("sdk-workspace-bootstrap-hash");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("AGENTS.md"), "Root workspace rule v1.").unwrap();

        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let session_id = SessionId::new();
        let bootstrap = WorkspaceBootstrap::new(&workspace);
        let mut options = SessionOptions::new(&workspace).with_session_id(session_id);
        options.workspace_bootstrap = Some(bootstrap);

        harness
            .create_session(options.clone())
            .await
            .expect("session v1 should be created");

        let events_after_v1: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        let (options_hash_v1, effective_hash_v1) = events_after_v1
            .iter()
            .find_map(|event| match event {
                Event::SessionCreated(created) => {
                    Some((created.options_hash, created.effective_config_hash))
                }
                _ => None,
            })
            .expect("session creation event should exist");

        std::fs::write(workspace.join("AGENTS.md"), "Root workspace rule v2.").unwrap();

        harness
            .create_session(options)
            .await
            .expect("session v2 should be created");

        let events_after_v2: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        let created_events: Vec<_> = events_after_v2
            .iter()
            .filter_map(|event| match event {
                Event::SessionCreated(created) => Some(created),
                _ => None,
            })
            .collect();
        assert_eq!(created_events.len(), 2);
        let options_hash_v2 = created_events[1].options_hash;
        let effective_hash_v2 = created_events[1].effective_config_hash;

        assert_eq!(options_hash_v1, options_hash_v2);
        assert_ne!(effective_hash_v1, effective_hash_v2);
    });
}

#[test]
fn conversation_session_uses_descriptor_protocol_when_options_omit_protocol() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-descriptor-api-mode");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let model = Arc::new(
            CapabilityScriptedProvider::new(
                ConversationModelCapability::default(),
                vec![vec![
                    ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::Text("answer".to_owned()),
                    },
                    ModelStreamEvent::MessageStop,
                ]],
            )
            .with_protocol(ModelProtocol::Responses),
        );
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let options = SessionOptions::new(&workspace)
            .with_session_id(session_id)
            .with_model_id("test-model");
        harness
            .open_or_create_conversation_session(options.clone())
            .await
            .expect("session should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                options,
                ConversationTurnInput::ask("plain user question"),
                None,
                None,
                None,
            ))
            .await
            .expect("turn should run");

        let requests = model.requests().await;
        assert_eq!(requests[0].protocol, ModelProtocol::Responses);
    });
}

#[test]
fn conversation_turn_input_renders_references_and_attachments_context_block() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-turn-input-command");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("answer".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]],
        ));
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        harness
            .open_or_create_conversation_session(
                SessionOptions::new(&workspace).with_session_id(session_id),
            )
            .await
            .expect("session should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                SessionOptions::new(&workspace).with_session_id(session_id),
                ConversationTurnInput {
                    client_message_id: None,
                    prompt: "use these references".to_owned(),
                    context_references: vec![
                        ConversationContextReference::WorkspaceFile {
                            path: "Cargo.toml".to_owned(),
                            label: "Cargo manifest".to_owned(),
                        },
                        ConversationContextReference::Skill {
                            id: "skill-review".to_owned(),
                            label: "Code review skill".to_owned(),
                        },
                        ConversationContextReference::Tool {
                            id: "builtin.grep".to_owned(),
                            label: "Search files".to_owned(),
                        },
                        ConversationContextReference::McpServer {
                            id: "mcp-filesystem".to_owned(),
                            label: "Filesystem MCP".to_owned(),
                        },
                    ],
                    attachments: vec![ConversationAttachmentReference {
                        id: "attachment-001".to_owned(),
                        name: "notes.txt".to_owned(),
                        mime_type: "text/plain".to_owned(),
                        size_bytes: 12,
                        blob_ref: test_blob_ref(12, "text/plain"),
                    }],
                },
                None,
                None,
                None,
            ))
            .await
            .expect("turn should run");

        let requests = model.requests().await;
        let text = request_text(&requests[0]);
        assert!(text.contains("<conversation-context>"));
        assert!(text.contains("workspace_file: Cargo manifest (Cargo.toml)"));
        assert!(text.contains("skill: Code review skill (skill-review)"));
        assert!(text.contains("tool: Search files (builtin.grep)"));
        assert!(text.contains("mcp_server: Filesystem MCP (mcp-filesystem)"));
        assert!(text.contains("attachment: notes.txt text/plain 12 bytes attachment-001"));
        assert!(!text.contains("Command intent only."));
        assert!(text.ends_with("use these references"));

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        let attachment = events
            .iter()
            .find_map(|event| match event {
                Event::UserMessageAppended(event) => event.attachments.first(),
                _ => None,
            })
            .expect("user event should keep attachment metadata");
        assert_eq!(attachment.id, "attachment-001");
        assert_eq!(attachment.name, "notes.txt");
        assert_eq!(attachment.mime_type, "text/plain");
        assert_eq!(attachment.size_bytes, 12);
    });
}

#[test]
fn conversation_turn_hydrates_memory_references_before_model_request() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-memory-reference-hydration");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("answer".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]],
        ));
        let memory_provider = Arc::new(InMemoryMemoryProvider::new("test-memory"));
        let record = memory_record(session_id, "Stored memory content from runtime.");
        memory_provider
            .upsert(record.clone())
            .await
            .expect("memory fixture should write");
        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
            .with_sandbox(NoopSandbox::new())
            .with_memory_provider_arc(memory_provider)
            .build()
            .await
            .expect("harness should build");

        harness
            .open_or_create_conversation_session(
                SessionOptions::new(&workspace).with_session_id(session_id),
            )
            .await
            .expect("session should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                SessionOptions::new(&workspace).with_session_id(session_id),
                ConversationTurnInput {
                    client_message_id: None,
                    prompt: "use this memory".to_owned(),
                    context_references: vec![ConversationContextReference::Memory {
                        id: record.id.to_string(),
                        label: "Runtime memory".to_owned(),
                        resolved_content: Some(
                            "frontend supplied content must be ignored".to_owned(),
                        ),
                    }],
                    attachments: Vec::new(),
                },
                None,
                None,
                None,
            ))
            .await
            .expect("turn should run");

        let requests = model.requests().await;
        let text = request_text(&requests[0]);
        assert!(text.contains("[memory-reference id="));
        assert!(text.contains("Stored memory content from runtime."));
        assert!(!text.contains("frontend supplied content must be ignored"));
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
