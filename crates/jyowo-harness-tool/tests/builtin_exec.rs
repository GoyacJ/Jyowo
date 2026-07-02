#![cfg(feature = "builtin-toolset")]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Instant;

use async_trait::async_trait;
use bytes::Bytes;
use futures::{future::BoxFuture, stream, StreamExt};
use harness_contracts::{
    BlobError, BlobMeta, BlobRef, BlobStore, CapabilityRegistry, ClarifyAnswer, ClarifyPrompt,
    Decision, DecisionScope, Event, FallbackPolicy, InteractivityLevel, OutboundUserMessage,
    PermissionError, PermissionMode, PermissionSubject, SandboxError, SandboxExecutionStartedEvent,
    SandboxExitStatus, SandboxPolicySummary, Severity, TenantId, ToolCapability, ToolError,
    ToolResult, ToolUseId, UserMessageDelivery, WorkspaceAccess,
};
use harness_contracts::{RedactRules, Redactor};
use harness_permission::{
    PermissionBroker, PermissionCheck, PermissionContext, PermissionRequest, RuleSnapshot,
};
use harness_sandbox::{
    ActivityHandle, ExecContext, ExecOutcome, ExecSpec, KillScope, ProcessHandle, SandboxBackend,
    SandboxBaseConfig, SandboxCapabilities, SessionSnapshotFile, SnapshotSpec,
};
use harness_tool::{
    builtin::{
        BashTool, ClarifyTool, SendMessageTool, WebFetchBackend, WebFetchRequest, WebFetchResponse,
        WebFetchTool, WebSearchBackend, WebSearchRequest, WebSearchResult, WebSearchTool,
    },
    BuiltinToolset, InterruptToken, OrchestratorContext, Tool, ToolCall, ToolContext,
    ToolOrchestrator, ToolPool, ToolPoolFilter, ToolPoolModelProfile, ToolRegistry, ToolSearchMode,
};
use parking_lot::Mutex;
use serde_json::{json, Value};

#[tokio::test]
async fn bash_requires_sandbox_and_maps_command_permission() {
    let tool = BashTool::default();
    let input = json!({ "command": "printf hi", "cwd": "/tmp" });
    let check = tool
        .check_permission(&input, &tool_ctx(CapabilityRegistry::default(), None))
        .await;

    assert!(matches!(
        check,
        PermissionCheck::AskUser {
            subject: PermissionSubject::CommandExec { ref command, .. },
            scope: DecisionScope::ExactCommand { command: ref scoped_command, .. },
        } if command == "printf hi" && scoped_command == "printf hi"
    ));

    let error = execute_error(&tool, input, tool_ctx(CapabilityRegistry::default(), None)).await;
    assert!(matches!(
        error,
        ToolError::CapabilityMissing(ToolCapability::Custom(ref cap)) if cap == "sandbox_backend"
    ));
}

#[tokio::test]
async fn bash_dangerous_command_precheck_sets_severity() {
    let tool = BashTool::default();
    let check = tool
        .check_permission(
            &json!({ "command": "rm -rf /" }),
            &tool_ctx(CapabilityRegistry::default(), None),
        )
        .await;

    assert!(matches!(
        check,
        PermissionCheck::DangerousCommand {
            ref pattern,
            severity: Severity::Critical,
        } if pattern == "unix-rm-rf-root"
    ));
}

