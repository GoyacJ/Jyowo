#![cfg(feature = "replay")]

use std::sync::Arc;
use std::task::{Context, Poll};

use futures::StreamExt;
use harness_contracts::{
    AgentId, AgentRef, AssistantMessageCompletedEvent, BackgroundAgentId,
    BackgroundAgentInputSubmittedEvent, BlobId, BlobRef, ConfigHash, ContextVisibility,
    DeferPolicy, EndReason, Event, MessageContent, MessageId, MessageMetadata, ModelRef,
    NoopRedactor, PricingSnapshotId, RequestId, RunEndedEvent, RunId, SessionCreatedEvent,
    SessionEndedEvent, SessionId, SnapshotId, StopReason, SubagentAnnouncedEvent,
    SubagentSpawnedEvent, SubagentStatus, TeamCreatedEvent, TeamId, TeamMemberJoinedEvent,
    TeamTerminationReason, TeamTurnCompletedEvent, TenantId, ToolProperties, ToolResult,
    ToolUseCompletedEvent, ToolUseId, ToolUseRequestedEvent, TopologyKind, TranscriptRef,
    UsageAccumulatedEvent, UsageSnapshot, UserMessageAppendedEvent,
};
use harness_journal::{
    EventStore, InMemoryEventStore, Projection, ReplayCursor, SessionProjection,
};
use harness_observability::{
    DefaultRedactor, ExportFormat, PricingBillingMode, PricingSource, PricingTableEntry,
    ReplayEngine,
};
use rust_decimal::Decimal;
use tokio::io::AsyncWrite;

#[tokio::test]
async fn replay_stream_respects_event_store_cursor() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let store = event_store();
    let events = session_events(session, tenant, "hello", "world", usage(3, 5));
    store.append(tenant, session, &events).await.unwrap();
    let engine = ReplayEngine::new(store);

    let replayed = engine
        .replay(
            tenant,
            session,
            ReplayCursor::FromOffset(harness_contracts::JournalOffset(1)),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert_eq!(replayed.len(), events.len() - 2);
    assert!(matches!(replayed[0], Event::AssistantMessageCompleted(_)));
}

#[tokio::test]
async fn reconstruct_projection_matches_journal_projection() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let store = event_store();
    let events = session_events(session, tenant, "list files", "Cargo.toml", usage(10, 4));
    store.append(tenant, session, &events).await.unwrap();
    let expected = SessionProjection::replay(events.iter()).unwrap();
    let engine = ReplayEngine::new(store);

    let projection = engine
        .reconstruct_projection(tenant, session, ReplayCursor::FromStart)
        .await
        .unwrap();

    assert_eq!(projection.messages, expected.messages);
    assert_eq!(projection.usage, expected.usage);
    assert_eq!(projection.end_reason, Some(EndReason::Completed));
    assert_eq!(projection.last_offset.0, 4);
}

#[tokio::test]
async fn reconstruct_usage_report_prices_each_event_by_historical_snapshot_id() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let model = model_ref();
    let first_run = RunId::new();
    let second_run = RunId::new();
    let first_snapshot = pricing_snapshot("test-pricing", 1);
    let second_snapshot = pricing_snapshot("test-pricing", 2);
    let store = event_store();
    store
        .append(
            tenant,
            session,
            &[
                Event::UsageAccumulated(usage_accumulated(
                    session,
                    first_run,
                    model.clone(),
                    first_snapshot.clone(),
                    usage(10, 1),
                )),
                Event::UsageAccumulated(usage_accumulated(
                    session,
                    second_run,
                    model.clone(),
                    second_snapshot.clone(),
                    usage(10, 1),
                )),
            ],
        )
        .await
        .unwrap();
    let engine = ReplayEngine::new(store);

    let report = engine
        .reconstruct_usage_report_with_pricing(
            tenant,
            session,
            ReplayCursor::FromStart,
            vec![
                pricing_entry(&first_snapshot, 1, 2),
                pricing_entry(&second_snapshot, 10, 20),
            ],
        )
        .await
        .unwrap();

    assert_eq!(report.global.input_tokens, 20);
    assert_eq!(report.global.output_tokens, 2);
    assert_eq!(report.global.cost_micros, 132);
    assert_eq!(
        report.models["test/usage-model"].cost_micros,
        report.global.cost_micros
    );
    assert_eq!(report.runs[&first_run].cost_micros, 12);
    assert_eq!(report.runs[&second_run].cost_micros, 120);
    assert_eq!(report.sessions[&session].cost_micros, 132);
    assert_eq!(report.tenants[&tenant].cost_micros, 132);
}

