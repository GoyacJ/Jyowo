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
        revisions: Vec::new(),
        source_message_id: None,
        source_run_id: "run-001".to_owned(),
        status: "ready",
        title: "Generated output".to_owned(),
        updated_at: None,
    })
    .unwrap();

    assert_eq!(value.get("preview"), None);
    assert_eq!(value.get("revisions"), None);
    assert_eq!(value.get("sourceMessageId"), None);
    assert_eq!(value.get("sourceRunId"), None);
}

#[test]
fn artifact_revision_content_payload_skips_missing_optional_ids() {
    let value = serde_json::to_value(
        jyowo_desktop_shell::commands::GetArtifactRevisionContentResponse {
            artifact_id: None,
            byte_length: 12,
            content_bytes: 12,
            offset_bytes: 0,
            limit_bytes: 65_536,
            total_bytes: 12,
            content: "artifact body".to_owned(),
            content_type: "text/plain".to_owned(),
            has_more: false,
            kind: "artifact-content".to_owned(),
            max_bytes: 65_536,
            next_cursor: None,
            content_hash: "artifact-content-hash".to_owned(),
            hash_algorithm: "blake3".to_owned(),
            redaction_state: "clean".to_owned(),
            ref_id: "evidence-ref".to_owned(),
            returned_bytes: 12,
            revision_id: None,
            truncated: false,
        },
    )
    .unwrap();

    assert_eq!(value.get("artifactId"), None);
    assert_eq!(value.get("revisionId"), None);
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
async fn list_artifacts_with_runtime_state_projects_revision_content_refs_newest_first() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    let run_id = RunId::new();
    let old_revision_id = ArtifactRevisionId::new();
    let new_revision_id = ArtifactRevisionId::new();
    let old_at = chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1);
    let new_at = chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2);
    let blob_store = FileBlobStore::open(
        state
            .workspace_root()
            .join(".jyowo")
            .join("runtime")
            .join("blobs"),
    )
    .expect("blob store opens");
    let old_content = bytes::Bytes::from_static(b"old artifact body");
    let new_content = bytes::Bytes::from_static(b"new artifact body sk-abcdefghijklmnopqrstuvwxyz");
    let old_hash = *blake3::hash(&old_content).as_bytes();
    let new_hash = *blake3::hash(&new_content).as_bytes();
    let old_blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            old_content.clone(),
            BlobMeta {
                content_type: Some("text/markdown".to_owned()),
                size: old_content.len() as u64,
                content_hash: old_hash,
                created_at: old_at,
                retention: BlobRetention::SessionScoped(session_id),
            },
        )
        .await
        .expect("old blob writes");
    let new_blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            new_content.clone(),
            BlobMeta {
                content_type: Some("text/markdown".to_owned()),
                size: new_content.len() as u64,
                content_hash: new_hash,
                created_at: new_at,
                retention: BlobRetention::SessionScoped(session_id),
            },
        )
        .await
        .expect("new blob writes");

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    revision_id: old_revision_id,
                    artifact_id: "artifact-revisions".to_owned(),
                    at: old_at,
                    blob_ref: Some(old_blob_ref),
                    content_hash: Some(old_hash.to_vec()),
                    kind: "markdown".to_owned(),
                    preview: Some("Old summary".to_owned()),
                    run_id,
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Ready,
                    title: "Revisioned artifact".to_owned(),
                }),
                Event::ArtifactUpdated(ArtifactUpdatedEvent {
                    revision_id: new_revision_id,
                    artifact_id: "artifact-revisions".to_owned(),
                    at: new_at,
                    blob_ref: Some(new_blob_ref),
                    content_hash: Some(new_hash.to_vec()),
                    kind: Some("markdown".to_owned()),
                    preview: Some("New summary".to_owned()),
                    run_id,
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: Some(ArtifactStatus::Ready),
                    title: Some("Revisioned artifact".to_owned()),
                }),
            ],
        )
        .await
        .expect("artifact revisions should append");

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
    let new_updated_at = new_at.to_rfc3339();
    assert_eq!(
        artifact.updated_at.as_deref(),
        Some(new_updated_at.as_str())
    );
    assert_eq!(artifact.revisions.len(), 2);
    assert_eq!(
        artifact.revisions[0].revision_id,
        new_revision_id.to_string()
    );
    assert_eq!(
        artifact.revisions[1].revision_id,
        old_revision_id.to_string()
    );
    let new_content_ref = artifact.revisions[0]
        .content_ref
        .as_ref()
        .expect("newest revision content ref")
        .clone();
    let old_content_ref = artifact.revisions[1]
        .content_ref
        .as_ref()
        .expect("oldest revision content ref")
        .clone();

    let newest_content = get_artifact_revision_content_with_runtime_state(
        GetArtifactRevisionContentRequest {
            conversation_id: session_id.to_string(),
            cursor: None,
            content_ref: new_content_ref,
            max_bytes: None,
        },
        &state,
    )
    .await
    .expect("artifact content evidence should read");
    assert!(newest_content.content.contains("new artifact body"));
    assert!(!newest_content
        .content
        .contains("sk-abcdefghijklmnopqrstuvwxyz"));
    assert_eq!(newest_content.content_type, "text/markdown");
    assert_eq!(newest_content.redaction_state, "redacted");

    let oldest_content = get_artifact_revision_content_with_runtime_state(
        GetArtifactRevisionContentRequest {
            conversation_id: session_id.to_string(),
            cursor: None,
            content_ref: old_content_ref,
            max_bytes: None,
        },
        &state,
    )
    .await
    .expect("old artifact content evidence should read");
    assert_eq!(oldest_content.content, "old artifact body");
    assert_eq!(oldest_content.content_type, "text/markdown");
}