#[tokio::test]
async fn bash_executes_through_sandbox_and_returns_output() {
    let sandbox = Arc::new(FakeSandbox::new(
        Bytes::from_static(b"hello\n"),
        Bytes::from_static(b"warn\n"),
        SandboxExitStatus::Code(0),
    ));
    let tool = BashTool::default();

    let events = execute_events(
        &tool,
        json!({ "command": "echo hello", "cwd": "/work" }),
        tool_ctx_with_root(
            CapabilityRegistry::default(),
            Some(sandbox.clone()),
            std::path::PathBuf::from("/workspace-root"),
        ),
    )
    .await;

    assert!(matches!(
        events.first(),
        Some(harness_tool::ToolEvent::Journal(
            Event::SandboxExecutionStarted(_)
        ))
    ));

    assert!(events.iter().any(|event| {
        matches!(
            event,
            harness_tool::ToolEvent::Partial(harness_contracts::MessagePart::Text(text))
                if text == "hello\n"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            harness_tool::ToolEvent::Partial(harness_contracts::MessagePart::Text(text))
                if text == "warn\n"
        )
    }));
    let Some(harness_tool::ToolEvent::Final(ToolResult::Structured(value))) = events.last() else {
        panic!("expected structured bash result");
    };
    assert_eq!(value["exit_status"], json!({ "code": 0 }));
    assert_eq!(value["stdout_bytes_observed"], 6);
    assert_eq!(value["stderr_bytes_observed"], 5);
    assert!(value.get("stdout").is_none());
    assert!(value.get("stderr").is_none());

    let specs = sandbox.recorded_execs();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].command, "echo hello");
    assert_eq!(
        specs[0].workspace_access,
        WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: vec![]
        }
    );
    assert_eq!(
        sandbox.recorded_contexts()[0].workspace_root,
        std::path::PathBuf::from("/workspace-root")
    );
    assert_eq!(sandbox.before_execute_count(), 0);
}

#[tokio::test]
async fn bash_exec_context_uses_tool_context_redactor() {
    let sandbox = Arc::new(FakeSandbox::new(
        Bytes::from_static(b"hello\n"),
        Bytes::new(),
        SandboxExitStatus::Code(0),
    ));
    let tool = BashTool::default();
    let mut ctx = tool_ctx_with_root(
        CapabilityRegistry::default(),
        Some(sandbox.clone()),
        std::path::PathBuf::from("/workspace-root"),
    );
    ctx.redactor = Arc::new(MarkerRedactor);

    let _ = execute_events(&tool, json!({ "command": "echo hello" }), ctx).await;

    let contexts = sandbox.recorded_contexts();
    assert_eq!(contexts.len(), 1);
    assert_eq!(
        contexts[0]
            .redactor
            .redact("secret value", &RedactRules::default()),
        "redacted:secret value"
    );
}

#[tokio::test]
async fn bash_preserves_successful_outcome_when_after_execute_fails() {
    let sandbox = Arc::new(
        FakeSandbox::new(
            Bytes::from_static(b"hello\n"),
            Bytes::new(),
            SandboxExitStatus::Code(0),
        )
        .with_after_execute_error("cleanup failed"),
    );
    let tool = BashTool::default();

    let events = execute_events(
        &tool,
        json!({ "command": "echo hello" }),
        tool_ctx(CapabilityRegistry::default(), Some(sandbox)),
    )
    .await;

    let Some(harness_tool::ToolEvent::Final(ToolResult::Structured(value))) = events.last() else {
        panic!("expected structured bash result");
    };
    assert_eq!(value["exit_status"], json!({ "code": 0 }));
    assert_eq!(value["stdout_bytes_observed"], 6);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            harness_tool::ToolEvent::Journal(Event::SandboxPostExecutionFailed(failed))
                if failed.backend_id == "fake"
                    && failed.error.to_string() == "cleanup failed"
        )
    }));
}

#[tokio::test]
async fn bash_streams_sandbox_journal_events_before_final_result() {
    let sandbox = Arc::new(FakeSandbox::new(
        Bytes::from_static(b"hello\n"),
        Bytes::new(),
        SandboxExitStatus::Code(0),
    ));
    let tool = BashTool::default();

    let events = execute_events(
        &tool,
        json!({ "command": "echo hello" }),
        tool_ctx(CapabilityRegistry::default(), Some(sandbox)),
    )
    .await;

    assert!(matches!(
        events.first(),
        Some(harness_tool::ToolEvent::Journal(
            Event::SandboxExecutionStarted(_)
        ))
    ));
    let Some(harness_tool::ToolEvent::Final(ToolResult::Structured(value))) = events.last() else {
        panic!("expected structured bash result");
    };
    assert_eq!(value["stdout_bytes_observed"], 6);
}

