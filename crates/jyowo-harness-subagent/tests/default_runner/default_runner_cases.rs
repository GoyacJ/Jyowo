use super::*;

#[tokio::test]
async fn default_runner_creates_child_session_runs_child_and_journals_lifecycle() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let runner = DefaultSubagentRunner::new(
        child.clone(),
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);

    let spec = SubagentSpec::minimal("reviewer", "inspect");
    let announcement = runner
        .spawn(spec.clone(), test_input("inspect"), parent.clone())
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.status, SubagentStatus::Completed);
    assert_eq!(announcement.summary, "child completed");
    let request = child.request.lock().await.clone().unwrap();
    assert_ne!(request.child_session_id, parent.parent_session_id);
    assert_eq!(request.parent_session_id, parent.parent_session_id);
    assert_eq!(request.spec, spec);
    assert_eq!(request.child_depth, parent.depth + 1);
    assert_eq!(request.correlation_id, parent.correlation_id);
    assert!(request.context_seed.is_empty());

    let parent_envelopes: Vec<_> = store
        .read_envelopes(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_envelopes
        .iter()
        .all(|envelope| envelope.correlation_id == parent.correlation_id));
    assert!(parent_envelopes
        .iter()
        .any(|envelope| matches!(envelope.payload, Event::SubagentSpawned(ref event) if event.spec_snapshot_id != harness_contracts::SnapshotId::from_u128(0))));
    assert!(parent_envelopes
        .iter()
        .any(|envelope| matches!(envelope.payload, Event::SubagentAnnounced(_))));
    let announced_index = parent_envelopes
        .iter()
        .position(|envelope| matches!(envelope.payload, Event::SubagentAnnounced(_)))
        .expect("subagent announcement should be journaled");
    let injected_index = parent_envelopes
        .iter()
        .position(|envelope| {
            matches!(
                &envelope.payload,
                Event::UserMessageAppended(appended)
                    if appended.metadata.source.as_deref() == Some("subagent")
                        && appended.metadata.labels.get("renderer_id")
                            == Some(&"xml-task-notification".to_owned())
                        && matches!(&appended.content, MessageContent::Text(text)
                            if text.contains("<task-notification>")
                                && text.contains("<rewrite-hint>"))
            )
        })
        .expect("subagent announcement should be injected as a parent user message");
    assert!(
        announced_index < injected_index,
        "SubagentAnnounced must precede UserMessageAppended"
    );
    assert!(parent_envelopes.iter().any(|envelope| {
        matches!(
            envelope.payload,
            Event::SubagentTerminated(ref event)
                if event.reason == SubagentTerminationReason::NaturalCompletion
        )
    }));

    let child_events: Vec<_> = store
        .read(
            parent.tenant_id,
            request.child_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(child_events
        .iter()
        .any(|event| matches!(event, Event::SessionCreated(_))));
}

#[tokio::test]
async fn default_runner_uses_aux_summarizer_for_announcement_summary() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let aux = Arc::new(RecordingAuxProvider::new(Ok(
        "aux rewritten summary".to_owned()
    )));
    let runner = DefaultSubagentRunner::new(
        child,
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    )
    .with_announcement_summarizer(Arc::new(AuxAnnouncementSummarizer::new(aux.clone())));

    let announcement = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            ParentContext::for_test(0),
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.summary, "aux rewritten summary");
    assert_eq!(aux.tasks().await, vec![AuxTask::Summarize]);
}

#[tokio::test]
async fn default_runner_accepts_engine_factory_as_production_path() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let factory = Arc::new(RecordingEngineFactory::default());
    let child_session_id = SessionId::new();
    let runner = DefaultSubagentRunner::new_with_engine_factory(
        factory.clone(),
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    )
    .with_child_session_id(child_session_id);
    let parent = ParentContext::for_test(0);

    let announcement = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            parent.clone(),
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.status, SubagentStatus::Completed);
    assert_eq!(
        factory
            .request
            .lock()
            .await
            .as_ref()
            .unwrap()
            .child_session_id,
        child_session_id
    );
    assert_eq!(
        factory
            .request
            .lock()
            .await
            .as_ref()
            .unwrap()
            .correlation_id,
        parent.correlation_id
    );
}

