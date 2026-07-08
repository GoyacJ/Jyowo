#![cfg(feature = "testing")]

mod runtime_assembly_support;
use runtime_assembly_support::*;

#[test]
fn create_session_uses_engine_runtime_path() {
    block_on(async {
        let workspace = unique_workspace("sdk-engine-runtime");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

        let harness = Harness::builder()
            .with_model(TestModelProvider::default().with_events(vec![
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
fn create_session_rejects_unknown_model_id_fail_closed() {
    block_on(async {
        let workspace = unique_workspace("sdk-unknown-model");
        std::fs::create_dir_all(&workspace).unwrap();
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let error = harness
            .create_session(SessionOptions::new(&workspace).with_model_id("missing-model"))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("unsupported model id"));
    });
}

#[test]
fn conversation_facade_opens_submits_and_pages_session_events() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-facade");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

        let harness = Harness::builder()
            .with_model(TestModelProvider::default().with_events(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("facade answer".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]))
            .with_store_arc(store)
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let opened = harness
            .open_or_create_conversation_session(
                SessionOptions::new(&workspace).with_session_id(session_id),
            )
            .await
            .expect("session should open through the conversation facade");
        assert_eq!(opened.session_id, session_id);
        assert_eq!(opened.tenant_id, TenantId::SINGLE);
        assert_eq!(opened.message_count, 0);

        let submitted = harness
            .submit_conversation_turn(conversation_turn_request(
                SessionOptions::new(&workspace).with_session_id(session_id),
                ConversationTurnInput::ask("use facade path"),
                None,
                None,
                None,
            ))
            .await
            .expect("turn should run through the conversation facade");
        assert_eq!(submitted.session_id, session_id);
        assert_ne!(submitted.run_id, RunId::from_u128(0));
        assert_eq!(submitted.message_count, 2);

        let reopened = harness
            .open_or_create_conversation_session(
                SessionOptions::new(&workspace).with_session_id(session_id),
            )
            .await
            .expect("existing session should reopen through the conversation facade");
        assert_eq!(reopened.message_count, 2);

        let first_page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: SessionOptions::new(&workspace).with_session_id(session_id),
                after_event_id: None,
                limit: 2,
            })
            .await
            .expect("events should page through the conversation facade");
        assert_eq!(first_page.events.len(), 2);
        assert!(first_page.next_event_id.is_some());

        let second_page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: SessionOptions::new(&workspace).with_session_id(session_id),
                after_event_id: first_page.next_event_id,
                limit: 50,
            })
            .await
            .expect("events should continue after the previous page");
        assert!(
            second_page
                .events
                .iter()
                .any(|envelope| matches!(envelope.payload, Event::AssistantMessageCompleted(_))),
            "paged events should include the completed assistant message"
        );

        let cancel_error = harness
            .cancel_conversation_run(submitted.run_id)
            .await
            .expect_err("completed runs must not report a fake cancellation");
        assert!(cancel_error.to_string().contains("not active"));
    });
}

#[test]
fn conversation_facade_pages_and_deletes_when_model_runtime_defaults_change() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-model-default-change");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let provider = Arc::new(TwoModelProvider);
        let harness = Harness::builder()
            .with_model_arc(provider)
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");

        let created_options = SessionOptions::new(&workspace)
            .with_session_id(session_id)
            .with_model_id("model-a")
            .with_protocol(ModelProtocol::Messages);
        harness
            .open_or_create_conversation_session(created_options)
            .await
            .expect("session should open with the original model defaults");

        let changed_defaults_options = SessionOptions::new(&workspace)
            .with_session_id(session_id)
            .with_model_id("model-b")
            .with_protocol(ModelProtocol::Responses);
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: changed_defaults_options.clone(),
                after_event_id: None,
                limit: 10,
            })
            .await
            .expect("historical conversation reads must survive model default changes");
        assert!(page
            .events
            .iter()
            .any(|envelope| matches!(envelope.payload, Event::SessionCreated(_))));

        let submitted = harness
            .submit_conversation_turn(conversation_turn_request(
                changed_defaults_options.clone(),
                ConversationTurnInput::ask("continue with the selected model"),
                None,
                None,
                None,
            ))
            .await
            .expect("historical conversation submit must survive model default changes");
        assert_eq!(submitted.session_id, session_id);

        let deleted = harness
            .delete_conversation_session(changed_defaults_options)
            .await
            .expect("historical conversation delete must survive model default changes");
        assert!(deleted);
    });
}

