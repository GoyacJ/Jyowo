use std::sync::Arc;

use async_trait::async_trait;
use harness_context::{
    BreakpointStrategy, CompactHint, ContextBuffer, ContextEngine, ContextOutcome, ContextProvider,
    ContextSessionView, PromptCachePolicy, TokenBudget,
};
use harness_contracts::{
    ContextError, ContextStageId, Message, MessageId, MessagePart, MessageRole, SessionId,
    ToolDescriptor, TurnInput,
};
use harness_model::{AnthropicCacheMode, BreakpointReason, PromptCacheStyle};
use serde_json::json;
use tokio::sync::Mutex;

#[tokio::test]
async fn empty_engine_compact_returns_no_change() {
    let engine = ContextEngine::builder().build().unwrap();
    let mut buffer = ContextBuffer::default();

    let outcome = engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap();

    assert_eq!(outcome, ContextOutcome::NoChange);
}

#[tokio::test]
async fn compact_runs_providers_in_fixed_stage_order() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let engine = ContextEngine::builder()
        .with_provider(RecordingProvider::new(
            "autocompact",
            ContextStageId::Autocompact,
            Arc::clone(&calls),
            ProviderAction::NoChange,
        ))
        .with_provider(RecordingProvider::new(
            "tool-result-budget",
            ContextStageId::ToolResultBudget,
            Arc::clone(&calls),
            ProviderAction::NoChange,
        ))
        .with_provider(RecordingProvider::new(
            "collapse",
            ContextStageId::Collapse,
            Arc::clone(&calls),
            ProviderAction::NoChange,
        ))
        .with_provider(RecordingProvider::new(
            "microcompact",
            ContextStageId::Microcompact,
            Arc::clone(&calls),
            ProviderAction::NoChange,
        ))
        .with_provider(RecordingProvider::new(
            "snip",
            ContextStageId::Snip,
            Arc::clone(&calls),
            ProviderAction::NoChange,
        ))
        .build()
        .unwrap();

    let mut buffer = ContextBuffer::default();
    engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap();

    assert_eq!(
        calls.lock().await.as_slice(),
        [
            "tool-result-budget",
            "snip",
            "microcompact",
            "collapse",
            "autocompact"
        ]
    );
}

#[tokio::test]
async fn compact_sorts_same_stage_by_provider_id() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let engine = ContextEngine::builder()
        .with_provider(RecordingProvider::new(
            "beta",
            ContextStageId::Snip,
            Arc::clone(&calls),
            ProviderAction::NoChange,
        ))
        .with_provider(RecordingProvider::new(
            "alpha",
            ContextStageId::Snip,
            Arc::clone(&calls),
            ProviderAction::NoChange,
        ))
        .build()
        .unwrap();

    let mut buffer = ContextBuffer::default();
    engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap();

    assert_eq!(calls.lock().await.as_slice(), ["alpha", "beta"]);
}

#[tokio::test]
async fn compact_stops_on_provider_error() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let engine = ContextEngine::builder()
        .with_provider(RecordingProvider::new(
            "first",
            ContextStageId::Snip,
            Arc::clone(&calls),
            ProviderAction::Error,
        ))
        .with_provider(RecordingProvider::new(
            "second",
            ContextStageId::Microcompact,
            Arc::clone(&calls),
            ProviderAction::NoChange,
        ))
        .build()
        .unwrap();

    let mut buffer = ContextBuffer::default();
    let error = engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap_err();

    assert_eq!(error, ContextError::Message("first failed".to_owned()));
    assert_eq!(calls.lock().await.as_slice(), ["first"]);
}

#[tokio::test]
async fn compact_stops_when_provider_forks() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let child = SessionId::new();
    let engine = ContextEngine::builder()
        .with_provider(RecordingProvider::new(
            "fork",
            ContextStageId::Snip,
            Arc::clone(&calls),
            ProviderAction::Fork(child),
        ))
        .with_provider(RecordingProvider::new(
            "after-fork",
            ContextStageId::Microcompact,
            Arc::clone(&calls),
            ProviderAction::NoChange,
        ))
        .build()
        .unwrap();

    let mut buffer = ContextBuffer::default();
    let outcome = engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap();

    assert_eq!(
        outcome,
        ContextOutcome::Forked {
            new_session_id: child
        }
    );
    assert_eq!(calls.lock().await.as_slice(), ["fork"]);
}

