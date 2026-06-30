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
async fn get_artifact_media_preview_with_runtime_state_returns_owned_image_data_url() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    let run_id = RunId::new();
    let image_bytes = b"\x89PNG\r\n\x1A\npreview".to_vec();
    let content_hash = *blake3::hash(&image_bytes).as_bytes();
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
            bytes::Bytes::from(image_bytes.clone()),
            BlobMeta {
                content_type: Some("image/png".to_owned()),
                size: image_bytes.len() as u64,
                content_hash,
                created_at: now(),
                retention: BlobRetention::SessionScoped(session_id),
            },
        )
        .await
        .expect("image blob writes");

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                artifact_id: "artifact-image".to_owned(),
                at: now(),
                blob_ref: Some(blob_ref),
                content_hash: Some(content_hash.to_vec()),
                kind: "image".to_owned(),
                preview: Some("生成的图片".to_owned()),
                run_id,
                session_id,
                source: ArtifactSource::Tool,
                source_message_id: None,
                source_tool_use_id: Some(ToolUseId::new()),
                status: ArtifactStatus::Ready,
                title: "生成的图片".to_owned(),
            })],
        )
        .await
        .expect("artifact event should append");

    let payload = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-image".to_owned(),
        },
        &state,
    )
    .await
    .expect("image preview should load");

    assert_eq!(payload.mime_type, "image/png");
    assert_eq!(payload.size_bytes, image_bytes.len() as u64);
    assert!(payload.data_url.starts_with("data:image/png;base64,"));
    assert!(!payload.data_url.contains(".jyowo"));
    assert!(!payload.data_url.contains("artifact-image"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_accepts_image_mime_artifact_kind() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-image-mime-kind",
        "image/png",
        ArtifactStatus::Ready,
        Some((
            "image/png",
            b"\x89PNG\r\n\x1A\npreview".to_vec(),
            session_id,
        )),
    )
    .await;

    let payload = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-image-mime-kind".to_owned(),
        },
        &state,
    )
    .await
    .expect("image MIME kind artifact should preview");

    assert_eq!(payload.mime_type, "image/png");
    assert!(payload.data_url.starts_with("data:image/png;base64,"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_falls_back_to_detected_image_mime() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-image-unsafe-mime",
        "image",
        ArtifactStatus::Ready,
        Some((
            "image/png /tmp/provider-output https://provider.example/blob",
            b"\x89PNG\r\n\x1A\npreview".to_vec(),
            session_id,
        )),
    )
    .await;

    let payload = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-image-unsafe-mime".to_owned(),
        },
        &state,
    )
    .await
    .expect("valid image bytes should preview without trusting unsafe MIME");

    assert_eq!(payload.mime_type, "image/png");
    assert!(payload.data_url.starts_with("data:image/png;base64,"));
    assert!(!payload.data_url.contains("/tmp/provider-output"));
    assert!(!payload.data_url.contains("provider.example"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_safe_non_image_mime() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-image-text-mime",
        "image",
        ArtifactStatus::Ready,
        Some((
            "text/plain",
            b"\x89PNG\r\n\x1A\npreview".to_vec(),
            session_id,
        )),
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-image-text-mime".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("safe non-image declared MIME should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("only available for images"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_cross_session_blob() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    let other_session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    open_conversation_session(&state, other_session_id).await;
    let run_id = RunId::new();
    let image_bytes = b"\x89PNG\r\n\x1A\npreview".to_vec();
    let image_size = image_bytes.len() as u64;
    let content_hash = *blake3::hash(&image_bytes).as_bytes();
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
            bytes::Bytes::from(image_bytes),
            BlobMeta {
                content_type: Some("image/png".to_owned()),
                size: image_size,
                content_hash,
                created_at: now(),
                retention: BlobRetention::SessionScoped(other_session_id),
            },
        )
        .await
        .expect("image blob writes");

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                artifact_id: "artifact-image".to_owned(),
                at: now(),
                blob_ref: Some(blob_ref),
                content_hash: Some(content_hash.to_vec()),
                kind: "image".to_owned(),
                preview: Some("生成的图片".to_owned()),
                run_id,
                session_id,
                source: ArtifactSource::Tool,
                source_message_id: None,
                source_tool_use_id: Some(ToolUseId::new()),
                status: ArtifactStatus::Ready,
                title: "生成的图片".to_owned(),
            })],
        )
        .await
        .expect("artifact event should append");

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-image".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("cross-session blob should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_missing_artifact() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "missing-artifact".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("missing artifact should be rejected");

    assert_eq!(error.code, "NOT_FOUND");
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_not_ready_artifact() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-running",
        "image",
        ArtifactStatus::Running,
        None,
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-running".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("not-ready artifact should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("not ready"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_non_image_artifact() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-file",
        "file",
        ArtifactStatus::Ready,
        Some(("text/plain", b"hello".to_vec(), session_id)),
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-file".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("non-image artifact should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("only available for images"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_svg_image_blob() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-svg",
        "image",
        ArtifactStatus::Ready,
        Some((
            "image/svg+xml",
            br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#.to_vec(),
            session_id,
        )),
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-svg".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("svg image artifact should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("only available for images"));
    assert!(!error.message.contains(".jyowo"));
    assert!(!error.message.contains("artifact-svg"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_mislabeled_image_blob() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-mislabeled",
        "image",
        ArtifactStatus::Ready,
        Some((
            "image/png",
            br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#.to_vec(),
            session_id,
        )),
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-mislabeled".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("mislabeled image artifact should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("only available for images"));
    assert!(!error.message.contains(".jyowo"));
    assert!(!error.message.contains("artifact-mislabeled"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_too_large_image_blob() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-large",
        "image",
        ArtifactStatus::Ready,
        Some(("image/png", vec![0; 10 * 1024 * 1024 + 1], session_id)),
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-large".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("too-large image artifact should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("too large"));
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_returns_current_conversation_image_data_url(
) {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    let image_bytes = minimal_png();
    append_user_message_attachment_for_preview(
        &state,
        session_id,
        "attachment-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "reference.png",
        "image/png",
        image_bytes.clone(),
        BlobRetention::TenantScoped,
    )
    .await;

    let payload = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id:
                "attachment-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_owned(),
        },
        &state,
    )
    .await
    .expect("image attachment preview should load");

    assert_eq!(payload.mime_type, "image/png");
    assert_eq!(payload.size_bytes, image_bytes.len() as u64);
    assert!(payload.data_url.starts_with("data:image/png;base64,"));
    assert!(!payload.data_url.contains(".jyowo"));
    assert!(!payload.data_url.contains("/Users/"));
    assert!(!payload.data_url.contains("attachment-aaaaaaaa"));
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_strips_png_metadata() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    let image_bytes = png_with_ancillary_chunk(
        *b"tEXt",
        b"path=/Users/goya/.jyowo/runtime/blobs/private.png",
    );
    append_user_message_attachment_for_preview(
        &state,
        session_id,
        "attachment-7777777777777777777777777777777777777777777777777777777777777777",
        "metadata.png",
        "image/png",
        image_bytes,
        BlobRetention::TenantScoped,
    )
    .await;

    let payload = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id:
                "attachment-7777777777777777777777777777777777777777777777777777777777777777"
                    .to_owned(),
        },
        &state,
    )
    .await
    .expect("metadata-bearing PNG should be sanitized");

    let sanitized = attachment_preview_data_url_bytes(&payload.data_url);
    assert!(sanitized.starts_with(b"\x89PNG\r\n\x1A\n"));
    assert!(!sanitized.windows(4).any(|window| window == b"tEXt"));
    assert!(!String::from_utf8_lossy(&sanitized).contains("/Users/goya"));
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_rejects_png_declaring_huge_dimensions() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_user_message_attachment_for_preview(
        &state,
        session_id,
        "attachment-9999999999999999999999999999999999999999999999999999999999999999",
        "huge.png",
        "image/png",
        png_with_dimensions(100_000, 100_000),
        BlobRetention::TenantScoped,
    )
    .await;

    let error = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id:
                "attachment-9999999999999999999999999999999999999999999999999999999999999999"
                    .to_owned(),
        },
        &state,
    )
    .await
    .expect_err("PNG dimensions must be bounded before returning a preview");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("too large"));
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_accepts_supported_image_formats_as_safe_png(
) {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;

    let cases = [
        (
            "jpeg",
            "a",
            "image/jpeg",
            supported_preview_image_with_metadata("image/jpeg", b"path=/Users/goya/.jyowo/jpeg"),
            "image/png",
        ),
        (
            "gif",
            "b",
            "image/gif",
            supported_preview_image_with_metadata("image/gif", b"path=/Users/goya/.jyowo/gif"),
            "image/png",
        ),
        (
            "webp",
            "c",
            "image/webp",
            supported_preview_image_with_metadata("image/webp", b"path=/Users/goya/.jyowo/webp"),
            "image/png",
        ),
        (
            "avif",
            "d",
            "image/avif",
            supported_preview_image_with_metadata("image/avif", b""),
            "image/avif",
        ),
    ];

    for (suffix, id_hex, mime_type, image_bytes, expected_preview_mime_type) in cases {
        let attachment_id = format!("attachment-{}", id_hex.repeat(64));
        append_user_message_attachment_for_preview(
            &state,
            session_id,
            &attachment_id,
            &format!("preview.{suffix}"),
            mime_type,
            image_bytes,
            BlobRetention::TenantScoped,
        )
        .await;

        let payload = get_attachment_media_preview_with_runtime_state(
            GetAttachmentMediaPreviewRequest {
                conversation_id: session_id.to_string(),
                attachment_id: attachment_id.clone(),
            },
            &state,
        )
        .await
        .expect("supported image attachment should return a safe preview");

        assert_eq!(payload.mime_type, expected_preview_mime_type);
        assert!(payload
            .data_url
            .starts_with(&format!("data:{expected_preview_mime_type};base64,")));
        assert!(!payload.data_url.contains(".jyowo"));
        assert!(!payload.data_url.contains("/Users/"));
        assert!(!payload.data_url.contains(&attachment_id));
        let sanitized = attachment_preview_data_url_bytes_with_mime(
            &payload.data_url,
            expected_preview_mime_type,
        );
        assert_eq!(payload.size_bytes, sanitized.len() as u64);
        assert_eq!(
            detect_test_image_mime(&sanitized),
            Some(expected_preview_mime_type)
        );
        assert!(!String::from_utf8_lossy(&sanitized).contains("/Users/goya"));
    }
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_rejects_avif_with_unsafe_metadata() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_user_message_attachment_for_preview(
        &state,
        session_id,
        "attachment-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        "unsafe.avif",
        "image/avif",
        supported_preview_image_with_metadata("image/avif", b"path=/Users/goya/.jyowo/avif"),
        BlobRetention::TenantScoped,
    )
    .await;

    let error = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id:
                "attachment-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                    .to_owned(),
        },
        &state,
    )
    .await
    .expect_err("AVIF metadata with private paths should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(
        error.message.contains("unsafe metadata"),
        "unexpected error message: {}",
        error.message
    );
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_rejects_avif_with_exif_metadata() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_user_message_attachment_for_preview(
        &state,
        session_id,
        "attachment-ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "exif.avif",
        "image/avif",
        avif_with_exif_metadata(),
        BlobRetention::TenantScoped,
    )
    .await;

    let error = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id:
                "attachment-ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                    .to_owned(),
        },
        &state,
    )
    .await
    .expect_err("AVIF Exif metadata should be rejected before preview");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(
        error.message.contains("unsafe metadata"),
        "unexpected error message: {}",
        error.message
    );
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_rejects_missing_attachment() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;

    let error = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id:
                "attachment-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    .to_owned(),
        },
        &state,
    )
    .await
    .expect_err("missing attachment should be rejected");

    assert_eq!(error.code, "NOT_FOUND");
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_rejects_non_image_attachment() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_user_message_attachment_for_preview(
        &state,
        session_id,
        "attachment-cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "notes.txt",
        "text/plain",
        b"hello".to_vec(),
        BlobRetention::TenantScoped,
    )
    .await;

    let error = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id:
                "attachment-cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                    .to_owned(),
        },
        &state,
    )
    .await
    .expect_err("non-image attachment should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("only available for images"));
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_rejects_mismatched_image_mime() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_user_message_attachment_for_preview_with_blob_mime(
        &state,
        session_id,
        "attachment-ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "mismatch.png",
        "image/png",
        "image/jpeg",
        minimal_png(),
        BlobRetention::TenantScoped,
    )
    .await;

    let error = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id:
                "attachment-ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                    .to_owned(),
        },
        &state,
    )
    .await
    .expect_err("mismatched attachment MIME should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("only available for images"));
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_rejects_unsafe_image_pixels_or_payload() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_user_message_attachment_for_preview(
        &state,
        session_id,
        "attachment-9999999999999999999999999999999999999999999999999999999999999999",
        "unsafe-metadata.png",
        "image/png",
        b"\x89PNG\r\n\x1A\ntext:/Users/goya/.jyowo/runtime/blobs/private.png token=secret-value"
            .to_vec(),
        BlobRetention::TenantScoped,
    )
    .await;

    let error = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id:
                "attachment-9999999999999999999999999999999999999999999999999999999999999999"
                    .to_owned(),
        },
        &state,
    )
    .await
    .expect_err("unsafe image payload should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("unsafe metadata") || error.message.contains("malformed"));
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_strips_attachment_id_in_png_metadata() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    let attachment_id =
        "attachment-8888888888888888888888888888888888888888888888888888888888888888";
    open_conversation_session(&state, session_id).await;
    let image_bytes = png_with_ancillary_chunk(*b"iTXt", attachment_id.as_bytes());
    append_user_message_attachment_for_preview(
        &state,
        session_id,
        attachment_id,
        "attachment-id-metadata.png",
        "image/png",
        image_bytes,
        BlobRetention::TenantScoped,
    )
    .await;

    let payload = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id: attachment_id.to_owned(),
        },
        &state,
    )
    .await
    .expect("attachment id in PNG metadata should be stripped");

    let sanitized = attachment_preview_data_url_bytes(&payload.data_url);
    assert!(!String::from_utf8_lossy(&sanitized).contains(attachment_id));
    assert!(!sanitized.windows(4).any(|window| window == b"iTXt"));
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_rejects_other_conversation_attachment() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    let other_session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    open_conversation_session(&state, other_session_id).await;
    append_user_message_attachment_for_preview(
        &state,
        other_session_id,
        "attachment-dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        "other.png",
        "image/png",
        minimal_png(),
        BlobRetention::TenantScoped,
    )
    .await;

    let error = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id:
                "attachment-dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                    .to_owned(),
        },
        &state,
    )
    .await
    .expect_err("attachment from another conversation should be rejected");

    assert_eq!(error.code, "NOT_FOUND");
}

