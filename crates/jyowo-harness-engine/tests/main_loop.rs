use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Instant;

use async_trait::async_trait;
use chrono::Utc;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    ActionResource, ArtifactSource, ArtifactStatus, BlobRef, BudgetMetric, CapabilityRegistry,
    DecidedBy, Decision, DecisionScope, DeferPolicy, DeltaChunk, EndReason, Event, HookEventKind,
    Message, MessageId, MessagePart, MessageRole, ModelError, NetworkAccess, NoopRedactor,
    OverflowAction, PermissionError, PermissionSubject, ProviderRestriction, ResourceLimits,
    ResultBudget, RunId, SandboxError, SandboxExecutionCompletedEvent,
    SandboxExecutionStartedEvent, SandboxExitStatus, SandboxMode, SandboxPolicySummary,
    SandboxScope, SessionId, SteeringId, SteeringKind, SteeringMessageAppliedEvent, StopReason,
    TenantId, ToolActionPlan, ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolProperties,
    ToolResult, ToolResultPart, ToolSearchMode, ToolUseId, TrustLevel, TurnInput, UsageSnapshot,
    WorkspaceAccess,
};
use harness_engine::{
    Engine, EngineError, EngineId, EngineRunner, RunContext, SessionHandle, SteeringDrain,
    SteeringMerge,
};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookHandler, HookOutcome, HookRegistry,
};
use harness_journal::{EventStore, InMemoryBlobStore, InMemoryEventStore, ReplayCursor};
#[cfg(feature = "recall-memory")]
use harness_memory::{
    MemoryLifecycle, MemoryManager, MemoryMetadata, MemoryQuery, MemoryRecord, MemoryStore,
    MemorySummary, RecallPolicy, RecallTriggerStrategy,
};
use harness_model::ModelModality;
use harness_model::{
    ContentDelta, ConversationModelCapability, ErrorClass, ErrorHints, HealthStatus, InferContext,
    ModelDescriptor, ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_sandbox::{
    ActivityHandle, ExecContext, ExecOutcome, ExecSpec, ProcessHandle, SandboxBackend,
    SandboxBaseConfig, SandboxCapabilities, SessionSnapshotFile, SnapshotSpec,
};
#[cfg(feature = "recall-memory")]
use harness_tool::default_result_budget;
use harness_tool::{
    action_plan_from_permission_check, authorized_file_path, AuthorizedFileResourceKind,
    AuthorizedToolInput, SchemaResolverContext, Tool, ToolContext, ToolEvent, ToolPool,
    ToolPoolFilter, ToolPoolModelProfile, ToolRegistry, ToolStream, ValidationError,
};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;

mod authorization_support;
use authorization_support::test_authorization_service;

#[tokio::test]
async fn builder_rejects_missing_required_dependencies() {
    let error = match Engine::builder()
        .with_engine_id(EngineId::new("missing-deps"))
        .build()
    {
        Ok(_) => panic!("engine builder should reject missing dependencies"),
        Err(error) => error,
    };

    assert!(matches!(error, EngineError::Message(message) if message.contains("event store")));
}

#[tokio::test]
async fn run_rejects_mismatched_session_context() {
    let harness = TestHarness::new(text_events("unused")).await;
    let error = match harness
        .engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: harness.session_id,
            },
            turn_input("hello"),
            RunContext::new(TenantId::from_u128(99), harness.session_id, RunId::new()),
        )
        .await
    {
        Ok(_) => panic!("engine run should reject mismatched context"),
        Err(error) => error,
    };

    assert!(matches!(error, EngineError::Message(message) if message.contains("context mismatch")));
}

#[tokio::test]
async fn text_stream_records_deltas_completion_and_run_end() {
    let harness = TestHarness::new(text_events("hello from model")).await;

    let events = harness.run("hello").await.unwrap();

    assert!(events
        .iter()
        .any(|event| matches!(event, Event::RunStarted(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::UserMessageAppended(_))));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantDeltaProduced(delta)
            if matches!(&delta.delta, DeltaChunk::Text(text) if text == "hello from model")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantMessageCompleted(completed)
            if completed.content == harness_contracts::MessageContent::Text("hello from model".to_owned())
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::RunEnded(ended) if ended.reason == EndReason::Completed
    )));
    assert_eq!(harness.user_prompt_hooks.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn user_message_is_recorded_before_assistant_stream_events() {
    let harness = TestHarness::new(text_events("hello from model")).await;

    let events = harness.run("hello").await.unwrap();

    let user_index = events
        .iter()
        .position(|event| matches!(event, Event::UserMessageAppended(_)))
        .expect("user message event exists");
    let assistant_delta_index = events
        .iter()
        .position(|event| matches!(event, Event::AssistantDeltaProduced(_)))
        .expect("assistant delta event exists");

    assert!(
        user_index < assistant_delta_index,
        "user message must be recorded before assistant streaming events"
    );
}

#[tokio::test]
async fn thinking_stream_keeps_thinking_out_of_final_assistant_message() {
    let harness =
        TestHarness::new(thinking_then_text_events("internal chain", "final answer")).await;

    let events = harness.run("hello").await.unwrap();
    let serialized = serde_json::to_string(&events).unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantDeltaProduced(delta)
            if matches!(&delta.delta, DeltaChunk::Thought(thought)
                if thought.text.is_none()
                    && thought.provider_native.is_none()
                    && thought.signature.is_none())
    )));
    assert!(!serialized.contains("internal chain"));
    assert!(!serialized.contains("provider native secret"));
    assert!(!serialized.contains("signature-secret"));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantMessageCompleted(completed)
            if completed.content == harness_contracts::MessageContent::Text("final answer".to_owned())
    )));
}