#[tokio::test]
async fn bash_large_stdout_is_offloaded_by_orchestrator_budget() {
    let sandbox = Arc::new(FakeSandbox::new(
        Bytes::from(vec![b'a'; 260_000]),
        Bytes::new(),
        SandboxExitStatus::Code(0),
    ));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Shell)
        .build()
        .unwrap();
    let pool = ToolPool::assemble(
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
    .unwrap();
    let blob_store = Arc::new(RecordingBlobStore::default());

    let results = ToolOrchestrator::default()
        .dispatch(
            vec![ToolCall {
                tool_use_id: ToolUseId::new(),
                tool_name: "Bash".to_owned(),
                input: json!({ "command": "printf large" }),
            }],
            orchestrator_ctx(pool, Some(sandbox), blob_store.clone()),
        )
        .await;

    assert!(results[0].overflow.is_some());
    assert!(matches!(results[0].result, Ok(ToolResult::Mixed(_))));
    assert_eq!(blob_store.puts().len(), 1);
}

#[tokio::test]
async fn bash_budget_overflow_stops_before_later_stdout_chunks_are_consumed() {
    let yielded_chunks = Arc::new(AtomicUsize::new(0));
    let sandbox = Arc::new(
        FakeSandbox::new(Bytes::new(), Bytes::new(), SandboxExitStatus::Code(0))
            .with_stdout_chunks(
                vec![
                    Bytes::from(vec![b'a'; 260_000]),
                    Bytes::from_static(b"later"),
                ],
                yielded_chunks.clone(),
            ),
    );
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Shell)
        .build()
        .unwrap();
    let pool = ToolPool::assemble(
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
    .unwrap();
    let blob_store = Arc::new(RecordingBlobStore::default());

    let results = ToolOrchestrator::default()
        .dispatch(
            vec![ToolCall {
                tool_use_id: ToolUseId::new(),
                tool_name: "Bash".to_owned(),
                input: json!({ "command": "printf large" }),
            }],
            orchestrator_ctx(pool, Some(sandbox.clone()), blob_store),
        )
        .await;

    assert!(results[0].overflow.is_some());
    assert_eq!(
        yielded_chunks.load(Ordering::SeqCst),
        1,
        "orchestrator budget overflow should drop Bash stream before later stdout chunks"
    );
    for _ in 0..10 {
        if sandbox.kill_count() > 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        sandbox.kill_count(),
        1,
        "dropping an unfinished Bash stream should best-effort kill the process group"
    );
}

#[tokio::test]
async fn web_search_uses_network_permission_and_backend() {
    let tool = WebSearchTool::new(vec![Arc::new(FakeWebSearchBackend)]);
    let input = json!({
        "query": "jyowo harness",
        "max_results": 1,
        "region": "us",
        "recency": "week"
    });
    let check = tool
        .check_permission(&input, &tool_ctx(CapabilityRegistry::default(), None))
        .await;

    assert!(matches!(
        check,
        PermissionCheck::AskUser {
            subject: PermissionSubject::NetworkAccess { ref host, .. },
            scope: DecisionScope::ToolName(ref tool),
        } if host == "web-search" && tool == "WebSearch"
    ));

    let result = execute_final(&tool, input, tool_ctx(CapabilityRegistry::default(), None)).await;
    assert_eq!(
        result,
        ToolResult::Structured(json!([{
            "title": "Jyowo",
            "url": "https://example.test/jyowo",
            "snippet": "Harness result",
            "score": 0.9
        }]))
    );

    let error = execute_error(
        &WebSearchTool::default(),
        json!({ "query": "jyowo" }),
        tool_ctx(CapabilityRegistry::default(), None),
    )
    .await;
    assert!(matches!(
        error,
        ToolError::CapabilityMissing(ToolCapability::Custom(ref cap)) if cap == "web_search_backend"
    ));
}

