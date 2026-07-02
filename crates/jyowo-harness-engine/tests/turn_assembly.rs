use harness_contracts::{
    DeltaChunk, Event, MessageId, RunId, StopReason, ToolUseId, UsageSnapshot,
};
use harness_engine::turn_assembly::TurnAssembly;
use harness_model::{ContentDelta, ModelStreamEvent, ReasoningSummaryDelta, ThinkingDelta};
use harness_provider_state::ProviderContinuationKind;
use serde_json::json;

#[tokio::test]
async fn turn_assembly_collects_visible_text_and_usage() {
    let run_id = RunId::new();
    let mut assembly = TurnAssembly::new(MessageId::new());

    let start_usage = UsageSnapshot {
        input_tokens: 3,
        output_tokens: 1,
        ..UsageSnapshot::default()
    };
    let delta_usage = UsageSnapshot {
        output_tokens: 5,
        ..UsageSnapshot::default()
    };

    let start = assembly.push_event(
        run_id,
        ModelStreamEvent::MessageStart {
            message_id: "provider-message".to_owned(),
            usage: start_usage.clone(),
        },
    );
    let text = assembly.push_event(
        run_id,
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("hello".to_owned()),
        },
    );
    let delta = assembly.push_event(
        run_id,
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            usage_delta: delta_usage.clone(),
        },
    );

    assert_eq!(assembly.assistant_text(), "hello");
    assert!(assembly.tool_calls().is_empty());
    assert_eq!(assembly.stop_reason(), StopReason::EndTurn);
    assert_eq!(assembly.model_call_usage().input_tokens, 3);
    assert_eq!(assembly.model_call_usage().output_tokens, 6);
    assert_eq!(start.usage_delta, start_usage);
    assert_eq!(delta.usage_delta, delta_usage);
    assert_eq!(text.events.len(), 1);
    assert!(matches!(
        &text.events[0],
        Event::AssistantDeltaProduced(event)
            if event.run_id == run_id
                && event.message_id == assembly.assistant_message_id()
                && matches!(&event.delta, DeltaChunk::Text(value) if value == "hello")
    ));
}

#[tokio::test]
async fn turn_assembly_collects_tool_calls_without_losing_visible_text() {
    let run_id = RunId::new();
    let mut assembly = TurnAssembly::new(MessageId::new());
    let tool_use_id = ToolUseId::new();

    assembly.push_event(
        run_id,
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("visible ".to_owned()),
        },
    );
    let tool = assembly.push_event(
        run_id,
        ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::ToolUseComplete {
                id: tool_use_id,
                name: "search".to_owned(),
                input: json!({ "query": "jyowo" }),
            },
        },
    );
    assembly.push_event(
        run_id,
        ModelStreamEvent::ContentBlockDelta {
            index: 2,
            delta: ContentDelta::ReasoningSummary(ReasoningSummaryDelta {
                text: "summary".to_owned(),
                provider_native: None,
            }),
        },
    );

    assert_eq!(assembly.assistant_text(), "visible ");
    assert_eq!(assembly.tool_calls().len(), 1);
    assert_eq!(assembly.tool_calls()[0].tool_use_id, tool_use_id);
    assert_eq!(assembly.tool_calls()[0].tool_name, "search");
    assert_eq!(assembly.tool_calls()[0].input, json!({ "query": "jyowo" }));
    assert!(tool.events.iter().any(|event| matches!(
        event,
        Event::AssistantDeltaProduced(delta)
            if matches!(delta.delta, DeltaChunk::ToolUseEnd { tool_use_id: id } if id == tool_use_id)
    )));
}

#[tokio::test]
async fn turn_assembly_drops_private_thinking_from_public_text() {
    let run_id = RunId::new();
    let mut assembly = TurnAssembly::new(MessageId::new());
    let output = assembly.push_event(
        run_id,
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Thinking(ThinkingDelta {
                text: Some("private reasoning".to_owned()),
                provider_native: Some(json!({ "provider_private": "secret" })),
                signature: Some("signature".to_owned()),
            }),
        },
    );

    assert_eq!(assembly.assistant_text(), "");
    assert_eq!(output.events.len(), 1);
    assert!(matches!(
        &output.events[0],
        Event::AssistantDeltaProduced(event)
            if matches!(
                &event.delta,
                DeltaChunk::Thought(thought)
                    if thought.text.is_none()
                        && thought.provider_id == "harness_model"
                        && thought.provider_native.is_none()
                        && thought.signature.is_none()
            )
    ));
}

#[tokio::test]
async fn turn_assembly_passes_provider_continuation_to_private_capture_only() {
    let run_id = RunId::new();
    let mut assembly = TurnAssembly::new(MessageId::new());
    let output = assembly.push_event(
        run_id,
        ModelStreamEvent::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload: json!({ "opaque": "private" }),
        },
    );

    assert!(output.events.is_empty());
    assert_eq!(assembly.assistant_text(), "");
    assert!(assembly.tool_calls().is_empty());
    assert_eq!(assembly.provider_continuations().len(), 1);
    assert_eq!(
        assembly.provider_continuations()[0].kind,
        ProviderContinuationKind::ReasoningReplay
    );
    assert_eq!(
        assembly.provider_continuations()[0].payload,
        json!({ "opaque": "private" })
    );
}

#[tokio::test]
async fn turn_assembly_debug_redacts_provider_continuation_payload() {
    let run_id = RunId::new();
    let mut assembly = TurnAssembly::new(MessageId::new());
    assembly.push_event(
        run_id,
        ModelStreamEvent::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload: json!({ "opaque": "private" }),
        },
    );

    let debug_output = format!("{assembly:?}");

    assert!(debug_output.contains("[redacted]"));
    assert!(!debug_output.contains("opaque"));
    assert!(!debug_output.contains("private"));
}