#[tokio::test]
async fn reasoning_summary_stream_records_safe_summary_delta() {
    let harness = TestHarness::new(reasoning_summary_then_text_events(
        "Checked context.",
        "final answer",
    ))
    .await;

    let events = harness.run("hello").await.unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantDeltaProduced(delta)
            if matches!(&delta.delta, DeltaChunk::ReasoningSummary(summary)
                if summary.text == "Checked context." && summary.provider_native.is_none())
    )));
    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains("provider native secret"));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantMessageCompleted(completed)
            if completed.content == harness_contracts::MessageContent::Text("final answer".to_owned())
    )));
}

#[tokio::test]
async fn tool_call_records_permission_and_tool_events() {
    let harness = TestHarness::new(tool_call_events("ListDir", json!({ "path": "" }))).await;
    std::fs::write(harness.workspace.path().join("marker.txt"), "m5").unwrap();
    harness
        .model
        .replace_response(ModelResponse::Sequence(vec![
            tool_call_events("ListDir", json!({ "path": harness.workspace.path() })),
            text_events("listed directory"),
        ]))
        .await;

    let events = harness.run("list current dir").await.unwrap();
    let journal_events = harness.events().await;

    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseRequested(_))));
    assert!(journal_events
        .iter()
        .any(|event| matches!(event, Event::PermissionRequested(_))));
    assert!(journal_events
        .iter()
        .any(|event| matches!(event, Event::PermissionResolved(resolved)
        if matches!(resolved.decided_by, DecidedBy::Broker { .. } | DecidedBy::Rule { .. }))));
    assert!(journal_events
        .iter()
        .any(|event| matches!(event, Event::PermissionResolved(_))));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ToolUseCompleted(completed)
            if matches!(&completed.result, ToolResult::Structured(value) if value.to_string().contains("marker.txt"))
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::RunEnded(ended)
        if ended.reason == EndReason::Completed)));
}

#[tokio::test]
async fn typed_image_artifact_tool_result_creates_image_artifact_event() {
    let harness = TestHarness::new_response_with_tool(
        ModelResponse::Sequence(vec![
            tool_call_events("ProviderImageService", json!({ "prompt": "grass carp" })),
            text_events("[REDACTED]"),
        ]),
        Box::new(TypedImageArtifactTool::new(
            "ProviderImageService",
            "image/png",
        )),
        None,
    )
    .await;

    let events = harness.run("生成一张草鱼图片").await.unwrap();
    let completed_tool_use_id = events
        .iter()
        .find_map(|event| match event {
            Event::ToolUseCompleted(completed) => Some(completed.tool_use_id),
            _ => None,
        })
        .expect("tool completed");
    let artifact = events
        .iter()
        .find_map(|event| match event {
            Event::ArtifactCreated(created) => Some(created),
            _ => None,
        })
        .expect("image artifact created");

    assert_eq!(artifact.kind, "image");
    assert_eq!(artifact.title, "Generated image");
    assert_eq!(artifact.status, ArtifactStatus::Ready);
    assert_eq!(artifact.source, ArtifactSource::Tool);
    assert_eq!(artifact.source_tool_use_id, Some(completed_tool_use_id));
    assert_eq!(
        artifact.blob_ref.as_ref().unwrap().content_type.as_deref(),
        Some("image/png")
    );
    assert_eq!(artifact.preview.as_deref(), Some("Generated image"));
}

#[tokio::test]
async fn svg_typed_artifact_tool_result_does_not_create_image_artifact_event() {
    let harness = TestHarness::new_response_with_tool(
        ModelResponse::Sequence(vec![
            tool_call_events("ProviderImageService", json!({ "prompt": "vector" })),
            text_events("[REDACTED]"),
        ]),
        Box::new(TypedImageArtifactTool::new(
            "ProviderImageService",
            "image/svg+xml",
        )),
        None,
    )
    .await;

    let events = harness.run("生成一张 SVG 图片").await.unwrap();

    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ArtifactCreated(_))));
}

#[tokio::test]
async fn legacy_blob_tool_result_without_typed_artifact_does_not_create_artifact_event() {
    let harness = TestHarness::new_response_with_tool(
        ModelResponse::Sequence(vec![
            tool_call_events("MiniMaxTextToImage", json!({ "prompt": "grass carp" })),
            text_events("[REDACTED]"),
        ]),
        Box::new(ImageBlobTool::new("MiniMaxTextToImage")),
        None,
    )
    .await;

    let events = harness.run("生成一张草鱼图片").await.unwrap();

    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ArtifactCreated(_))));
}

#[tokio::test]
async fn bash_sandbox_events_are_present_in_engine_event_stream() {
    let harness = TestHarness::new_response_with_shell_and_sandbox(
        ModelResponse::Sequence(vec![
            tool_call_events("Bash", json!({ "command": "printf hello" })),
            text_events("done"),
        ]),
        None,
        Some(Arc::new(EventfulSandbox)),
    )
    .await;

    let events = harness.run("run bash").await.unwrap();

    assert!(
        events.iter().any(|event| {
            matches!(
                event,
                Event::SandboxExecutionStarted(_) | Event::SandboxPreflightPassed(_)
            )
        }),
        "expected sandbox lifecycle event"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::ToolUseCompleted(_) | Event::ToolUseFailed(_))),
        "expected tool execution event"
    );
    assert_single_run_end(&events);
}

