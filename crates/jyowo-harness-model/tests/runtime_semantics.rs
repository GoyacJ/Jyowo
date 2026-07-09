use harness_model::{
    ConversationModelCapability, ModelDescriptor, ModelLifecycle, ModelProtocol,
    ModelRuntimeSemantics, ModelRuntimeSnapshot, ReasoningProtocolSemantics,
};
use harness_provider_state::ProviderContinuationKind;
use serde_json::Value;

#[test]
fn runtime_semantics_conversation_capability_shape_does_not_leak_private_fields() {
    let serialized = serde_json::to_value(ConversationModelCapability::default()).unwrap();
    let object = serialized.as_object().unwrap();

    assert!(!object.contains_key("runtimeSemantics"));
    assert!(!object.contains_key("runtime_semantics"));
    assert!(!object.contains_key("providerContinuation"));
    assert!(!object.contains_key("provider_continuation"));
    assert!(!object.contains_key("reasoningContent"));
    assert!(!object.contains_key(&private_reasoning_key()));
}

#[test]
fn runtime_semantics_snapshot_preserves_descriptor_semantics() {
    let descriptor = ModelDescriptor {
        provider_id: "deepseek".to_owned(),
        model_id: "deepseek-v4-flash".to_owned(),
        display_name: "DeepSeek V4 Flash".to_owned(),
        protocol: ModelProtocol::ChatCompletions,
        context_window: 1_000_000,
        max_output_tokens: 384_000,
        provider_declared_capability: ConversationModelCapability::default(),
        conversation_capability: ConversationModelCapability::default(),
        runtime_semantics: ModelRuntimeSemantics::openai_chat_deepseek(),
        lifecycle: ModelLifecycle::Stable,
        pricing: None,
    };

    let snapshot = ModelRuntimeSnapshot::from_descriptor(descriptor);

    assert_eq!(
        snapshot.runtime_semantics,
        ModelRuntimeSemantics::openai_chat_deepseek()
    );
}

#[test]
fn runtime_semantics_deepseek_requires_private_reasoning_replay() {
    assert_eq!(
        ModelRuntimeSemantics::openai_chat_deepseek().reasoning_protocol,
        ReasoningProtocolSemantics::ProviderPrivateReplay {
            continuation_kind: ProviderContinuationKind::ReasoningReplay,
            required_for_assistant_tool_replay: true,
        }
    );
}

#[test]
fn runtime_semantics_zhipu_requires_private_reasoning_replay() {
    assert_eq!(
        ModelRuntimeSemantics::openai_chat_zhipu().reasoning_protocol,
        ReasoningProtocolSemantics::ProviderPrivateReplay {
            continuation_kind: ProviderContinuationKind::ReasoningReplay,
            required_for_assistant_tool_replay: true,
        }
    );
    assert_eq!(
        ModelRuntimeSemantics::openai_chat_zhipu()
            .provider_continuation_dialect
            .as_deref(),
        Some("openai_chat.zhipu")
    );
}

#[test]
fn runtime_semantics_minimax_plain_chat_does_not_require_private_replay() {
    assert_eq!(
        ModelRuntimeSemantics::openai_chat_minimax().reasoning_protocol,
        ReasoningProtocolSemantics::ProviderPrivateReplay {
            continuation_kind: ProviderContinuationKind::ReasoningReplay,
            required_for_assistant_tool_replay: false,
        }
    );
    assert_eq!(
        ModelRuntimeSemantics::openai_chat_minimax()
            .provider_continuation_dialect
            .as_deref(),
        Some("openai_chat.minimax")
    );
}

#[test]
fn runtime_semantics_are_not_serialized_by_public_capability_contract() {
    let serialized = serde_json::to_string(&ConversationModelCapability::default()).unwrap();
    let parsed: Value = serde_json::from_str(&serialized).unwrap();

    assert_eq!(
        parsed.get("tool_calling"),
        Some(&Value::Bool(
            ConversationModelCapability::default().tool_calling
        ))
    );
    assert!(!serialized.contains("runtime"));
    assert!(!serialized.contains("continuation"));
}

fn private_reasoning_key() -> String {
    ["reasoning", "_", "content"].concat()
}
