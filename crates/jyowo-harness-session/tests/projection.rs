use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{
    ActionPlanHash, AssistantMessageCompletedEvent, CacheImpact, CompactOutcome, CompactTrigger,
    CompactionAppliedEvent, DecidedBy, Decision, DecisionId, DecisionLifetime, DecisionMatcherKind,
    DecisionMatcherSummary, DecisionScope, DeferPolicy, DeferredToolHint, EndReason, Event,
    EventId, MessageContent, MessageId, MessageMetadata, NoopRedactor, PermissionActorSource,
    PermissionDecisionOption, PermissionMode, PermissionOptionId, PermissionRequestedEvent,
    PermissionResolvedEvent, PermissionReview, PermissionSubject, RequestId, RunEndedEvent, RunId,
    SandboxPolicySummary, SessionCreatedEvent, SessionEndedEvent, SessionId, Severity, StopReason,
    TenantId, ToolDeferredPoolChangedEvent, ToolPoolChangeSource, ToolProperties, ToolResult,
    ToolSchemaMaterializedEvent, ToolUseCompletedEvent, ToolUseId, ToolUseRequestedEvent,
    UsageSnapshot,
};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_session::SessionProjection;
use serde_json::json;

#[tokio::test]
async fn projection_replay_is_idempotent() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let run = RunId::new();
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    let store = event_store();
    let events = vec![
        Event::SessionCreated(SessionCreatedEvent {
            session_id: session,
            tenant_id: tenant,
            options_hash: [0; 32],
            snapshot_id: harness_contracts::SnapshotId::from_u128(7),
            effective_config_hash: harness_contracts::ConfigHash([1; 32]),
            created_at: harness_contracts::now(),
        }),
        Event::UserMessageAppended(harness_contracts::UserMessageAppendedEvent {
            run_id: run,
            message_id: MessageId::new(),
            content: MessageContent::Text("list files".to_owned()),
            metadata: MessageMetadata::default(),
            attachments: Vec::new(),
            at: harness_contracts::now(),
        }),
        Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
            run_id: run,
            message_id: MessageId::new(),
            content: MessageContent::Text("calling tool".to_owned()),
            tool_uses: Vec::new(),
            usage: usage(10, 4),
            pricing_snapshot_id: None,
            stop_reason: StopReason::ToolUse,
            at: harness_contracts::now(),
        }),
        Event::ToolUseRequested(ToolUseRequestedEvent {
            run_id: run,
            tool_use_id,
            tool_name: "list_dir".to_owned(),
            input: json!({"path":"."}),
            properties: read_only_tool(),
            causation_id: EventId::new(),
            at: harness_contracts::now(),
        }),
        Event::PermissionRequested(PermissionRequestedEvent {
            request_id,
            run_id: run,
            session_id: session,
            tenant_id: tenant,
            tool_use_id,
            tool_name: "list_dir".to_owned(),
            subject: PermissionSubject::ToolInvocation {
                tool: "list_dir".to_owned(),
                input: json!({"path":"."}),
            },
            severity: Severity::Low,
            scope_hint: DecisionScope::ToolName("list_dir".to_owned()),
            fingerprint: None,
            presented_options: vec![PermissionDecisionOption {
                option_id: PermissionOptionId::new(),
                decision: Decision::AllowOnce,
                scope: DecisionScope::Any,
                lifetime: DecisionLifetime::Once,
                matcher_summary: DecisionMatcherSummary {
                    kind: DecisionMatcherKind::Any,
                    label: "allow once".to_owned(),
                },
                label: "Allow once".to_owned(),
                requires_confirmation: false,
                action_plan_hash: ActionPlanHash::default(),
                fingerprint: None,
            }],
            interactivity: harness_contracts::InteractivityLevel::FullyInteractive,
            auto_resolved: false,
            actor_source: PermissionActorSource::ParentRun,
            action_plan_hash: ActionPlanHash::default(),
            review: PermissionReview::default(),
            effective_mode: PermissionMode::Default,
            sandbox_policy: SandboxPolicySummary::default(),
            causation_id: EventId::new(),
            at: harness_contracts::now(),
        }),
        Event::PermissionResolved(PermissionResolvedEvent {
            request_id,
            decision: Decision::AllowSession,
            decided_by: DecidedBy::User,
            scope: DecisionScope::ToolName("list_dir".to_owned()),
            fingerprint: None,
            rationale: Some("ok".to_owned()),
            action_plan_hash: ActionPlanHash::default(),
            decision_id: DecisionId::new(),
            auto_resolved: false,
            at: harness_contracts::now(),
        }),
        Event::ToolUseCompleted(ToolUseCompletedEvent {
            tool_use_id,
            result: ToolResult::Text("Cargo.toml".to_owned()),
            usage: Some(usage(0, 2)),
            duration_ms: 12,
            at: harness_contracts::now(),
        }),
        Event::RunEnded(RunEndedEvent {
            run_id: run,
            reason: EndReason::Completed,
            usage: Some(usage(1, 1)),
            ended_at: harness_contracts::now(),
        }),
    ];
    store.append(tenant, session, &events).await.unwrap();
    let envelopes = store
        .read_envelopes(tenant, session, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    let first = SessionProjection::replay(envelopes.clone()).unwrap();
    let second = SessionProjection::replay(envelopes).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.messages.len(), 2);
    assert_eq!(
        first.tool_uses.get(&tool_use_id).unwrap().result.as_ref(),
        Some(&ToolResult::Text("Cargo.toml".to_owned()))
    );
    assert_eq!(first.permission_log.len(), 1);
    assert_eq!(first.allowlist.len(), 1);
    assert_eq!(first.usage.output_tokens, 7);
    assert_eq!(first.end_reason, None);
}