#[tokio::test]
async fn streaming_tool_use_start_and_input_delta_dispatches_tool() {
    let harness = TestHarness::new_sequence(vec![
        streaming_tool_call_events("provider-call-1", "ListDir", &[r#"{ "path": ""#, " }"]),
        text_events("listed directory"),
    ])
    .await;
    std::fs::write(harness.workspace.path().join("marker.txt"), "m5").unwrap();
    harness
        .model
        .replace_response(ModelResponse::Sequence(vec![
            streaming_tool_call_events(
                "provider-call-1",
                "ListDir",
                &[
                    &format!(r#"{{ "path": "{}""#, harness.workspace.path().display()),
                    " }",
                ],
            ),
            text_events("listed directory"),
        ]))
        .await;

    let events = harness.run("list current dir").await.unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::ToolUseRequested(requested)
            if requested.tool_name == "ListDir"
                && requested.input["path"].as_str() == Some(harness.workspace.path().to_str().unwrap())
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ToolUseCompleted(completed)
            if matches!(&completed.result, ToolResult::Structured(value) if value.to_string().contains("marker.txt"))
    )));
    assert_single_run_end(&events);
}

#[tokio::test]
async fn invalid_streaming_tool_json_stops_before_tool_execution() {
    let harness = TestHarness::new(streaming_tool_call_events(
        "provider-call-1",
        "ListDir",
        &["{"],
    ))
    .await;

    let error = harness.run("list current dir").await.unwrap_err();
    let events = harness.events().await;

    assert!(error.to_string().contains("invalid tool input json"));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolUseRequested(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::PermissionRequested(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::RunEnded(ended)
            if matches!(&ended.reason, EndReason::Error(message) if message.contains("invalid tool input json"))
    )));
    assert_single_run_end(&events);
}

#[tokio::test]
async fn tool_result_is_reinjected_before_final_model_call() {
    let harness = TestHarness::new_sequence(vec![
        tool_call_events("ListDir", json!({ "path": "" })),
        text_events("saw tool result"),
    ])
    .await;
    std::fs::write(harness.workspace.path().join("marker.txt"), "m5").unwrap();
    harness
        .model
        .replace_response(ModelResponse::Sequence(vec![
            tool_call_events("ListDir", json!({ "path": harness.workspace.path() })),
            text_events("saw tool result"),
        ]))
        .await;

    let events = harness.run("list current dir").await.unwrap();
    let requests = harness.model.requests().await;

    assert_eq!(requests.len(), 2);
    assert!(requests[1].messages.iter().any(|message| {
        message.role == MessageRole::Assistant
            && message
                .parts
                .iter()
                .any(|part| matches!(part, MessagePart::ToolUse { name, .. } if name == "ListDir"))
    }));
    assert!(requests[1].messages.iter().any(|message| {
        message.role == MessageRole::Tool
            && message.parts.iter().any(|part| matches!(part, MessagePart::ToolResult { content, .. }
                if matches!(content, ToolResult::Structured(value) if value.to_string().contains("marker.txt"))))
    }));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantMessageCompleted(completed)
            if completed.content == harness_contracts::MessageContent::Text("saw tool result".to_owned())
    )));
    assert_single_run_end(&events);
}

#[cfg(feature = "recall-memory")]
#[tokio::test]
async fn tool_result_history_hint_enqueues_memory_recall_for_next_engine_iteration() {
    let memory = Arc::new(StaticMemoryProvider::new(
        "engine-memory",
        vec![memory_record("project prefers terse answers")],
    ));
    let manager = MemoryManager::new().with_recall_policy(RecallPolicy {
        trigger: RecallTriggerStrategy::OnQuestionMark,
        ..RecallPolicy::default()
    });
    manager
        .register_provider(memory)
        .expect("memory manager should install provider");
    let context = ContextEngine::builder()
        .with_memory_manager(Arc::new(manager))
        .build()
        .unwrap();
    let harness = TestHarness::new_response_with_context_and_tool(
        ModelResponse::Sequence(vec![
            tool_call_events("HistoryHint", json!({})),
            text_events("done"),
        ]),
        context,
        Box::new(TextTool::new(
            "HistoryHint",
            "需要查阅历史: project preference?",
            default_result_budget(),
        )),
        None,
    )
    .await;

    let events = harness.run("use tool").await.unwrap();
    let requests = harness.model.requests().await;
    let second_request = requests.get(1).expect("second model request");
    let second_text = request_text(second_request);

    assert!(second_text.contains("<memory-context>"));
    assert!(second_text.contains("project prefers terse answers"));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::MemoryRecalled(recalled)
            if recalled.provider_id == "engine-memory" && recalled.deadline_used_ms == 200
    )));
}