#[tokio::test]
async fn reconstruct_usage_report_does_not_replace_event_snapshot_with_latest_pricing() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let run = RunId::new();
    let historical_snapshot = pricing_snapshot("test-pricing", 1);
    let latest_snapshot = pricing_snapshot("test-pricing", 2);
    let store = event_store();
    store
        .append(
            tenant,
            session,
            &[Event::UsageAccumulated(usage_accumulated(
                session,
                run,
                model_ref(),
                historical_snapshot.clone(),
                usage(2, 1),
            ))],
        )
        .await
        .unwrap();
    let engine = ReplayEngine::new(store);

    let report = engine
        .reconstruct_usage_report_with_pricing(
            tenant,
            session,
            ReplayCursor::FromStart,
            vec![
                pricing_entry(&historical_snapshot, 1, 1),
                pricing_entry(&latest_snapshot, 1_000, 1_000),
            ],
        )
        .await
        .unwrap();

    assert_eq!(report.global.cost_micros, 3);
    assert_eq!(report.runs[&run].cost_micros, 3);
}

#[tokio::test]
async fn reconstruct_usage_report_keeps_missing_pricing_cost_zero() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let run = RunId::new();
    let missing_snapshot = pricing_snapshot("test-pricing", 99);
    let store = event_store();
    store
        .append(
            tenant,
            session,
            &[Event::UsageAccumulated(usage_accumulated(
                session,
                run,
                model_ref(),
                missing_snapshot,
                usage(5, 7),
            ))],
        )
        .await
        .unwrap();
    let engine = ReplayEngine::new(store);

    let report = engine
        .reconstruct_usage_report_with_pricing(
            tenant,
            session,
            ReplayCursor::FromStart,
            vec![pricing_entry(
                &pricing_snapshot("test-pricing", 100),
                1_000,
                1_000,
            )],
        )
        .await
        .unwrap();

    assert_eq!(report.global.input_tokens, 5);
    assert_eq!(report.global.output_tokens, 7);
    assert_eq!(report.global.cost_micros, 0);
    assert_eq!(report.runs[&run].cost_micros, 0);
}

#[tokio::test]
async fn diff_reports_added_messages_and_usage_delta() {
    let tenant = TenantId::SINGLE;
    let first = SessionId::new();
    let second = SessionId::new();
    let store = event_store();
    store
        .append(
            tenant,
            first,
            &session_events(first, tenant, "same", "short", usage(1, 2)),
        )
        .await
        .unwrap();
    store
        .append(
            tenant,
            second,
            &session_events(second, tenant, "same", "longer", usage(3, 8)),
        )
        .await
        .unwrap();
    let engine = ReplayEngine::new(store);

    let diff = engine.diff(tenant, first, second).await.unwrap();

    assert_eq!(diff.added_messages.len(), 2);
    assert_eq!(diff.removed_messages.len(), 2);
    assert_eq!(diff.usage_delta.input_tokens, 2);
    assert_eq!(diff.usage_delta.output_tokens, 6);
    assert!(diff.tool_divergence.is_empty());
}

#[tokio::test]
async fn diff_reports_tool_divergence() {
    let tenant = TenantId::SINGLE;
    let first = SessionId::new();
    let second = SessionId::new();
    let store = event_store();
    let tool_use_id = ToolUseId::new();
    let mut first_events = session_events(first, tenant, "same", "short", usage(1, 2));
    let mut second_events = session_events(second, tenant, "same", "short", usage(1, 2));
    first_events.insert(2, tool_requested(tool_use_id, "read"));
    second_events.insert(2, tool_requested(tool_use_id, "write"));
    second_events.insert(3, tool_completed(tool_use_id));
    store.append(tenant, first, &first_events).await.unwrap();
    store.append(tenant, second, &second_events).await.unwrap();
    let engine = ReplayEngine::new(store);

    let diff = engine.diff(tenant, first, second).await.unwrap();

    assert_eq!(diff.tool_divergence.len(), 1);
    assert_eq!(diff.tool_divergence[0].tool_use_id, tool_use_id.to_string());
    assert!(diff.tool_divergence[0].reason.contains("read:requested"));
    assert!(diff.tool_divergence[0].reason.contains("write:completed"));
}

