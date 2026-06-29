use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex as StdMutex,
};
use std::{future, time::Duration};

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::{
    BudgetMetric, CapabilityRegistry, ClarifyAnswer, ClarifyChannelCap, ClarifyPrompt, DecidedBy,
    Decision, DecisionScope, DeferPolicy, Event, HookEventKind, Message, MessagePart, MessageRole,
    ModelError, NoopRedactor, OverflowAction, PermissionError, PermissionMode, PermissionSubject,
    ProviderRestriction, RedactRules, Redactor, ResultBudget, RunId, SessionId, StopReason,
    TenantId, ToolCapability, ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolProperties,
    ToolResult, ToolUseId, TrustLevel, UsageSnapshot,
};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookHandler, HookOutcome, HookRegistry,
};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_model::{
    ContentDelta, ConversationModelCapability, ErrorClass, ErrorHints, HealthStatus, InferContext,
    ModelDescriptor, ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_session::{Session, SessionOptions, SessionTurnRuntime};
use harness_tool::{
    BuiltinToolset, SchemaResolverContext, Tool, ToolContext, ToolEvent, ToolPool, ToolPoolFilter,
    ToolPoolModelProfile, ToolRegistry, ToolSearchMode, ToolStream, ValidationError,
};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::sync::Notify;

#[tokio::test]
async fn run_turn_executes_list_dir_with_formal_runtime() {
    let harness = TestHarness::new(tool_call_events("ListDir", json!({ "path": "" }))).await;
    std::fs::write(harness.workspace.path().join("marker.txt"), "m3").unwrap();
    harness
        .model
        .replace_events(tool_call_events(
            "ListDir",
            json!({ "path": harness.workspace.path() }),
        ))
        .await;

    harness.session.run_turn("list current dir").await.unwrap();

    let projection = harness.session.projection().await;
    let assistant = projection
        .messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::Assistant)
        .map(message_text)
        .unwrap_or_default();
    assert!(!assistant.contains("marker.txt"));
    assert!(assistant.contains("Tool result withheld from conversation transcript."));
    assert_eq!(harness.user_prompt_hooks.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn run_turn_records_run_tool_permission_assistant_events() {
    let harness = TestHarness::new(tool_call_events("ListDir", json!({ "path": "" }))).await;
    harness
        .model
        .replace_events(tool_call_events(
            "ListDir",
            json!({ "path": harness.workspace.path() }),
        ))
        .await;

    harness.session.run_turn("list current dir").await.unwrap();

    let events = harness.events().await;
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::RunStarted(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::UserMessageAppended(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseRequested(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::PermissionRequested(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::PermissionResolved(resolved)
            if matches!(resolved.decided_by, DecidedBy::Broker { .. }))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseApproved(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::AssistantMessageCompleted(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::RunEnded(_))));
}

#[tokio::test]
async fn run_turn_hook_payload_and_recent_messages_are_redacted() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let harness = TestHarness::new_with_hooks_and_redactor(
        text_events("assistant secret-token"),
        vec![Box::new(CaptureUserPromptHook {
            captured: Arc::clone(&captured),
        })],
        Arc::new(SecretRedactor),
    )
    .await;

    harness.session.run_turn("user secret-token").await.unwrap();
    harness.model.replace_events(text_events("done")).await;
    harness.session.run_turn("next secret-token").await.unwrap();

    let captured = captured.lock().await.clone();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].prompt, "user [redacted]");
    assert!(captured[0].recent_messages.is_empty());
    assert_eq!(captured[1].prompt, "next [redacted]");
    assert!(captured[1]
        .recent_messages
        .iter()
        .any(|message| message.contains("user [redacted]")));
    assert!(captured[1]
        .recent_messages
        .iter()
        .any(|message| message.contains("assistant [redacted]")));
    assert!(!captured[1]
        .recent_messages
        .iter()
        .any(|message| message.contains("secret-token")));
}

#[tokio::test]
async fn run_turn_uses_session_permission_mode_for_hooks_and_permissions() {
    let captured_hooks = Arc::new(Mutex::new(Vec::new()));
    let captured_permissions = Arc::new(Mutex::new(Vec::new()));
    let broker = Arc::new(RecordingPermissionBroker {
        captured: Arc::clone(&captured_permissions),
    });
    let harness = TestHarness::new_with_permission_mode_hooks_redactor_and_broker(
        tool_call_events("ListDir", json!({ "path": "" })),
        vec![Box::new(CaptureUserPromptHook {
            captured: Arc::clone(&captured_hooks),
        })],
        Arc::new(harness_contracts::NoopRedactor),
        PermissionMode::BypassPermissions,
        broker,
    )
    .await;

    harness.session.run_turn("list current dir").await.unwrap();

    let captured_hooks = captured_hooks.lock().await.clone();
    assert_eq!(
        captured_hooks[0].permission_mode,
        PermissionMode::BypassPermissions
    );
    assert_eq!(
        captured_hooks[0].view_permission_mode,
        PermissionMode::BypassPermissions
    );
    assert_eq!(
        captured_permissions.lock().await.as_slice(),
        &[PermissionMode::BypassPermissions]
    );
}

#[tokio::test]
async fn run_turn_bypass_permission_mode_keeps_permission_audit_context() {
    let captured_permissions = Arc::new(Mutex::new(Vec::new()));
    let broker = Arc::new(RecordingPermissionBroker {
        captured: Arc::clone(&captured_permissions),
    });
    let harness = TestHarness::new_with_permission_mode_hooks_redactor_and_broker(
        tool_call_events("ListDir", json!({ "path": "" })),
        Vec::new(),
        Arc::new(harness_contracts::NoopRedactor),
        PermissionMode::BypassPermissions,
        broker,
    )
    .await;

    harness.session.run_turn("list current dir").await.unwrap();

    let events = harness.events().await;
    let requested = events
        .iter()
        .find_map(|event| match event {
            Event::PermissionRequested(requested) => Some(requested),
            _ => None,
        })
        .expect("bypass mode should still journal the permission request context");
    let resolved = events
        .iter()
        .find_map(|event| match event {
            Event::PermissionResolved(resolved) if resolved.request_id == requested.request_id => {
                Some(resolved)
            }
            _ => None,
        })
        .expect("bypass mode should resolve the journaled permission request");

    let run_id = events
        .iter()
        .find_map(|event| match event {
            Event::RunStarted(started) => Some(started.run_id),
            _ => None,
        })
        .expect("run should start");
    let tool_use_id = events
        .iter()
        .find_map(|event| match event {
            Event::ToolUseRequested(requested) => Some(requested.tool_use_id),
            _ => None,
        })
        .expect("tool use should be requested");

    assert_eq!(requested.run_id, run_id);
    assert_eq!(requested.session_id, harness.session_id);
    assert_eq!(requested.tenant_id, harness.tenant_id);
    assert_eq!(requested.tool_use_id, tool_use_id);
    assert!(matches!(resolved.decision, Decision::AllowOnce));
}

#[tokio::test]
async fn run_turn_bypass_permission_mode_converts_escalation_to_auto_resolved_allow() {
    let harness = TestHarness::new_with_permission_mode_hooks_redactor_and_broker(
        tool_call_events("ListDir", json!({ "path": "" })),
        Vec::new(),
        Arc::new(harness_contracts::NoopRedactor),
        PermissionMode::BypassPermissions,
        Arc::new(EscalatingPermissionBroker),
    )
    .await;
    harness
        .model
        .replace_events(tool_call_events(
            "ListDir",
            json!({ "path": harness.workspace.path() }),
        ))
        .await;

    harness.session.run_turn("list current dir").await.unwrap();

    let events = harness.events().await;
    let requested = events
        .iter()
        .find_map(|event| match event {
            Event::PermissionRequested(requested) => Some(requested),
            _ => None,
        })
        .expect("bypass mode should journal the auto-resolved permission request");
    let resolved = events
        .iter()
        .find_map(|event| match event {
            Event::PermissionResolved(resolved) if resolved.request_id == requested.request_id => {
                Some(resolved)
            }
            _ => None,
        })
        .expect("bypass mode should journal the resolved permission decision");

    assert!(requested.auto_resolved);
    assert_eq!(resolved.decision, Decision::AllowOnce);
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
}

#[tokio::test]
async fn run_turn_persists_blocking_tool_journal_event_before_answer() {
    let channel = BlockingClarifyChannel::new();
    let mut caps = CapabilityRegistry::default();
    let clarify: Arc<dyn ClarifyChannelCap> = Arc::new(channel.clone());
    caps.install(ToolCapability::ClarifyChannel, clarify);
    let harness = TestHarness::with_toolset_and_cap_registry(
        tool_call_events("Clarify", json!({ "prompt": "Pick one" })),
        BuiltinToolset::Clarification,
        caps,
    )
    .await;

    let run = tokio::spawn({
        let session = harness.session.clone();
        async move { session.run_turn("need clarification").await }
    });
    channel.wait_until_waiting().await;

    let events = harness.events().await;
    let permission_requested_index = events
        .iter()
        .position(|event| matches!(event, Event::PermissionRequested(_)))
        .expect("permission request should be persisted");
    let tool_approved_index = events
        .iter()
        .position(|event| matches!(event, Event::ToolUseApproved(_)))
        .expect("tool approval should be persisted");
    let clarification_index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::AssistantClarificationRequested(requested)
                    if requested.prompt.as_str() == "Pick one"
            )
        })
        .expect("clarification request should be persisted");

    assert!(permission_requested_index < clarification_index);
    assert!(tool_approved_index < clarification_index);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::AssistantClarificationRequested(requested)
                if requested.prompt.as_str() == "Pick one"
        )
    }));

    channel.answer("A");
    run.await.unwrap().unwrap();
}