#[tokio::test]
async fn get_artifact_revision_content_with_runtime_state_hides_evidence_ref_errors() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;

    let error = get_artifact_revision_content_with_runtime_state(
        GetArtifactRevisionContentRequest {
            conversation_id: session_id.to_string(),
            cursor: None,
            content_ref: "evidence:artifact-content:missing-ref".to_owned(),
            max_bytes: None,
        },
        &state,
    )
    .await
    .expect_err("missing artifact content ref should fail closed");

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");
    assert_eq!(error.message, "artifact content unavailable");
    assert!(!error.message.contains("missing-ref"));
    assert!(!error.message.contains("evidence:"));
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_uses_sanitized_content_refs_after_worktree_projection() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    let run_id = RunId::new();
    let revision_id = ArtifactRevisionId::new();
    let artifact_at = now();
    let content = bytes::Bytes::from_static(b"artifact after skipped delta");
    let content_hash = *blake3::hash(&content).as_bytes();
    let blob_store = FileBlobStore::open(
        state
            .workspace_root()
            .join(".jyowo")
            .join("runtime")
            .join("blobs"),
    )
    .expect("blob store opens");
    let blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            content.clone(),
            BlobMeta {
                content_type: Some("text/markdown".to_owned()),
                size: content.len() as u64,
                content_hash,
                created_at: artifact_at,
                retention: BlobRetention::SessionScoped(session_id),
            },
        )
        .await
        .expect("artifact blob writes");

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                    run_id,
                    message_id: MessageId::new(),
                    delta: DeltaChunk::ToolUseInputDelta {
                        tool_use_id: ToolUseId::new(),
                        delta: "{\"path\":\"src/lib.rs\"}".to_owned(),
                    },
                    at: artifact_at,
                }),
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    revision_id,
                    artifact_id: "artifact-worktree-first".to_owned(),
                    at: artifact_at,
                    blob_ref: Some(blob_ref),
                    content_hash: Some(content_hash.to_vec()),
                    kind: "markdown".to_owned(),
                    preview: Some("Preview".to_owned()),
                    run_id,
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Ready,
                    title: "Worktree first".to_owned(),
                }),
            ],
        )
        .await
        .expect("events should append");

    page_conversation_worktree_with_runtime_state(
        PageConversationWorktreeRequest {
            conversation_id: session_id.to_string(),
            page_cursor: None,
            direction: PageConversationWorktreeDirection::After,
            limit: Some(10),
        },
        &state,
    )
    .await
    .expect("worktree should register artifact evidence first");

    let payload = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("artifact listing should load sanitized evidence ref");

    let content_ref = payload.artifacts[0].revisions[0]
        .content_ref
        .as_ref()
        .expect("artifact content ref should be present");
    assert!(content_ref.starts_with("evidence:artifact-content-redacted:"));
    let content = get_artifact_revision_content_with_runtime_state(
        GetArtifactRevisionContentRequest {
            conversation_id: session_id.to_string(),
            cursor: None,
            content_ref: content_ref.clone(),
            max_bytes: None,
        },
        &state,
    )
    .await
    .expect("reused artifact content evidence should read");
    assert_eq!(content.content, "artifact after skipped delta");
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