#[tokio::test]
async fn offloaded_tool_result_is_reinjected_without_original_full_text() {
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let harness = TestHarness::new_response_with_tool(
        ModelResponse::Sequence(vec![
            tool_call_events("BigText", json!({})),
            text_events("done"),
        ]),
        Box::new(TextTool::new(
            "BigText",
            "abcdefghij",
            budget(5, OverflowAction::Offload),
        )),
        Some(blob_store),
    )
    .await;

    let events = harness.run("make large output").await.unwrap();
    let requests = harness.model.requests().await;

    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolResultOffloaded(_))));
    let injected = tool_result_debug(&requests[1]);
    assert!(!injected.contains("abcdefghij"));
    assert!(injected.contains("blob_ref"));
    assert_single_run_end(&events);
}

#[tokio::test]
async fn rejected_tool_result_is_failure_event_and_reinjected_as_tool_result() {
    let harness = TestHarness::new_response_with_tool(
        ModelResponse::Sequence(vec![
            tool_call_events("TooLarge", json!({})),
            text_events("done"),
        ]),
        Box::new(TextTool::new(
            "TooLarge",
            "too large",
            budget(3, OverflowAction::Reject),
        )),
        None,
    )
    .await;

    let events = harness.run("make rejected output").await.unwrap();
    let requests = harness.model.requests().await;

    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseFailed(_))));
    assert!(tool_result_debug(&requests[1]).contains("result too large"));
    assert_single_run_end(&events);
}

#[tokio::test]
async fn repeated_tool_calls_stop_at_iteration_budget_without_duplicate_run_end() {
    let harness = TestHarness::new_sequence(vec![
        tool_call_events("ListDir", json!({ "path": "" })),
        tool_call_events("ListDir", json!({ "path": "" })),
    ])
    .await;
    harness
        .model
        .replace_response(ModelResponse::Sequence(vec![
            tool_call_events("ListDir", json!({ "path": harness.workspace.path() })),
            tool_call_events("ListDir", json!({ "path": harness.workspace.path() })),
        ]))
        .await;
    let engine = harness.engine_with_iterations(2).await;

    let events = engine
        .run(
            harness.session_handle(),
            turn_input("repeat tools"),
            harness.run_context(),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert_eq!(harness.model.requests().await.len(), 2);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::RunEnded(ended) if ended.reason == EndReason::MaxIterationsReached
    )));
    assert_single_run_end(&events);
}

