use chrono::Utc;
use harness_contracts::{
    export_all_schemas, Message, MessageId, MessagePart, MessageRole, ModelProtocol, RunId,
    SessionId, TenantId,
};
use harness_model::{
    ModelRequest, ModelStreamEvent, ProviderRequestContext, StreamAggregate, StreamAggregator,
};
use harness_provider_state::{
    ProviderContinuationKind, ProviderContinuationRecord, ProviderContinuationScope,
};
use serde_json::{json, Value};

const SENTINEL: &str = "PRIVATE_PROVIDER_CONTINUATION_SENTINEL";

#[test]
fn provider_continuation_stream_event_is_preserved_by_aggregation() {
    let mut aggregator = StreamAggregator::default();
    let output = aggregator.push(ModelStreamEvent::ProviderContinuationDelta {
        kind: ProviderContinuationKind::ReasoningReplay,
        payload: json!({ "secret": SENTINEL }),
    });

    assert_eq!(
        output,
        vec![StreamAggregate::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload: json!({ "secret": SENTINEL }),
        }]
    );
}

#[test]
fn provider_continuation_stream_event_does_not_become_public_content() {
    let mut aggregator = StreamAggregator::default();
    let output = aggregator.push(ModelStreamEvent::ProviderContinuationDelta {
        kind: ProviderContinuationKind::ReasoningReplay,
        payload: json!({ "secret": SENTINEL }),
    });

    assert!(!output.iter().any(|event| matches!(
        event,
        StreamAggregate::TextChunk { .. }
            | StreamAggregate::ThinkingChunk { .. }
            | StreamAggregate::ReasoningSummaryChunk { .. }
            | StreamAggregate::ToolUseStart { .. }
            | StreamAggregate::ToolUseInputDelta { .. }
            | StreamAggregate::ToolCallReady { .. }
    )));
}

#[test]
fn provider_continuation_request_context_debug_redacts_payload() {
    let context = ProviderRequestContext {
        provider_id: "deepseek".to_owned(),
        model_config_id: Some("config-1".to_owned()),
        dialect: Some("deepseek".to_owned()),
        setup_fingerprint: None,
        continuations: vec![record()],
    };

    let debug = format!("{context:?}");

    assert!(debug.contains("continuation_count"));
    assert!(!debug.contains(SENTINEL));
    assert!(!debug.contains(&private_reasoning_key()));
}

#[test]
fn provider_continuation_model_request_debug_redacts_payload() {
    let request = ModelRequest {
        model_id: "deepseek-v4-flash".to_owned(),
        messages: vec![Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text("hello".to_owned())],
            created_at: Utc::now(),
        }],
        tools: None,
        system: None,
        temperature: None,
        max_tokens: Some(64),
        stream: true,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::ChatCompletions,
        extra: Value::Null,
        options: harness_contracts::ModelRequestOptions::default(),
        provider_context: ProviderRequestContext {
            provider_id: "deepseek".to_owned(),
            model_config_id: Some("config-1".to_owned()),
            dialect: Some("deepseek".to_owned()),
            setup_fingerprint: None,
            continuations: vec![record()],
        },
    };

    let debug = format!("{request:?}");

    assert!(debug.contains("provider_context"));
    assert!(!debug.contains(SENTINEL));
    assert!(!debug.contains(&private_reasoning_key()));
}

#[test]
fn provider_continuation_stream_event_debug_redacts_payload() {
    let event = ModelStreamEvent::ProviderContinuationDelta {
        kind: ProviderContinuationKind::ReasoningReplay,
        payload: json!({ private_reasoning_key(): SENTINEL }),
    };

    let debug = format!("{event:?}");

    assert!(debug.contains("ProviderContinuationDelta"));
    assert!(debug.contains("ReasoningReplay"));
    assert!(!debug.contains(SENTINEL));
    assert!(!debug.contains(&private_reasoning_key()));
}

#[test]
fn provider_continuation_stream_aggregate_debug_redacts_payload() {
    let aggregate = StreamAggregate::ProviderContinuationDelta {
        kind: ProviderContinuationKind::ReasoningReplay,
        payload: json!({ private_reasoning_key(): SENTINEL }),
    };

    let debug = format!("{aggregate:?}");

    assert!(debug.contains("ProviderContinuationDelta"));
    assert!(debug.contains("ReasoningReplay"));
    assert!(!debug.contains(SENTINEL));
    assert!(!debug.contains(&private_reasoning_key()));
}

#[test]
fn provider_continuation_types_remain_private_aggregate_only() {
    let event = ModelStreamEvent::ProviderContinuationDelta {
        kind: ProviderContinuationKind::ReasoningReplay,
        payload: json!({ private_reasoning_key(): SENTINEL }),
    };
    let aggregate = StreamAggregator::default().push(event);

    assert!(matches!(
        aggregate.as_slice(),
        [StreamAggregate::ProviderContinuationDelta { .. }]
    ));
}

#[test]
fn provider_continuation_types_are_not_public_contract_schema() {
    let schemas = export_all_schemas();
    let schema_json = serde_json::to_string(&schemas).expect("schemas should serialize");

    for key in schemas.keys() {
        assert!(!key.contains("provider_continuation"));
        assert!(!key.contains("ProviderContinuation"));
    }
    assert!(!schema_json.contains("provider_continuation"));
    assert!(!schema_json.contains("ProviderContinuation"));
    assert!(!schema_json.contains("ProviderContinuationDelta"));
    assert!(!schema_json.contains(&private_reasoning_key()));
}

fn record() -> ProviderContinuationRecord {
    ProviderContinuationRecord {
        provider_id: "deepseek".to_owned(),
        model_config_id: Some("config-1".to_owned()),
        protocol: ModelProtocol::ChatCompletions,
        dialect: "deepseek".to_owned(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        producing_run_id: RunId::new(),
        message_id: MessageId::new(),
        scope: ProviderContinuationScope::Conversation,
        kind: ProviderContinuationKind::ReasoningReplay,
        payload: json!({ private_reasoning_key(): SENTINEL }),
        created_at: Utc::now(),
    }
}

fn private_reasoning_key() -> String {
    ["reasoning", "_", "content"].concat()
}
