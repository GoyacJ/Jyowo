use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use harness_contracts::{
    CacheImpact, EndReason, HookEventKind, HookFailureMode, InconsistentReason, InteractivityLevel,
    McpServerId, MessageRole, PermissionMode, RequestId, RunId, SubagentId, SubagentStatus,
    TenantId, ToolResult, ToolSearchQueryKind, ToolUseId, TrustLevel, UsageSnapshot,
};
use harness_hook::{
    ContextPatch, ContextPatchRole, HookContext, HookEvent, HookHandler, HookMessageView,
    HookOutcome, HookSessionView, ModelRequestView, NotificationKind, PreToolUseOutcome,
    ReplayMode, ToolDescriptorView, ToolErrorView, UpstreamOutcomeView,
};
use serde_json::json;

#[tokio::test]
async fn hook_handler_is_dyn_safe_and_has_defaults() {
    let handler: Arc<dyn HookHandler> = Arc::new(NoopHook {
        id: "noop".to_owned(),
    });

    assert_eq!(handler.handler_id(), "noop");
    assert_eq!(handler.priority(), 0);
    assert_eq!(handler.failure_mode(), HookFailureMode::FailOpen);
    assert_eq!(handler.interested_events(), &[HookEventKind::PreToolUse]);

    let outcome = handler
        .handle(sample_pre_tool_use(), sample_context())
        .await
        .unwrap();
    assert_eq!(outcome, HookOutcome::Continue);
}

#[test]
fn hook_events_cover_twenty_kinds_and_report_their_kind() {
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    let run_id = RunId::new();
    let subagent_id = SubagentId::new();

    let cases = vec![
        (
            HookEvent::UserPromptSubmit {
                run_id,
                input: json!({ "message": "hello" }),
            },
            HookEventKind::UserPromptSubmit,
        ),
        (sample_pre_tool_use(), HookEventKind::PreToolUse),
        (
            HookEvent::PostToolUse {
                tool_use_id,
                result: ToolResult::Text("ok".to_owned()),
            },
            HookEventKind::PostToolUse,
        ),
        (
            HookEvent::PostToolUseFailure {
                tool_use_id,
                error: ToolErrorView {
                    message: "denied".to_owned(),
                },
            },
            HookEventKind::PostToolUseFailure,
        ),
        (
            HookEvent::PermissionRequest {
                request_id,
                subject: "tool:bash".to_owned(),
                detail: Some("run command".to_owned()),
            },
            HookEventKind::PermissionRequest,
        ),
        (
            HookEvent::SessionStart {
                session_id: harness_contracts::SessionId::new(),
            },
            HookEventKind::SessionStart,
        ),
        (
            HookEvent::Setup {
                workspace_root: Some(PathBuf::from("/tmp/workspace")),
            },
            HookEventKind::Setup,
        ),
        (
            HookEvent::SessionEnd {
                session_id: harness_contracts::SessionId::new(),
                reason: EndReason::Completed,
            },
            HookEventKind::SessionEnd,
        ),
        (
            HookEvent::SubagentStart {
                subagent_id,
                spec: harness_hook::SubagentSpecView {
                    name: "worker".to_owned(),
                    description: Some("test worker".to_owned()),
                },
            },
            HookEventKind::SubagentStart,
        ),
        (
            HookEvent::SubagentStop {
                subagent_id,
                status: SubagentStatus::Completed,
            },
            HookEventKind::SubagentStop,
        ),
        (
            HookEvent::Notification {
                kind: NotificationKind::Info,
                body: json!({ "message": "done" }),
            },
            HookEventKind::Notification,
        ),
        (
            HookEvent::PreLlmCall {
                run_id,
                request_view: ModelRequestView {
                    provider_id: "test".to_owned(),
                    model_id: "model".to_owned(),
                    message_count: 1,
                    tool_count: 0,
                },
            },
            HookEventKind::PreLlmCall,
        ),
        (
            HookEvent::PostLlmCall {
                run_id,
                usage: UsageSnapshot::default(),
            },
            HookEventKind::PostLlmCall,
        ),
        (
            HookEvent::PreApiRequest {
                request_id,
                endpoint: "/v1/messages".to_owned(),
            },
            HookEventKind::PreApiRequest,
        ),
        (
            HookEvent::PostApiRequest {
                request_id,
                status: 200,
            },
            HookEventKind::PostApiRequest,
        ),
        (
            HookEvent::TransformToolResult {
                tool_use_id,
                result: ToolResult::Text("raw".to_owned()),
            },
            HookEventKind::TransformToolResult,
        ),
        (
            HookEvent::TransformTerminalOutput {
                tool_use_id,
                raw: Bytes::from_static(b"stdout"),
            },
            HookEventKind::TransformTerminalOutput,
        ),
        (
            HookEvent::Elicitation {
                mcp_server_id: McpServerId("server".to_owned()),
                schema: json!({ "type": "object" }),
            },
            HookEventKind::Elicitation,
        ),
        (
            HookEvent::PreToolSearch {
                tool_use_id,
                query: "filesystem".to_owned(),
                query_kind: ToolSearchQueryKind::Keyword,
            },
            HookEventKind::PreToolSearch,
        ),
        (
            HookEvent::PostToolSearchMaterialize {
                tool_use_id,
                materialized: vec!["read".to_owned()],
                backend: "inline".to_owned(),
                cache_impact: CacheImpact {
                    prompt_cache_invalidated: false,
                    reason: None,
                },
            },
            HookEventKind::PostToolSearchMaterialize,
        ),
    ];

    assert_eq!(cases.len(), 20);
    for (event, kind) in cases {
        assert_eq!(event.kind(), kind);
    }
}