#[tokio::test]
async fn session_ended_sets_projection_end_reason() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let envelopes = envelopes(
        tenant,
        session,
        vec![Event::SessionEnded(SessionEndedEvent {
            session_id: session,
            tenant_id: tenant,
            reason: EndReason::Completed,
            final_usage: usage(3, 4),
            at: harness_contracts::now(),
        })],
    )
    .await;

    let projection = SessionProjection::replay(envelopes).unwrap();

    assert_eq!(projection.end_reason, Some(EndReason::Completed));
    assert_eq!(projection.usage.input_tokens, 3);
    assert_eq!(projection.usage.output_tokens, 4);
}

#[tokio::test]
async fn discovered_tools_are_materialized_removed_and_cleared_by_compaction() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let run = RunId::new();
    let tool_use_id = ToolUseId::new();
    let envelopes = envelopes(
        tenant,
        session,
        vec![
            Event::ToolSchemaMaterialized(ToolSchemaMaterializedEvent {
                session_id: session,
                run_id: run,
                tool_use_id,
                names: vec!["grep".to_owned(), "glob".to_owned()],
                backend: "inline".to_owned(),
                cache_impact: CacheImpact {
                    prompt_cache_invalidated: true,
                    reason: Some("materialized".to_owned()),
                },
                triggered_session_reload: false,
                coalesced_count: 0,
                at: harness_contracts::now(),
            }),
            Event::ToolDeferredPoolChanged(ToolDeferredPoolChangedEvent {
                session_id: session,
                added: vec![DeferredToolHint {
                    name: "read_file".to_owned(),
                    hint: None,
                }],
                removed: vec!["glob".to_owned()],
                source: ToolPoolChangeSource::InitialClassification,
                deferred_total: 1,
                at: harness_contracts::now(),
            }),
            Event::CompactionApplied(CompactionAppliedEvent {
                session_id: session,
                strategy: "autocompact".to_owned(),
                trigger: CompactTrigger::HardBudget,
                outcome: CompactOutcome::Succeeded,
                before_tokens: 100,
                after_tokens: 20,
                summary_ref: blob_ref(),
                child_session_id: Some(SessionId::new()),
                handoff: None,
                at: harness_contracts::now(),
            }),
        ],
    )
    .await;

    let projection = SessionProjection::replay(envelopes).unwrap();

    assert_eq!(projection.discovered_tools.len(), 0);
}

#[tokio::test]
async fn deferred_pool_change_generates_pending_delta_and_take_clears_it() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let envelopes = envelopes(
        tenant,
        session,
        vec![Event::ToolDeferredPoolChanged(
            ToolDeferredPoolChangedEvent {
                session_id: session,
                added: vec![DeferredToolHint {
                    name: "mcp__fixture__lookup".to_owned(),
                    hint: None,
                }],
                removed: vec!["legacy_tool".to_owned()],
                source: ToolPoolChangeSource::InitialClassification,
                deferred_total: 1,
                at: harness_contracts::now(),
            },
        )],
    )
    .await;

    let mut projection = SessionProjection::replay(envelopes).unwrap();
    let pending = projection
        .pending_deferred_tools_delta()
        .expect("deferred pool change should queue a prompt delta");
    assert_eq!(pending.added_names, vec!["mcp__fixture__lookup".to_owned()]);
    assert_eq!(pending.removed_names, vec!["legacy_tool".to_owned()]);
    assert!(pending.initial);

    let taken = projection
        .take_pending_deferred_tools_delta()
        .expect("pending delta should be consumable by the next turn");
    assert_eq!(taken.added_names, vec!["mcp__fixture__lookup".to_owned()]);
    assert!(projection.pending_deferred_tools_delta().is_none());
}

