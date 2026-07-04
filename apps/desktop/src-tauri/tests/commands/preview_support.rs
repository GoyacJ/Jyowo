#![allow(dead_code)]
#![allow(unused_imports)]

use super::support::*;
use super::*;

pub(crate) async fn append_user_message_attachment_for_preview(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    attachment_id: &str,
    name: &str,
    mime_type: &str,
    bytes: Vec<u8>,
    retention: BlobRetention,
) {
    append_user_message_attachment_for_preview_with_blob_mime(
        state,
        session_id,
        attachment_id,
        name,
        mime_type,
        mime_type,
        bytes,
        retention,
    )
    .await;
}

pub(crate) async fn append_user_message_attachment_for_preview_with_blob_mime(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    attachment_id: &str,
    name: &str,
    attachment_mime_type: &str,
    blob_mime_type: &str,
    bytes: Vec<u8>,
    retention: BlobRetention,
) {
    let size = bytes.len() as u64;
    let content_hash = *blake3::hash(&bytes).as_bytes();
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
            bytes::Bytes::from(bytes),
            BlobMeta {
                content_type: Some(blob_mime_type.to_owned()),
                size,
                content_hash,
                created_at: now(),
                retention,
            },
        )
        .await
        .expect("attachment blob writes");

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::UserMessageAppended(UserMessageAppendedEvent {
                run_id: RunId::new(),
                message_id: MessageId::new(),
                content: MessageContent::Text("attached file".to_owned()),
                metadata: MessageMetadata::default(),
                attachments: vec![ConversationAttachmentReference {
                    id: attachment_id.to_owned(),
                    name: name.to_owned(),
                    mime_type: attachment_mime_type.to_owned(),
                    size_bytes: size,
                    blob_ref,
                }],
                at: now(),
            })],
        )
        .await
        .expect("user message attachment event should append");
}

pub(crate) fn minimal_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

pub(crate) fn supported_preview_image_with_metadata(mime_type: &str, metadata: &[u8]) -> Vec<u8> {
    let rgba = [
        0xE5, 0x2B, 0x50, 0xFF, 0x1E, 0x88, 0xE5, 0xFF, 0xF9, 0xA8, 0x25, 0xFF, 0x12, 0x12, 0x12,
        0xFF,
    ];
    match mime_type {
        "image/jpeg" => {
            let rgb = rgba
                .chunks_exact(4)
                .flat_map(|pixel| pixel[..3].iter().copied())
                .collect::<Vec<_>>();
            let mut encoded = Vec::new();
            JpegEncoder::new(&mut encoded)
                .write_image(&rgb, 2, 2, ExtendedColorType::Rgb8)
                .expect("test JPEG encodes");
            jpeg_with_app_metadata(encoded, metadata)
        }
        "image/gif" => {
            let mut encoded = Vec::new();
            GifEncoder::new(&mut encoded)
                .encode(&rgba, 2, 2, ExtendedColorType::Rgba8)
                .expect("test GIF encodes");
            gif_with_comment_metadata(encoded, metadata)
        }
        "image/webp" => {
            let mut encoded = Vec::new();
            WebPEncoder::new_lossless(&mut encoded)
                .write_image(&rgba, 2, 2, ExtendedColorType::Rgba8)
                .expect("test WebP encodes");
            webp_with_exif_metadata(encoded, metadata)
        }
        "image/avif" => {
            let encoded = minimal_avif();
            if metadata.is_empty() {
                encoded
            } else {
                iso_bmff_with_free_metadata(encoded, metadata)
            }
        }
        _ => panic!("unsupported test MIME type: {mime_type}"),
    }
}

pub(crate) fn minimal_avif() -> Vec<u8> {
    general_purpose::STANDARD
        .decode(
            "AAAAIGZ0eXBhdmlmAAAAAGF2aWZtaWYxbWlhZk1BMUEAAADrbWV0YQAAAAAAAAAhaGRscgAAAAAAAAAAcGljdAAAAAAAAAAAAAAAAAAAAAAOcGl0bQAAAAAAAQAAAB5pbG9jAAAAAEQAAAEAAQAAAAEAAAETAAAAJAAAAChpaW5mAAAAAAABAAAAGmluZmUCAAAAAAEAAGF2MDFDb2xvcgAAAABqaXBycAAAAEtpcGNvAAAAFGlzcGUAAAAAAAAAQAAAAEAAAAAQcGl4aQAAAAADCAgIAAAADGF2MUOBIAAAAAAAE2NvbHJuY2x4AAEAAgAAgAAAABdpcG1hAAAAAAAAAAEAAQQBAoMEAAAALG1kYXQSAAoGOBV//YJAMhgQAAC0UbTwxPOBGQHm72pfRNB5F8X+BlQ=",
        )
        .expect("embedded AVIF fixture decodes")
}

pub(crate) fn avif_with_exif_metadata() -> Vec<u8> {
    general_purpose::STANDARD
        .decode(include_str!("../fixtures/avif-with-exif-metadata.b64").replace(['\n', '\r'], ""))
        .expect("embedded AVIF Exif fixture decodes")
}

