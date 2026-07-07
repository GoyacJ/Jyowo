#![cfg(feature = "builtin-toolset")]

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures::{future::BoxFuture, stream};
use harness_contracts::{
    ActionPlanHash, ActionPlanId, AssistantClarificationRequestedEvent, CapabilityRegistry,
    ClarifyAnswer, ClarifyChannelCap, ClarifyPrompt, Decision, DecisionScope, DeferredToolHint,
    Event, ExecFingerprint, ExecuteCodeStepInvokedEvent, NetworkAccess, PermissionActorSource,
    PermissionReview, PermissionSubject, ProviderRestriction, RedactRules, Redactor, RequestId,
    ResourceLimits, SandboxExecutionStartedEvent, SandboxMode, SandboxPolicy, SandboxPolicySummary,
    SandboxScope, Severity, TenantId, ToolActionPlan, ToolCapability, ToolDeferredPoolChangedEvent,
    ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolPoolChangeSource, ToolProperties,
    ToolResult, ToolUseHeartbeatEvent, ToolUseId, TrustLevel, UiSafeText, WorkspaceAccess,
};
use harness_permission::PermissionCheck;
use harness_tool::{
    default_result_budget, AuthorizedTicketSummary, AuthorizedToolCall, AuthorizedToolInput,
    BuiltinToolset, InterruptToken, NoopToolEventEmitter, OrchestratorContext, Tool, ToolContext,
    ToolEvent, ToolEventEmitter, ToolOrchestrator, ToolPool, ToolPoolFilter, ToolPoolModelProfile,
    ToolRegistry, ToolSearchMode, ValidationError,
};
use parking_lot::Mutex;
use serde_json::{json, Value};

#[tokio::test]
async fn safe_tools_run_in_parallel_and_preserve_input_order() {
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool(
            "b",
            true,
            Behavior::Delay {
                active: Arc::clone(&active),
                max_active: Arc::clone(&max_active),
            },
        )))
        .with_tool(Box::new(test_tool(
            "a",
            true,
            Behavior::Delay {
                active,
                max_active: Arc::clone(&max_active),
            },
        )))
        .build()
        .unwrap();

    let pool = pool(&registry).await;
    let orchestrator = ToolOrchestrator::new(2);
    let ctx = orchestrator_ctx(pool, vec![Decision::AllowOnce, Decision::AllowOnce]);

    let results = orchestrator.dispatch(vec![call("b"), call("a")], ctx).await;

    assert_eq!(names(&results), ["b", "a"]);
    assert!(results.iter().all(|result| result.result.is_ok()));
    assert_eq!(max_active.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn unsafe_tools_run_serially() {
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool(
            "a",
            false,
            Behavior::Delay {
                active: Arc::clone(&active),
                max_active: Arc::clone(&max_active),
            },
        )))
        .with_tool(Box::new(test_tool(
            "b",
            false,
            Behavior::Delay { active, max_active },
        )))
        .build()
        .unwrap();

    let pool = pool(&registry).await;
    let results = ToolOrchestrator::new(2)
        .dispatch(
            vec![call("a"), call("b")],
            orchestrator_ctx(pool, vec![Decision::AllowOnce, Decision::AllowOnce]),
        )
        .await;

    assert_eq!(names(&results), ["a", "b"]);
    assert!(results.iter().all(|result| result.result.is_ok()));
    assert_eq!(
        match results[0].result.as_ref().unwrap() {
            ToolResult::Structured(value) => value["active_at_start"].as_u64().unwrap(),
            other => panic!("unexpected result: {other:?}"),
        },
        1
    );
    assert_eq!(
        match results[1].result.as_ref().unwrap() {
            ToolResult::Structured(value) => value["active_at_start"].as_u64().unwrap(),
            other => panic!("unexpected result: {other:?}"),
        },
        1
    );
}