#[tokio::test]
async fn assemble_uses_read_only_session_projection() {
    let engine = ContextEngine::builder().build().unwrap();
    let session = TestSession {
        system: Some("system header".to_owned()),
        history: vec![message(MessageRole::User, "hello")],
        tools: Vec::new(),
    };
    let input = TurnInput {
        message: message(MessageRole::User, "next"),
        metadata: json!({}),
    };

    let prompt = engine.assemble(&session, &input).await.unwrap();

    assert_eq!(prompt.system.as_deref(), Some("system header"));
    assert_eq!(prompt.messages.len(), 2);
    assert_eq!(prompt.messages[1], input.message);
    assert!(prompt.tools_snapshot.is_empty());
    assert!(prompt.cache_breakpoints.is_empty());
    assert!(prompt.tokens_estimate > 0);
    assert!(prompt.budget_utilization >= 0.0);
}

#[tokio::test]
async fn cloned_engine_can_replace_token_budget_without_mutating_base() {
    let base = ContextEngine::builder()
        .with_budget(TokenBudget {
            max_tokens_per_turn: 1_000,
            ..TokenBudget::default()
        })
        .build()
        .unwrap();
    let scoped = base.clone_with_budget(TokenBudget {
        max_tokens_per_turn: 1,
        ..TokenBudget::default()
    });
    let session = TestSession {
        system: None,
        history: vec![message(MessageRole::User, "history")],
        tools: Vec::new(),
    };
    let input = TurnInput {
        message: message(MessageRole::User, "next"),
        metadata: json!({}),
    };

    let base_prompt = base.assemble(&session, &input).await.unwrap();
    let scoped_prompt = scoped.assemble(&session, &input).await.unwrap();

    assert!(scoped_prompt.budget_utilization > base_prompt.budget_utilization);
    assert!(scoped_prompt.budget_utilization >= 1.0);
}

#[tokio::test]
async fn system_only_cache_policy_emits_no_message_breakpoints() {
    let engine = ContextEngine::builder()
        .with_cache_policy(PromptCachePolicy {
            style: PromptCacheStyle::Anthropic {
                mode: AnthropicCacheMode::SystemAnd3,
            },
            max_breakpoints: 4,
            breakpoint_strategy: BreakpointStrategy::SystemOnly,
        })
        .build()
        .unwrap();
    let session = TestSession {
        system: Some("system header".to_owned()),
        history: vec![message(MessageRole::User, "history")],
        tools: Vec::new(),
    };

    let prompt = engine
        .assemble(
            &session,
            &TurnInput {
                message: message(MessageRole::User, "next"),
                metadata: json!({}),
            },
        )
        .await
        .unwrap();

    assert!(prompt.cache_breakpoints.is_empty());
}

#[tokio::test]
async fn system_and3_cache_policy_selects_recent_three_non_system_messages() {
    let engine = ContextEngine::builder()
        .with_cache_policy(PromptCachePolicy {
            style: PromptCacheStyle::Anthropic {
                mode: AnthropicCacheMode::SystemAnd3,
            },
            max_breakpoints: 4,
            breakpoint_strategy: BreakpointStrategy::SystemAnd3,
        })
        .build()
        .unwrap();
    let history = vec![
        message(
            MessageRole::System,
            "do not use this as a message breakpoint",
        ),
        message(MessageRole::User, "one"),
        message(MessageRole::Assistant, "two"),
        message(MessageRole::User, "three"),
        message(MessageRole::Assistant, "four"),
    ];
    let current = message(MessageRole::User, "current");
    let expected = vec![history[3].id, history[4].id, current.id];
    let session = TestSession {
        system: Some("system header".to_owned()),
        history,
        tools: Vec::new(),
    };

    let prompt = engine
        .assemble(
            &session,
            &TurnInput {
                message: current,
                metadata: json!({}),
            },
        )
        .await
        .unwrap();

    let ids = prompt
        .cache_breakpoints
        .iter()
        .map(|breakpoint| breakpoint.after_message_id)
        .collect::<Vec<_>>();
    assert_eq!(ids, expected);
    assert!(prompt
        .cache_breakpoints
        .iter()
        .all(|breakpoint| breakpoint.reason == BreakpointReason::RecentMessage));
}

