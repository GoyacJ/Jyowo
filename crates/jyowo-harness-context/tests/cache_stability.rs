use chrono::Utc;
use harness_context::{ContextEngine, ContextSessionView, PromptCachePolicy};
use harness_contracts::{
    Message, MessageId, MessagePart, MessageRole, SessionId, TenantId, ToolDescriptor, TurnInput,
};
use harness_model::PromptCacheStyle;

#[tokio::test]
async fn repeated_assemble_keeps_frozen_prefix_and_cache_breakpoints_stable() {
    let engine = ContextEngine::builder()
        .with_cache_policy(PromptCachePolicy {
            style: PromptCacheStyle::None,
            max_breakpoints: 4,
            ..PromptCachePolicy::default()
        })
        .build()
        .unwrap();
    let session = StableSession {
        system: Some("system\nmemdir snapshot".to_owned()),
        history: vec![message(MessageRole::User, "history")],
    };

    let mut systems = Vec::new();
    let mut breakpoints = Vec::new();
    for turn in 0..5 {
        let prompt = engine
            .assemble(&session, &turn_input(turn, "next"))
            .await
            .unwrap();
        systems.push(prompt.system);
        breakpoints.push(prompt.cache_breakpoints);
    }

    assert!(systems
        .iter()
        .all(|system| system.as_deref() == Some("system\nmemdir snapshot")));
    assert!(breakpoints.windows(2).all(|pair| pair[0] == pair[1]));
}

#[tokio::test]
async fn assemble_does_not_rewrite_session_history() {
    let engine = ContextEngine::builder().build().unwrap();
    let session = StableSession {
        system: Some("system".to_owned()),
        history: vec![message(MessageRole::User, "history")],
    };

    for turn in 0..5 {
        let prompt = engine
            .assemble(&session, &turn_input(turn, "next"))
            .await
            .unwrap();
        assert_eq!(prompt.messages[0].parts, session.history[0].parts);
        assert_eq!(session.history.len(), 1);
    }
}

struct StableSession {
    system: Option<String>,
    history: Vec<Message>,
}

impl ContextSessionView for StableSession {
    fn tenant_id(&self) -> TenantId {
        TenantId::SINGLE
    }

    fn session_id(&self) -> Option<SessionId> {
        Some(SessionId::new())
    }

    fn system(&self) -> Option<String> {
        self.system.clone()
    }

    fn messages(&self) -> Vec<Message> {
        self.history.clone()
    }

    fn tools_snapshot(&self) -> Vec<ToolDescriptor> {
        Vec::new()
    }
}

fn turn_input(turn: u64, text: &str) -> TurnInput {
    TurnInput {
        message: message(MessageRole::User, text),
        metadata: serde_json::json!({ "turn": turn }),
    }
}

fn message(role: MessageRole, text: &str) -> Message {
    Message {
        id: MessageId::new(),
        role,
        parts: vec![MessagePart::Text(text.to_owned())],
        created_at: Utc::now(),
    }
}