#[tokio::test]
async fn replay_reconstructs_team_and_subagent_projection() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let team_id = harness_contracts::TeamId::new();
    let agent_id = AgentId::new();
    let subagent_id = harness_contracts::SubagentId::new();
    let store = event_store();
    let events = vec![
        Event::TeamCreated(TeamCreatedEvent {
            team_id,
            tenant_id: tenant,
            name: "research".to_owned(),
            topology_kind: TopologyKind::CoordinatorWorker,
            member_specs_hash: [1; 32],
            created_at: harness_contracts::now(),
        }),
        Event::TeamMemberJoined(TeamMemberJoinedEvent {
            team_id,
            agent_id,
            role: "analyst".to_owned(),
            session_id: session,
            visibility: ContextVisibility::All,
            spec_snapshot_id: blob_ref(),
            spec_hash: [2; 32],
            joined_at: harness_contracts::now(),
        }),
        Event::TeamTurnCompleted(TeamTurnCompletedEvent {
            team_id,
            turn_id: RunId::new(),
            participating_agents: vec![agent_id],
            usage: usage(2, 3),
            transcript_ref: None,
            at: harness_contracts::now(),
        }),
        Event::SubagentSpawned(SubagentSpawnedEvent {
            subagent_id,
            parent_session_id: session,
            parent_run_id: RunId::new(),
            agent_ref: AgentRef {
                id: agent_id,
                name: "analyst".to_owned(),
            },
            spec_snapshot_id: SnapshotId::from_u128(7),
            spec_hash: [3; 32],
            depth: 1,
            trigger_tool_use_id: None,
            trigger_tool_name: None,
            at: harness_contracts::now(),
        }),
        Event::SubagentAnnounced(SubagentAnnouncedEvent {
            subagent_id,
            parent_session_id: session,
            status: SubagentStatus::Completed,
            summary: "done".to_owned(),
            result: None,
            usage: usage(4, 5),
            transcript_ref: None,
            context_report: None,
            renderer_id: "test".to_owned(),
            at: harness_contracts::now(),
        }),
        Event::TeamTerminated(harness_contracts::TeamTerminatedEvent {
            team_id,
            reason: TeamTerminationReason::Completed,
            at: harness_contracts::now(),
        }),
    ];
    store.append(tenant, session, &events).await.unwrap();
    let engine = ReplayEngine::new(store);

    let team = engine
        .reconstruct_team_projection(tenant, session, ReplayCursor::FromStart)
        .await
        .unwrap();
    let subagent = engine
        .reconstruct_subagent_projection(tenant, session, ReplayCursor::FromStart)
        .await
        .unwrap();

    let team_state = team.teams.get(&team_id).unwrap();
    assert_eq!(team_state.name, "research");
    assert_eq!(team_state.turns_completed, 1);
    assert_eq!(team_state.usage.input_tokens, 2);
    assert_eq!(
        team_state.terminated,
        Some(TeamTerminationReason::Completed)
    );
    assert!(team_state.members.get(&agent_id).unwrap().active);

    let subagent_state = subagent.subagents.get(&subagent_id).unwrap();
    assert_eq!(subagent_state.agent_name, "analyst");
    assert_eq!(subagent_state.status, Some(SubagentStatus::Completed));
    assert_eq!(subagent_state.usage.output_tokens, 5);
}

#[tokio::test]
async fn export_session_writes_json_lines_and_markdown() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let store = event_store();
    store
        .append(
            tenant,
            session,
            &session_events(session, tenant, "hi", "there", usage(1, 1)),
        )
        .await
        .unwrap();
    let engine = ReplayEngine::new(store);
    let mut jsonl = MemoryWriter::default();
    engine
        .export_session(tenant, session, ExportFormat::JsonLines, &mut jsonl)
        .await
        .unwrap();
    let jsonl = jsonl.into_string();
    assert_eq!(jsonl.lines().count(), 5);
    assert!(jsonl.contains("assistant_message_completed"));

    let mut markdown = MemoryWriter::default();
    engine
        .export_session(tenant, session, ExportFormat::Markdown, &mut markdown)
        .await
        .unwrap();
    let markdown = markdown.into_string();
    assert!(markdown.contains("## User"));
    assert!(markdown.contains("## Assistant"));
}