#[tokio::test]
async fn run_turn_keeps_session_open_after_completed_run() {
    let harness = TestHarness::new(text_events("ok")).await;

    harness.session.run_turn("hello").await.unwrap();

    assert_eq!(harness.session.projection().await.end_reason, None);
}

#[tokio::test]
async fn run_turn_sends_previous_user_and_assistant_messages_to_next_model_request() {
    let harness = TestHarness::new(text_events("first assistant answer")).await;

    harness
        .session
        .run_turn("first user request")
        .await
        .unwrap();
    harness
        .model
        .replace_events(text_events("second assistant answer"))
        .await;
    harness
        .session
        .run_turn("second user request")
        .await
        .unwrap();

    let requests = harness.model.requests().await;
    assert_eq!(requests.len(), 2);
    let second_request_messages = requests[1]
        .messages
        .iter()
        .map(|message| (message.role, message_text(message)))
        .collect::<Vec<_>>();
    assert_eq!(
        second_request_messages,
        vec![
            (MessageRole::User, "first user request".to_owned()),
            (MessageRole::Assistant, "first assistant answer".to_owned()),
            (MessageRole::User, "second user request".to_owned()),
        ]
    );
}

#[tokio::test]
async fn run_turn_keeps_thinking_out_of_durable_events() {
    let harness =
        TestHarness::new(thinking_then_text_events("internal chain", "final answer")).await;

    harness.session.run_turn("hello").await.unwrap();

    let events = harness.events().await;
    assert!(!events.iter().any(|event| {
        matches!(
            event,
            Event::AssistantDeltaProduced(delta)
                if matches!(&delta.delta, harness_contracts::DeltaChunk::Thought(thought)
                    if thought.text.as_deref() == Some("internal chain"))
        )
    }));
    let completed = events
        .iter()
        .find_map(|event| match event {
            Event::AssistantMessageCompleted(completed) => Some(completed),
            _ => None,
        })
        .expect("completed assistant message should be emitted");

    assert_eq!(
        completed.content,
        harness_contracts::MessageContent::Text("final answer".to_owned())
    );
}

