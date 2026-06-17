use futures::StreamExt;
use harness_contracts::{Event, RunId, SessionId, UsageSnapshot};
use harness_subagent::{
    SubagentAnnouncement, SubagentAnnouncementRenderer, SubagentCancellationToken, SubagentHandle,
    SubagentHandleEvent, SubagentStatus,
};

#[tokio::test]
async fn handle_exposes_event_stream_and_cancel_handle() {
    let announcement = announcement();
    let token = SubagentCancellationToken::new();
    let handle = SubagentHandle::ready(announcement.clone()).with_cancellation(token.clone());

    assert_eq!(
        handle.events(),
        &[SubagentHandleEvent::Announced(announcement.clone())]
    );
    let events: Vec<_> = handle.event_stream().collect().await;
    assert_eq!(events, vec![SubagentHandleEvent::Announced(announcement)]);

    handle.cancel().expect("cancel should reach token");
    assert!(token.is_cancelled());
}

#[test]
fn announcement_renderer_emits_parent_user_message_event() {
    let announcement = announcement();
    let run_id = RunId::new();
    let event = SubagentAnnouncementRenderer::default().render_user_message(&announcement, run_id);

    assert!(matches!(
        event,
        Event::UserMessageAppended(appended)
            if appended.run_id == run_id
                && appended.metadata.source.as_deref() == Some("subagent")
                && appended.metadata.labels.get("subagent_id")
                    == Some(&announcement.subagent_id.to_string())
                && appended.metadata.labels.get("renderer_id")
                    == Some(&"xml-task-notification".to_owned())
                && matches!(&appended.content, harness_contracts::MessageContent::Text(text)
                    if text.contains("<task-notification>")
                        && text.contains("<rewrite-hint>"))
    ));
}

fn announcement() -> SubagentAnnouncement {
    SubagentAnnouncement {
        subagent_id: harness_contracts::SubagentId::new(),
        parent_session_id: SessionId::new(),
        status: SubagentStatus::Completed,
        summary: "review complete".to_owned(),
        result: None,
        usage: UsageSnapshot::default(),
        transcript_ref: None,
        context_report: None,
    }
}