#[tokio::test]
async fn export_session_writes_har_archive() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let store = event_store();
    store
        .append(
            tenant,
            session,
            &session_events(session, tenant, "hi", "there", usage(1, 1)),
        )
        .await
        .unwrap();
    let engine = ReplayEngine::new(store);
    let mut har = MemoryWriter::default();

    engine
        .export_session(tenant, session, ExportFormat::Har, &mut har)
        .await
        .unwrap();

    let har: serde_json::Value = serde_json::from_str(&har.into_string()).unwrap();
    assert_eq!(har["log"]["version"], "1.2");
    assert_eq!(har["log"]["creator"]["name"], "jyowo-harness-observability");
    assert_eq!(har["log"]["entries"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn export_session_withholds_child_agent_internals_from_json_lines_and_har() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let subagent_id = harness_contracts::SubagentId::new();
    let team_id = TeamId::new();
    let background_agent_id = BackgroundAgentId::new();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(
        DefaultRedactor::default(),
    )));
    let secret = "sk-abcdefghijklmnopqrstuvwxyz";
    store
        .append(
            tenant,
            session,
            &[
                Event::SubagentAnnounced(SubagentAnnouncedEvent {
                    subagent_id,
                    parent_session_id: session,
                    status: SubagentStatus::Completed,
                    summary: format!("done with {secret}"),
                    result: Some(serde_json::json!({ "rawOutput": secret })),
                    usage: usage(1, 1),
                    transcript_ref: Some(transcript_ref()),
                    context_report: None,
                    renderer_id: "default".to_owned(),
                    at: harness_contracts::now(),
                }),
                Event::TeamTurnCompleted(TeamTurnCompletedEvent {
                    team_id,
                    turn_id: RunId::new(),
                    participating_agents: vec![AgentId::new()],
                    usage: usage(1, 1),
                    transcript_ref: Some(transcript_ref()),
                    at: harness_contracts::now(),
                }),
                Event::BackgroundAgentInputSubmitted(BackgroundAgentInputSubmittedEvent {
                    background_agent_id,
                    request_id: RequestId::new(),
                    input: String::from_redacted_display(
                        format!("continue with {secret}"),
                        &DefaultRedactor::default(),
                    ),
                    at: harness_contracts::now(),
                }),
            ],
        )
        .await
        .unwrap();
    let engine = ReplayEngine::new(store);

    let mut jsonl = MemoryWriter::default();
    engine
        .export_session(tenant, session, ExportFormat::JsonLines, &mut jsonl)
        .await
        .unwrap();
    let mut har = MemoryWriter::default();
    engine
        .export_session(tenant, session, ExportFormat::Har, &mut har)
        .await
        .unwrap();
    let exported = format!("{}\n{}", jsonl.into_string(), har.into_string());

    assert!(exported.contains("subagent_announced"));
    assert!(exported.contains("team_turn_completed"));
    assert!(exported.contains("background_agent_input_submitted"));
    assert!(exported.contains(&subagent_id.to_string()));
    assert!(exported.contains(&team_id.to_string()));
    assert!(exported.contains(&background_agent_id.to_string()));
    assert!(!exported.contains(secret));
    assert!(!exported.contains("rawOutput"));
    assert!(!exported.contains("transcript_ref"));
    assert!(!exported.contains("continue with"));
}

fn transcript_ref() -> TranscriptRef {
    TranscriptRef {
        blob: BlobRef {
            id: BlobId::new(),
            size: 10,
            content_hash: [7; 32],
            content_type: Some("application/json".to_owned()),
        },
        from_offset: harness_contracts::JournalOffset(1),
        to_offset: harness_contracts::JournalOffset(2),
    }
}

fn event_store() -> Arc<InMemoryEventStore> {
    Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)))
}