#[tokio::test]
async fn run_turn_records_run_end_on_model_infer_error() {
    let harness = TestHarness::new(text_events("unused")).await;
    harness
        .model
        .replace_response(ModelResponse::Error(ModelError::ProviderUnavailable(
            "offline".to_owned(),
        )))
        .await;

    let error = harness.session.run_turn("hello").await.unwrap_err();

    assert!(error.to_string().contains("offline"));
    assert!(harness
        .events()
        .await
        .iter()
        .any(|event| matches!(event, Event::RunEnded(ended)
            if matches!(&ended.reason, harness_contracts::EndReason::Error(message)
                if message.contains("offline")))));
}

#[tokio::test]
async fn run_turn_records_run_end_on_model_stream_error() {
    let harness = TestHarness::new(vec![ModelStreamEvent::StreamError {
        error: ModelError::UnexpectedResponse("bad chunk".to_owned()),
        class: ErrorClass::Fatal,
        hints: ErrorHints::default(),
    }])
    .await;

    let error = harness.session.run_turn("hello").await.unwrap_err();

    assert!(error.to_string().contains("bad chunk"));
    assert!(harness
        .events()
        .await
        .iter()
        .any(|event| matches!(event, Event::RunEnded(ended)
            if matches!(&ended.reason, harness_contracts::EndReason::Error(message)
                if message.contains("bad chunk")))));
}

