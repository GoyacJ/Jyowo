#![cfg(feature = "sqlite")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use harness_contracts::*;
use harness_journal::*;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "jyowo-conversation-read-model-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn envelope(
    tenant_id: TenantId,
    session_id: SessionId,
    offset: u64,
    payload: Event,
) -> EventEnvelope {
    EventEnvelope {
        offset: JournalOffset(offset),
        event_id: EventId::new(),
        session_id,
        tenant_id,
        run_id: None,
        correlation_id: CorrelationId::new(),
        causation_id: None,
        schema_version: SchemaVersion::CURRENT,
        recorded_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH
            + chrono::Duration::seconds(offset as i64),
        payload,
    }
}

fn user_message(run_id: RunId, message_id: MessageId, body: &str) -> Event {
    let mut labels = BTreeMap::new();
    labels.insert(
        "clientMessageId".to_owned(),
        "550e8400-e29b-41d4-a716-446655440000".to_owned(),
    );
    Event::UserMessageAppended(UserMessageAppendedEvent {
        run_id,
        message_id,
        content: MessageContent::Text(body.to_owned()),
        metadata: MessageMetadata {
            source: None,
            labels,
        },
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
    })
}

fn assistant_message(run_id: RunId, message_id: MessageId, body: &str) -> Event {
    Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
        run_id,
        message_id,
        content: MessageContent::Text(body.to_owned()),
        tool_uses: Vec::new(),
        usage: UsageSnapshot::default(),
        pricing_snapshot_id: None,
        stop_reason: StopReason::EndTurn,
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
    })
}

fn tool_properties() -> ToolProperties {
    ToolProperties {
        is_concurrency_safe: true,
        is_read_only: true,
        is_destructive: false,
        long_running: None,
        defer_policy: DeferPolicy::AlwaysLoad,
    }
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_summary_snapshot_and_timeline_idempotently() {
    let root = temp_root("sqlite");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let user_message_id = MessageId::new();
    let assistant_message_id = MessageId::new();
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            0,
            user_message(run_id, user_message_id, "open /Users/goya/.ssh/config"),
        ),
        envelope(
            tenant_id,
            session_id,
            1,
            assistant_message(run_id, assistant_message_id, "Done"),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");
    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection is idempotent");

    let summaries = store
        .list_summaries(tenant_id, 10)
        .await
        .expect("summaries load");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, session_id.to_string());
    assert_eq!(summaries[0].title.as_str(), "[REDACTED]");
    assert_eq!(
        summaries[0].last_message_preview.as_ref().unwrap().as_str(),
        "Done"
    );
    assert_eq!(summaries[0].cursor.unwrap().conversation_sequence, 2);

    let snapshot = store
        .snapshot(tenant_id, session_id, 200)
        .await
        .expect("snapshot loads")
        .expect("snapshot exists");
    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(snapshot.messages[0].body.as_str(), "[REDACTED]");
    assert_eq!(
        snapshot.messages[0].client_message_id.as_deref(),
        Some("550e8400-e29b-41d4-a716-446655440000")
    );
    assert_eq!(snapshot.messages[1].body.as_str(), "Done");

    let page = store
        .page_timeline(tenant_id, session_id, None, 1)
        .await
        .expect("first page loads");
    assert_eq!(page.events.len(), 1);
    let next = store
        .page_timeline(tenant_id, session_id, page.cursor, 10)
        .await
        .expect("second page loads");
    assert_eq!(next.events.len(), 1);
    assert_eq!(next.events[0].cursor.conversation_sequence, 2);
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_tool_permission_and_artifact_events() {
    let root = temp_root("timeline-events");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            0,
            Event::RunStarted(RunStartedEvent {
                run_id,
                session_id,
                tenant_id,
                parent_run_id: None,
                input: TurnInput {
                    message: Message {
                        id: MessageId::new(),
                        role: MessageRole::User,
                        parts: vec![MessagePart::Text("run".to_owned())],
                        created_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
                    },
                    metadata: serde_json::Value::Null,
                },
                snapshot_id: SnapshotId::new(),
                effective_config_hash: ConfigHash([0; 32]),
                started_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
                correlation_id: CorrelationId::new(),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            1,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id,
                tool_name: "shell".to_owned(),
                input: serde_json::json!({ "secret": "sk-abcdefghijklmnopqrstuvwxyz" }),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            2,
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id,
                result: ToolResult::Text("wrote /Users/goya/.ssh/config".to_owned()),
                usage: None,
                duration_ms: 12,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            3,
            Event::PermissionRequested(PermissionRequestedEvent {
                request_id,
                run_id,
                session_id,
                tenant_id,
                tool_use_id,
                tool_name: "shell".to_owned(),
                subject: PermissionSubject::CommandExec {
                    command: "/bin/rm -rf target".to_owned(),
                    argv: vec!["rm".to_owned()],
                    cwd: None,
                    fingerprint: None,
                },
                severity: Severity::High,
                scope_hint: DecisionScope::Any,
                fingerprint: None,
                presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
                interactivity: InteractivityLevel::FullyInteractive,
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(3),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            4,
            Event::PermissionResolved(PermissionResolvedEvent {
                request_id,
                decision: Decision::DenyOnce,
                decided_by: DecidedBy::User,
                scope: DecisionScope::Any,
                fingerprint: None,
                rationale: None,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(4),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            5,
            Event::ArtifactCreated(ArtifactCreatedEvent {
                session_id,
                run_id,
                artifact_id: "artifact-001".to_owned(),
                title: "Report".to_owned(),
                kind: "markdown".to_owned(),
                status: ArtifactStatus::Ready,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                blob_ref: None,
                preview: None,
                content_hash: None,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(5),
            }),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let event_types = page
        .events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        event_types,
        vec![
            "run.started",
            "tool.requested",
            "tool.completed",
            "permission.requested",
            "permission.resolved",
            "artifact.created"
        ]
    );
    assert_eq!(
        page.events[1].payload["argumentsSummary"],
        "Input withheld from conversation timeline."
    );
    assert_eq!(
        page.events[2].payload["outputSummary"],
        "Output withheld from conversation timeline."
    );
    assert_eq!(page.events[3].payload["operation"], "Execute command");
    assert_eq!(page.events[3].payload["target"], "rm");
    assert_eq!(page.events[4].payload["decision"], "deny");
    assert_eq!(page.events[5].payload["artifactId"], "artifact-001");
}