#[tokio::test]
async fn every_n_cache_policy_is_stable_deduped_and_capped() {
    let engine = ContextEngine::builder()
        .with_cache_policy(PromptCachePolicy {
            style: PromptCacheStyle::Anthropic {
                mode: AnthropicCacheMode::SystemAnd3,
            },
            max_breakpoints: 2,
            breakpoint_strategy: BreakpointStrategy::EveryN(2),
        })
        .build()
        .unwrap();
    let history = vec![
        message(MessageRole::User, "one"),
        message(MessageRole::Assistant, "two"),
        message(MessageRole::System, "system"),
        message(MessageRole::User, "three"),
        message(MessageRole::Assistant, "four"),
    ];
    let expected = vec![history[1].id, history[4].id];
    let session = TestSession {
        system: None,
        history,
        tools: Vec::new(),
    };

    let prompt = engine
        .assemble(
            &session,
            &TurnInput {
                message: message(MessageRole::User, "current"),
                metadata: json!({}),
            },
        )
        .await
        .unwrap();

    let ids = prompt
        .cache_breakpoints
        .iter()
        .map(|breakpoint| breakpoint.after_message_id)
        .collect::<Vec<_>>();
    assert_eq!(ids, expected);
}

#[tokio::test]
async fn repeated_assemble_produces_identical_cache_breakpoints() {
    let engine = ContextEngine::builder()
        .with_cache_policy(PromptCachePolicy {
            style: PromptCacheStyle::Anthropic {
                mode: AnthropicCacheMode::SystemAnd3,
            },
            max_breakpoints: 2,
            breakpoint_strategy: BreakpointStrategy::EveryN(2),
        })
        .build()
        .unwrap();
    let session = TestSession {
        system: None,
        history: vec![
            message(MessageRole::User, "one"),
            message(MessageRole::Assistant, "two"),
            message(MessageRole::User, "three"),
            message(MessageRole::Assistant, "four"),
        ],
        tools: Vec::new(),
    };
    let mut breakpoints = Vec::new();

    for turn in 0..5 {
        let prompt = engine
            .assemble(
                &session,
                &TurnInput {
                    message: message(MessageRole::User, &format!("current-{turn}")),
                    metadata: json!({ "turn": turn }),
                },
            )
            .await
            .unwrap();
        breakpoints.push(prompt.cache_breakpoints);
    }

    assert!(breakpoints.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn context_crate_keeps_dependency_boundary() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();

    assert!(!manifest.contains("jyowo-harness-tool"));
    assert!(!manifest.contains("jyowo-harness-session"));
    assert!(!manifest.contains("jyowo-harness-engine"));
    assert!(!manifest.contains("jyowo-harness-hook"));
    assert!(!manifest.contains("jyowo-harness-observability"));
}

#[derive(Clone, Copy)]
enum ProviderAction {
    NoChange,
    Error,
    Fork(SessionId),
}

struct RecordingProvider {
    id: String,
    stage: ContextStageId,
    calls: Arc<Mutex<Vec<String>>>,
    action: ProviderAction,
}

impl RecordingProvider {
    fn new(
        id: &str,
        stage: ContextStageId,
        calls: Arc<Mutex<Vec<String>>>,
        action: ProviderAction,
    ) -> Self {
        Self {
            id: id.to_owned(),
            stage,
            calls,
            action,
        }
    }
}

#[async_trait]
impl ContextProvider for RecordingProvider {
    fn provider_id(&self) -> &str {
        &self.id
    }

    fn stage(&self) -> ContextStageId {
        self.stage.clone()
    }

    async fn apply(
        &self,
        _ctx: &mut ContextBuffer,
        _hint: &CompactHint,
    ) -> Result<ContextOutcome, ContextError> {
        self.calls.lock().await.push(self.id.clone());
        match self.action {
            ProviderAction::NoChange => Ok(ContextOutcome::NoChange),
            ProviderAction::Error => Err(ContextError::Message(format!("{} failed", self.id))),
            ProviderAction::Fork(new_session_id) => Ok(ContextOutcome::Forked { new_session_id }),
        }
    }
}

struct TestSession {
    system: Option<String>,
    history: Vec<Message>,
    tools: Vec<ToolDescriptor>,
}

impl ContextSessionView for TestSession {
    fn system(&self) -> Option<String> {
        self.system.clone()
    }

    fn messages(&self) -> Vec<Message> {
        self.history.clone()
    }

    fn tools_snapshot(&self) -> Vec<ToolDescriptor> {
        self.tools.clone()
    }
}

fn message(role: MessageRole, text: &str) -> Message {
    Message {
        id: MessageId::new(),
        role,
        parts: vec![MessagePart::Text(text.to_owned())],
        created_at: chrono::Utc::now(),
    }
}