#[tokio::test]
async fn grace_tool_call_does_not_emit_tool_use_requested() {
    let harness = TestHarness::new(tool_call_events("ListDir", json!({ "path": "" }))).await;
    let engine = harness.engine_with_iterations(1).await;

    let events = engine
        .run(
            harness.session_handle(),
            turn_input("finish without tools"),
            harness.run_context(),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(events.iter().any(|event| matches!(
        event,
        Event::RunEnded(ended) if ended.reason == EndReason::MaxIterationsReached
    )));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolUseRequested(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::PermissionRequested(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
    assert_single_run_end(&events);
}

#[tokio::test]
async fn model_infer_error_records_run_end_error() {
    let harness = TestHarness::new(text_events("unused")).await;
    harness
        .model
        .replace_response(ModelResponse::Error(ModelError::ProviderUnavailable(
            "offline".to_owned(),
        )))
        .await;

    let error = harness.run("hello").await.unwrap_err();

    assert!(error.to_string().contains("offline"));
    assert!(harness.events().await.iter().any(|event| matches!(
        event,
        Event::RunEnded(ended)
            if matches!(&ended.reason, EndReason::Error(message) if message.contains("offline"))
    )));
}

#[tokio::test]
async fn model_stream_error_records_run_end_error() {
    let harness = TestHarness::new(vec![ModelStreamEvent::StreamError {
        error: ModelError::UnexpectedResponse("bad chunk".to_owned()),
        class: ErrorClass::Fatal,
        hints: ErrorHints::default(),
    }])
    .await;

    let error = harness.run("hello").await.unwrap_err();

    assert!(error.to_string().contains("bad chunk"));
    assert!(harness.events().await.iter().any(|event| matches!(
        event,
        Event::RunEnded(ended)
            if matches!(&ended.reason, EndReason::Error(message) if message.contains("bad chunk"))
    )));
}

#[tokio::test]
async fn grace_call_emits_event_before_final_response() {
    let harness = TestHarness::new(text_events("final")).await;
    let engine = harness.engine_with_iterations(1).await;

    let events = engine
        .run(
            harness.session_handle(),
            turn_input("finish"),
            harness.run_context(),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    let grace_index = events
        .iter()
        .position(|event| matches!(event, Event::GraceCallTriggered(_)))
        .unwrap();
    let ended_index = events
        .iter()
        .position(|event| matches!(event, Event::RunEnded(_)))
        .unwrap();
    assert!(grace_index < ended_index);
}

#[tokio::test]
async fn steering_drain_runs_before_each_model_infer_and_merges_prompt() {
    let harness = TestHarness::new_sequence(vec![
        tool_call_events("ListDir", json!({ "path": "" })),
        text_events("done"),
    ])
    .await;
    std::fs::write(harness.workspace.path().join("marker.txt"), "m5").unwrap();
    harness
        .model
        .replace_response(ModelResponse::Sequence(vec![
            tool_call_events("ListDir", json!({ "path": harness.workspace.path() })),
            text_events("done"),
        ]))
        .await;
    let steering = Arc::new(FakeSteeringDrain::new(vec![Some("steer now"), None]));
    let engine = harness
        .engine
        .clone()
        .into_builder()
        .with_steering_drain(steering.clone())
        .build()
        .unwrap();

    let events = engine
        .run(
            harness.session_handle(),
            turn_input("list current dir"),
            harness.run_context(),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    let requests = harness.model.requests().await;

    assert_eq!(steering.call_count(), 2);
    assert!(requests[0].messages.iter().any(|message| {
        message.role == MessageRole::User
            && message
                .parts
                .iter()
                .any(|part| matches!(part, MessagePart::Text(text) if text.contains("steer now")))
    }));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SteeringMessageApplied(_))));
    assert_single_run_end(&events);
}

struct TestHarness {
    workspace: TempDir,
    tenant_id: TenantId,
    session_id: SessionId,
    store: Arc<InMemoryEventStore>,
    engine: Engine,
    model: Arc<RecordingModelProvider>,
    user_prompt_hooks: Arc<AtomicUsize>,
}

impl TestHarness {
    async fn new(events: Vec<ModelStreamEvent>) -> Self {
        Self::new_response(ModelResponse::Events(events)).await
    }

    async fn new_sequence(responses: Vec<Vec<ModelStreamEvent>>) -> Self {
        Self::new_response(ModelResponse::Sequence(responses)).await
    }

    async fn new_response(response: ModelResponse) -> Self {
        Self::new_response_with_tool(response, Box::new(TestListDirTool::new()), None).await
    }

    async fn new_response_with_tool(
        response: ModelResponse,
        tool: Box<dyn Tool>,
        blob_store: Option<Arc<dyn harness_contracts::BlobStore>>,
    ) -> Self {
        Self::new_response_with_tool_and_sandbox(response, tool, blob_store, None).await
    }

    #[cfg(feature = "recall-memory")]
    async fn new_response_with_context_and_tool(
        response: ModelResponse,
        context: ContextEngine,
        tool: Box<dyn Tool>,
        blob_store: Option<Arc<dyn harness_contracts::BlobStore>>,
    ) -> Self {
        Self::new_response_with_context_tool_and_sandbox(response, context, tool, blob_store, None)
            .await
    }

    async fn new_response_with_tool_and_sandbox(
        response: ModelResponse,
        tool: Box<dyn Tool>,
        blob_store: Option<Arc<dyn harness_contracts::BlobStore>>,
        sandbox: Option<Arc<dyn SandboxBackend>>,
    ) -> Self {
        Self::new_response_with_context_tool_and_sandbox(
            response,
            ContextEngine::builder().build().unwrap(),
            tool,
            blob_store,
            sandbox,
        )
        .await
    }

    async fn new_response_with_shell_and_sandbox(
        response: ModelResponse,
        blob_store: Option<Arc<dyn harness_contracts::BlobStore>>,
        sandbox: Option<Arc<dyn SandboxBackend>>,
    ) -> Self {
        Self::new_response_with_context_toolset_and_sandbox(
            response,
            ContextEngine::builder().build().unwrap(),
            harness_tool::BuiltinToolset::Shell,
            blob_store,
            sandbox,
        )
        .await
    }

    async fn new_response_with_context_tool_and_sandbox(
        response: ModelResponse,
        context: ContextEngine,
        tool: Box<dyn Tool>,
        blob_store: Option<Arc<dyn harness_contracts::BlobStore>>,
        sandbox: Option<Arc<dyn SandboxBackend>>,
    ) -> Self {
        Self::new_response_with_context_toolset_and_sandbox(
            response,
            context,
            harness_tool::BuiltinToolset::Custom(vec![tool]),
            blob_store,
            sandbox,
        )
        .await
    }

    async fn new_response_with_context_toolset_and_sandbox(
        response: ModelResponse,
        context: ContextEngine,
        builtin_toolset: harness_tool::BuiltinToolset,
        blob_store: Option<Arc<dyn harness_contracts::BlobStore>>,
        sandbox: Option<Arc<dyn SandboxBackend>>,
    ) -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let tenant_id = TenantId::SINGLE;
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(RecordingModelProvider::new(response));
        let user_prompt_hooks = Arc::new(AtomicUsize::new(0));
        let hooks = HookRegistry::builder()
            .with_hook(Box::new(CountingHook {
                calls: user_prompt_hooks.clone(),
            }))
            .build()
            .unwrap();
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(builtin_toolset)
            .build()
            .unwrap();
        let tools = ToolPool::assemble(
            &registry.snapshot(),
            &ToolPoolFilter::default(),
            &ToolSearchMode::Disabled,
            &ToolPoolModelProfile {
                provider: harness_contracts::ModelProvider("test".to_owned()),
                max_context_tokens: Some(8_000),
            },
            &SchemaResolverContext {
                run_id: RunId::new(),
                session_id,
                tenant_id,
            },
        )
        .await
        .unwrap();
        let mut builder = Engine::builder()
            .with_engine_id(EngineId::new("main-loop-test"))
            .with_event_store(store.clone())
            .with_context(context)
            .with_hooks(HookDispatcher::new(hooks.snapshot()))
            .with_model(model.clone())
            .with_tools(tools)
            .with_authorization_service(test_authorization_service(
                Arc::new(AllowBroker),
                store.clone(),
            ))
            .with_workspace_root(workspace.path())
            .with_model_id("test-model")
            .with_protocol(ModelProtocol::Messages)
            .with_system_prompt(Some("system"))
            .with_cap_registry(Arc::new(CapabilityRegistry::default()));
        if let Some(blob_store) = blob_store {
            builder = builder.with_blob_store(blob_store);
        }
        if let Some(sandbox) = sandbox {
            builder = builder.with_sandbox(sandbox);
        }
        let engine = builder.build().unwrap();

        Self {
            workspace,
            tenant_id,
            session_id,
            store,
            engine,
            model,
            user_prompt_hooks,
        }
    }

    async fn engine_with_iterations(&self, max_iterations: u32) -> Engine {
        self.engine
            .clone()
            .into_builder()
            .with_max_iterations(max_iterations)
            .build()
            .unwrap()
    }

    fn session_handle(&self) -> SessionHandle {
        SessionHandle {
            tenant_id: self.tenant_id,
            session_id: self.session_id,
        }
    }

    fn run_context(&self) -> RunContext {
        RunContext::new(self.tenant_id, self.session_id, RunId::new())
    }

    async fn run(&self, text: &str) -> Result<Vec<Event>, EngineError> {
        Ok(self
            .engine
            .run(self.session_handle(), turn_input(text), self.run_context())
            .await?
            .collect::<Vec<_>>()
            .await)
    }

    async fn events(&self) -> Vec<Event> {
        self.store
            .read_envelopes(self.tenant_id, self.session_id, ReplayCursor::FromStart)
            .await
            .unwrap()
            .map(|envelope| envelope.payload)
            .collect()
            .await
    }
}

struct TextTool {
    descriptor: ToolDescriptor,
    output: String,
}

impl TextTool {
    fn new(name: &str, output: &str, budget: ResultBudget) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: name.to_owned(),
                description: "Return fixed text.".to_owned(),
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
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::AdminTrusted,
                required_capabilities: Vec::new(),
                budget,
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
            },
            output: output.to_owned(),
        }
    }
}

