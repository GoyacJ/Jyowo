#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[tokio::test]
async fn page_conversation_timeline_with_runtime_state_accepts_assistant_interaction_events() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    open_conversation_session(&state, session_id).await;
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::RunStarted(test_run_started_event(session_id, run_id)),
                Event::AssistantReviewRequested(AssistantReviewRequestedEvent {
                    run_id,
                    request_id: RequestId::new(),
                    title: UiSafeText::from_trusted_redacted(
                        "Review Authorization: Bearer synthetic-token",
                    ),
                    body: Some(UiSafeText::from_trusted_redacted(
                        "Approve /Users/example/private?",
                    )),
                    at: now(),
                }),
                Event::AssistantClarificationRequested(AssistantClarificationRequestedEvent {
                    run_id,
                    request_id: RequestId::new(),
                    prompt: UiSafeText::from_trusted_redacted("Which size uses sk-synthetic?"),
                    at: now(),
                }),
                Event::AssistantNotice(AssistantNoticeEvent {
                    run_id,
                    notice_id: RequestId::new(),
                    body: UiSafeText::from_trusted_redacted(
                        "Generation queued from /home/example/private.",
                    ),
                    code: None,
                    at: now(),
                }),
            ],
        )
        .await
        .expect("assistant interaction events should append");

    let page = page_conversation_timeline_with_runtime_state(
        PageConversationTimelineRequest {
            conversation_id: session_id.to_string(),
            after_cursor: None,
            limit: None,
        },
        &state,
    )
    .await
    .expect("timeline page should load");

    let event_types = page
        .events
        .iter()
        .map(|event| event.event_type)
        .collect::<Vec<_>>();
    assert!(event_types.contains(&"assistant.review.requested"));
    assert!(event_types.contains(&"assistant.clarification.requested"));
    assert!(event_types.contains(&"assistant.notice"));
    let review = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.review.requested")
        .expect("review event should be mapped");
    assert_eq!(
        review.payload["title"],
        json!("Review [REDACTED] [REDACTED] [REDACTED]")
    );
    assert_eq!(review.payload["body"], json!("Approve [REDACTED]"));
    let clarification = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.clarification.requested")
        .expect("clarification event should be mapped");
    assert_eq!(
        clarification.payload["prompt"],
        json!("Which size uses [REDACTED]")
    );
    let notice = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.notice")
        .expect("notice event should be mapped");
    assert_eq!(
        notice.payload["body"],
        json!("Generation queued from [REDACTED]")
    );
}