#[tokio::test]
async fn unsafe_tool_is_a_barrier_between_safe_batches() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool(
            "safe_a",
            true,
            Behavior::Log(Arc::clone(&log)),
        )))
        .with_tool(Box::new(test_tool(
            "safe_b",
            true,
            Behavior::Log(Arc::clone(&log)),
        )))
        .with_tool(Box::new(test_tool(
            "unsafe",
            false,
            Behavior::Log(Arc::clone(&log)),
        )))
        .with_tool(Box::new(test_tool(
            "safe_c",
            true,
            Behavior::Log(Arc::clone(&log)),
        )))
        .build()
        .unwrap();

    let pool = pool(&registry).await;
    let results = ToolOrchestrator::new(3)
        .dispatch(
            vec![
                call("safe_a"),
                call("safe_b"),
                call("unsafe"),
                call("safe_c"),
            ],
            orchestrator_ctx(
                pool,
                vec![
                    Decision::AllowOnce,
                    Decision::AllowOnce,
                    Decision::AllowOnce,
                    Decision::AllowOnce,
                ],
            ),
        )
        .await;

    assert!(results.iter().all(|result| result.result.is_ok()));
    let log = log.lock().clone();
    assert!(index_of(&log, "end:safe_a") < index_of(&log, "start:unsafe"));
    assert!(index_of(&log, "end:safe_b") < index_of(&log, "start:unsafe"));
    assert!(index_of(&log, "end:unsafe") < index_of(&log, "start:safe_c"));
}

#[tokio::test]
async fn validation_failure_skips_permission_and_execute() {
    let executed = Arc::new(AtomicBool::new(false));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool(
            "bad",
            true,
            Behavior::ValidationError(Arc::clone(&executed)),
        )))
        .build()
        .unwrap();

    let pool = pool(&registry).await;
    let ctx = orchestrator_ctx(pool, vec![]);
    let results = ToolOrchestrator::default()
        .dispatch(vec![call("bad")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::Validation(ref message)) if message == "invalid input"
    ));
    assert!(!executed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn authorized_tool_calls_execute_without_permission_broker() {
    let executed = Arc::new(AtomicBool::new(false));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool(
            "guarded",
            true,
            Behavior::MarkExecuted(Arc::clone(&executed)),
        )))
        .build()
        .unwrap();

    let pool = pool(&registry).await;
    let ctx = orchestrator_ctx(pool, vec![]);
    let results = ToolOrchestrator::default()
        .dispatch(vec![call("guarded")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Ok(ToolResult::Text(ref message)) if message == "executed"
    ));
    assert!(executed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn progress_final_error_unknown_interrupted_and_timeout_paths_are_reported() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool("progress", true, Behavior::Progress)))
        .with_tool(Box::new(test_tool("error", true, Behavior::StreamError)))
        .with_tool(Box::new(test_tool("slow", true, Behavior::Slow)))
        .build()
        .unwrap();
    let pool = pool(&registry).await;

    let results = ToolOrchestrator::default()
        .dispatch(
            vec![call("progress"), call("missing")],
            orchestrator_ctx(pool.clone(), vec![Decision::AllowOnce]),
        )
        .await;
    assert_eq!(results[0].progress_emitted, 2);
    assert!(matches!(results[0].result, Ok(ToolResult::Text(ref text)) if text == "done"));
    assert!(
        matches!(results[1].result, Err(ToolError::Internal(ref message)) if message.contains("tool not found"))
    );

    let results = ToolOrchestrator::default()
        .dispatch(
            vec![call("error")],
            orchestrator_ctx(pool.clone(), vec![Decision::AllowOnce]),
        )
        .await;
    assert!(
        matches!(results[0].result, Err(ToolError::Message(ref message)) if message == "stream failed")
    );

    let interrupted = InterruptToken::default();
    interrupted.interrupt();
    let results = ToolOrchestrator::default()
        .dispatch(
            vec![call("progress")],
            orchestrator_ctx_with_interrupt(pool.clone(), vec![Decision::AllowOnce], interrupted),
        )
        .await;
    assert!(matches!(results[0].result, Err(ToolError::Interrupted)));

    let results = ToolOrchestrator::default()
        .dispatch(
            vec![call("slow")],
            orchestrator_ctx(pool, vec![Decision::AllowOnce]),
        )
        .await;
    assert!(matches!(results[0].result, Err(ToolError::Timeout)));
}

#[tokio::test]
async fn long_running_tool_emits_heartbeat_when_stalled() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool("stalled", true, Behavior::Stalled)))
        .build()
        .unwrap();
    let pool = pool(&registry).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let mut ctx = orchestrator_ctx(pool, vec![Decision::AllowOnce]);
    ctx.event_emitter = emitter.clone();
    let call = call_with_input("stalled", json!({}));
    let tool_use_id = call.tool_use_id;
    let results = ToolOrchestrator::default().dispatch(vec![call], ctx).await;

    assert!(matches!(results[0].result, Ok(ToolResult::Text(ref text)) if text == "late"));
    assert!(results[0].progress_emitted >= 1);
    assert!(emitter.events().iter().any(|event| {
        matches!(event, Event::ToolUseHeartbeat(heartbeat) if heartbeat.tool_use_id == tool_use_id)
    }));
}