#[async_trait]
impl Tool for TextTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            harness_permission::PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            },
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text(self.output.clone()),
        )])))
    }
}

struct TypedImageArtifactTool {
    descriptor: ToolDescriptor,
    content_type: String,
}

impl TypedImageArtifactTool {
    fn new(name: &str, content_type: &str) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: name.to_owned(),
                description: "Return typed image artifact output.".to_owned(),
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
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::AdminTrusted,
                required_capabilities: Vec::new(),
                budget: ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 32_000,
                    on_overflow: OverflowAction::Offload,
                    preview_head_chars: 2_000,
                    preview_tail_chars: 2_000,
                },
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
            },
            content_type: content_type.to_owned(),
        }
    }
}

#[async_trait]
impl Tool for TypedImageArtifactTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            harness_permission::PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            },
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let blob_ref = BlobRef {
            id: harness_contracts::BlobId::new(),
            size: 128,
            content_hash: [9; 32],
            content_type: Some(self.content_type.clone()),
        };
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Mixed(vec![ToolResultPart::Artifact {
                artifact_kind: ModelModality::Image,
                content_type: self.content_type.clone(),
                blob_ref,
                title: "Generated image".to_owned(),
                preview: Some("Generated image".to_owned()),
            }]),
        )])))
    }
}

struct ImageBlobTool {
    descriptor: ToolDescriptor,
    content_type: String,
}

impl ImageBlobTool {
    fn new(name: &str) -> Self {
        Self::new_with_content_type(name, "image/png")
    }

    fn new_with_content_type(name: &str, content_type: &str) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: name.to_owned(),
                description: "Return fixed image blob.".to_owned(),
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
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::AdminTrusted,
                required_capabilities: Vec::new(),
                budget: ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 32_000,
                    on_overflow: OverflowAction::Offload,
                    preview_head_chars: 2_000,
                    preview_tail_chars: 2_000,
                },
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
            },
            content_type: content_type.to_owned(),
        }
    }
}

#[async_trait]
impl Tool for ImageBlobTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            harness_permission::PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            },
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let blob_ref = BlobRef {
            id: harness_contracts::BlobId::new(),
            size: 128,
            content_hash: [9; 32],
            content_type: Some(self.content_type.clone()),
        };
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Blob {
                content_type: self.content_type.clone(),
                blob_ref,
            },
        )])))
    }
}

struct EventfulSandbox;