#[tokio::test]
async fn get_attachment_media_preview_with_runtime_state_rejects_other_session_scoped_blob() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    let other_session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    open_conversation_session(&state, other_session_id).await;
    append_user_message_attachment_for_preview(
        &state,
        session_id,
        "attachment-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        "cross-session.png",
        "image/png",
        minimal_png(),
        BlobRetention::SessionScoped(other_session_id),
    )
    .await;

    let error = get_attachment_media_preview_with_runtime_state(
        GetAttachmentMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            attachment_id:
                "attachment-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                    .to_owned(),
        },
        &state,
    )
    .await
    .expect_err("other session scoped blob should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
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

#[tokio::test]
async fn list_reference_candidates_includes_workspace_files() {
    let workspace = unique_workspace("reference-candidates");
    let file_path = workspace.join("apps/desktop/src-tauri/src/commands/mod.rs");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    std::fs::write(&file_path, "fn main() {}").unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;
    register_test_skill(&state, "shell-state", "Shell state");
    register_test_tool(&state, "list_dir", "List directory");
    save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &state,
    )
    .await
    .expect("mcp server should register");

    let payload = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: state.default_conversation_id().to_string(),
        },
        &state,
    )
    .await
    .expect("reference candidates should load");

    assert!(payload.files.iter().any(|candidate| {
        candidate.path.as_deref() == Some("apps/desktop/src-tauri/src/commands/mod.rs")
    }));
    assert!(payload
        .skills
        .iter()
        .any(|candidate| candidate.id.as_deref() == Some("shell-state")));
    assert!(payload
        .tools
        .iter()
        .any(|candidate| candidate.id.as_deref() == Some("mcp__stdio__echo")));
    assert!(payload
        .mcp_servers
        .iter()
        .any(|candidate| candidate.id.as_deref() == Some("stdio")));
}

#[tokio::test]
async fn list_reference_candidates_accepts_conversation_beyond_summary_page() {
    let state = runtime_state_with_harness().await;
    let file_path = state.workspace_root().join("apps/desktop/src/main.tsx");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    std::fs::write(&file_path, "export {}").unwrap();
    let requested_session_id = SessionId::new();
    open_conversation_session(&state, requested_session_id).await;
    for _ in 0..60 {
        open_conversation_session(&state, SessionId::new()).await;
    }

    let payload = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: requested_session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("reference candidates should load for existing conversations beyond summaries");

    assert!(payload
        .files
        .iter()
        .any(|candidate| candidate.path.as_deref() == Some("apps/desktop/src/main.tsx")));
}