#[tokio::test]
async fn run_turn_finalizes_when_model_stream_message_stop_arrives_without_eof() {
    let harness = TestHarness::new(text_events("unused")).await;
    harness
        .model
        .replace_response(ModelResponse::EventsThenPending(text_events(
            "complete answer",
        )))
        .await;

    let result = tokio::time::timeout(
        Duration::from_millis(200),
        harness.session.run_turn("hello"),
    )
    .await
    .expect("run should finalize after MessageStop without waiting for stream EOF");

    result.unwrap();

    let events = harness.events().await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::AssistantMessageCompleted(completed)
                if completed.content
                    == harness_contracts::MessageContent::Text("complete answer".to_owned())
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::RunEnded(ended)
                if matches!(ended.reason, harness_contracts::EndReason::Completed)
        )
    }));
}

#[tokio::test]
async fn run_turn_rejects_missing_runtime() {
    let root = tempfile::tempdir().unwrap();
    let session = Session::builder()
        .with_options(SessionOptions::new(root.path()))
        .with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .build()
        .await
        .unwrap();

    let error = session.run_turn("hello").await.unwrap_err();

    assert!(error.to_string().contains("turn runtime missing"));
}

#[cfg(feature = "steering")]
#[tokio::test]
async fn run_turn_drains_steering_before_model_infer() {
    use harness_contracts::{SteeringBody, SteeringKind, SteeringSource};
    use harness_session::SteeringRequest;

    let harness = TestHarness::new(text_events("ok")).await;
    harness
        .session
        .push_steering(SteeringRequest {
            kind: SteeringKind::Append,
            body: SteeringBody::Text("include hidden files".to_owned()),
            priority: None,
            correlation_id: None,
            source: SteeringSource::User,
        })
        .await
        .unwrap();

    harness.session.run_turn("list current dir").await.unwrap();

    let requests = harness.model.requests().await;
    let user_text = requests[0]
        .messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::User)
        .map(message_text)
        .unwrap_or_default();
    assert!(user_text.contains("list current dir"));
    assert!(user_text.contains("include hidden files"));
    assert!(harness
        .events()
        .await
        .iter()
        .any(|event| matches!(event, Event::SteeringMessageApplied(_))));
}

struct TestHarness {
    workspace: TempDir,
    tenant_id: TenantId,
    session_id: SessionId,
    store: Arc<InMemoryEventStore>,
    session: Arc<Session>,
    model: Arc<RecordingModelProvider>,
    user_prompt_hooks: Arc<AtomicUsize>,
}

impl TestHarness {
    async fn new(events: Vec<ModelStreamEvent>) -> Self {
        let user_prompt_hooks = Arc::new(AtomicUsize::new(0));
        Self::new_with_hooks_and_redactor(
            events,
            vec![Box::new(CountingHook {
                calls: user_prompt_hooks.clone(),
            })],
            Arc::new(harness_contracts::NoopRedactor),
        )
        .await
        .with_user_prompt_hooks(user_prompt_hooks)
    }

    async fn new_with_hooks_and_redactor(
        events: Vec<ModelStreamEvent>,
        hooks: Vec<Box<dyn HookHandler>>,
        redactor: Arc<dyn Redactor>,
    ) -> Self {
        Self::new_with_permission_mode_hooks_redactor_and_broker(
            events,
            hooks,
            redactor,
            PermissionMode::Default,
            Arc::new(AllowBroker),
        )
        .await
    }