#[tokio::test]
async fn materialized_tools_are_removed_from_pending_deferred_delta() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let run = RunId::new();
    let tool_use_id = ToolUseId::new();
    let envelopes = envelopes(
        tenant,
        session,
        vec![
            Event::ToolDeferredPoolChanged(ToolDeferredPoolChangedEvent {
                session_id: session,
                added: vec![
                    DeferredToolHint {
                        name: "grep".to_owned(),
                        hint: None,
                    },
                    DeferredToolHint {
                        name: "glob".to_owned(),
                        hint: None,
                    },
                ],
                removed: Vec::new(),
                source: ToolPoolChangeSource::InitialClassification,
                deferred_total: 2,
                at: harness_contracts::now(),
            }),
            Event::ToolSchemaMaterialized(ToolSchemaMaterializedEvent {
                session_id: session,
                run_id: run,
                tool_use_id,
                names: vec!["grep".to_owned()],
                backend: "inline_reinjection".to_owned(),
                cache_impact: CacheImpact {
                    prompt_cache_invalidated: true,
                    reason: Some("materialized".to_owned()),
                },
                triggered_session_reload: true,
                coalesced_count: 0,
                at: harness_contracts::now(),
            }),
        ],
    )
    .await;

    let projection = SessionProjection::replay(envelopes).unwrap();
    let pending = projection
        .pending_deferred_tools_delta()
        .expect("unmaterialized deferred tool should stay pending");
    assert_eq!(pending.added_names, vec!["glob".to_owned()]);
}

#[tokio::test]
async fn snapshot_id_is_stable_across_tool_map_insertion_order() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let run = RunId::new();
    let first_tool = ToolUseId::new();
    let second_tool = ToolUseId::new();

    let first = SessionProjection::replay(
        envelopes(
            tenant,
            session,
            vec![
                tool_requested(run, first_tool, "read_file"),
                tool_requested(run, second_tool, "list_dir"),
            ],
        )
        .await,
    )
    .unwrap();
    let second = SessionProjection::replay(
        envelopes(
            tenant,
            session,
            vec![
                tool_requested(run, second_tool, "list_dir"),
                tool_requested(run, first_tool, "read_file"),
            ],
        )
        .await,
    )
    .unwrap();

    assert_eq!(first.snapshot_id, second.snapshot_id);
}

fn event_store() -> Arc<InMemoryEventStore> {
    Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)))
}

async fn envelopes(
    tenant: TenantId,
    session: SessionId,
    events: Vec<Event>,
) -> Vec<harness_journal::EventEnvelope> {
    let store = event_store();
    store.append(tenant, session, &events).await.unwrap();
    store
        .read_envelopes(tenant, session, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await
}

fn read_only_tool() -> ToolProperties {
    ToolProperties {
        is_concurrency_safe: true,
        is_read_only: true,
        is_destructive: false,
        long_running: None,
        defer_policy: DeferPolicy::AlwaysLoad,
    }
}

fn usage(input_tokens: u64, output_tokens: u64) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens,
        output_tokens,
        ..UsageSnapshot::default()
    }
}

fn tool_requested(run: RunId, tool_use_id: ToolUseId, tool_name: &str) -> Event {
    Event::ToolUseRequested(ToolUseRequestedEvent {
        run_id: run,
        tool_use_id,
        tool_name: tool_name.to_owned(),
        input: json!({}),
        properties: read_only_tool(),
        causation_id: EventId::new(),
        at: harness_contracts::now(),
    })
}

fn blob_ref() -> harness_contracts::BlobRef {
    harness_contracts::BlobRef {
        id: harness_contracts::BlobId::new(),
        size: 1,
        content_hash: [9; 32],
        content_type: Some("text/plain".to_owned()),
    }
}