fn session_events(
    session: SessionId,
    tenant: TenantId,
    user: &str,
    assistant: &str,
    usage: UsageSnapshot,
) -> Vec<Event> {
    let run = RunId::new();
    vec![
        Event::SessionCreated(SessionCreatedEvent {
            session_id: session,
            tenant_id: tenant,
            options_hash: [0; 32],
            snapshot_id: SnapshotId::from_u128(1),
            effective_config_hash: ConfigHash([1; 32]),
            created_at: harness_contracts::now(),
        }),
        Event::UserMessageAppended(UserMessageAppendedEvent {
            run_id: run,
            message_id: MessageId::new(),
            content: MessageContent::Text(user.to_owned()),
            metadata: MessageMetadata::default(),
            attachments: Vec::new(),
            at: harness_contracts::now(),
        }),
        Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
            run_id: run,
            message_id: MessageId::new(),
            content: MessageContent::Text(assistant.to_owned()),
            tool_uses: Vec::new(),
            usage: usage.clone(),
            pricing_snapshot_id: None,
            stop_reason: StopReason::EndTurn,
            at: harness_contracts::now(),
        }),
        Event::RunEnded(RunEndedEvent {
            run_id: run,
            reason: EndReason::Completed,
            usage: None,
            ended_at: harness_contracts::now(),
        }),
        Event::SessionEnded(SessionEndedEvent {
            session_id: session,
            tenant_id: tenant,
            reason: EndReason::Completed,
            final_usage: usage,
            at: harness_contracts::now(),
        }),
    ]
}

fn tool_requested(tool_use_id: ToolUseId, name: &str) -> Event {
    Event::ToolUseRequested(ToolUseRequestedEvent {
        run_id: RunId::new(),
        tool_use_id,
        tool_name: name.to_owned(),
        input: serde_json::json!({}),
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_read_only: true,
            is_destructive: false,
            long_running: None,
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        causation_id: harness_contracts::EventId::new(),
        at: harness_contracts::now(),
    })
}

fn tool_completed(tool_use_id: ToolUseId) -> Event {
    Event::ToolUseCompleted(ToolUseCompletedEvent {
        tool_use_id,
        result: ToolResult::Text("ok".to_owned()),
        usage: None,
        duration_ms: 1,
        at: harness_contracts::now(),
    })
}

fn blob_ref() -> BlobRef {
    BlobRef {
        id: BlobId::new(),
        size: 0,
        content_hash: [0; 32],
        content_type: None,
    }
}

fn usage(input_tokens: u64, output_tokens: u64) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens,
        output_tokens,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        cost_micros: 0,
        tool_calls: 0,
    }
}

fn model_ref() -> ModelRef {
    ModelRef {
        provider_id: "test".to_owned(),
        model_id: "usage-model".to_owned(),
    }
}

fn pricing_snapshot(pricing_id: &str, version: u32) -> PricingSnapshotId {
    PricingSnapshotId {
        pricing_id: pricing_id.to_owned(),
        version,
    }
}

fn pricing_entry(
    snapshot: &PricingSnapshotId,
    input_per_million: i64,
    output_per_million: i64,
) -> PricingTableEntry {
    PricingTableEntry {
        pricing_id: snapshot.pricing_id.clone(),
        pricing_version: snapshot.version,
        input_per_million: Decimal::new(input_per_million, 0),
        output_per_million: Decimal::new(output_per_million, 0),
        cache_creation_per_million: None,
        cache_read_per_million: None,
        last_updated: harness_contracts::now(),
        source: PricingSource::BusinessProvided,
        billing_mode: PricingBillingMode::Standard,
    }
}

fn usage_accumulated(
    session_id: SessionId,
    run_id: RunId,
    model_ref: ModelRef,
    pricing_snapshot_id: PricingSnapshotId,
    delta: UsageSnapshot,
) -> UsageAccumulatedEvent {
    UsageAccumulatedEvent {
        session_id,
        run_id: Some(run_id),
        delta,
        model_ref: Some(model_ref),
        pricing_snapshot_id: Some(pricing_snapshot_id),
        at: harness_contracts::now(),
        diagnostic: false,
    }
}

#[derive(Default)]
struct MemoryWriter {
    bytes: Vec<u8>,
}

impl MemoryWriter {
    fn into_string(self) -> String {
        String::from_utf8(self.bytes).unwrap()
    }
}

impl AsyncWrite for MemoryWriter {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.bytes.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