#[tokio::test]
async fn tool_journal_rejects_unowned_event_types() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool("journal", true, Behavior::Journal)))
        .build()
        .unwrap();
    let pool = pool(&registry).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let mut ctx = orchestrator_ctx(pool, vec![Decision::AllowOnce]);
    ctx.event_emitter = emitter.clone();

    let results = ToolOrchestrator::default()
        .dispatch(vec![call("journal")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::PermissionDenied(ref message))
            if message == "tool journal event type is not allowed"
    ));
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn tool_journal_rejects_non_owner_clarification_event() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool("Clarify", true, Behavior::SpoofClarify)))
        .build()
        .unwrap();
    let pool = pool(&registry).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let mut ctx = orchestrator_ctx(pool, vec![Decision::AllowOnce]);
    ctx.event_emitter = emitter.clone();

    let results = ToolOrchestrator::default()
        .dispatch(vec![call("Clarify")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::PermissionDenied(ref message))
            if message == "tool journal event producer is not allowed"
    ));
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn tool_journal_rejects_non_owner_sandbox_event() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool("Bash", true, Behavior::SpoofSandbox)))
        .build()
        .unwrap();
    let pool = pool(&registry).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let mut ctx = orchestrator_ctx(pool, vec![Decision::AllowOnce]);
    ctx.event_emitter = emitter.clone();

    let results = ToolOrchestrator::default()
        .dispatch(vec![call("Bash")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::PermissionDenied(ref message))
            if message == "tool journal event producer is not allowed"
    ));
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn tool_journal_rejects_non_owner_execute_code_event() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool(
            "execute_code",
            true,
            Behavior::SpoofExecuteCode,
        )))
        .build()
        .unwrap();
    let pool = pool(&registry).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let mut ctx = orchestrator_ctx(pool, vec![Decision::AllowOnce]);
    ctx.event_emitter = emitter.clone();

    let results = ToolOrchestrator::default()
        .dispatch(vec![call("execute_code")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::PermissionDenied(ref message))
            if message == "tool journal event producer is not allowed"
    ));
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn tool_journal_rejects_deferred_pool_change_from_tool_stream() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool(
            "emit_deferred_delta",
            true,
            Behavior::DeferredPoolChange,
        )))
        .build()
        .unwrap();
    let pool = pool(&registry).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let mut ctx = orchestrator_ctx(pool, vec![Decision::AllowOnce]);
    ctx.event_emitter = emitter.clone();

    let results = ToolOrchestrator::default()
        .dispatch(vec![call("emit_deferred_delta")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::PermissionDenied(ref message))
            if message == "tool journal event type is not allowed"
    ));
    assert!(emitter.events().is_empty());
}

#[tokio::test]
#[cfg(feature = "builtin-toolset")]
async fn clarify_tool_emits_assistant_clarification_requested_event() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();
    let pool = pool(&registry).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let mut caps = CapabilityRegistry::default();
    let clarify: Arc<dyn ClarifyChannelCap> = Arc::new(StaticClarify);
    caps.install(ToolCapability::ClarifyChannel, clarify);
    let mut ctx = orchestrator_ctx(pool, vec![Decision::AllowOnce]);
    ctx.tool_context.cap_registry = Arc::new(caps);
    ctx.event_emitter = emitter.clone();
    let run_id = ctx.tool_context.run_id;

    let results = ToolOrchestrator::default()
        .dispatch(
            vec![call_with_input("Clarify", json!({ "prompt": "Pick one" }))],
            ctx,
        )
        .await;

    assert!(matches!(results[0].result, Ok(ToolResult::Structured(_))));
    assert!(emitter.events().iter().any(|event| {
        matches!(
            event,
            Event::AssistantClarificationRequested(requested)
                if requested.run_id == run_id && requested.prompt.as_str() == "Pick one"
        )
    }));
}