    async fn new_with_permission_mode_hooks_redactor_and_broker(
        events: Vec<ModelStreamEvent>,
        hooks: Vec<Box<dyn HookHandler>>,
        redactor: Arc<dyn Redactor>,
        permission_mode: PermissionMode,
        permission_broker: Arc<dyn PermissionBroker>,
    ) -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let tenant_id = TenantId::SINGLE;
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(RecordingModelProvider::new(events));
        let user_prompt_hooks = Arc::new(AtomicUsize::new(0));
        let mut hook_builder = HookRegistry::builder();
        for hook in hooks {
            hook_builder = hook_builder.with_hook(hook);
        }
        let hooks = hook_builder.build().unwrap();
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Custom(vec![Box::new(
                TestListDirTool::new(),
            )]))
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
        let runtime = SessionTurnRuntime {
            context: ContextEngine::builder().build().unwrap(),
            hooks: HookDispatcher::new(hooks.snapshot()),
            model: model.clone(),
            tools,
            permission_broker,
            sandbox: None,
            cap_registry: Arc::new(CapabilityRegistry::default()),
            redactor,
            blob_store: None,
            model_id: "test-model".to_owned(),
            model_extra: serde_json::Value::Null,
            protocol: ModelProtocol::Messages,
            system_prompt: Some("system".to_owned()),
        };
        let session = Arc::new(
            Session::builder()
                .with_options(
                    SessionOptions::new(workspace.path())
                        .with_tenant_id(tenant_id)
                        .with_session_id(session_id)
                        .with_permission_mode(permission_mode),
                )
                .with_event_store(store.clone())
                .with_turn_runtime(runtime)
                .build()
                .await
                .unwrap(),
        );

        Self {
            workspace,
            tenant_id,
            session_id,
            store,
            session,
            model,
            user_prompt_hooks,
        }
    }

    fn with_user_prompt_hooks(mut self, user_prompt_hooks: Arc<AtomicUsize>) -> Self {
        self.user_prompt_hooks = user_prompt_hooks;
        self
    }

    async fn with_toolset_and_cap_registry(
        events: Vec<ModelStreamEvent>,
        builtin_toolset: BuiltinToolset,
        cap_registry: CapabilityRegistry,
    ) -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let tenant_id = TenantId::SINGLE;
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(RecordingModelProvider::new(events));
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
        let runtime = SessionTurnRuntime {
            context: ContextEngine::builder().build().unwrap(),
            hooks: HookDispatcher::new(hooks.snapshot()),
            model: model.clone(),
            tools,
            permission_broker: Arc::new(AllowBroker),
            sandbox: None,
            cap_registry: Arc::new(cap_registry),
            redactor: Arc::new(harness_contracts::NoopRedactor),
            blob_store: None,
            model_id: "test-model".to_owned(),
            model_extra: serde_json::Value::Null,
            protocol: ModelProtocol::Messages,
            system_prompt: Some("system".to_owned()),
        };
        let session = Arc::new(
            Session::builder()
                .with_options(
                    SessionOptions::new(workspace.path())
                        .with_tenant_id(tenant_id)
                        .with_session_id(session_id),
                )
                .with_event_store(store.clone())
                .with_turn_runtime(runtime)
                .build()
                .await
                .unwrap(),
        );

        Self {
            workspace,
            tenant_id,
            session_id,
            store,
            session,
            model,
            user_prompt_hooks,
        }
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

#[derive(Clone)]
struct BlockingClarifyChannel {
    state: Arc<BlockingClarifyState>,
}

struct BlockingClarifyState {
    waiting: Notify,
    answer_sender: StdMutex<Option<oneshot::Sender<String>>>,
}

impl BlockingClarifyChannel {
    fn new() -> Self {
        Self {
            state: Arc::new(BlockingClarifyState {
                waiting: Notify::new(),
                answer_sender: StdMutex::new(None),
            }),
        }
    }

    async fn wait_until_waiting(&self) {
        self.state.waiting.notified().await;
    }

    fn answer(&self, answer: &str) {
        let sender = self
            .state
            .answer_sender
            .lock()
            .unwrap()
            .take()
            .expect("blocking journal tool should be waiting");
        sender
            .send(answer.to_owned())
            .expect("blocking journal tool receiver should be open");
    }
}

