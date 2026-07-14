use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    AssistantMessageCompletedEvent, BlobId, BlobRef, DeferPolicy, DenyReason, Event, EventId,
    MessageContent, MessageId, MessagePart, MessageRole, NoopRedactor, RunId, SessionCreatedEvent,
    SessionError, SessionId, StopReason, TenantId, ToolProperties, ToolUseDeniedEvent, ToolUseId,
    ToolUseRequestedEvent, ToolUseSummary, UsageSnapshot,
};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_session::{
    Session, SessionOptions, SessionProjection, SessionTurnContext, SessionTurnRunner,
};
use serde_json::json;
use tokio::sync::Mutex;

#[tokio::test]
async fn run_turn_delegates_to_configured_runner_and_returns_run_id() {
    let root = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let session = Session::builder()
        .with_options(SessionOptions::new(root.path()))
        .with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_turn_runner(runner.clone())
        .build()
        .await
        .unwrap();

    let run_id = session.run_turn("hello").await.unwrap();

    let calls = runner.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].parts, vec![MessagePart::Text("hello".to_owned())]);
    assert_eq!(calls[0].ctx.run_id, run_id);
    assert_eq!(calls[0].ctx.turn_index, 0);
}

#[tokio::test]
async fn run_turn_delegates_multimodal_input_to_configured_runner() {
    let root = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let session = Session::builder()
        .with_options(SessionOptions::new(root.path()))
        .with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_turn_runner(runner.clone())
        .build()
        .await
        .unwrap();
    let image = MessagePart::Image {
        mime_type: "image/png".to_owned(),
        blob_ref: BlobRef {
            id: BlobId::new(),
            size: 0,
            content_hash: [0; 32],
            content_type: Some("image/png".to_owned()),
        },
    };

    session
        .run_turn_parts(vec![MessagePart::Text("hello".to_owned()), image.clone()])
        .await
        .unwrap();

    let calls = runner.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].parts,
        vec![MessagePart::Text("hello".to_owned()), image]
    );
}

#[tokio::test]
async fn resumed_turn_receives_reconstructed_tool_result_context() {
    let root = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    store
        .append(
            tenant_id,
            session_id,
            &[
                Event::SessionCreated(SessionCreatedEvent {
                    session_id,
                    tenant_id,
                    options_hash: [0; 32],
                    snapshot_id: harness_contracts::SnapshotId::from_u128(1),
                    effective_config_hash: harness_contracts::ConfigHash([0; 32]),
                    created_at: harness_contracts::now(),
                }),
                Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                    run_id,
                    message_id: MessageId::new(),
                    content: MessageContent::Multimodal(vec![MessagePart::ToolUse {
                        id: tool_use_id,
                        name: "WebSearch".to_owned(),
                        input: json!({"query": "news"}),
                    }]),
                    tool_uses: vec![ToolUseSummary {
                        tool_use_id,
                        tool_name: "WebSearch".to_owned(),
                    }],
                    usage: UsageSnapshot::default(),
                    pricing_snapshot_id: None,
                    stop_reason: StopReason::ToolUse,
                    at: harness_contracts::now(),
                }),
                Event::ToolUseRequested(ToolUseRequestedEvent {
                    run_id,
                    tool_use_id,
                    tool_name: "WebSearch".to_owned(),
                    input: json!({"query": "news"}),
                    properties: ToolProperties {
                        is_concurrency_safe: true,
                        is_read_only: true,
                        is_destructive: false,
                        long_running: None,
                        defer_policy: DeferPolicy::AlwaysLoad,
                    },
                    causation_id: EventId::new(),
                    at: harness_contracts::now(),
                }),
                Event::ToolUseDenied(ToolUseDeniedEvent {
                    tool_use_id,
                    reason: DenyReason::PolicyDenied,
                    at: harness_contracts::now(),
                }),
            ],
        )
        .await
        .unwrap();
    let projection = SessionProjection::replay(
        store
            .read_envelopes(tenant_id, session_id, ReplayCursor::FromStart)
            .await
            .unwrap()
            .collect()
            .await,
    )
    .unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let session = Session::builder()
        .with_options(SessionOptions::new(root.path()).with_session_id(session_id))
        .with_event_store(store)
        .with_projection(projection)
        .with_turn_runner(runner.clone())
        .build()
        .await
        .unwrap();

    session.run_turn("retry").await.unwrap();

    let calls = runner.calls.lock().await;
    assert_eq!(
        calls[0]
            .ctx
            .context_seed
            .iter()
            .map(|message| message.role)
            .collect::<Vec<_>>(),
        vec![MessageRole::Assistant, MessageRole::Tool]
    );
    assert!(matches!(
        calls[0].ctx.context_seed[1].parts.as_slice(),
        [MessagePart::ToolResult { tool_use_id: id, .. }] if *id == tool_use_id
    ));
}

#[derive(Default)]
struct RecordingRunner {
    calls: Mutex<Vec<RecordedCall>>,
}

struct RecordedCall {
    ctx: SessionTurnContext,
    parts: Vec<MessagePart>,
}

#[async_trait]
impl SessionTurnRunner for RecordingRunner {
    async fn run_turn(
        &self,
        ctx: SessionTurnContext,
        parts: Vec<MessagePart>,
    ) -> Result<Vec<Event>, SessionError> {
        self.calls.lock().await.push(RecordedCall { ctx, parts });
        Ok(Vec::new())
    }
}