#[test]
fn conversation_turn_permission_override_is_run_scoped() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-run-permission-override");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![
                vec![ModelStreamEvent::MessageStop],
                vec![ModelStreamEvent::MessageStop],
            ],
        ));
        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store.clone())
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
                options.clone(),
                ConversationTurnInput::ask("use full access for this run"),
                Some(PermissionMode::BypassPermissions),
                None,
                None,
            ))
            .await
            .expect("override turn should run");
        harness
            .submit_conversation_turn(conversation_turn_request(
                options.clone(),
                ConversationTurnInput::ask("use default permission mode again"),
                None,
                None,
                None,
            ))
            .await
            .expect("next turn should run with session default");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        let created_hash = events
            .iter()
            .find_map(|event| match event {
                Event::SessionCreated(created) => Some(created.options_hash),
                _ => None,
            })
            .expect("session creation event should be emitted");
        let run_modes = events
            .iter()
            .filter_map(|event| match event {
                Event::RunStarted(started) => Some(started.permission_mode),
                _ => None,
            })
            .collect::<Vec<_>>();

        let mut expected_options = options.clone();
        expected_options.workspace_root = expected_options
            .workspace_root
            .canonicalize()
            .expect("workspace root should canonicalize");
        assert_eq!(created_hash, session_options_hash(&expected_options));
        assert_eq!(
            run_modes,
            vec![PermissionMode::BypassPermissions, PermissionMode::Default]
        );
    });
}

#[test]
fn session_hash_accepts_permission_mode_variant_payload() {
    block_on(async {
        let workspace = unique_workspace("sdk-old-session-permission-hash");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![vec![ModelStreamEvent::MessageStop]],
        ));
        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");
        let mut old_options = SessionOptions::new(&workspace)
            .with_session_id(session_id)
            .with_permission_mode(PermissionMode::BypassPermissions);
        old_options.workspace_root = old_options
            .workspace_root
            .canonicalize()
            .expect("workspace root should canonicalize");
        let current_options = SessionOptions::new(&workspace).with_session_id(session_id);

        store
            .append(
                TenantId::SINGLE,
                session_id,
                &[Event::SessionCreated(SessionCreatedEvent {
                    session_id,
                    tenant_id: TenantId::SINGLE,
                    options_hash: session_options_hash(&old_options),
                    snapshot_id: SnapshotId::from_u128(1),
                    effective_config_hash: ConfigHash([1; 32]),
                    created_at: harness_contracts::now(),
                })],
            )
            .await
            .expect("old session should be written");
        let receipt = harness
            .submit_conversation_turn(conversation_turn_request(
                current_options,
                ConversationTurnInput::ask("continue old conversation"),
                None,
                None,
                None,
            ))
            .await
            .expect("old session should continue under current default identity");

        assert_eq!(receipt.session_id, session_id);
    });
}

#[test]
fn conversation_session_hash_allows_permission_mode_variant() {
    block_on(async {
        let workspace = unique_workspace("sdk-current-session-permission-hash");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![
                vec![ModelStreamEvent::MessageStop],
                vec![ModelStreamEvent::MessageStop],
            ],
        ));
        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store)
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");
        harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_permission_mode(PermissionMode::BypassPermissions),
            )
            .await
            .expect("session should be created");

        let receipt = harness
            .submit_conversation_turn(conversation_turn_request(
                SessionOptions::new(&workspace).with_session_id(session_id),
                ConversationTurnInput::ask("continue current conversation"),
                None,
                None,
                None,
            ))
            .await
            .expect("permission mode changes are run-level and must not reject the session");

        assert_eq!(receipt.session_id, session_id);
    });
}