#[async_trait]
impl SandboxBackend for EventfulSandbox {
    fn backend_id(&self) -> &str {
        "eventful"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            supports_streaming: true,
            supports_network: true,
            supports_filesystem_write: true,
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        }
    }

    async fn execute(
        &self,
        spec: ExecSpec,
        ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        let fingerprint = spec.canonical_fingerprint(&SandboxBaseConfig::default());
        ctx.event_sink.emit(Event::SandboxExecutionStarted(
            SandboxExecutionStartedEvent {
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                tool_use_id: ctx.tool_use_id,
                backend_id: "eventful".to_owned(),
                fingerprint,
                policy: SandboxPolicySummary {
                    mode: SandboxMode::None,
                    scope: SandboxScope::WorkspaceOnly,
                    network: NetworkAccess::None,
                    resource_limits: ResourceLimits {
                        max_memory_bytes: None,
                        max_cpu_cores: None,
                        max_pids: None,
                        max_wall_clock_ms: None,
                        max_open_files: None,
                    },
                },
                at: Utc::now(),
            },
        ))?;

        Ok(ProcessHandle {
            pid: Some(7),
            stdout: None,
            stderr: None,
            stdin: None,
            cwd_marker: None,
            activity: Arc::new(EventfulActivity { ctx, fingerprint }),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Ok(SessionSnapshotFile::default())
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

struct EventfulActivity {
    ctx: ExecContext,
    fingerprint: harness_contracts::ExecFingerprint,
}

#[async_trait]
impl ActivityHandle for EventfulActivity {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        let now = Utc::now();
        self.ctx.event_sink.emit(Event::SandboxExecutionCompleted(
            SandboxExecutionCompletedEvent {
                session_id: self.ctx.session_id,
                run_id: self.ctx.run_id,
                tool_use_id: self.ctx.tool_use_id,
                backend_id: "eventful".to_owned(),
                fingerprint: self.fingerprint,
                exit_status: SandboxExitStatus::Code(0),
                stdout_bytes_observed: 0,
                stderr_bytes_observed: 0,
                duration_ms: 0,
                overflow: None,
                at: now,
            },
        ))?;
        Ok(ExecOutcome {
            exit_status: SandboxExitStatus::Code(0),
            started_at: now,
            finished_at: now,
            stdout_bytes_observed: 0,
            stderr_bytes_observed: 0,
            overflow: None,
        })
    }

    async fn kill(
        &self,
        _signal: i32,
        _scope: harness_contracts::KillScope,
    ) -> Result<(), SandboxError> {
        Ok(())
    }

    fn touch(&self) {}

    fn last_activity(&self) -> Instant {
        Instant::now()
    }
}

struct RecordingModelProvider {
    response: Mutex<ModelResponse>,
    requests: Mutex<Vec<ModelRequest>>,
}

impl RecordingModelProvider {
    fn new(response: ModelResponse) -> Self {
        Self {
            response: Mutex::new(response),
            requests: Mutex::new(Vec::new()),
        }
    }

    async fn replace_response(&self, response: ModelResponse) {
        *self.response.lock().await = response;
    }

    async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for RecordingModelProvider {
    fn provider_id(&self) -> &'static str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "test".to_owned(),
            model_id: "test-model".to_owned(),
            display_name: "Test model".to_owned(),
            context_window: 8_000,
            max_output_tokens: 1_000,
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
                harness_model::ModelProtocol::Messages,
            ),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);
        let response = {
            let mut response = self.response.lock().await;
            match &mut *response {
                ModelResponse::Sequence(responses) => ModelResponse::Events(responses.remove(0)),
                ModelResponse::Events(events) => ModelResponse::Events(events.clone()),
                ModelResponse::Error(error) => ModelResponse::Error(error.clone()),
            }
        };
        match response {
            ModelResponse::Events(events) => Ok(Box::pin(stream::iter(events))),
            ModelResponse::Sequence(_) => {
                unreachable!("sequence response is expanded before infer")
            }
            ModelResponse::Error(error) => Err(error),
        }
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[derive(Clone)]
enum ModelResponse {
    Events(Vec<ModelStreamEvent>),
    Sequence(Vec<Vec<ModelStreamEvent>>),
    Error(ModelError),
}

#[derive(Default)]
struct AllowBroker;

#[async_trait]
impl PermissionBroker for AllowBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}

struct CountingHook {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl HookHandler for CountingHook {
    fn handler_id(&self) -> &'static str {
        "count-user-prompt"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::UserPromptSubmit]
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(HookOutcome::Continue)
    }
}

struct FakeSteeringDrain {
    responses: Mutex<Vec<Option<&'static str>>>,
    calls: AtomicUsize,
}

impl FakeSteeringDrain {
    fn new(responses: Vec<Option<&'static str>>) -> Self {
        Self {
            responses: Mutex::new(responses),
            calls: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SteeringDrain for FakeSteeringDrain {
    async fn drain_and_merge(
        &self,
        session: &SessionHandle,
        run_id: RunId,
        merged_into_message_id: MessageId,
    ) -> Result<Option<SteeringMerge>, EngineError> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        let body = self
            .responses
            .lock()
            .await
            .get(call_index)
            .copied()
            .flatten();
        let Some(body) = body else {
            return Ok(None);
        };
        let id = SteeringId::new();
        let mut kind_distribution = BTreeMap::new();
        kind_distribution.insert(SteeringKind::Append, 1);
        Ok(Some(SteeringMerge {
            body: body.to_owned(),
            applied_event: Event::SteeringMessageApplied(SteeringMessageAppliedEvent {
                ids: vec![id],
                session_id: session.session_id,
                run_id,
                merged_into_message_id: Some(merged_into_message_id),
                kind_distribution,
                at: harness_contracts::now(),
            }),
            already_persisted: false,
        }))
    }
}

struct TestListDirTool {
    descriptor: ToolDescriptor,
}

impl TestListDirTool {
    fn new() -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "ListDir".to_owned(),
                display_name: "List directory".to_owned(),
                description: "List workspace directory entries.".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::FileSystem,
                version: "0.1.0".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["path"],
                    "properties": { "path": { "type": "string" } }
                }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::AdminTrusted,
                required_capabilities: Vec::new(),
                budget: ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 32_000,
                    on_overflow: OverflowAction::Offload,
                    preview_head_chars: 2_000,
                    preview_tail_chars: 2_000,
                },
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
            },
        }
    }
}

