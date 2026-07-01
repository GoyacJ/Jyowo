#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

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