#[tokio::test]
async fn web_search_rejects_invalid_max_results() {
    let tool = WebSearchTool::default();
    let ctx = tool_ctx(CapabilityRegistry::default(), None);

    let zero = validate_error_message(
        &tool,
        json!({ "query": "jyowo harness", "max_results": 0 }),
        ctx.clone(),
    )
    .await;
    assert_eq!(zero, "max_results must be greater than 0");

    let overflow = validate_error_message(
        &tool,
        json!({ "query": "jyowo harness", "max_results": u64::MAX }),
        ctx,
    )
    .await;
    assert_eq!(overflow, "max_results must fit in u32");
}

#[tokio::test]
async fn web_search_rejects_invalid_region_and_recency() {
    let tool = WebSearchTool::default();
    let ctx = tool_ctx(CapabilityRegistry::default(), None);

    for (input, expected) in [
        (
            json!({ "query": "jyowo harness", "region": 1 }),
            "region must be a non-empty string",
        ),
        (
            json!({ "query": "jyowo harness", "region": "" }),
            "region must be a non-empty string",
        ),
        (
            json!({ "query": "jyowo harness", "recency": 1 }),
            "recency must be one of day, week, month, year",
        ),
        (
            json!({ "query": "jyowo harness", "recency": "" }),
            "recency must be one of day, week, month, year",
        ),
        (
            json!({ "query": "jyowo harness", "recency": "hour" }),
            "recency must be one of day, week, month, year",
        ),
    ] {
        let error = validate_error_message(&tool, input, ctx.clone()).await;
        assert_eq!(error, expected);
    }
}

#[tokio::test]
async fn web_fetch_fails_closed_by_default_and_uses_injected_backend() {
    let default_tool = WebFetchTool::default();

    let dangerous_check = default_tool
        .check_permission(
            &json!({ "url": "http://169.254.169.254/latest/meta-data" }),
            &tool_ctx(CapabilityRegistry::default(), None),
        )
        .await;
    assert!(matches!(
        dangerous_check,
        PermissionCheck::DangerousPattern {
            ref kind,
            ref pattern,
            severity: Severity::High,
            ..
        } if kind == "url" && pattern == "url-cloud-metadata"
    ));

    let error = execute_error(
        &default_tool,
        json!({ "url": "https://example.test/page" }),
        tool_ctx(CapabilityRegistry::default(), None),
    )
    .await;
    assert!(matches!(
        error,
        ToolError::CapabilityMissing(ToolCapability::Custom(ref cap)) if cap == "web_fetch_backend"
    ));

    let injected_tool = WebFetchTool::new(vec![Arc::new(FakeWebFetchBackend)]);
    let result = execute_final(
        &injected_tool,
        json!({ "url": "https://example.test/page", "max_bytes": 5 }),
        tool_ctx(CapabilityRegistry::default(), None),
    )
    .await;
    assert_eq!(
        result,
        ToolResult::Structured(json!({
            "url": "https://example.test/page",
            "status": 200,
            "content_type": "text/plain",
            "body": "hello",
            "truncated": true
        }))
    );
}

#[tokio::test]
async fn web_fetch_rejects_invalid_max_bytes_and_truncates_on_utf8_boundary() {
    let tool = WebFetchTool::new(vec![Arc::new(Utf8WebFetchBackend)]);
    let ctx = tool_ctx(CapabilityRegistry::default(), None);

    let zero = validate_error_message(
        &tool,
        json!({ "url": "https://example.test/utf8", "max_bytes": 0 }),
        ctx.clone(),
    )
    .await;
    assert_eq!(zero, "max_bytes must be greater than 0");

    let negative = validate_error_message(
        &tool,
        json!({ "url": "https://example.test/utf8", "max_bytes": -1 }),
        ctx.clone(),
    )
    .await;
    assert_eq!(negative, "max_bytes must be a positive integer");

    let result = execute_final(
        &tool,
        json!({ "url": "https://example.test/utf8", "max_bytes": 3 }),
        ctx,
    )
    .await;
    assert_eq!(
        result,
        ToolResult::Structured(json!({
            "url": "https://example.test/utf8",
            "status": 200,
            "content_type": "text/plain",
            "body": "é",
            "truncated": true
        }))
    );
}