#[test]
fn session_options_hash_ignores_run_level_options() {
    let workspace = unique_workspace("sdk-session-options-runtime-hash");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = SessionId::new();

    let default_hash =
        session_options_hash(&SessionOptions::new(&workspace).with_session_id(session_id));
    let permission_hash = session_options_hash(
        &SessionOptions::new(&workspace)
            .with_session_id(session_id)
            .with_permission_mode(PermissionMode::BypassPermissions),
    );
    let compression_hash = session_options_hash(
        &SessionOptions::new(&workspace)
            .with_session_id(session_id)
            .with_context_compression_trigger_ratio(0.5),
    );

    assert_eq!(default_hash, permission_hash);
    assert_eq!(default_hash, compression_hash);
}

#[test]
fn runtime_assembly_conversation_allows_provider_model_switch_per_run() {
    block_on(async {
        let workspace = unique_workspace("sdk-runtime-assembly-run-model-switch");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let deepseek = Arc::new(
            CapabilityScriptedProvider::new(
                ConversationModelCapability::default(),
                vec![vec![ModelStreamEvent::MessageStop]],
            )
            .with_identity("deepseek", "deepseek-chat", "DeepSeek Chat"),
        );
        let minimax = Arc::new(
            CapabilityScriptedProvider::new(
                ConversationModelCapability::default(),
                vec![vec![ModelStreamEvent::MessageStop]],
            )
            .with_identity("minimax", "minimax-m3", "MiniMax M3"),
        );
        let deepseek_harness = Harness::builder()
            .with_model_arc(deepseek)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("deepseek harness should build");
        let minimax_harness = Harness::builder()
            .with_model_arc(minimax)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("minimax harness should build");
        let identity_options = SessionOptions::new(&workspace).with_session_id(session_id);
        let deepseek_options = identity_options.clone().with_model_id("deepseek-chat");
        deepseek_harness
            .open_or_create_conversation_session(deepseek_options.clone())
            .await
            .expect("conversation session should open with first run model");

        let deepseek_run_options = ConversationRunOptions::from_session_options(&deepseek_options)
            .with_model_config_id("deepseek-config");
        deepseek_harness
            .submit_conversation_turn(ConversationTurnRequest {
                options: identity_options.clone(),
                run_options: deepseek_run_options,
                input: ConversationTurnInput::ask("first run"),
                permission_actor_source: None,
            })
            .await
            .expect("deepseek run should submit");

        let minimax_options = identity_options.clone().with_model_id("minimax-m3");
        let minimax_run_options = ConversationRunOptions::from_session_options(&minimax_options)
            .with_model_config_id("minimax-config");
        let second = minimax_harness
            .submit_conversation_turn(ConversationTurnRequest {
                options: identity_options,
                run_options: minimax_run_options,
                input: ConversationTurnInput::ask("second run"),
                permission_actor_source: None,
            })
            .await
            .expect("minimax run should not hit session options mismatch");
        assert_eq!(second.session_id, session_id);

        let run_models: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .filter_map(|event| async move {
                match event {
                    Event::RunStarted(started) => Some(started.model),
                    _ => None,
                }
            })
            .collect()
            .await;
        assert_eq!(run_models.len(), 2);
        assert_eq!(run_models[0].provider_id, "deepseek");
        assert_eq!(run_models[0].model_id, "deepseek-chat");
        assert_eq!(
            run_models[0].model_config_id.as_deref(),
            Some("deepseek-config")
        );
        assert_eq!(run_models[1].provider_id, "minimax");
        assert_eq!(run_models[1].model_id, "minimax-m3");
        assert_eq!(
            run_models[1].model_config_id.as_deref(),
            Some("minimax-config")
        );
    });
}

