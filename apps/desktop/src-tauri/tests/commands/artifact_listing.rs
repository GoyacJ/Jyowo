#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[test]
fn artifact_payload_skips_missing_optional_fields() {
    let value = serde_json::to_value(ArtifactSummaryPayload {
        action_label: "Open".to_owned(),
        description: "Generated implementation plan".to_owned(),
        id: "artifact-no-preview".to_owned(),
        kind: "markdown".to_owned(),
        preview: None,
        source_message_id: None,
        source_run_id: "run-001".to_owned(),
        status: "ready",
        title: "Generated output".to_owned(),
    })
    .unwrap();

    assert_eq!(value.get("preview"), None);
    assert_eq!(value.get("sourceMessageId"), None);
    assert_eq!(value.get("sourceRunId"), None);
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_ignores_assistant_outputs() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text(
                "# Runtime artifact\n\nGenerated from the conversation.".to_owned(),
            ),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = state.default_conversation_id();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Create an artifact".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        let conversation = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .expect("runtime conversation should load");
        if conversation
            .conversation
            .messages
            .iter()
            .any(|message| message.body.contains("Runtime artifact"))
        {
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("runtime assistant output should complete");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    let payload = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: state.default_conversation_id().to_string(),
        },
        &state,
    )
    .await
    .expect("runtime artifact projection should load");

    assert!(payload.artifacts.is_empty());
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_projects_artifact_events() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Created a durable artifact.".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = state.default_conversation_id();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Create an artifact".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    let run_id = loop {
        let conversation = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .expect("runtime conversation should load");
        if conversation
            .conversation
            .messages
            .iter()
            .any(|message| message.body.contains("Created a durable artifact"))
        {
            let activity = list_activity_with_runtime_state(
                ListActivityRequest {
                    conversation_id: Some(session_id.to_string()),
                    run_id: None,
                },
                &state,
            )
            .await
            .expect("activity should load");
            let run_id = activity
                .events
                .iter()
                .find(|event| event.event_type == "run.started")
                .map(|event| event.run_id.clone())
                .expect("run id should be visible in activity");
            break RunId::parse(&run_id).expect("run id should be canonical");
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("runtime assistant output should complete");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    };

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                revision_id: ArtifactRevisionId::new(),
                artifact_id: "artifact-runtime-notes".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("# Runtime artifact\n\nGenerated as a durable result.".to_owned()),
                run_id,
                session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Runtime artifact".to_owned(),
            })],
        )
        .await
        .expect("artifact event should append");

    let payload = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("runtime artifact projection should load");

    let artifact = payload
        .artifacts
        .first()
        .expect("artifact event should project");
    assert_eq!(artifact.id, "artifact-runtime-notes");
    assert_eq!(artifact.kind, "markdown");
    assert_eq!(artifact.status, "ready");
    assert_eq!(artifact.title, "Runtime artifact");
    assert!(artifact
        .preview
        .as_deref()
        .unwrap_or_default()
        .contains("Runtime artifact"));
    assert_eq!(artifact.source_message_id, None);
    assert_eq!(artifact.source_run_id, run_id.to_string());
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_scopes_artifacts_to_requested_conversation() {
    let state = runtime_state_with_harness().await;
    let default_session_id = state.default_conversation_id();
    let other_session_id = SessionId::new();
    open_conversation_session(&state, default_session_id).await;
    open_conversation_session(&state, other_session_id).await;
    let run_id = RunId::new();

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            default_session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                revision_id: ArtifactRevisionId::new(),
                artifact_id: "artifact-default".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("Default conversation artifact".to_owned()),
                run_id,
                session_id: default_session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Default artifact".to_owned(),
            })],
        )
        .await
        .expect("default artifact should append");
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            other_session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                revision_id: ArtifactRevisionId::new(),
                artifact_id: "artifact-other".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("Other conversation artifact".to_owned()),
                run_id,
                session_id: other_session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Other artifact".to_owned(),
            })],
        )
        .await
        .expect("other artifact should append");

    let payload = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: other_session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("runtime artifact projection should load");

    assert_eq!(payload.artifacts.len(), 1);
    assert_eq!(payload.artifacts[0].id, "artifact-other");
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_requires_conversation_id() {
    let state = runtime_state_with_harness().await;

    let error = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: String::new(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_ignores_mismatched_artifact_session_ids() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                revision_id: ArtifactRevisionId::new(),
                artifact_id: "artifact-mismatched".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("Wrong session".to_owned()),
                run_id: RunId::new(),
                session_id: SessionId::new(),
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Mismatched artifact".to_owned(),
            })],
        )
        .await
        .expect("artifact event should append");

    let payload = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("runtime artifact projection should load");

    assert!(payload.artifacts.is_empty());
}