#[tokio::test]
async fn clarify_and_send_message_use_capability_registry() {
    let mut caps = CapabilityRegistry::default();
    let clarify: Arc<dyn harness_contracts::ClarifyChannelCap> = Arc::new(FakeClarify);
    let messenger: Arc<dyn harness_contracts::UserMessengerCap> = Arc::new(FakeMessenger);
    caps.install(ToolCapability::ClarifyChannel, clarify);
    caps.install(ToolCapability::UserMessenger, messenger);

    let clarify_result = execute_final(
        &ClarifyTool::default(),
        json!({
            "prompt": "Pick one",
            "choices": [{ "id": "a", "label": "A" }],
            "multiple": false
        }),
        tool_ctx(caps.clone(), None),
    )
    .await;
    let ToolResult::Structured(clarify_value) = clarify_result else {
        panic!("expected structured clarify result");
    };
    assert_eq!(clarify_value["answer"], json!("A"));
    assert_eq!(clarify_value["chosen_ids"], json!(["a"]));
    assert!(clarify_value["answered_at"].as_str().is_some());

    let send_result = execute_final(
        &SendMessageTool::default(),
        json!({ "channel": "desktop", "body": "done" }),
        tool_ctx(caps, None),
    )
    .await;
    assert_eq!(
        send_result,
        ToolResult::Structured(json!({
            "message_id": "msg-1",
            "delivered": true
        }))
    );
}

#[tokio::test]
async fn clarify_rejects_invalid_timeout_seconds() {
    let tool = ClarifyTool::default();
    let ctx = tool_ctx(CapabilityRegistry::default(), None);

    let zero = validate_error_message(
        &tool,
        json!({ "prompt": "Pick one", "timeout_seconds": 0 }),
        ctx.clone(),
    )
    .await;
    assert_eq!(zero, "timeout_seconds must be greater than 0");

    let overflow = validate_error_message(
        &tool,
        json!({ "prompt": "Pick one", "timeout_seconds": u64::MAX }),
        ctx,
    )
    .await;
    assert_eq!(overflow, "timeout_seconds must fit in u32");
}

#[tokio::test]
async fn clarify_rejects_invalid_choices() {
    let tool = ClarifyTool::default();
    let ctx = tool_ctx(CapabilityRegistry::default(), None);

    for (input, expected) in [
        (
            json!({ "prompt": "Pick one", "choices": "a" }),
            "choices must be an array",
        ),
        (
            json!({ "prompt": "Pick one", "choices": ["a"] }),
            "choice must be an object",
        ),
        (
            json!({ "prompt": "Pick one", "choices": [{ "label": "A" }] }),
            "choice.id is required",
        ),
        (
            json!({ "prompt": "Pick one", "choices": [{ "id": "", "label": "A" }] }),
            "choice.id is required",
        ),
        (
            json!({ "prompt": "Pick one", "choices": [{ "id": "a" }] }),
            "choice.label is required",
        ),
        (
            json!({ "prompt": "Pick one", "choices": [{ "id": "a", "label": "" }] }),
            "choice.label is required",
        ),
        (
            json!({ "prompt": "Pick one", "choices": [{ "id": "a", "label": "A", "hint": 1 }] }),
            "choice.hint must be a string",
        ),
    ] {
        let error = validate_error_message(&tool, input, ctx.clone()).await;
        assert_eq!(error, expected);
    }
}