impl ClarifyChannelCap for BlockingClarifyChannel {
    fn ask(
        &self,
        _prompt: ClarifyPrompt,
    ) -> futures::future::BoxFuture<'static, Result<ClarifyAnswer, ToolError>> {
        let state = Arc::clone(&self.state);
        let (sender, receiver) = oneshot::channel();
        *state.answer_sender.lock().unwrap() = Some(sender);
        Box::pin(async move {
            state.waiting.notify_waiters();
            match receiver.await {
                Ok(answer) => Ok(ClarifyAnswer {
                    answer,
                    chosen_ids: Vec::new(),
                }),
                Err(_) => Err(ToolError::Message(
                    "blocking journal answer sender dropped".to_owned(),
                )),
            }
        })
    }
}

struct RecordingModelProvider {
    response: Mutex<ModelResponse>,
    requests: Mutex<Vec<ModelRequest>>,
}

impl RecordingModelProvider {
    fn new(events: Vec<ModelStreamEvent>) -> Self {
        Self {
            response: Mutex::new(ModelResponse::Events(events)),
            requests: Mutex::new(Vec::new()),
        }
    }

    async fn replace_events(&self, events: Vec<ModelStreamEvent>) {
        self.replace_response(ModelResponse::Events(events)).await;
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
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);
        match self.response.lock().await.clone() {
            ModelResponse::Events(events) => Ok(Box::pin(stream::iter(events))),
            ModelResponse::EventsThenPending(events) => Ok(Box::pin(stream::unfold(
                events.into_iter(),
                |mut events| async move {
                    if let Some(event) = events.next() {
                        Some((event, events))
                    } else {
                        future::pending().await
                    }
                },
            ))),
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
    EventsThenPending(Vec<ModelStreamEvent>),
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

struct RecordingPermissionBroker {
    captured: Arc<Mutex<Vec<PermissionMode>>>,
}

#[async_trait]
impl PermissionBroker for RecordingPermissionBroker {
    async fn decide(&self, _request: PermissionRequest, ctx: PermissionContext) -> Decision {
        self.captured.lock().await.push(ctx.permission_mode);
        Decision::AllowOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}

struct EscalatingPermissionBroker;

#[async_trait]
impl PermissionBroker for EscalatingPermissionBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::Escalate
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct CapturedUserPromptHook {
    prompt: String,
    recent_messages: Vec<String>,
    permission_mode: PermissionMode,
    view_permission_mode: PermissionMode,
}

struct CaptureUserPromptHook {
    captured: Arc<Mutex<Vec<CapturedUserPromptHook>>>,
}

#[async_trait]
impl HookHandler for CaptureUserPromptHook {
    fn handler_id(&self) -> &'static str {
        "capture-user-prompt"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::UserPromptSubmit]
    }

    async fn handle(
        &self,
        event: HookEvent,
        ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        let HookEvent::UserPromptSubmit { input, .. } = event else {
            unreachable!("unexpected event");
        };
        let prompt = input["prompt"].as_str().unwrap_or_default().to_owned();
        let recent_messages = ctx
            .view
            .recent_messages(8)
            .into_iter()
            .map(|message| message.text_snippet)
            .collect();
        self.captured.lock().await.push(CapturedUserPromptHook {
            prompt,
            recent_messages,
            permission_mode: ctx.permission_mode,
            view_permission_mode: ctx.view.permission_mode(),
        });
        Ok(HookOutcome::Continue)
    }
}

struct SecretRedactor;

impl Redactor for SecretRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("secret-token", "[redacted]")
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

    async fn check_permission(
        &self,
        input: &Value,
        _ctx: &ToolContext,
    ) -> harness_permission::PermissionCheck {
        harness_permission::PermissionCheck::AskUser {
            subject: PermissionSubject::ToolInvocation {
                tool: "ListDir".to_owned(),
                input: input.clone(),
            },
            scope: DecisionScope::PathPrefix(input["path"].as_str().unwrap_or_default().into()),
        }
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolContext,
    ) -> Result<ToolStream, harness_contracts::ToolError> {
        let path = input["path"].as_str().unwrap_or_default();
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)
            .map_err(|error| harness_contracts::ToolError::Message(error.to_string()))?
        {
            let entry =
                entry.map_err(|error| harness_contracts::ToolError::Message(error.to_string()))?;
            entries.push(entry.file_name().to_string_lossy().into_owned());
        }
        entries.sort();
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!(entries)),
        )])))
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
                provider_native: None,
                signature: None,
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

fn message_text(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