pub(crate) fn jpeg_with_app_metadata(encoded: Vec<u8>, metadata: &[u8]) -> Vec<u8> {
    assert!(encoded.starts_with(&[0xFF, 0xD8]));
    let segment_len = u16::try_from(metadata.len() + 2).expect("test metadata fits JPEG segment");
    let mut output = Vec::with_capacity(encoded.len() + metadata.len() + 4);
    output.extend_from_slice(&encoded[..2]);
    output.extend_from_slice(&[0xFF, 0xE1]);
    output.extend_from_slice(&segment_len.to_be_bytes());
    output.extend_from_slice(metadata);
    output.extend_from_slice(&encoded[2..]);
    output
}

pub(crate) fn gif_with_comment_metadata(mut encoded: Vec<u8>, metadata: &[u8]) -> Vec<u8> {
    assert_eq!(encoded.last(), Some(&0x3B));
    let mut comment = vec![0x21, 0xFE];
    for chunk in metadata.chunks(255) {
        comment.push(u8::try_from(chunk.len()).expect("GIF comment chunk length fits"));
        comment.extend_from_slice(chunk);
    }
    comment.push(0);
    encoded.splice(encoded.len() - 1..encoded.len() - 1, comment);
    encoded
}

pub(crate) fn webp_with_exif_metadata(mut encoded: Vec<u8>, metadata: &[u8]) -> Vec<u8> {
    assert!(encoded.len() >= 12 && encoded.starts_with(b"RIFF") && &encoded[8..12] == b"WEBP");
    encoded.extend_from_slice(b"EXIF");
    encoded.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    encoded.extend_from_slice(metadata);
    if metadata.len() % 2 == 1 {
        encoded.push(0);
    }
    let riff_size = u32::try_from(encoded.len() - 8).expect("test WebP fits RIFF size");
    encoded[4..8].copy_from_slice(&riff_size.to_le_bytes());
    encoded
}

pub(crate) fn iso_bmff_with_free_metadata(mut encoded: Vec<u8>, metadata: &[u8]) -> Vec<u8> {
    assert!(encoded.len() >= 12 && &encoded[4..8] == b"ftyp");
    let box_size = u32::try_from(metadata.len() + 8).expect("test metadata fits BMFF box");
    encoded.extend_from_slice(&box_size.to_be_bytes());
    encoded.extend_from_slice(b"free");
    encoded.extend_from_slice(metadata);
    encoded
}

pub(crate) fn png_with_ancillary_chunk(chunk_type: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut png = minimal_png();
    let iend_offset = png
        .windows(4)
        .position(|window| window == b"IEND")
        .expect("minimal png has IEND")
        - 4;
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(&chunk_type);
    chunk.extend_from_slice(data);
    chunk.extend_from_slice(&[0, 0, 0, 0]);
    png.splice(iend_offset..iend_offset, chunk);
    png
}

pub(crate) fn png_with_dimensions(width: u32, height: u32) -> Vec<u8> {
    let mut png = minimal_png();
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    png
}

pub(crate) fn attachment_preview_data_url_bytes(data_url: &str) -> Vec<u8> {
    attachment_preview_data_url_bytes_with_mime(data_url, "image/png")
}

pub(crate) fn attachment_preview_data_url_bytes_with_mime(
    data_url: &str,
    mime_type: &str,
) -> Vec<u8> {
    let encoded = data_url
        .strip_prefix(&format!("data:{mime_type};base64,"))
        .expect("preview uses expected data URL MIME type");
    general_purpose::STANDARD
        .decode(encoded)
        .expect("preview data URL decodes")
}

pub(crate) fn detect_test_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1A\n") {
        return Some("image/png");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let major_brand = &bytes[8..12];
        if major_brand == b"avif" || major_brand == b"avis" {
            return Some("image/avif");
        }
    }
    None
}

pub(crate) async fn append_artifact_event_for_preview(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    artifact_id: &str,
    kind: &str,
    status: ArtifactStatus,
    blob: Option<(&str, Vec<u8>, SessionId)>,
) {
    let run_id = RunId::new();
    let (blob_ref, content_hash) = if let Some((content_type, bytes, retention_session_id)) = blob {
        let size = bytes.len() as u64;
        let content_hash = *blake3::hash(&bytes).as_bytes();
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
                bytes::Bytes::from(bytes),
                BlobMeta {
                    content_type: Some(content_type.to_owned()),
                    size,
                    content_hash,
                    created_at: now(),
                    retention: BlobRetention::SessionScoped(retention_session_id),
                },
            )
            .await
            .expect("blob writes");
        (Some(blob_ref), Some(content_hash.to_vec()))
    } else {
        (None, None)
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
                artifact_id: artifact_id.to_owned(),
                at: now(),
                blob_ref,
                content_hash,
                kind: kind.to_owned(),
                preview: Some("Generated artifact".to_owned()),
                run_id,
                session_id,
                source: ArtifactSource::Tool,
                source_message_id: None,
                source_tool_use_id: Some(ToolUseId::new()),
                status,
                title: "Generated artifact".to_owned(),
            })],
        )
        .await
        .expect("artifact event should append");
}