#[tokio::test]
async fn external_lifecycle_owner_runs_isolated_child_without_parent_journal_writes() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let factory = Arc::new(RecordingEngineFactory::default());
    let child_session_id = SessionId::new();
    let runner = DefaultSubagentRunner::new_with_engine_factory(
        factory.clone(),
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    )
    .with_child_session_id(child_session_id)
    .with_external_lifecycle_owner();
    let parent = ParentContext::for_test(0);

    let announcement = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            parent.clone(),
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.status, SubagentStatus::Completed);
    assert_eq!(
        factory
            .request
            .lock()
            .await
            .as_ref()
            .unwrap()
            .child_session_id,
        child_session_id
    );
    assert!(store
        .read(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await
        .is_empty());
}

#[tokio::test]
async fn external_lifecycle_owner_rejects_parent_fork_context() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let runner = DefaultSubagentRunner::new_with_engine_factory(
        Arc::new(RecordingEngineFactory::default()),
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    )
    .with_child_session_id(SessionId::new())
    .with_external_lifecycle_owner();
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.context_mode = SubagentContextMode::ForkFromParent {
        include_tool_results: false,
    };

    let error = runner
        .spawn(spec, test_input("inspect"), ParentContext::for_test(0))
        .await
        .expect_err("daemon-owned runner must not read a parent stream through the child store");

    assert!(error
        .to_string()
        .contains("externally owned subagent lifecycle requires isolated context"));
}

#[tokio::test]
async fn default_runner_keeps_original_summary_when_aux_summarizer_fails_open() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let aux = Arc::new(RecordingAuxProvider::new(Err(
        ModelError::ProviderUnavailable("aux down".to_owned()),
    )));
    let runner = DefaultSubagentRunner::new(
        child,
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    )
    .with_announcement_summarizer(Arc::new(AuxAnnouncementSummarizer::new(aux)));

    let announcement = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            ParentContext::for_test(0),
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.summary, "child completed");
}

#[tokio::test]
async fn structured_announcement_drops_child_transcript_ref() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(TranscriptChildRunner);
    let runner = DefaultSubagentRunner::new(
        child,
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);
    let spec = SubagentSpec::minimal("reviewer", "inspect");

    let announcement = runner
        .spawn(spec, test_input("inspect"), parent.clone())
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.transcript_ref, None);
    let parent_events: Vec<_> = store
        .read(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentAnnounced(announced) if announced.transcript_ref.is_none()
        )
    }));
}

#[tokio::test]
async fn full_transcript_announcement_keeps_child_transcript_ref() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(TranscriptChildRunner);
    let runner = DefaultSubagentRunner::new(
        child,
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.announce_mode = AnnounceMode::FullTranscript;

    let announcement = runner
        .spawn(spec, test_input("inspect"), parent)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let transcript_ref = announcement
        .transcript_ref
        .expect("full transcript mode should retain transcript ref");
    assert_eq!(transcript_ref.from_offset, JournalOffset(1));
    assert_eq!(transcript_ref.to_offset, JournalOffset(2));
    assert_eq!(
        transcript_ref.blob.content_hash,
        *blake3::hash(b"[]").as_bytes()
    );
}

#[tokio::test]
async fn default_runner_persists_child_context_report_on_announcement() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(ContextReportChildRunner);
    let runner = DefaultSubagentRunner::new(
        child,
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);

    let announcement = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            parent.clone(),
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let expected = fake_context_report();
    assert_eq!(announcement.context_report, Some(expected.clone()));
    let events: Vec<_> = store
        .read(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentAnnounced(announced)
                if announced.context_report == Some(expected.clone())
        )
    }));
}

#[tokio::test]
async fn fork_latest_user_seeds_only_latest_parent_user_and_writes_session_forked() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let runner = DefaultSubagentRunner::new(
        child.clone(),
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);
    let parent_events = parent_transcript_events("old user", "assistant context", "latest user");
    let last_parent_offset = store
        .append(parent.tenant_id, parent.parent_session_id, &parent_events)
        .await
        .unwrap();

    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.context_mode = SubagentContextMode::ForkFromParent {
        include_tool_results: false,
    };
    spec.input_strategy = SubagentInputStrategy::LatestUserOnly;
    let announcement = runner
        .spawn(spec, test_input("inspect"), parent.clone())
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.status, SubagentStatus::Completed);
    let request = child.request.lock().await.clone().unwrap();
    assert_eq!(message_texts(&request.context_seed), vec!["latest user"]);

    let parent_envelopes: Vec<_> = store
        .read_envelopes(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_envelopes.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::SessionForked(forked)
                if forked.parent_session_id == parent.parent_session_id
                    && forked.child_session_id == request.child_session_id
                    && forked.from_offset == last_parent_offset
                    && !forked.cache_impact.prompt_cache_invalidated
        )
    }));
}