#[test]
fn default_builtin_toolset_registers_m3_t04b_tools_without_forbidden_deps() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    for name in ["Bash", "WebSearch", "Clarify", "SendMessage"] {
        assert!(registry.get(name).is_some(), "{name} should be registered");
    }

    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    #[cfg(not(feature = "minimax-tools"))]
    assert!(!manifest.lines().any(|line| {
        line.trim_start().starts_with("jyowo-harness-model =") && !line.contains("optional = true")
    }));
    assert!(!manifest.contains("jyowo-harness-journal"));
    assert!(!manifest.contains("jyowo-harness-hook"));
}

async fn execute_final(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolResult {
    tool.validate(&input, &ctx).await.unwrap();
    let mut stream = tool.execute(input, ctx).await.unwrap();
    while let Some(event) = stream.next().await {
        if let harness_tool::ToolEvent::Final(result) = event {
            return result;
        }
    }
    panic!("expected final result")
}

async fn execute_events(
    tool: &dyn Tool,
    input: Value,
    ctx: ToolContext,
) -> Vec<harness_tool::ToolEvent> {
    tool.validate(&input, &ctx).await.unwrap();
    let mut stream = tool.execute(input, ctx).await.unwrap();
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }
    events
}

async fn execute_error(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolError {
    tool.validate(&input, &ctx).await.unwrap();
    match tool.execute(input, ctx).await {
        Ok(_) => panic!("expected tool error"),
        Err(error) => error,
    }
}

async fn validate_error_message(tool: &dyn Tool, input: Value, ctx: ToolContext) -> String {
    tool.validate(&input, &ctx)
        .await
        .expect_err("expected validation error")
        .to_string()
}

fn tool_ctx(
    cap_registry: CapabilityRegistry,
    sandbox: Option<Arc<dyn SandboxBackend>>,
) -> ToolContext {
    tool_ctx_with_root(cap_registry, sandbox, std::env::temp_dir())
}

fn tool_ctx_with_root(
    cap_registry: CapabilityRegistry,
    sandbox: Option<Arc<dyn SandboxBackend>>,
    workspace_root: std::path::PathBuf,
) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root,
        sandbox,
        permission_broker: Arc::new(AllowBroker),
        cap_registry: Arc::new(cap_registry),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
    }
}

fn orchestrator_ctx(
    pool: ToolPool,
    sandbox: Option<Arc<dyn SandboxBackend>>,
    blob_store: Arc<dyn BlobStore>,
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
            sandbox,
            permission_broker: Arc::new(AllowBroker),
            cap_registry: Arc::new(CapabilityRegistry::default()),
            redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
            interrupt: InterruptToken::default(),
            parent_run: None,
            model: None,
            model_config_id: None,
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
            hook_overrides: vec![],
        },
        blob_store: Some(blob_store),
        event_emitter: Arc::new(harness_tool::NoopToolEventEmitter),
    }
}

#[derive(Debug)]
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

struct MarkerRedactor;

impl Redactor for MarkerRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        format!("redacted:{input}")
    }
}

struct FakeSandbox {
    before_execute_count: AtomicUsize,
    recorded_execs: Mutex<Vec<ExecSpec>>,
    recorded_contexts: Mutex<Vec<ExecContext>>,
    stdout: Bytes,
    stderr: Bytes,
    stdout_chunks: Option<(Vec<Bytes>, Arc<AtomicUsize>)>,
    exit_status: SandboxExitStatus,
    after_execute_error: Option<String>,
    killed: Arc<AtomicUsize>,
}

#[derive(Default)]
struct RecordingBlobStore {
    puts: Mutex<Vec<(Bytes, BlobMeta)>>,
}

impl RecordingBlobStore {
    fn puts(&self) -> Vec<(Bytes, BlobMeta)> {
        self.puts.lock().clone()
    }
}

