#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[tokio::test]
async fn get_replay_timeline_with_runtime_state_does_not_expose_raw_thinking_delta_text() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Thinking(ThinkingDelta {
                text: Some("private chain of thought".to_owned()),
                provider_native: Some(json!({ "thinking": "provider native secret" })),
                signature: Some("signature-secret".to_owned()),
            }),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::Text("Visible answer".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Think privately".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let request = ReplayTimelineRequest {
        conversation_id: Some(session_id.to_string()),
        run_id: Some(started.run_id),
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        let payload = get_replay_timeline_with_runtime_state(request.clone(), &state)
            .await
            .unwrap();

        if payload
            .events
            .iter()
            .any(|event| event.event_type == "assistant.completed")
        {
            let serialized = serde_json::to_string(&payload).unwrap();
            let thinking = payload
                .events
                .iter()
                .find(|event| event.event_type == "assistant.thinking.delta")
                .expect("thinking status event should be projected");
            assert_eq!(thinking.payload["status"], json!("running"));
            assert!(thinking.payload.get("text").is_none());
            assert!(thinking.payload.get("providerNative").is_none());
            assert!(thinking.payload.get("signature").is_none());
            assert!(!serialized.contains("private chain of thought"));
            assert!(!serialized.contains("provider native secret"));
            assert!(!serialized.contains("signature-secret"));
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("replay should include completed assistant event");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn get_replay_timeline_with_runtime_state_reads_redacted_journal_events_without_running_tools(
) {
    let secret_command =
        "git push https://ghp_abcdefghijklmnopqrstuvwxyz0123456789@github.com/org/repo";
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": secret_command }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Run a command".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;
    let request_id = pending.request.request_id;

    let payload = get_replay_timeline_with_runtime_state(
        ReplayTimelineRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(started.run_id.clone()),
        },
        &state,
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(payload.replayed);
    let run_started = payload
        .events
        .iter()
        .find(|event| event.event_type == "run.started")
        .expect("replay should include run started event");
    assert_eq!(run_started.payload["permissionMode"], json!("default"));
    assert!(payload
        .events
        .iter()
        .any(|event| event.event_type == "permission.requested"));
    assert!(!serialized.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(!serialized.contains(secret_command));
    assert!(serialized.contains("\"target\":\"git\""));
    assert_eq!(
        state.pending_permission_requests().len(),
        1,
        "replay read mode must not resolve or execute pending tools"
    );

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn get_replay_timeline_with_runtime_state_reads_beyond_first_event_page() {
    let mut stream_events = (0..205)
        .map(|index| ModelStreamEvent::ContentBlockDelta {
            index,
            delta: ContentDelta::Text(format!("delta-{index}")),
        })
        .collect::<Vec<_>>();
    stream_events.push(ModelStreamEvent::MessageStop);
    let state =
        runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(stream_events)]).await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Write many deltas".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let request = ReplayTimelineRequest {
        conversation_id: Some(session_id.to_string()),
        run_id: Some(started.run_id),
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        let payload = get_replay_timeline_with_runtime_state(request.clone(), &state)
            .await
            .unwrap();
        let serialized = serde_json::to_string(&payload).unwrap();
        if payload.events.len() > 200 && serialized.contains("delta-204") {
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("replay timeline should include events past the first page");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}