#[tokio::test]
async fn fork_inherit_all_can_exclude_tool_results() {
    let request = spawn_with_context_seed(
        SubagentContextMode::ForkFromParent {
            include_tool_results: false,
        },
        SubagentInputStrategy::InheritAll,
    )
    .await;

    assert_eq!(
        message_roles(&request.context_seed),
        vec![MessageRole::User, MessageRole::Assistant, MessageRole::User]
    );
    assert_eq!(
        message_texts(&request.context_seed),
        vec!["old user", "assistant context", "latest user"]
    );
}

#[tokio::test]
async fn fork_inherit_all_can_include_tool_results() {
    let request = spawn_with_context_seed(
        SubagentContextMode::ForkFromParent {
            include_tool_results: true,
        },
        SubagentInputStrategy::InheritAll,
    )
    .await;

    assert_eq!(
        message_roles(&request.context_seed),
        vec![
            MessageRole::User,
            MessageRole::Assistant,
            MessageRole::Tool,
            MessageRole::User
        ]
    );
    assert!(matches!(
        request.context_seed[2].parts.as_slice(),
        [MessagePart::ToolResult { content: ToolResult::Text(text), .. }] if text == "tool output"
    ));
}

#[tokio::test]
async fn custom_input_strategy_fails_closed_before_child_run() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let runner = DefaultSubagentRunner::new(
        child.clone(),
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.context_mode = SubagentContextMode::ForkFromParent {
        include_tool_results: false,
    };
    spec.input_strategy = SubagentInputStrategy::Custom {
        selector_id: "missing-selector".to_owned(),
    };

    let error = runner
        .spawn(spec, test_input("inspect"), parent)
        .await
        .unwrap_err();

    assert!(
        error.to_string().contains("missing-selector"),
        "unexpected error: {error}"
    );
    assert!(child.request.lock().await.is_none());
}

#[tokio::test]
async fn custom_input_strategy_uses_registered_selector() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let runner = DefaultSubagentRunner::new(
        child.clone(),
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    )
    .with_input_selector(Arc::new(AssistantOnlySelector));
    let parent = ParentContext::for_test(0);
    store
        .append(
            parent.tenant_id,
            parent.parent_session_id,
            &parent_transcript_events("old user", "assistant context", "latest user"),
        )
        .await
        .unwrap();
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.context_mode = SubagentContextMode::ForkFromParent {
        include_tool_results: true,
    };
    spec.input_strategy = SubagentInputStrategy::Custom {
        selector_id: "assistant-only".to_owned(),
    };

    runner
        .spawn(spec, test_input("inspect"), parent)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let request = child.request.lock().await.clone().unwrap();
    assert_eq!(
        message_roles(&request.context_seed),
        vec![MessageRole::Assistant]
    );
    assert_eq!(
        message_texts(&request.context_seed),
        vec!["assistant context"]
    );
}

#[tokio::test]
async fn memory_scope_subset_uses_registered_resolver() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let runner = DefaultSubagentRunner::new(
        child.clone(),
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    )
    .with_memory_scope_resolver(Arc::new(TagMemoryResolver));
    let parent = ParentContext::for_test(0);
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.memory_scope = SubagentMemoryScope::Subset {
        selectors: vec![MemorySelector::Tag("safe".to_owned())],
    };

    runner
        .spawn(spec, test_input("inspect"), parent)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let request = child.request.lock().await.clone().unwrap();
    assert!(request.memory_scope_resolved);
    assert_eq!(
        message_texts(&request.context_seed),
        vec!["memory tag: safe"]
    );
}

#[tokio::test]
async fn memory_scope_empty_resolves_without_resolver() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(RecordingChildRunner::default());
    let runner = DefaultSubagentRunner::new(
        child.clone(),
        store,
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.memory_scope = SubagentMemoryScope::Empty;

    runner
        .spawn(spec, test_input("inspect"), parent)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let request = child.request.lock().await.clone().unwrap();
    assert!(request.memory_scope_resolved);
    assert!(request.context_seed.is_empty());
}