#[test]
fn runtime_assembly_run_config_changes_do_not_block_session_and_change_run_hash() {
    block_on(async {
        let workspace = unique_workspace("sdk-runtime-assembly-run-config-hash");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![
                vec![ModelStreamEvent::MessageStop],
                vec![ModelStreamEvent::MessageStop],
            ],
        ));
        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        harness
            .open_or_create_conversation_session(options.clone().with_model_id("test-model"))
            .await
            .expect("conversation session should open");

        let first_run_options = ConversationRunOptions::from_session_options(
            &options.clone().with_model_id("test-model"),
        )
        .with_permission_mode(PermissionMode::Default)
        .with_tool_profile(ToolProfile::Full)
        .with_context_compression_trigger_ratio(0.8);
        harness
            .submit_conversation_turn(ConversationTurnRequest {
                options: options.clone(),
                run_options: first_run_options,
                input: ConversationTurnInput::ask("first config"),
                permission_actor_source: None,
            })
            .await
            .expect("first run should submit");

        let second_run_options = ConversationRunOptions::from_session_options(
            &options.clone().with_model_id("test-model"),
        )
        .with_permission_mode(PermissionMode::BypassPermissions)
        .with_tool_profile(ToolProfile::Minimal)
        .with_context_compression_trigger_ratio(0.6);
        harness
            .submit_conversation_turn(ConversationTurnRequest {
                options,
                run_options: second_run_options,
                input: ConversationTurnInput::ask("second config"),
                permission_actor_source: None,
            })
            .await
            .expect("run-level config changes must not reject the session");

        let run_starts: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .filter_map(|event| async move {
                match event {
                    Event::RunStarted(started) => Some(started),
                    _ => None,
                }
            })
            .collect()
            .await;
        assert_eq!(run_starts.len(), 2);
        assert_ne!(
            run_starts[0].effective_config_hash,
            run_starts[1].effective_config_hash
        );
        assert_eq!(run_starts[0].permission_mode, PermissionMode::Default);
        assert_eq!(
            run_starts[1].permission_mode,
            PermissionMode::BypassPermissions
        );
    });
}

