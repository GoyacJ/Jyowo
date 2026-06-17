use harness_contracts::*;
use harness_journal::*;

fn usage(input_tokens: u64, output_tokens: u64) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens,
        output_tokens,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        cost_micros: input_tokens + output_tokens,
        tool_calls: 0,
    }
}

#[test]
fn session_projection_replays_message_usage_and_end_state() {
    let run_id = RunId::new();
    let user_message_id = MessageId::new();
    let assistant_message_id = MessageId::new();
    let now = harness_contracts::now();
    let events = [
        Event::UserMessageAppended(UserMessageAppendedEvent {
            run_id,
            message_id: user_message_id,
            content: MessageContent::Text("hello".to_owned()),
            metadata: MessageMetadata::default(),
            at: now,
        }),
        Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
            run_id,
            message_id: assistant_message_id,
            content: MessageContent::Text("world".to_owned()),
            tool_uses: Vec::new(),
            usage: usage(3, 5),
            pricing_snapshot_id: None,
            stop_reason: StopReason::EndTurn,
            at: now,
        }),
        Event::RunEnded(RunEndedEvent {
            run_id,
            reason: EndReason::Completed,
            usage: Some(usage(7, 11)),
            ended_at: now,
        }),
    ];

    let projected = SessionProjection::replay(events.iter()).expect("replay succeeds");

    assert_eq!(projected.messages.len(), 2);
    assert_eq!(projected.messages[0].id, user_message_id);
    assert_eq!(projected.messages[0].role, MessageRole::User);
    assert_eq!(projected.messages[1].id, assistant_message_id);
    assert_eq!(projected.messages[1].role, MessageRole::Assistant);
    assert_eq!(projected.usage.input_tokens, 10);
    assert_eq!(projected.usage.output_tokens, 16);
    assert_eq!(projected.usage.cost_micros, 26);
    assert_eq!(projected.end_reason, None);
}

#[test]
fn session_projection_sets_end_reason_from_session_ended_only() {
    let session_id = SessionId::new();
    let now = harness_contracts::now();
    let events = [Event::SessionEnded(SessionEndedEvent {
        session_id,
        tenant_id: TenantId::SINGLE,
        reason: EndReason::Completed,
        final_usage: usage(2, 3),
        at: now,
    })];

    let projected = SessionProjection::replay(events.iter()).expect("replay succeeds");

    assert_eq!(projected.end_reason, Some(EndReason::Completed));
    assert_eq!(projected.usage.input_tokens, 2);
    assert_eq!(projected.usage.output_tokens, 3);
}
