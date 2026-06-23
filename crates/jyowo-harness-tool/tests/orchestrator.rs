use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    CapabilityRegistry, Decision, DecisionScope, Event, FallbackPolicy, InteractivityLevel,
    PermissionMode, PermissionSubject, ProviderRestriction, Severity, TenantId, ToolDescriptor,
    ToolError, ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolUseHeartbeatEvent, ToolUseId,
    TrustLevel,
};
use harness_permission::{
    PermissionBroker, PermissionCheck, PermissionContext, PermissionRequest, RuleSnapshot,
};
use harness_tool::{
    default_result_budget, BuiltinToolset, InterruptToken, NoopToolEventEmitter,
    OrchestratorContext, Tool, ToolCall, ToolContext, ToolEvent, ToolEventEmitter,
    ToolOrchestrator, ToolPool, ToolPoolFilter, ToolPoolModelProfile, ToolRegistry, ToolSearchMode,
    ValidationError,
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
    let broker = RecordingBroker::new(vec![Decision::AllowOnce]);
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
    let ctx = orchestrator_ctx_with_broker(pool, Arc::new(broker.clone()));
    let results = ToolOrchestrator::default()
        .dispatch(vec![call("bad")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::Validation(ref message)) if message == "invalid input"
    ));
    assert_eq!(broker.calls().len(), 0);
    assert!(!executed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn allowed_tools_still_use_permission_broker_and_broker_denial_blocks_execution() {
    let executed = Arc::new(AtomicBool::new(false));
    let broker = RecordingBroker::new(vec![Decision::DenyOnce]);
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
    let ctx = orchestrator_ctx_with_broker(pool, Arc::new(broker.clone()));
    let results = ToolOrchestrator::default()
        .dispatch(vec![call("guarded")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::PermissionDenied(ref message)) if message.contains("DenyOnce")
    ));
    let calls = broker.calls();
    assert_eq!(calls.len(), 1);
    assert!(matches!(
        &calls[0].subject,
        PermissionSubject::ToolInvocation { tool, .. } if tool == "guarded"
    ));
    assert!(!executed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn allowed_tools_execute_only_after_broker_allows() {
    let executed = Arc::new(AtomicBool::new(false));
    let broker = RecordingBroker::new(vec![Decision::AllowOnce]);
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
    let ctx = orchestrator_ctx_with_broker(pool, Arc::new(broker.clone()));
    let results = ToolOrchestrator::default()
        .dispatch(vec![call("guarded")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Ok(ToolResult::Text(ref message)) if message == "executed"
    ));
    let calls = broker.calls();
    assert_eq!(calls.len(), 1);
    assert!(matches!(
        &calls[0].subject,
        PermissionSubject::ToolInvocation { tool, .. } if tool == "guarded"
    ));
    assert!(executed.load(Ordering::SeqCst));
}

#[tokio::test]
async fn permission_check_denied_short_circuits_before_broker() {
    let broker = RecordingBroker::new(vec![Decision::AllowOnce]);
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool(
            "denied",
            true,
            Behavior::PermissionDenied,
        )))
        .build()
        .unwrap();

    let pool = pool(&registry).await;
    let ctx = orchestrator_ctx_with_broker(pool, Arc::new(broker.clone()));
    let results = ToolOrchestrator::default()
        .dispatch(vec![call("denied")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::PermissionDenied(ref message)) if message == "tool refused"
    ));
    assert_eq!(broker.calls().len(), 0);
}

#[tokio::test]
async fn dangerous_command_check_is_mapped_to_permission_request() {
    let broker = RecordingBroker::new(vec![Decision::AllowOnce]);
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool("danger", true, Behavior::Dangerous)))
        .build()
        .unwrap();

    let pool = pool(&registry).await;
    let ctx = orchestrator_ctx_with_broker(pool, Arc::new(broker.clone()));
    let results = ToolOrchestrator::default()
        .dispatch(vec![call("danger")], ctx)
        .await;

    assert!(results[0].result.is_ok());
    let calls = broker.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].severity, Severity::Critical);
    assert!(matches!(
        &calls[0].subject,
        PermissionSubject::DangerousCommand { pattern_id, .. } if pattern_id == "rm-rf"
    ));
}

#[tokio::test]
async fn dangerous_pattern_check_is_mapped_to_permission_request() {
    let executed = Arc::new(AtomicBool::new(false));
    let broker = RecordingBroker::new(vec![Decision::DenyOnce]);
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(test_tool(
            "dangerous_path",
            true,
            Behavior::DangerousPattern(Arc::clone(&executed)),
        )))
        .build()
        .unwrap();

    let pool = pool(&registry).await;
    let ctx = orchestrator_ctx_with_broker(pool, Arc::new(broker.clone()));
    let results = ToolOrchestrator::default()
        .dispatch(vec![call("dangerous_path")], ctx)
        .await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::PermissionDenied(ref message)) if message.contains("DenyOnce")
    ));
    let calls = broker.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].severity, Severity::High);
    assert_eq!(
        calls[0].scope_hint,
        DecisionScope::PathPrefix("/tmp/workspace/.ssh/id_rsa".into())
    );
    assert!(matches!(
        &calls[0].subject,
        PermissionSubject::FileWrite { path, .. }
            if path == &std::path::PathBuf::from("/tmp/workspace/.ssh/id_rsa")
    ));
    assert!(!executed.load(Ordering::SeqCst));
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
    let tool_use_id = ToolUseId::new();

    let results = ToolOrchestrator::default()
        .dispatch(
            vec![ToolCall {
                tool_use_id,
                tool_name: "stalled".to_owned(),
                input: json!({}),
            }],
            ctx,
        )
        .await;

    assert!(matches!(results[0].result, Ok(ToolResult::Text(ref text)) if text == "late"));
    assert!(results[0].progress_emitted >= 1);
    assert!(emitter.events().iter().any(|event| {
        matches!(event, Event::ToolUseHeartbeat(heartbeat) if heartbeat.tool_use_id == tool_use_id)
    }));
}