#[test]
fn effective_config_hash_tracks_runtime_prompt_context() {
    block_on(async {
        let workspace = unique_workspace("sdk-effective-config-runtime-context");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

        let mut no_tool_calling = ConversationModelCapability::default();
        no_tool_calling.tool_calling = false;
        let first_model = Arc::new(CapabilityScriptedProvider::new(
            no_tool_calling,
            vec![vec![ModelStreamEvent::MessageStop]],
        ));
        let first_harness = Harness::builder()
            .with_model_arc(first_model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("first harness should build");
        first_harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("first session should be created");

        let second_model = Arc::new(CapabilityScriptedProvider::new(
            ConversationModelCapability::default(),
            vec![vec![ModelStreamEvent::MessageStop]],
        ));
        let second_harness = Harness::builder()
            .with_model_arc(second_model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("second harness should build");
        second_harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("second session should be created");

        let created_hashes = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .filter_map(|event| async move {
                match event {
                    Event::SessionCreated(created) => Some(created.effective_config_hash),
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
            .await;

        assert_eq!(created_hashes.len(), 2);
        assert_ne!(created_hashes[0], created_hashes[1]);
    });
}

#[tokio::test]
async fn conversation_facade_cancels_active_run_through_sdk_registry() {
    let workspace = unique_workspace("sdk-conversation-active-cancel");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let provider = Arc::new(BlockingSkillListProvider::new(ToolUseId::new()));

    let harness = Harness::builder()
        .with_model_arc(provider.clone())
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
        .expect("session should open through the conversation facade");

    let run_harness = harness.clone();
    let run_workspace = workspace.clone();
    let submitted = tokio::spawn(async move {
        run_harness
            .submit_conversation_turn(conversation_turn_request(
                SessionOptions::new(&run_workspace).with_session_id(session_id),
                ConversationTurnInput::ask("cancel active facade run"),
                None,
                None,
                None,
            ))
            .await
    });

    provider.started.notified().await;
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("events should be readable")
        .collect()
        .await;
    let run_id = events
        .iter()
        .find_map(|event| match event {
            Event::RunStarted(started) => Some(started.run_id),
            _ => None,
        })
        .expect("active run should have emitted RunStarted");

    harness
        .cancel_conversation_run(run_id)
        .await
        .expect("active run should cancel through the SDK facade");

    provider.release.notify_one();
    submitted
        .await
        .expect("submit task should join")
        .expect("cancelled run should finish cleanly");
}

#[tokio::test]
async fn conversation_facade_delete_cancels_active_run_and_blocks_late_appends() {
    let workspace = unique_workspace("sdk-conversation-delete-active-run");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let provider = Arc::new(BlockingSkillListProvider::new(ToolUseId::new()));

    let harness = Harness::builder()
        .with_model_arc(provider.clone())
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
        .expect("session should open through the conversation facade");

    let run_harness = harness.clone();
    let run_workspace = workspace.clone();
    let submitted = tokio::spawn(async move {
        run_harness
            .submit_conversation_turn(conversation_turn_request(
                SessionOptions::new(&run_workspace).with_session_id(session_id),
                ConversationTurnInput::ask("delete active facade run"),
                None,
                None,
                None,
            ))
            .await
    });

    provider.started.notified().await;
    let deleted = harness
        .delete_conversation_session(SessionOptions::new(&workspace).with_session_id(session_id))
        .await
        .expect("active conversation delete should reach the store");
    assert!(deleted);

    provider.release.notify_one();
    let error = submitted
        .await
        .expect("submit task should join")
        .expect_err("deleted sessions must reject late run appends");
    assert!(error
        .to_string()
        .contains("conversation session was deleted"));

    let sessions = harness
        .list_conversation_sessions(TenantId::SINGLE, 50)
        .await
        .expect("sessions should list after delete");
    assert!(sessions.is_empty());

    let reopen_error = harness
        .open_or_create_conversation_session(
            SessionOptions::new(&workspace).with_session_id(session_id),
        )
        .await
        .expect_err("deleted session ids must not be recreated in the same runtime");
    assert!(reopen_error.to_string().contains("session not found"));
}

#[tokio::test]
async fn conversation_facade_hides_and_deletes_malformed_session_streams() {
    let workspace = unique_workspace("sdk-conversation-malformed-stream");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

    store
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::ToolDeferredPoolChanged(
                ToolDeferredPoolChangedEvent {
                    session_id,
                    added: Vec::new(),
                    removed: Vec::new(),
                    source: ToolPoolChangeSource::InitialClassification,
                    deferred_total: 0,
                    at: harness_contracts::now(),
                },
            )],
        )
        .await
        .expect("malformed stream should be written for the regression test");

    let harness = Harness::builder()
        .with_model(TestModelProvider::default())
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");

    let sessions = harness
        .list_conversation_sessions(TenantId::SINGLE, 50)
        .await
        .expect("malformed streams should not break conversation listing");
    assert!(sessions
        .iter()
        .all(|session| session.session_id != session_id));

    let read_error = harness
        .page_conversation_events(ConversationEventsPageRequest {
            options: SessionOptions::new(&workspace).with_session_id(session_id),
            after_event_id: None,
            limit: 10,
        })
        .await
        .expect_err("malformed streams must still fail closed on reads");
    assert!(read_error
        .to_string()
        .contains("session event stream does not start with SessionCreated"));

    let deleted = harness
        .delete_conversation_session(SessionOptions::new(&workspace).with_session_id(session_id))
        .await
        .expect("malformed streams should be directly deletable");
    assert!(deleted);

    let remaining: Vec<_> = store
        .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("store read should succeed after delete")
        .collect()
        .await;
    assert!(remaining.is_empty());
}

#[test]
fn conversation_facade_rejects_tenant_policy_bypass_before_reading_events() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-tenant-boundary");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

        let permissive = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_tenant_policy(TenantPolicy {
                allow_scoped_tenants: true,
                ..TenantPolicy::default()
            })
            .build()
            .await
            .expect("permissive harness should build");
        permissive
            .create_session(
                SessionOptions::new(&workspace)
                    .with_tenant_id(TenantId::SHARED)
                    .with_session_id(session_id),
            )
            .await
            .expect("shared tenant session should be created");

        let restricted = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store)
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("restricted harness should build");

        let error = restricted
            .open_or_create_conversation_session(
                SessionOptions::new(&workspace)
                    .with_tenant_id(TenantId::SHARED)
                    .with_session_id(session_id),
            )
            .await
            .expect_err("restricted tenant policy must block open before event replay");
        assert!(matches!(error, HarnessError::InvalidTenant(tenant) if tenant == TenantId::SHARED));

        let error = restricted
            .page_conversation_events(ConversationEventsPageRequest {
                options: SessionOptions::new(&workspace)
                    .with_tenant_id(TenantId::SHARED)
                    .with_session_id(session_id),
                after_event_id: None,
                limit: 10,
            })
            .await
            .expect_err("restricted tenant policy must block event paging");
        assert!(matches!(error, HarnessError::InvalidTenant(tenant) if tenant == TenantId::SHARED));

        let error = restricted
            .submit_conversation_turn(conversation_turn_request(
                SessionOptions::new(&workspace)
                    .with_tenant_id(TenantId::SHARED)
                    .with_session_id(session_id),
                ConversationTurnInput::ask("must not read shared tenant"),
                None,
                None,
                None,
            ))
            .await
            .expect_err("restricted tenant policy must block submit before event replay");
        assert!(matches!(error, HarnessError::InvalidTenant(tenant) if tenant == TenantId::SHARED));
    });
}