#[tokio::test]
async fn list_reference_candidates_with_runtime_state_scopes_artifacts_to_requested_conversation() {
    let state = runtime_state_with_harness().await;
    let default_session_id = state.default_conversation_id();
    let other_session_id = SessionId::new();
    open_conversation_session(&state, default_session_id).await;
    open_conversation_session(&state, other_session_id).await;
    let run_id = RunId::new();

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            default_session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                revision_id: ArtifactRevisionId::new(),
                artifact_id: "artifact-default".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("Default conversation artifact".to_owned()),
                run_id,
                session_id: default_session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Default artifact".to_owned(),
            })],
        )
        .await
        .expect("default artifact should append");
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            other_session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                revision_id: ArtifactRevisionId::new(),
                artifact_id: "artifact-other".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("Other conversation artifact".to_owned()),
                run_id,
                session_id: other_session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Other artifact".to_owned(),
            })],
        )
        .await
        .expect("other artifact should append");

    let payload = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: other_session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("reference candidates should load");

    assert_eq!(payload.artifacts.len(), 1);
    assert_eq!(payload.artifacts[0].id.as_deref(), Some("artifact-other"));
}

#[tokio::test]
async fn list_reference_candidates_with_runtime_state_rejects_invalid_conversation_id() {
    let state = runtime_state_with_harness().await;

    let error = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: "not-a-session-id".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn list_reference_candidates_with_runtime_state_rejects_unknown_conversation_id() {
    let state = runtime_state_with_harness().await;
    open_conversation_session(&state, state.default_conversation_id()).await;

    let error = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: SessionId::new().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "NOT_FOUND");
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_redacts_artifact_metadata() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    let token = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    open_conversation_session(&state, session_id).await;

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    revision_id: ArtifactRevisionId::new(),
                    artifact_id: "artifact-sensitive".to_owned(),
                    at: now(),
                    blob_ref: None,
                    content_hash: None,
                    kind: format!("markdown {token} data:image/svg+xml,<svg onload=alert(1)>。"),
                    preview: Some(
                        "Blob:.jyowo/runtime/blobs/blob-001 log/tmp/provider-output".to_owned(),
                    ),
                    run_id: RunId::new(),
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Running,
                    title: format!("Review {token} https://provider.example/artifact"),
                }),
                Event::ArtifactUpdated(ArtifactUpdatedEvent {
                    revision_id: ArtifactRevisionId::new(),
                    artifact_id: "artifact-sensitive".to_owned(),
                    at: now(),
                    blob_ref: None,
                    content_hash: None,
                    kind: Some("markdown file:/tmp/provider-output".to_owned()),
                    preview: Some(
                        "Updated 路径：.jyowo/runtime/blobs/blob-002 home~/secret blob:null/provider"
                            .to_owned(),
                    ),
                    run_id: RunId::new(),
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: Some(ArtifactStatus::Ready),
                    title: Some("Updated链接https://provider.example/updated".to_owned()),
                }),
            ],
        )
        .await
        .expect("artifact event should append");

    let payload = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("runtime artifact projection should load");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(!serialized.contains(token));
    assert!(!serialized.contains("https://provider.example"));
    assert!(!serialized.contains(".jyowo/runtime/blobs"));
    assert!(!serialized.contains("/tmp/provider-output"));
    assert!(!serialized.contains("~/secret"));
    assert!(!serialized.contains("data:image"));
    assert!(!serialized.contains("blob:null"));
    assert!(!serialized.contains("file:"));
    assert!(serialized.contains("[REDACTED]"));
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_hides_runtime_read_errors() {
    let state = runtime_state_with_harness().await;

    let error = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: SessionId::new().to_string(),
        },
        &state,
    )
    .await
    .expect_err("missing conversation session should fail safely");

    assert_eq!(error.code, "NOT_FOUND");
    assert!(!error
        .message
        .contains(&state.default_conversation_id().to_string()));
}