#[tokio::test]
async fn journal_events_are_emitted_by_orchestrator() {
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

    assert!(matches!(results[0].result, Ok(ToolResult::Text(ref text)) if text == "done"));
    assert!(emitter
        .events()
        .iter()
        .any(|event| matches!(event, Event::ToolUseHeartbeat(_))));
}

#[test]
fn tool_crate_does_not_depend_on_model_or_hook_crates_for_orchestrator() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    #[cfg(not(feature = "minimax-tools"))]
    assert!(!manifest.contains("jyowo-harness-model"));
    assert!(!manifest.contains("jyowo-harness-hook"));
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
    PermissionDenied,
    Dangerous,
    DangerousPattern(Arc<AtomicBool>),
    Progress,
    StreamError,
    Slow,
    Stalled,
    Journal,
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

    async fn check_permission(&self, input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        match &self.behavior {
            Behavior::PermissionDenied => PermissionCheck::Denied {
                reason: "tool refused".to_owned(),
            },
            Behavior::MarkExecuted(_) => PermissionCheck::Allowed,
            Behavior::Dangerous => PermissionCheck::DangerousCommand {
                pattern: "rm-rf".to_owned(),
                severity: Severity::Critical,
            },
            Behavior::DangerousPattern(_) => PermissionCheck::DangerousPattern {
                kind: "path".to_owned(),
                pattern: "path-unix-ssh-credential".to_owned(),
                severity: Severity::High,
                subject: PermissionSubject::FileWrite {
                    path: "/tmp/workspace/.ssh/id_rsa".into(),
                    bytes_preview: b"secret".to_vec(),
                },
                scope: DecisionScope::PathPrefix("/tmp/workspace/.ssh/id_rsa".into()),
            },
            _ => PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            },
        }
    }

    async fn execute(
        &self,
        _input: Value,
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
            Behavior::ValidationError(executed)
            | Behavior::MarkExecuted(executed)
            | Behavior::DangerousPattern(executed) => {
                executed.store(true, Ordering::SeqCst);
                Ok(Box::pin(stream::iter([ToolEvent::Final(
                    ToolResult::Text("executed".to_owned()),
                )])))
            }
            Behavior::PermissionDenied => Ok(Box::pin(stream::iter([ToolEvent::Final(
                ToolResult::Text("unexpected".to_owned()),
            )]))),
            Behavior::Dangerous => Ok(Box::pin(stream::iter([ToolEvent::Final(
                ToolResult::Text("allowed".to_owned()),
            )]))),
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

#[derive(Clone)]
struct RecordingBroker {
    decisions: Arc<Mutex<VecDeque<Decision>>>,
    calls: Arc<Mutex<Vec<PermissionRequest>>>,
}

impl RecordingBroker {
    fn new(decisions: Vec<Decision>) -> Self {
        Self {
            decisions: Arc::new(Mutex::new(decisions.into())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<PermissionRequest> {
        self.calls.lock().clone()
    }
}

#[async_trait]
impl PermissionBroker for RecordingBroker {
    async fn decide(&self, request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.calls.lock().push(request);
        self.decisions
            .lock()
            .pop_front()
            .unwrap_or(Decision::DenyOnce)
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
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

fn orchestrator_ctx(pool: ToolPool, decisions: Vec<Decision>) -> OrchestratorContext {
    orchestrator_ctx_with_broker(pool, Arc::new(RecordingBroker::new(decisions)))
}

fn orchestrator_ctx_with_broker(
    pool: ToolPool,
    broker: Arc<dyn PermissionBroker>,
) -> OrchestratorContext {
    orchestrator_ctx_with_interrupt(pool, vec![], InterruptToken::default()).with_broker(broker)
}

fn orchestrator_ctx_with_interrupt(
    pool: ToolPool,
    decisions: Vec<Decision>,
    interrupt: InterruptToken,
) -> OrchestratorContext {
    let broker: Arc<dyn PermissionBroker> = Arc::new(RecordingBroker::new(decisions));
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
            sandbox: None,
            permission_broker: broker,
            cap_registry: Arc::new(CapabilityRegistry::default()),
            interrupt,
            parent_run: None,
        },
        permission_context: PermissionContext {
            permission_mode: PermissionMode::Default,
            previous_mode: None,
            session_id,
            tenant_id: TenantId::SINGLE,
            run_id: None,
            interactivity: InteractivityLevel::FullyInteractive,
            timeout_policy: None,
            fallback_policy: FallbackPolicy::DenyAll,
            rule_snapshot: Arc::new(RuleSnapshot {
                rules: vec![],
                generation: 0,
                built_at: chrono::Utc::now(),
            }),
            hook_overrides: vec![],
        },
        blob_store: None,
        event_emitter: Arc::new(NoopToolEventEmitter),
    }
}

trait WithBroker {
    fn with_broker(self, broker: Arc<dyn PermissionBroker>) -> Self;
}

impl WithBroker for OrchestratorContext {
    fn with_broker(mut self, broker: Arc<dyn PermissionBroker>) -> Self {
        self.tool_context.permission_broker = broker;
        self
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
        },
        behavior,
    }
}

fn call(name: &str) -> ToolCall {
    ToolCall {
        tool_use_id: ToolUseId::new(),
        tool_name: name.to_owned(),
        input: json!({ "tool": name }),
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