#[test]
fn conversation_facade_reopens_with_workspace_bound_options() {
    block_on(async {
        let workspace_root = unique_workspace("sdk-conversation-workspace-bound");
        std::fs::create_dir_all(&workspace_root).unwrap();
        let session_id = SessionId::new();
        let model = Arc::new(TestModelProvider::default().with_events(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("workspace answer".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]));
        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");
        let workspace = harness
            .create_workspace(
                WorkspaceSpec::new(&workspace_root, "Conversation Workspace")
                    .with_default_session_options(
                        SessionOptions::default().with_model_id("test-model"),
                    ),
            )
            .await
            .expect("workspace should be registered");
        let options = SessionOptions::default()
            .with_workspace(workspace.id)
            .with_session_id(session_id);

        harness
            .open_or_create_conversation_session(options.clone())
            .await
            .expect("workspace-bound conversation should open");
        harness
            .submit_conversation_turn(conversation_turn_request(
                options.clone(),
                ConversationTurnInput::ask("use workspace model"),
                None,
                None,
                None,
            ))
            .await
            .expect("workspace-bound conversation should submit");

        let requests = model.requests().await;
        assert_eq!(requests[0].model_id, "test-model");

        let mismatched = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: SessionOptions::new(&workspace_root).with_session_id(session_id),
                after_event_id: None,
                limit: 10,
            })
            .await
            .expect_err("mismatched session options must not replay an existing conversation");
        assert!(matches!(mismatched, HarnessError::PermissionDenied(_)));
    });
}

#[test]
fn conversation_facade_rejects_duplicate_session_created_with_mismatched_options() {
    block_on(async {
        let workspace = unique_workspace("sdk-conversation-duplicate-created");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");
        let options = SessionOptions::new(&workspace).with_session_id(session_id);

        harness
            .open_or_create_conversation_session(options.clone())
            .await
            .expect("session should be created");
        store
            .append(
                TenantId::SINGLE,
                session_id,
                &[Event::SessionCreated(SessionCreatedEvent {
                    session_id,
                    tenant_id: TenantId::SINGLE,
                    options_hash: [1; 32],
                    snapshot_id: SnapshotId::from_u128(0),
                    effective_config_hash: ConfigHash([1; 32]),
                    created_at: harness_contracts::now(),
                })],
            )
            .await
            .expect("duplicate created event should append");

        let error = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options,
                after_event_id: None,
                limit: 10,
            })
            .await
            .expect_err("mismatched duplicate SessionCreated must be rejected");
        assert!(matches!(error, HarnessError::PermissionDenied(_)));
    });
}