#[tokio::test]
#[cfg(feature = "builtin-toolset")]
async fn clarify_tool_redacts_prompt_before_journaling_clarification_request() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();
    let pool = pool(&registry).await;
    let emitter = Arc::new(RecordingEmitter::default());
    let mut caps = CapabilityRegistry::default();
    let clarify: Arc<dyn ClarifyChannelCap> = Arc::new(StaticClarify);
    caps.install(ToolCapability::ClarifyChannel, clarify);
    let mut ctx = orchestrator_ctx(pool, vec![Decision::AllowOnce]);
    ctx.tool_context.cap_registry = Arc::new(caps);
    ctx.tool_context.redactor = Arc::new(TestRedactor);
    ctx.event_emitter = emitter.clone();

    let results = ToolOrchestrator::default()
        .dispatch(
            vec![call_with_input(
                "Clarify",
                json!({ "prompt": "Deploy token SECRET-123?" }),
            )],
            ctx,
        )
        .await;

    assert!(matches!(results[0].result, Ok(ToolResult::Structured(_))));
    assert!(emitter.events().iter().any(|event| {
        matches!(
            event,
            Event::AssistantClarificationRequested(requested)
                if requested.prompt.as_str() == "Deploy token [REDACTED]?"
        )
    }));
}

#[test]
fn tool_crate_does_not_depend_on_model_or_hook_crates_for_orchestrator() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    #[cfg(not(feature = "minimax-tools"))]
    assert!(!manifest.lines().any(|line| {
        line.trim_start().starts_with("jyowo-harness-model =") && !line.contains("optional = true")
    }));
    assert!(!manifest.contains("jyowo-harness-hook"));
}

struct TestRedactor;

impl Redactor for TestRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("SECRET-123", "[REDACTED]")
    }
}

#[derive(Clone)]
struct TestTool {
    descriptor: ToolDescriptor,
    behavior: Behavior,
}

#[derive(Clone)]
enum Behavior {
    Delay {
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
    },
    Log(Arc<Mutex<Vec<String>>>),
    ValidationError(Arc<AtomicBool>),
    MarkExecuted(Arc<AtomicBool>),
    Progress,
    StreamError,
    Slow,
    Stalled,
    Journal,
    SpoofClarify,
    SpoofSandbox,
    SpoofExecuteCode,
    DeferredPoolChange,
}

