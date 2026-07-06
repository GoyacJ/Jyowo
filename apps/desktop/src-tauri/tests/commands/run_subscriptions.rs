#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[tokio::test]
async fn subscribe_conversation_events_emits_live_batches_and_unsubscribes() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    let conversation_id = session_id.to_string();
    let batches = Arc::new(Mutex::new(Vec::<ConversationEventBatchPayload>::new()));
    let emitted_batches = Arc::clone(&batches);

    let subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id: conversation_id.clone(),
            after_cursor: None,
        },
        "main".to_owned(),
        Arc::new(move |batch| {
            emitted_batches.lock().unwrap().push(batch);
            Ok(())
        }),
        &state,
    )
    .await
    .expect("subscription should be accepted");

    assert_eq!(subscription.conversation_id, conversation_id);
    assert!(subscription.replay_events.is_empty());
    assert!(!subscription.gap);

    let started = start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: Some("00000000-0000-4000-8000-000000000001".to_owned()),
            context_references: None,
            conversation_id: conversation_id.clone(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Continue implementation".to_owned(),
        },
        &state,
    )
    .await
    .expect("run should start after subscribing");

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if batches.lock().unwrap().iter().any(|batch| {
                batch.subscription_id == subscription.subscription_id
                    && batch.conversation_id == conversation_id
                    && batch.phase == "live"
                    && batch.events.iter().any(|event| {
                        event.run_id == started.run_id && event.event_type == "run.started"
                    })
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("live subscription should emit the new run event");

    let emitted = batches.lock().unwrap();
    let live_events = emitted
        .iter()
        .filter(|batch| batch.subscription_id == subscription.subscription_id)
        .flat_map(|batch| batch.events.iter())
        .collect::<Vec<_>>();
    let live_run_started = live_events
        .iter()
        .find(|event| event.run_id == started.run_id && event.event_type == "run.started")
        .expect("live batch should include the started run event");
    assert!(live_run_started.conversation_sequence > 0);
    assert!(live_events
        .windows(2)
        .all(|pair| pair[0].conversation_sequence < pair[1].conversation_sequence));

    let unsubscribed = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id.clone(),
        },
        "main".to_owned(),
        &state,
    )
    .await
    .expect("unsubscribe should succeed");
    assert_eq!(unsubscribed.status, "unsubscribed");

    let already_closed = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .expect("unsubscribe should be idempotent");
    assert_eq!(already_closed.status, "alreadyClosed");
}

#[tokio::test]
async fn unsubscribe_conversation_events_rejects_other_window_subscription() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    let conversation_id = session_id.to_string();
    let subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id,
            after_cursor: None,
        },
        "main".to_owned(),
        Arc::new(|_batch| Ok(())),
        &state,
    )
    .await
    .expect("subscription should be created");

    let denied = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id.clone(),
        },
        "secondary".to_owned(),
        &state,
    )
    .await
    .expect_err("another window must not close the subscription");
    assert_eq!(denied.code, "INVALID_PAYLOAD");

    let unsubscribed = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .expect("owning window can close the subscription");
    assert_eq!(unsubscribed.status, "unsubscribed");
}

#[tokio::test]
async fn subscribe_conversation_events_accepts_cursor_after_replayed_permission_request() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": "pwd" }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let conversation_id = session_id.to_string();

    start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: Some("00000000-0000-4000-8000-000000000001".to_owned()),
            context_references: None,
            conversation_id: conversation_id.clone(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Run a command".to_owned(),
        },
        &state,
    )
    .await
    .expect("run should start and wait on permission");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;

    let first_subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id: conversation_id.clone(),
            after_cursor: None,
        },
        "main".to_owned(),
        Arc::new(|_batch| Ok(())),
        &state,
    )
    .await
    .expect("subscription replay should include pending permission");
    assert!(first_subscription
        .replay_events
        .iter()
        .any(|event| event.event_type == "permission.requested"));
    let cursor = first_subscription
        .cursor
        .clone()
        .expect("subscription replay should return a cursor");

    let second_subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id: conversation_id.clone(),
            after_cursor: Some(cursor),
        },
        "main".to_owned(),
        Arc::new(|_batch| Ok(())),
        &state,
    )
    .await
    .expect("cursor from permission replay should be accepted by the next subscription");
    assert!(second_subscription.replay_events.is_empty());

    unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: first_subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .unwrap();
    unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: second_subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .unwrap();
    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id,
            decision: PermissionDecision::Deny,
            option_id: deny_permission_option_id(&pending),
            request_id: pending.request.request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
}
