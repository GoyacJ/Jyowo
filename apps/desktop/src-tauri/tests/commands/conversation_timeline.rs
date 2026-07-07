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
                    title: UiSafeText::from_redacted_display(
                        "Review Authorization: Bearer synthetic-token",
                        &DefaultRedactor::default(),
                    ),
                    body: Some(UiSafeText::from_redacted_display(
                        "Approve /Users/example/private?",
                        &DefaultRedactor::default(),
                    )),
                    at: now(),
                }),
                Event::AssistantClarificationRequested(AssistantClarificationRequestedEvent {
                    run_id,
                    request_id: RequestId::new(),
                    prompt: UiSafeText::from_redacted_display(
                        "Which size uses sk-synthetic?",
                        &DefaultRedactor::default(),
                    ),
                    at: now(),
                }),
                Event::AssistantNotice(AssistantNoticeEvent {
                    run_id,
                    notice_id: RequestId::new(),
                    body: UiSafeText::from_redacted_display(
                        "Generation queued from /home/example/private.",
                        &DefaultRedactor::default(),
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
    assert_eq!(review.payload["title"], json!("[REDACTED]"));
    assert_eq!(review.payload["body"], json!("Approve [REDACTED]"));
    let clarification = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.clarification.requested")
        .expect("clarification event should be mapped");
    assert_eq!(clarification.payload["prompt"], json!("[REDACTED]"));
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

#[tokio::test]
async fn page_conversation_timeline_keeps_background_started_before_real_run_started() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let attempt_id = RunId::new();
    let run_id = RunId::new();
    let background_agent_id = harness_contracts::BackgroundAgentId::new();
    open_conversation_session(&state, session_id).await;
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::BackgroundAgentStarted(harness_contracts::BackgroundAgentStartedEvent {
                    background_agent_id,
                    conversation_id: session_id,
                    attempt_id,
                    title: UiSafeText::from_redacted_display(
                        "Background run",
                        &DefaultRedactor::default(),
                    ),
                    at: now(),
                }),
                Event::RunStarted(test_run_started_event(session_id, run_id)),
            ],
        )
        .await
        .expect("background and run events should append");

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

    let background_started = page
        .events
        .iter()
        .find(|event| event.event_type == "background.started")
        .expect("background start should be preserved before the real run starts");
    assert_eq!(
        background_started.payload["backgroundAgentId"],
        json!(background_agent_id.to_string())
    );
    assert_eq!(background_started.run_id, attempt_id.to_string());
}