#[async_trait]
impl Tool for TestTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        match &self.behavior {
            Behavior::ValidationError(_) => Err(ValidationError::from("invalid input")),
            _ => Ok(()),
        }
    }

    async fn plan(
        &self,
        input: &Value,
        ctx: &ToolContext,
    ) -> Result<harness_contracts::ToolActionPlan, ToolError> {
        let check = match &self.behavior {
            Behavior::MarkExecuted(_) => PermissionCheck::Allowed,
            _ => PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            },
        };
        harness_tool::action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            check,
            Vec::new(),
            harness_contracts::WorkspaceAccess::None,
            harness_contracts::NetworkAccess::None,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: harness_tool::AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<harness_tool::ToolStream, ToolError> {
        match &self.behavior {
            Behavior::Delay { active, max_active } => {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_active.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(50)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(Box::pin(stream::iter([ToolEvent::Final(
                    ToolResult::Structured(json!({ "active_at_start": current })),
                )])))
            }
            Behavior::Log(log) => {
                log.lock().push(format!("start:{}", self.descriptor.name));
                tokio::time::sleep(Duration::from_millis(20)).await;
                log.lock().push(format!("end:{}", self.descriptor.name));
                Ok(Box::pin(stream::iter([ToolEvent::Final(
                    ToolResult::Text(self.descriptor.name.clone()),
                )])))
            }
            Behavior::ValidationError(executed) | Behavior::MarkExecuted(executed) => {
                executed.store(true, Ordering::SeqCst);
                Ok(Box::pin(stream::iter([ToolEvent::Final(
                    ToolResult::Text("executed".to_owned()),
                )])))
            }
            Behavior::Progress => Ok(Box::pin(stream::iter([
                ToolEvent::Progress(harness_tool::ToolProgress::now("one")),
                ToolEvent::Progress(harness_tool::ToolProgress::now("two")),
                ToolEvent::Final(ToolResult::Text("done".to_owned())),
            ]))),
            Behavior::StreamError => Ok(Box::pin(stream::iter([ToolEvent::Error(
                ToolError::Message("stream failed".to_owned()),
            )]))),
            Behavior::Slow => Ok(Box::pin(stream::once(async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                ToolEvent::Final(ToolResult::Text("late".to_owned()))
            }))),
            Behavior::Stalled => Ok(Box::pin(stream::once(async {
                tokio::time::sleep(Duration::from_millis(35)).await;
                ToolEvent::Final(ToolResult::Text("late".to_owned()))
            }))),
            Behavior::Journal => Ok(Box::pin(stream::iter([
                ToolEvent::Journal(Event::ToolUseHeartbeat(ToolUseHeartbeatEvent {
                    tool_use_id: ctx.tool_use_id,
                    run_id: ctx.run_id,
                    message: "sandbox event".to_owned(),
                    fraction: None,
                    silent_for_ms: 0,
                    at: chrono::Utc::now(),
                })),
                ToolEvent::Final(ToolResult::Text("done".to_owned())),
            ]))),
            Behavior::SpoofClarify => Ok(Box::pin(stream::iter([
                ToolEvent::Journal(Event::AssistantClarificationRequested(
                    AssistantClarificationRequestedEvent {
                        run_id: ctx.run_id,
                        request_id: RequestId::new(),
                        prompt: UiSafeText::from_trusted_redacted("spoofed"),
                        at: chrono::Utc::now(),
                    },
                )),
                ToolEvent::Final(ToolResult::Text("done".to_owned())),
            ]))),
            Behavior::SpoofSandbox => Ok(Box::pin(stream::iter([
                ToolEvent::Journal(Event::SandboxExecutionStarted(
                    SandboxExecutionStartedEvent {
                        session_id: ctx.session_id,
                        run_id: ctx.run_id,
                        tool_use_id: Some(ctx.tool_use_id),
                        backend_id: "spoof".to_owned(),
                        fingerprint: ExecFingerprint([7; 32]),
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
                        at: chrono::Utc::now(),
                    },
                )),
                ToolEvent::Final(ToolResult::Text("done".to_owned())),
            ]))),
            Behavior::SpoofExecuteCode => Ok(Box::pin(stream::iter([
                ToolEvent::Journal(Event::ExecuteCodeStepInvoked(ExecuteCodeStepInvokedEvent {
                    parent_tool_use_id: ctx.tool_use_id,
                    run_id: ctx.run_id,
                    session_id: ctx.session_id,
                    embedded_tool: "Bash".to_owned(),
                    args_hash: [9; 32],
                    step_seq: 1,
                    duration_ms: 0,
                    overflow: None,
                    refused_reason: None,
                    at: chrono::Utc::now(),
                })),
                ToolEvent::Final(ToolResult::Text("done".to_owned())),
            ]))),
            Behavior::DeferredPoolChange => Ok(Box::pin(stream::iter([
                ToolEvent::Journal(Event::ToolDeferredPoolChanged(
                    ToolDeferredPoolChangedEvent {
                        session_id: ctx.session_id,
                        added: vec![DeferredToolHint {
                            name: "deferred_tool".to_owned(),
                            hint: None,
                        }],
                        removed: Vec::new(),
                        source: ToolPoolChangeSource::InitialClassification,
                        deferred_total: 1,
                        at: chrono::Utc::now(),
                    },
                )),
                ToolEvent::Final(ToolResult::Text("done".to_owned())),
            ]))),
        }
    }
}

#[derive(Default)]
struct RecordingEmitter {
    events: Mutex<Vec<Event>>,
}

impl RecordingEmitter {
    fn events(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

impl ToolEventEmitter for RecordingEmitter {
    fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }
}

#[cfg(feature = "builtin-toolset")]
struct StaticClarify;

#[cfg(feature = "builtin-toolset")]
impl ClarifyChannelCap for StaticClarify {
    fn ask(&self, _prompt: ClarifyPrompt) -> BoxFuture<'static, Result<ClarifyAnswer, ToolError>> {
        Box::pin(async {
            Ok(ClarifyAnswer {
                answer: "A".to_owned(),
                chosen_ids: Vec::new(),
            })
        })
    }
}

async fn pool(registry: &ToolRegistry) -> ToolPool {
    ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile::default(),
        &harness_tool::SchemaResolverContext {
            run_id: harness_contracts::RunId::new(),
            session_id: harness_contracts::SessionId::new(),
            tenant_id: TenantId::SINGLE,
        },
    )
    .await
    .unwrap()
}

fn orchestrator_ctx(pool: ToolPool, _decisions: Vec<Decision>) -> OrchestratorContext {
    orchestrator_ctx_with_interrupt(pool, vec![], InterruptToken::default())
}