#[async_trait]
impl Tool for TestListDirTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        if input.get("path").and_then(Value::as_str).is_none() {
            return Err(ValidationError::from("path is required"));
        }
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let path: std::path::PathBuf = input["path"].as_str().unwrap_or_default().into();
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            harness_permission::PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: "ListDir".to_owned(),
                    input: input.clone(),
                },
                scope: DecisionScope::PathPrefix(path.clone()),
            },
            vec![ActionResource::FileRead { path }],
            WorkspaceAccess::ReadOnly,
            NetworkAccess::None,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let path = authorized_file_path(&authorized, AuthorizedFileResourceKind::Read)?;
        let mut entries = Vec::new();
        for entry in
            std::fs::read_dir(path).map_err(|error| ToolError::Message(error.to_string()))?
        {
            let entry = entry.map_err(|error| ToolError::Message(error.to_string()))?;
            entries.push(entry.file_name().to_string_lossy().into_owned());
        }
        entries.sort();
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!(entries)),
        )])))
    }
}

fn turn_input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
        },
        metadata: json!({}),
    }
}

fn tool_call_events(name: &str, input: Value) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-1".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: name.to_owned(),
                input,
            },
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::ToolUse),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
}

fn streaming_tool_call_events(id: &str, name: &str, input_parts: &[&str]) -> Vec<ModelStreamEvent> {
    let mut events = vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-1".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockStart {
            index: 0,
            content_type: harness_model::ContentType::ToolUse,
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseStart {
                id: id.to_owned(),
                name: name.to_owned(),
            },
        },
    ];
    events.extend(
        input_parts
            .iter()
            .map(|part| ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::ToolUseInputJson((*part).to_owned()),
            }),
    );
    events.extend([
        ModelStreamEvent::ContentBlockStop { index: 0 },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::ToolUse),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]);
    events
}

fn text_events(text: &str) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-1".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text(text.to_owned()),
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
}

fn thinking_then_text_events(thinking: &str, text: &str) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-1".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Thinking(harness_model::ThinkingDelta {
                provider_native: Some(json!({ "thinking": "provider native secret" })),
                signature: Some("signature-secret".to_owned()),
                text: Some(thinking.to_owned()),
            }),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::Text(text.to_owned()),
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
}

fn reasoning_summary_then_text_events(summary: &str, text: &str) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-1".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ReasoningSummary(harness_model::ReasoningSummaryDelta {
                provider_native: Some(json!({ "reasoning": "provider native secret" })),
                text: summary.to_owned(),
            }),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::Text(text.to_owned()),
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockStop { index: 0 },
        ModelStreamEvent::ContentBlockStop { index: 1 },
        ModelStreamEvent::MessageStop,
    ]
}

fn budget(limit: u64, on_overflow: OverflowAction) -> ResultBudget {
    ResultBudget {
        metric: BudgetMetric::Chars,
        limit,
        on_overflow,
        preview_head_chars: 2,
        preview_tail_chars: 2,
    }
}

fn tool_result_debug(request: &ModelRequest) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            MessagePart::ToolResult { content, .. } => Some(format!("{content:?}")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(feature = "recall-memory")]
fn request_text(request: &ModelRequest) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            MessagePart::Text(text) => Some(text.as_str()),
            MessagePart::ToolResult {
                content: ToolResult::Text(text),
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(feature = "recall-memory")]
struct StaticMemoryProvider {
    provider_id: String,
    records: Vec<MemoryRecord>,
}

#[cfg(feature = "recall-memory")]
impl StaticMemoryProvider {
    fn new(provider_id: &str, records: Vec<MemoryRecord>) -> Self {
        Self {
            provider_id: provider_id.to_owned(),
            records,
        }
    }
}

#[cfg(feature = "recall-memory")]
#[async_trait]
impl MemoryStore for StaticMemoryProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    async fn recall(
        &self,
        _query: MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, harness_contracts::MemoryError> {
        Ok(self.records.clone())
    }

    async fn upsert(
        &self,
        record: MemoryRecord,
    ) -> Result<harness_contracts::MemoryId, harness_contracts::MemoryError> {
        Ok(record.id)
    }

    async fn forget(
        &self,
        _id: harness_contracts::MemoryId,
    ) -> Result<(), harness_contracts::MemoryError> {
        Ok(())
    }

    async fn list(
        &self,
        _scope: harness_memory::MemoryListScope,
    ) -> Result<Vec<MemorySummary>, harness_contracts::MemoryError> {
        Ok(Vec::new())
    }
}

#[cfg(feature = "recall-memory")]
impl MemoryLifecycle for StaticMemoryProvider {}

#[cfg(feature = "recall-memory")]
impl harness_memory::MemoryProvider for StaticMemoryProvider {}

#[cfg(feature = "recall-memory")]
fn memory_record(content: &str) -> MemoryRecord {
    MemoryRecord {
        id: harness_contracts::MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: harness_contracts::MemoryKind::ProjectFact,
        visibility: harness_contracts::MemoryVisibility::Tenant,
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: harness_contracts::MemorySource::UserInput,
            confidence: 1.0,
            evidence: None,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 1.0,
            recall_score_breakdown: None,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: harness_contracts::now(),
        updated_at: harness_contracts::now(),
    }
}

fn assert_single_run_end(events: &[Event]) {
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::RunEnded(_)))
            .count(),
        1
    );
}