#[async_trait]
impl BlobStore for RecordingBlobStore {
    fn store_id(&self) -> &'static str {
        "recording"
    }

    async fn put(
        &self,
        _tenant: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        self.puts.lock().push((bytes, meta.clone()));
        Ok(BlobRef {
            id: harness_contracts::BlobId::new(),
            size: meta.size,
            content_hash: meta.content_hash,
            content_type: meta.content_type,
        })
    }

    async fn get(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<futures::stream::BoxStream<'static, Bytes>, BlobError> {
        Err(BlobError::NotFound(harness_contracts::BlobId::new()))
    }

    async fn head(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<Option<BlobMeta>, BlobError> {
        Ok(None)
    }

    async fn delete(&self, _tenant: TenantId, _blob: &BlobRef) -> Result<(), BlobError> {
        Ok(())
    }
}

impl FakeSandbox {
    fn new(stdout: Bytes, stderr: Bytes, exit_status: SandboxExitStatus) -> Self {
        Self {
            before_execute_count: AtomicUsize::new(0),
            recorded_execs: Mutex::new(Vec::new()),
            recorded_contexts: Mutex::new(Vec::new()),
            stdout,
            stderr,
            stdout_chunks: None,
            exit_status,
            after_execute_error: None,
            killed: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn with_stdout_chunks(mut self, chunks: Vec<Bytes>, yielded: Arc<AtomicUsize>) -> Self {
        self.stdout_chunks = Some((chunks, yielded));
        self
    }

    fn with_after_execute_error(mut self, message: &str) -> Self {
        self.after_execute_error = Some(message.to_owned());
        self
    }

    fn recorded_execs(&self) -> Vec<ExecSpec> {
        self.recorded_execs.lock().clone()
    }

    fn recorded_contexts(&self) -> Vec<ExecContext> {
        self.recorded_contexts.lock().clone()
    }

    fn before_execute_count(&self) -> usize {
        self.before_execute_count.load(Ordering::SeqCst)
    }

    fn kill_count(&self) -> usize {
        self.killed.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SandboxBackend for FakeSandbox {
    fn backend_id(&self) -> &'static str {
        "fake"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            supports_streaming: true,
            ..SandboxCapabilities::default()
        }
    }

    async fn before_execute(
        &self,
        _spec: &ExecSpec,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        self.before_execute_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn execute(
        &self,
        spec: ExecSpec,
        ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        ctx.event_sink.emit(Event::SandboxExecutionStarted(
            SandboxExecutionStartedEvent {
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                tool_use_id: ctx.tool_use_id,
                backend_id: self.backend_id().to_owned(),
                fingerprint: spec.canonical_fingerprint(&SandboxBaseConfig::default()),
                policy: SandboxPolicySummary {
                    mode: spec.policy.mode.clone(),
                    scope: spec.policy.scope.clone(),
                    network: spec.policy.network.clone(),
                    resource_limits: spec.policy.resource_limits.clone(),
                },
                at: chrono::Utc::now(),
            },
        ))?;
        self.recorded_execs.lock().push(spec);
        self.recorded_contexts.lock().push(ctx);
        Ok(ProcessHandle {
            pid: Some(42),
            stdout: Some(match &self.stdout_chunks {
                Some((chunks, yielded)) => {
                    Box::pin(counted_chunks(chunks.clone(), yielded.clone()))
                }
                None => Box::pin(stream::once({
                    let stdout = self.stdout.clone();
                    async move { stdout }
                })),
            }),
            stderr: Some(Box::pin(stream::once({
                let stderr = self.stderr.clone();
                async move { stderr }
            }))),
            stdin: None,
            cwd_marker: None,
            activity: Arc::new(FakeActivity {
                exit_status: self.exit_status.clone(),
                stdout_bytes_observed: self.stdout.len() as u64,
                stderr_bytes_observed: self.stderr.len() as u64,
                killed: self.killed.clone(),
            }),
        })
    }

    async fn after_execute(
        &self,
        _outcome: &ExecOutcome,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        if let Some(message) = &self.after_execute_error {
            return Err(SandboxError::Message(message.clone()));
        }
        Ok(())
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::Message("not implemented".to_owned()))
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::Message("not implemented".to_owned()))
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

#[derive(Debug)]
struct FakeActivity {
    exit_status: SandboxExitStatus,
    stdout_bytes_observed: u64,
    stderr_bytes_observed: u64,
    killed: Arc<AtomicUsize>,
}

#[async_trait]
impl ActivityHandle for FakeActivity {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        Ok(ExecOutcome {
            exit_status: self.exit_status.clone(),
            stdout_bytes_observed: self.stdout_bytes_observed,
            stderr_bytes_observed: self.stderr_bytes_observed,
            ..ExecOutcome::default()
        })
    }

    async fn kill(&self, _signal: i32, _scope: KillScope) -> Result<(), SandboxError> {
        self.killed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn touch(&self) {}

    fn last_activity(&self) -> Instant {
        Instant::now()
    }
}

fn counted_chunks(
    chunks: Vec<Bytes>,
    yielded: Arc<AtomicUsize>,
) -> impl futures::Stream<Item = Bytes> + Send + 'static {
    stream::iter(chunks.into_iter().inspect(move |_chunk| {
        yielded.fetch_add(1, Ordering::SeqCst);
    }))
}

struct FakeWebSearchBackend;

#[async_trait]
impl WebSearchBackend for FakeWebSearchBackend {
    async fn search(&self, request: WebSearchRequest) -> Result<Vec<WebSearchResult>, ToolError> {
        assert_eq!(request.query, "jyowo harness");
        assert_eq!(request.max_results, Some(1));
        assert_eq!(request.region.as_deref(), Some("us"));
        assert_eq!(request.recency.as_deref(), Some("week"));
        Ok(vec![WebSearchResult {
            title: "Jyowo".to_owned(),
            url: "https://example.test/jyowo".to_owned(),
            snippet: "Harness result".to_owned(),
            score: 0.9,
        }])
    }
}

struct FakeWebFetchBackend;

#[async_trait]
impl WebFetchBackend for FakeWebFetchBackend {
    async fn fetch(&self, request: WebFetchRequest) -> Result<WebFetchResponse, ToolError> {
        assert_eq!(request.url.as_str(), "https://example.test/page");
        Ok(WebFetchResponse {
            url: request.url,
            status: 200,
            content_type: Some("text/plain".to_owned()),
            body: "hello world".to_owned(),
        })
    }
}

struct Utf8WebFetchBackend;

#[async_trait]
impl WebFetchBackend for Utf8WebFetchBackend {
    async fn fetch(&self, request: WebFetchRequest) -> Result<WebFetchResponse, ToolError> {
        assert_eq!(request.url.as_str(), "https://example.test/utf8");
        assert_eq!(request.max_bytes, 3);
        Ok(WebFetchResponse {
            url: request.url,
            status: 200,
            content_type: Some("text/plain".to_owned()),
            body: "éé".to_owned(),
        })
    }
}

struct FakeClarify;

impl harness_contracts::ClarifyChannelCap for FakeClarify {
    fn ask(&self, prompt: ClarifyPrompt) -> BoxFuture<'static, Result<ClarifyAnswer, ToolError>> {
        assert_eq!(prompt.prompt, "Pick one");
        Box::pin(async {
            Ok(ClarifyAnswer {
                answer: "A".to_owned(),
                chosen_ids: vec!["a".to_owned()],
            })
        })
    }
}

struct FakeMessenger;

impl harness_contracts::UserMessengerCap for FakeMessenger {
    fn send(
        &self,
        message: OutboundUserMessage,
    ) -> BoxFuture<'static, Result<UserMessageDelivery, ToolError>> {
        assert_eq!(message.channel, "desktop");
        assert_eq!(message.body, "done");
        Box::pin(async {
            Ok(UserMessageDelivery {
                message_id: "msg-1".to_owned(),
                delivered: true,
            })
        })
    }
}