#[test]
fn hook_context_is_cloneable_and_uses_read_only_session_view() {
    let context = sample_context();
    let cloned = context.clone();

    assert_eq!(cloned.tenant_id, TenantId::SINGLE);
    assert_eq!(cloned.view.workspace_root(), Some(Path::new("/workspace")));
    assert_eq!(
        cloned.view.recent_messages(1),
        vec![HookMessageView {
            role: MessageRole::User,
            text_snippet: "hello".to_owned(),
            tool_use_id: None,
        }]
    );
    assert_eq!(cloned.view.permission_mode(), PermissionMode::Default);
    assert!(cloned.view.current_tool_descriptor().is_some());
}

#[test]
fn pre_tool_use_outcome_validates_block_exclusivity() {
    assert_eq!(PreToolUseOutcome::default().validate(), Ok(()));

    let context_patch = ContextPatch {
        role: ContextPatchRole::UserSuffix,
        content: "reason".to_owned(),
        apply_to_next_turn_only: true,
    };
    assert!(matches!(
        HookOutcome::AddContext(context_patch.clone()),
        HookOutcome::AddContext(patch) if patch.role == ContextPatchRole::UserSuffix
    ));

    let invalid = PreToolUseOutcome {
        rewrite_input: Some(json!({ "command": "ls" })),
        additional_context: Some(context_patch),
        block: Some("blocked".to_owned()),
        ..PreToolUseOutcome::default()
    };

    assert_eq!(
        invalid.validate(),
        Err(InconsistentReason::PreToolUseBlockExclusive)
    );
}

#[test]
fn hook_crate_stays_inside_allowed_dependency_boundary() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();

    assert!(!manifest.contains("jyowo-harness-model"));
    assert!(!manifest.contains("jyowo-harness-session"));
    assert!(!manifest.contains("jyowo-harness-engine"));
    assert!(!manifest.contains("jyowo-harness-journal"));
    assert!(!manifest.contains("jyowo-harness-tool"));
    assert!(!manifest.contains("jyowo-harness-observability"));
}

struct NoopHook {
    id: String,
}

#[async_trait]
impl HookHandler for NoopHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::Continue)
    }
}

#[derive(Debug)]
struct TestSessionView;

impl HookSessionView for TestSessionView {
    fn workspace_root(&self) -> Option<&Path> {
        Some(Path::new("/workspace"))
    }

    fn recent_messages(&self, limit: usize) -> Vec<HookMessageView> {
        vec![HookMessageView {
            role: MessageRole::User,
            text_snippet: "hello".to_owned(),
            tool_use_id: None,
        }]
        .into_iter()
        .take(limit)
        .collect()
    }

    fn permission_mode(&self) -> PermissionMode {
        PermissionMode::Default
    }

    fn redacted(&self) -> &dyn harness_contracts::Redactor {
        &harness_contracts::NoopRedactor
    }

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView> {
        Some(ToolDescriptorView {
            name: "bash".to_owned(),
            display_name: "Bash".to_owned(),
            description: "Run shell commands".to_owned(),
        })
    }
}

fn sample_pre_tool_use() -> HookEvent {
    HookEvent::PreToolUse {
        tool_use_id: ToolUseId::new(),
        tool_name: "bash".to_owned(),
        input: json!({ "command": "ls" }),
    }
}

fn sample_context() -> HookContext {
    HookContext {
        tenant_id: TenantId::SINGLE,
        session_id: harness_contracts::SessionId::new(),
        run_id: Some(RunId::new()),
        turn_index: Some(1),
        correlation_id: harness_contracts::CorrelationId::new(),
        causation_id: harness_contracts::CausationId::new(),
        trust_level: TrustLevel::AdminTrusted,
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::FullyInteractive,
        at: chrono::Utc::now(),
        view: Arc::new(TestSessionView),
        upstream_outcome: Some(UpstreamOutcomeView {
            last_handler_id: "previous".to_owned(),
            rewrote_input: true,
            override_permission_present: false,
            additional_context_bytes: Some(8),
        }),
        replay_mode: ReplayMode::Live,
    }
}