#[tokio::test]
async fn default_runner_terminates_failed_child_runs() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let runner = DefaultSubagentRunner::new(
        Arc::new(FailingChildRunner),
        store.clone(),
        workspace.path(),
        harness_subagent::DelegationPolicy::default(),
    );
    let parent = ParentContext::for_test(0);

    let err = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            test_input("inspect"),
            parent.clone(),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, harness_subagent::SubagentError::Engine(_)));
    let parent_events: Vec<_> = store
        .read(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(terminated.reason, SubagentTerminationReason::Failed { .. })
        )
    }));
}

#[tokio::test]
async fn runner_watchdog_tick_cancels_stalled_child_and_writes_termination() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(CancellableChildRunner::default());
    let pool = ConcurrentSubagentPool::with_policy(ConcurrencyPolicy {
        per_bucket_limit: 1,
        global_limit: 128,
        acquire_timeout: Duration::from_millis(10),
        activity_timeout: Duration::ZERO,
    });
    let runner = Arc::new(
        DefaultSubagentRunner::new(
            child.clone(),
            store.clone(),
            workspace.path(),
            harness_subagent::DelegationPolicy::default(),
        )
        .with_pool(pool),
    );
    let parent = ParentContext::for_test(0);
    let spawn = {
        let runner = runner.clone();
        let parent = parent.clone();
        tokio::spawn(async move {
            runner
                .spawn(
                    SubagentSpec::minimal("reviewer", "inspect"),
                    test_input("inspect"),
                    parent,
                )
                .await
        })
    };

    child.started.notified().await;
    let cancelled = runner.watchdog_tick().await.unwrap();
    assert_eq!(cancelled.len(), 1);
    let result = spawn.await.unwrap();
    assert!(matches!(
        result,
        Err(harness_subagent::SubagentError::Cancelled)
    ));

    let parent_events: Vec<_> = store
        .read(
            parent.tenant_id,
            parent.parent_session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect()
        .await;
    assert_eq!(
        parent_events
            .iter()
            .filter(|event| matches!(event, Event::SubagentTerminated(_)))
            .count(),
        1
    );
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(terminated.reason, SubagentTerminationReason::Stalled { .. })
        )
    }));
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentStalled(stalled) if stalled.subagent_id == cancelled[0].subagent_id
        )
    }));
}

#[test]
fn runner_starts_watchdog_lazily_when_constructed_outside_runtime() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let child = Arc::new(CancellableChildRunner::default());
    let pool = ConcurrentSubagentPool::with_policy(ConcurrencyPolicy {
        per_bucket_limit: 1,
        global_limit: 128,
        acquire_timeout: Duration::from_millis(10),
        activity_timeout: Duration::ZERO,
    });
    let runner = Arc::new(
        DefaultSubagentRunner::new(
            child.clone(),
            store.clone(),
            workspace.path(),
            harness_subagent::DelegationPolicy::default(),
        )
        .with_pool(pool)
        .with_watchdog_interval(Duration::from_millis(10)),
    );
    let parent = ParentContext::for_test(0);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async move {
        let spawn = {
            let runner = runner.clone();
            let parent = parent.clone();
            tokio::spawn(async move {
                runner
                    .spawn(
                        SubagentSpec::minimal("reviewer", "inspect"),
                        test_input("inspect"),
                        parent,
                    )
                    .await
            })
        };

        child.started.notified().await;
        let result = tokio::time::timeout(Duration::from_secs(1), spawn)
            .await
            .expect("lazy watchdog should cancel stalled child")
            .unwrap();
        assert!(matches!(
            result,
            Err(harness_subagent::SubagentError::Cancelled)
        ));

        let parent_events = wait_for_terminated_events(store.as_ref(), &parent, 1).await;
        assert_eq!(
            parent_events
                .iter()
                .filter(|event| matches!(event, Event::SubagentTerminated(_)))
                .count(),
            1
        );
        assert!(parent_events.iter().any(|event| {
            matches!(
                event,
                Event::SubagentTerminated(terminated)
                    if matches!(terminated.reason, SubagentTerminationReason::Stalled { .. })
            )
        }));
    });
}
