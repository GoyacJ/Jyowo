use harness_context::{ContextEngine, ContextSessionView};
use harness_contracts::{
    ContextPatchLifecycle, ContextPatchRequest, ContextPatchSinkCap, ContextPatchSource,
    DeferredToolsDeltaAttachment, Event, Message, MessageId, MessagePart, MessageRole, RunId,
    SessionId, SkillId, SkillInjectionId, TenantId, ToolDescriptor, ToolPoolChangeSource,
    ToolResult, ToolUseId, TurnInput,
};

#[tokio::test]
async fn transient_skill_and_hook_patches_are_user_messages_after_tool_results() {
    let engine = ContextEngine::builder().build().unwrap();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();

    engine
        .push_patch(ContextPatchRequest {
            tenant_id: TenantId::SINGLE,
            session_id,
            run_id,
            source: ContextPatchSource::SkillInjection {
                skill_id: SkillId("skill-review".to_owned()),
                skill_name: "review-pr".to_owned(),
                injection_id: SkillInjectionId("skill:review-pr:1".to_owned()),
                tool_use_id,
                consumed_config_keys: vec!["github.org".to_owned()],
            },
            body: "Review checklist".to_owned(),
            lifecycle: ContextPatchLifecycle::Transient,
        })
        .await
        .unwrap();
    engine
        .push_patch(ContextPatchRequest {
            tenant_id: TenantId::SINGLE,
            session_id,
            run_id,
            source: ContextPatchSource::HookAddContext {
                handler_id: "policy-hook".to_owned(),
                hook_event_kind: harness_contracts::HookEventKind::PostToolUse,
            },
            body: "Policy note".to_owned(),
            lifecycle: ContextPatchLifecycle::Transient,
        })
        .await
        .unwrap();

    let prompt = engine
        .assemble(
            &TestSession { session_id },
            &TurnInput {
                message: tool_result_message(tool_use_id, "tool output"),
                metadata: serde_json::json!({ "run_id": run_id.to_string(), "turn": 1 }),
            },
        )
        .await
        .unwrap();

    assert_eq!(prompt.messages.len(), 2);
    assert_eq!(prompt.messages[0].role, MessageRole::Tool);
    assert_eq!(prompt.messages[1].role, MessageRole::User);
    let patch_text = text(&prompt.messages[1]);
    assert!(patch_text.contains("---SKILL-BEGIN: review-pr---"));
    assert!(patch_text.contains("Review checklist"));
    assert!(patch_text.contains("<hook-add-context"));
    assert!(patch_text.contains("Policy note"));
    assert!(patch_text.find("Review checklist") < patch_text.find("Policy note"));
    assert!(prompt.events.iter().any(|event| matches!(
        event,
        Event::SkillInvoked(invoked)
            if invoked.skill_name == "review-pr"
                && invoked.bytes_injected == "Review checklist".len() as u64
    )));

    let second = engine
        .assemble(
            &TestSession { session_id },
            &TurnInput {
                message: user_message("next user turn"),
                metadata: serde_json::json!({ "run_id": run_id.to_string(), "turn": 2 }),
            },
        )
        .await
        .unwrap();

    assert_eq!(text(second.messages.last().unwrap()), "next user turn");
}

#[tokio::test]
async fn knowledge_retrieval_patch_is_fenced_user_data() {
    let engine = ContextEngine::builder().build().unwrap();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    engine
        .push_patch(ContextPatchRequest {
            tenant_id: TenantId::SINGLE,
            session_id,
            run_id,
            source: ContextPatchSource::KnowledgeRetrieval {
                provider_id: "knowledge-runtime".to_owned(),
                knowledge_base_ids: vec!["kb-runtime".to_owned()],
                reference_chunk_count: 1,
            },
            body: "---KNOWLEDGE-CONTEXT-BEGIN---\n<knowledge-context role=\"data\" instruction=\"false\">\n<chunk>alpha fact</chunk>\n</knowledge-context>\n---KNOWLEDGE-CONTEXT-END---".to_owned(),
            lifecycle: ContextPatchLifecycle::Transient,
        })
        .await
        .unwrap();

    let prompt = engine
        .assemble(
            &TestSession { session_id },
            &TurnInput {
                message: user_message("answer with data"),
                metadata: serde_json::json!({ "run_id": run_id.to_string(), "turn": 1 }),
            },
        )
        .await
        .unwrap();

    let prompt_text = text(prompt.messages.last().unwrap());
    assert!(prompt_text.contains("---KNOWLEDGE-CONTEXT-BEGIN---"));
    assert!(prompt_text.contains("<knowledge-context"));
    assert!(prompt_text.contains("instruction=\"false\""));
    assert!(prompt_text.contains("alpha fact"));
    assert!(prompt_text.ends_with("answer with data"));

    let second = engine
        .assemble(
            &TestSession { session_id },
            &TurnInput {
                message: user_message("next turn"),
                metadata: serde_json::json!({ "run_id": run_id.to_string(), "turn": 2 }),
            },
        )
        .await
        .unwrap();
    assert_eq!(text(second.messages.last().unwrap()), "next turn");
}

#[tokio::test]
async fn deferred_tools_delta_is_injected_into_next_user_turn_once() {
    let engine = ContextEngine::builder().build().unwrap();
    let session_id = SessionId::new();
    engine
        .push_deferred_tools_delta(
            TenantId::SINGLE,
            session_id,
            DeferredToolsDeltaAttachment {
                added_names: vec!["mcp__fixture__lookup".to_owned()],
                removed_names: vec!["legacy_tool".to_owned()],
                source: ToolPoolChangeSource::InitialClassification,
                at: chrono::Utc::now(),
                initial: true,
            },
        )
        .unwrap();

    let first = engine
        .assemble(
            &TestSession { session_id },
            &TurnInput {
                message: user_message("next turn"),
                metadata: serde_json::json!({ "turn": 1 }),
            },
        )
        .await
        .unwrap();

    let first_text = text(first.messages.last().unwrap());
    assert!(first_text.contains("<deferred-tools initial=\"true\""));
    assert!(first_text.contains("mcp__fixture__lookup"));
    assert!(first_text.contains("legacy_tool"));
    assert!(first_text.ends_with("next turn"));

    let second = engine
        .assemble(
            &TestSession { session_id },
            &TurnInput {
                message: user_message("later turn"),
                metadata: serde_json::json!({ "turn": 2 }),
            },
        )
        .await
        .unwrap();

    assert_eq!(text(second.messages.last().unwrap()), "later turn");
}

#[derive(Clone, Copy)]
struct TestSession {
    session_id: SessionId,
}

impl ContextSessionView for TestSession {
    fn tenant_id(&self) -> TenantId {
        TenantId::SINGLE
    }

    fn session_id(&self) -> Option<SessionId> {
        Some(self.session_id)
    }

    fn system(&self) -> Option<String> {
        None
    }

    fn messages(&self) -> Vec<Message> {
        Vec::new()
    }

    fn tools_snapshot(&self) -> Vec<ToolDescriptor> {
        Vec::new()
    }
}

fn user_message(value: &str) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::User,
        parts: vec![MessagePart::Text(value.to_owned())],
        created_at: harness_contracts::now(),
    }
}

fn tool_result_message(tool_use_id: ToolUseId, value: &str) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::Tool,
        parts: vec![MessagePart::ToolResult {
            tool_use_id,
            content: ToolResult::Text(value.to_owned()),
        }],
        created_at: harness_contracts::now(),
    }
}

fn text(message: &Message) -> &str {
    match &message.parts[0] {
        MessagePart::Text(value) => value,
        other => panic!("unexpected message part: {other:?}"),
    }
}
