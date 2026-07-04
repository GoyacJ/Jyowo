#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_returns_owned_image_data_url() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    let run_id = RunId::new();
    let image_bytes = minimal_png();
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
                revision_id: ArtifactRevisionId::new(),
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
        Some(("image/png", minimal_png(), session_id)),
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
            minimal_png(),
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
        Some(("text/plain", minimal_png(), session_id)),
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
    let image_bytes = minimal_png();
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
                revision_id: ArtifactRevisionId::new(),
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
async fn get_artifact_media_preview_with_runtime_state_strips_png_text_metadata() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-png-metadata",
        "image",
        ArtifactStatus::Ready,
        Some((
            "image/png",
            png_with_ancillary_chunk(
                *b"tEXt",
                b"path=/Users/goya/.jyowo/runtime/blobs/private.png token=secret-value",
            ),
            session_id,
        )),
    )
    .await;

    let payload = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-png-metadata".to_owned(),
        },
        &state,
    )
    .await
    .expect("PNG text metadata should be stripped");

    let preview = attachment_preview_data_url_bytes(&payload.data_url);
    assert_eq!(payload.mime_type, "image/png");
    assert!(!String::from_utf8_lossy(&preview).contains("/Users/goya"));
    assert!(!String::from_utf8_lossy(&preview).contains("secret-value"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_strips_jpeg_exif_metadata() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-jpeg-metadata",
        "image",
        ArtifactStatus::Ready,
        Some((
            "image/jpeg",
            supported_preview_image_with_metadata(
                "image/jpeg",
                b"Exif\0\0path=/Users/goya/.jyowo/runtime/blobs/private.jpg token=secret-value",
            ),
            session_id,
        )),
    )
    .await;

    let payload = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-jpeg-metadata".to_owned(),
        },
        &state,
    )
    .await
    .expect("JPEG EXIF metadata should be stripped by transcoding");

    let preview = attachment_preview_data_url_bytes(&payload.data_url);
    assert_eq!(payload.mime_type, "image/png");
    assert!(!String::from_utf8_lossy(&preview).contains("/Users/goya"));
    assert!(!String::from_utf8_lossy(&preview).contains("secret-value"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_strips_gif_comment_metadata() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-gif-metadata",
        "image",
        ArtifactStatus::Ready,
        Some((
            "image/gif",
            supported_preview_image_with_metadata(
                "image/gif",
                b"path=/Users/goya/.jyowo/runtime/blobs/private.gif token=secret-value",
            ),
            session_id,
        )),
    )
    .await;

    let payload = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-gif-metadata".to_owned(),
        },
        &state,
    )
    .await
    .expect("GIF comment metadata should be stripped by transcoding");

    let preview = attachment_preview_data_url_bytes(&payload.data_url);
    assert_eq!(payload.mime_type, "image/png");
    assert!(!String::from_utf8_lossy(&preview).contains("/Users/goya"));
    assert!(!String::from_utf8_lossy(&preview).contains("secret-value"));
}
