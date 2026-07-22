use harness_contracts::{StopReason, UsageSnapshot};
use harness_model::{
    ContentDelta, ErrorClass, ModelStreamEvent, StreamAggregate, StreamAggregator,
};

#[test]
fn aggregates_tool_json_fragments_into_ready_call() {
    let mut aggregator = StreamAggregator::default();
    let mut output = Vec::new();

    output.extend(aggregator.push(ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseStart {
            id: "provider-call-1".to_owned(),
            name: "Search".to_owned(),
        },
    }));
    output.extend(aggregator.push(ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseInputJson(r#"{"query":"#.to_owned()),
    }));
    output.extend(aggregator.push(ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseInputJson(r#""docs"}"#.to_owned()),
    }));
    output.extend(aggregator.push(ModelStreamEvent::ContentBlockStop { index: 1 }));
    output.extend(aggregator.push(ModelStreamEvent::MessageStop));

    assert!(output.iter().any(|event| matches!(
        event,
        StreamAggregate::ToolUseStart { tool_name, .. } if tool_name == "Search"
    )));
    assert!(output.iter().any(|event| matches!(
        event,
        StreamAggregate::ToolUseInputDelta { delta, .. } if delta == r#"{"query":"#
    )));
    assert!(output.iter().any(|event| matches!(
        event,
        StreamAggregate::ToolCallReady { tool_name, input, .. }
            if tool_name == "Search" && input["query"] == "docs"
    )));
}

#[test]
fn invalid_tool_json_emits_fatal_stream_error_and_stops() {
    let mut aggregator = StreamAggregator::default();
    let mut output = Vec::new();

    output.extend(aggregator.push(ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseStart {
            id: "provider-call-1".to_owned(),
            name: "Search".to_owned(),
        },
    }));
    output.extend(aggregator.push(ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseInputJson("{".to_owned()),
    }));
    output.extend(aggregator.push(ModelStreamEvent::ContentBlockStop { index: 1 }));
    output.extend(aggregator.push(ModelStreamEvent::MessageStop));
    let after_fatal = aggregator.push(ModelStreamEvent::MessageStop);

    assert!(output.iter().any(|event| matches!(
        event,
        StreamAggregate::StreamError {
            class: ErrorClass::Fatal,
            ..
        }
    )));
    assert!(!output
        .iter()
        .any(|event| matches!(event, StreamAggregate::ToolCallReady { .. })));
    assert!(matches!(
        output.last(),
        Some(StreamAggregate::StreamError {
            class: ErrorClass::Fatal,
            ..
        })
    ));
    assert!(after_fatal.is_empty());
    assert!(aggregator.is_terminal());
}

#[test]
fn max_token_stop_discards_incomplete_tool_json() {
    let mut aggregator = StreamAggregator::default();
    let mut output = Vec::new();

    output.extend(aggregator.push(ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseStart {
            id: "provider-call-1".to_owned(),
            name: "FileWrite".to_owned(),
        },
    }));
    output.extend(aggregator.push(ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseInputJson(r#"{"content":"incomplete"#.to_owned()),
    }));
    output.extend(aggregator.push(ModelStreamEvent::ContentBlockStop { index: 1 }));
    output.extend(aggregator.push(ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::MaxIterations),
        usage_delta: UsageSnapshot::default(),
    }));
    output.extend(aggregator.push(ModelStreamEvent::MessageStop));

    assert!(!output.iter().any(|event| matches!(
        event,
        StreamAggregate::ToolCallReady { .. } | StreamAggregate::StreamError { .. }
    )));
    assert_eq!(output.last(), Some(&StreamAggregate::MessageDone));
    assert!(aggregator.is_terminal());
}

#[test]
fn message_stop_flushes_pending_tool_call_and_marks_done() {
    let mut aggregator = StreamAggregator::default();
    let mut output = Vec::new();

    output.extend(aggregator.push(ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseStart {
            id: "provider-call-1".to_owned(),
            name: "Search".to_owned(),
        },
    }));
    output.extend(aggregator.push(ModelStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ContentDelta::ToolUseInputJson(r#"{"query":"docs"}"#.to_owned()),
    }));
    output.extend(aggregator.push(ModelStreamEvent::MessageStop));

    assert!(output
        .iter()
        .any(|event| matches!(event, StreamAggregate::ToolCallReady { .. })));
    assert_eq!(output.last(), Some(&StreamAggregate::MessageDone));
}