fn orchestrator_ctx_with_interrupt(
    pool: ToolPool,
    _decisions: Vec<Decision>,
    interrupt: InterruptToken,
) -> OrchestratorContext {
    let run_id = harness_contracts::RunId::new();
    let session_id = harness_contracts::SessionId::new();
    OrchestratorContext {
        pool,
        tool_context: ToolContext {
            tool_use_id: ToolUseId::new(),
            run_id,
            session_id,
            tenant_id: TenantId::SINGLE,
            correlation_id: harness_contracts::CorrelationId::new(),
            agent_id: harness_contracts::AgentId::from_u128(1),
            subagent_depth: 0,
            workspace_root: std::env::temp_dir(),
            project_workspace_root: None,
            sandbox: None,
            cap_registry: Arc::new(CapabilityRegistry::default()),
            redactor: Arc::new(harness_contracts::NoopRedactor),
            interrupt,
            parent_run: None,
            model: None,
            model_config_id: None,
            memory_thread_settings: None,
            actor_source: harness_contracts::PermissionActorSource::ParentRun,
        },
        blob_store: None,
        event_emitter: Arc::new(NoopToolEventEmitter),
    }
}

fn test_tool(name: &str, is_concurrency_safe: bool, behavior: Behavior) -> TestTool {
    let long_running =
        matches!(behavior, Behavior::Slow).then_some(harness_contracts::LongRunningPolicy {
            stall_threshold: Duration::from_secs(5),
            hard_timeout: Duration::from_millis(25),
        });
    let long_running = if matches!(behavior, Behavior::Stalled) {
        Some(harness_contracts::LongRunningPolicy {
            stall_threshold: Duration::from_millis(10),
            hard_timeout: Duration::from_millis(200),
        })
    } else {
        long_running
    };
    TestTool {
        descriptor: ToolDescriptor {
            name: name.to_owned(),
            display_name: name.to_owned(),
            description: format!("{name} tool"),
            category: "test".to_owned(),
            group: ToolGroup::FileSystem,
            version: "0.0.1".to_owned(),
            input_schema: json!({ "type": "object" }),
            output_schema: None,
            dynamic_schema: false,
            properties: ToolProperties {
                is_concurrency_safe,
                is_read_only: true,
                is_destructive: false,
                long_running,
                defer_policy: harness_contracts::DeferPolicy::AlwaysLoad,
            },
            trust_level: TrustLevel::AdminTrusted,
            required_capabilities: vec![],
            budget: default_result_budget(),
            provider_restriction: ProviderRestriction::All,
            origin: ToolOrigin::Builtin,
            search_hint: None,
            service_binding: None,
        },
        behavior,
    }
}

fn call(name: &str) -> AuthorizedToolCall {
    call_with_input(name, json!({ "tool": name }))
}

fn call_with_input(name: &str, raw_input: Value) -> AuthorizedToolCall {
    let tool_use_id = ToolUseId::new();
    let plan = ToolActionPlan {
        plan_id: ActionPlanId::new(),
        tool_use_id,
        tool_name: name.to_owned(),
        actor_source: PermissionActorSource::ParentRun,
        subject: PermissionSubject::ToolInvocation {
            tool: name.to_owned(),
            input: raw_input.clone(),
        },
        scope: DecisionScope::ToolName(name.to_owned()),
        severity: Severity::Medium,
        resources: Vec::new(),
        sandbox_policy: SandboxPolicy {
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
            denied_host_paths: Vec::new(),
        },
        workspace_access: WorkspaceAccess::None,
        network_access: NetworkAccess::None,
        review: PermissionReview::default(),
        plan_hash: ActionPlanHash::from_bytes([1; 32]),
        created_at: Utc::now(),
    };
    let authorized = AuthorizedToolInput::new(raw_input, plan.clone(), ticket_for(&plan)).unwrap();
    AuthorizedToolCall {
        tool_use_id,
        tool_name: name.to_owned(),
        input: authorized,
    }
}

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    AuthorizedTicketSummary {
        ticket_id: harness_contracts::AuthorizationTicketId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: harness_contracts::SessionId::new(),
        run_id: harness_contracts::RunId::new(),
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
        consumed_at: Utc::now(),
    }
}

fn names(results: &[harness_tool::ToolResultEnvelope]) -> Vec<&str> {
    results
        .iter()
        .map(|result| result.tool_name.as_str())
        .collect()
}

fn index_of(log: &[String], needle: &str) -> usize {
    log.iter().position(|entry| entry == needle).unwrap()
}
