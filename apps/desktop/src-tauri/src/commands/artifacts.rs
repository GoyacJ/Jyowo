#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::automations::*;
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
use super::contracts::*;
#[allow(unused_imports)]
use super::conversations::*;
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
use super::evals::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
use super::memory::*;
#[allow(unused_imports)]
use super::plugins::*;
#[allow(unused_imports)]
use super::providers::*;
#[allow(unused_imports)]
use super::runtime::*;
#[allow(unused_imports)]
use super::skills::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;

pub async fn list_artifacts_with_runtime_state(
    request: ListArtifactsRequest,
    state: &DesktopRuntimeState,
) -> Result<ListArtifactsResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    collect_artifacts_from_runtime_state(state, session_id).await
}

pub async fn get_artifact_media_preview_with_runtime_state(
    request: GetArtifactMediaPreviewRequest,
    state: &DesktopRuntimeState,
) -> Result<GetArtifactMediaPreviewResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("artifactId", &request.artifact_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    let record = find_artifact_media_record(
        state,
        session_id,
        &request.artifact_id,
        request.revision_id.as_deref(),
    )
    .await?;
    if !matches!(
        record.status,
        Some(jyowo_harness_sdk::ext::ArtifactStatus::Ready)
    ) {
        return Err(invalid_payload(
            "artifact image preview is not ready".to_owned(),
        ));
    }
    if !is_preview_image_artifact_kind(record.kind.as_deref()) {
        return Err(invalid_payload(
            "artifact media preview is only available for images".to_owned(),
        ));
    }
    let blob_ref = record.blob_ref.ok_or_else(|| {
        runtime_operation_failed("artifact image preview is unavailable".to_owned())
    })?;
    read_artifact_image_blob_preview(state, session_id, &request.artifact_id, &blob_ref).await
}

pub async fn get_attachment_media_preview_with_runtime_state(
    request: GetAttachmentMediaPreviewRequest,
    state: &DesktopRuntimeState,
) -> Result<GetAttachmentMediaPreviewResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("attachmentId", &request.attachment_id)?;
    ensure_attachment_id(&request.attachment_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    let attachment =
        find_attachment_media_record(state, session_id, &request.attachment_id).await?;
    let declared_attachment_mime =
        safe_preview_image_mime(&attachment.mime_type).ok_or_else(|| {
            invalid_payload("attachment media preview is only available for images".to_owned())
        })?;
    read_attachment_image_blob_preview(
        state,
        session_id,
        &request.attachment_id,
        &attachment.blob_ref,
        declared_attachment_mime,
    )
    .await
}

pub(crate) async fn collect_artifacts_from_runtime_state(
    state: &DesktopRuntimeState,
    session_id: SessionId,
) -> Result<ListArtifactsResponse, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Listing artifacts requires the runtime conversation facade.",
        ));
    };
    if !harness
        .conversation_session_exists(state.conversation_session_options(session_id))
        .await
        .map_err(|error| runtime_operation_failed(error.to_string()))?
    {
        return Err(not_found(format!("conversation not found: {session_id}")));
    }
    let redactor = DefaultRedactor::default();
    let mut after_event_id = None;
    let mut artifacts_by_id = BTreeMap::<String, ArtifactSummaryPayload>::new();
    let mut has_artifact_content_blob = false;
    let mut order = Vec::<String>::new();

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: state.conversation_session_options(session_id),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|_| runtime_operation_failed("artifact read failed".to_owned()))?;
        if page.events.is_empty() {
            break;
        }

        for envelope in page.events {
            has_artifact_content_blob |=
                artifact_event_has_content_blob(&envelope.payload, session_id);
            project_artifact_event(
                envelope.payload,
                session_id,
                &redactor,
                &mut artifacts_by_id,
                &mut order,
            );
        }

        after_event_id = page.next_event_id;
    }

    let content_refs = if has_artifact_content_blob {
        collect_artifact_content_refs_from_evidence(&harness, &session_id.to_string()).await?
    } else {
        BTreeMap::new()
    };
    for artifact in artifacts_by_id.values_mut() {
        for revision in &mut artifact.revisions {
            if let Some(content_ref) =
                content_refs.get(&(artifact.id.clone(), revision.revision_id.clone()))
            {
                revision.content_ref = Some(content_ref.clone());
            }
        }
        artifact
            .revisions
            .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    }

    let mut artifacts = order
        .into_iter()
        .filter_map(|artifact_id| artifacts_by_id.remove(&artifact_id))
        .collect::<Vec<_>>();
    artifacts.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(ListArtifactsResponse { artifacts })
}

pub(crate) fn artifact_event_has_content_blob(event: &Event, session_id: SessionId) -> bool {
    match event {
        Event::ArtifactCreated(event) => event.session_id == session_id && event.blob_ref.is_some(),
        Event::ArtifactUpdated(event) => event.session_id == session_id && event.blob_ref.is_some(),
        _ => false,
    }
}

pub(crate) async fn collect_artifact_content_refs_from_evidence(
    harness: &Harness,
    conversation_id: &str,
) -> Result<BTreeMap<(String, String), String>, CommandErrorPayload> {
    let mut refs = BTreeMap::<(String, String), String>::new();
    let evidence_refs = harness
        .list_artifact_content_evidence_refs(TenantId::SINGLE, conversation_id)
        .await
        .map_err(|_| runtime_operation_failed("artifact read failed".to_owned()))?;
    for evidence_ref in evidence_refs {
        refs.insert(
            (evidence_ref.artifact_id, evidence_ref.revision_id),
            evidence_ref.content_ref.to_string(),
        );
    }

    Ok(refs)
}

#[derive(Debug, Clone)]
pub(crate) struct ArtifactMediaRecord {
    blob_ref: Option<BlobRef>,
    kind: Option<String>,
    status: Option<jyowo_harness_sdk::ext::ArtifactStatus>,
}

pub(crate) async fn find_artifact_media_record(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    artifact_id: &str,
    revision_id: Option<&str>,
) -> Result<ArtifactMediaRecord, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading artifact media requires the runtime conversation facade.",
        ));
    };
    if !harness
        .conversation_session_exists(state.conversation_session_options(session_id))
        .await
        .map_err(|error| runtime_operation_failed(error.to_string()))?
    {
        return Err(not_found(format!("conversation not found: {session_id}")));
    }

    let mut after_event_id = None;
    let mut record: Option<ArtifactMediaRecord> = None;
    let mut current_kind: Option<String> = None;
    let mut current_status: Option<jyowo_harness_sdk::ext::ArtifactStatus> = None;
    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: state.conversation_session_options(session_id),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|_| runtime_operation_failed("artifact media read failed".to_owned()))?;
        if page.events.is_empty() {
            break;
        }

        for envelope in page.events {
            match envelope.payload {
                Event::ArtifactCreated(event) => {
                    if event.session_id == session_id && event.artifact_id == artifact_id {
                        let event_revision_id = event.revision_id.to_string();
                        current_kind = Some(event.kind.clone());
                        current_status = Some(event.status);
                        if artifact_revision_media_candidate_matches(
                            revision_id,
                            &event_revision_id,
                        ) {
                            record = Some(ArtifactMediaRecord {
                                blob_ref: event.blob_ref,
                                kind: Some(event.kind),
                                status: Some(event.status),
                            });
                        }
                    }
                }
                Event::ArtifactUpdated(event) => {
                    if event.session_id != session_id || event.artifact_id != artifact_id {
                        continue;
                    }
                    let event_revision_id = event.revision_id.to_string();
                    let event_kind = event.kind.or_else(|| current_kind.clone());
                    let event_status = event.status.or(current_status);
                    current_kind = event_kind.clone();
                    current_status = event_status;

                    if artifact_revision_media_candidate_matches(revision_id, &event_revision_id) {
                        let entry = record.get_or_insert_with(|| ArtifactMediaRecord {
                            blob_ref: None,
                            kind: None,
                            status: None,
                        });
                        if let Some(blob_ref) = event.blob_ref {
                            entry.blob_ref = Some(blob_ref);
                        }
                        if let Some(kind) = event_kind {
                            entry.kind = Some(kind);
                        }
                        if let Some(status) = event_status {
                            entry.status = Some(status);
                        }
                    }
                }
                _ => {}
            }
        }

        after_event_id = page.next_event_id;
    }

    record.ok_or_else(|| not_found("artifact not found".to_owned()))
}

fn artifact_revision_media_candidate_matches(
    requested_revision_id: Option<&str>,
    event_revision_id: &str,
) -> bool {
    match requested_revision_id {
        Some(requested_revision_id) => requested_revision_id == event_revision_id,
        None => true,
    }
}

pub(crate) async fn find_attachment_media_record(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    attachment_id: &str,
) -> Result<ConversationAttachmentReference, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading attachment media requires the runtime conversation facade.",
        ));
    };
    if !harness
        .conversation_session_exists(state.conversation_session_options(session_id))
        .await
        .map_err(|error| runtime_operation_failed(error.to_string()))?
    {
        return Err(not_found(format!("conversation not found: {session_id}")));
    }

    let mut after_event_id = None;
    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: state.conversation_session_options(session_id),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|_| runtime_operation_failed("attachment media read failed".to_owned()))?;
        if page.events.is_empty() {
            break;
        }

        for envelope in page.events {
            let Event::UserMessageAppended(event) = envelope.payload else {
                continue;
            };
            for attachment in event.attachments {
                if attachment.id == attachment_id {
                    return Ok(attachment);
                }
            }
        }

        after_event_id = page.next_event_id;
    }

    Err(not_found("attachment not found".to_owned()))
}

pub(crate) async fn read_artifact_image_blob_preview(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    artifact_id: &str,
    blob_ref: &BlobRef,
) -> Result<GetArtifactMediaPreviewResponse, CommandErrorPayload> {
    let blob_store = FileBlobStore::open(
        state
            .workspace_root()
            .join(".jyowo")
            .join("runtime")
            .join("blobs"),
    )
    .map_err(|_| runtime_operation_failed("artifact image preview is unavailable".to_owned()))?;
    let meta = blob_store
        .head(TenantId::SINGLE, blob_ref)
        .await
        .map_err(|_| runtime_operation_failed("artifact image preview is unavailable".to_owned()))?
        .ok_or_else(|| {
            runtime_operation_failed("artifact image preview is unavailable".to_owned())
        })?;
    if meta.retention != BlobRetention::SessionScoped(session_id) {
        return Err(invalid_payload(
            "artifact image preview is unavailable for this conversation".to_owned(),
        ));
    }
    let declared_content_type = meta
        .content_type
        .as_deref()
        .or(blob_ref.content_type.as_deref());
    let declared_mime_type = match declared_content_type.and_then(declared_mime_token) {
        Some(mime_type) => match safe_preview_image_mime(mime_type) {
            Some(image_mime_type) => Some(image_mime_type.to_owned()),
            None if safe_artifact_mime_type(mime_type).is_some()
                || mime_type.starts_with("image/") =>
            {
                return Err(invalid_payload(
                    "artifact media preview is only available for images".to_owned(),
                ));
            }
            None => None,
        },
        None => None,
    };
    let size_bytes = meta.size;
    if size_bytes > MAX_ARTIFACT_MEDIA_PREVIEW_BYTES {
        return Err(invalid_payload(
            "artifact image preview is too large".to_owned(),
        ));
    }

    let mut stream = blob_store
        .get(TenantId::SINGLE, blob_ref)
        .await
        .map_err(|_| {
            runtime_operation_failed("artifact image preview is unavailable".to_owned())
        })?;
    let mut bytes = Vec::with_capacity(size_bytes.min(MAX_ARTIFACT_MEDIA_PREVIEW_BYTES) as usize);
    while let Some(chunk) = stream.next().await {
        let next_len = bytes.len().saturating_add(chunk.len());
        if u64::try_from(next_len).unwrap_or(u64::MAX) > MAX_ARTIFACT_MEDIA_PREVIEW_BYTES {
            return Err(invalid_payload(
                "artifact image preview is too large".to_owned(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let detected_mime = detect_preview_image_mime(&bytes).ok_or_else(|| {
        invalid_payload("artifact media preview is only available for images".to_owned())
    })?;
    if declared_mime_type
        .as_deref()
        .is_some_and(|mime_type| mime_type != detected_mime)
    {
        return Err(invalid_payload(
            "artifact media preview is only available for images".to_owned(),
        ));
    }
    let (sanitized_bytes, mime_type) =
        sanitize_artifact_preview_image(&bytes, detected_mime, artifact_id)?;
    let size_bytes = sanitized_bytes.len() as u64;

    Ok(GetArtifactMediaPreviewResponse {
        data_url: format!(
            "data:{mime_type};base64,{}",
            general_purpose::STANDARD.encode(sanitized_bytes)
        ),
        mime_type: mime_type.to_owned(),
        size_bytes,
    })
}

pub(crate) async fn read_attachment_image_blob_preview(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    attachment_id: &str,
    blob_ref: &BlobRef,
    declared_attachment_mime: &str,
) -> Result<GetAttachmentMediaPreviewResponse, CommandErrorPayload> {
    let blob_store = FileBlobStore::open(
        state
            .workspace_root()
            .join(".jyowo")
            .join("runtime")
            .join("blobs"),
    )
    .map_err(|_| runtime_operation_failed("attachment image preview is unavailable".to_owned()))?;
    let meta = blob_store
        .head(TenantId::SINGLE, blob_ref)
        .await
        .map_err(|_| {
            runtime_operation_failed("attachment image preview is unavailable".to_owned())
        })?
        .ok_or_else(|| {
            runtime_operation_failed("attachment image preview is unavailable".to_owned())
        })?;
    match meta.retention {
        BlobRetention::TenantScoped => {}
        BlobRetention::SessionScoped(retention_session_id)
            if retention_session_id == session_id => {}
        _ => {
            return Err(invalid_payload(
                "attachment image preview is unavailable for this conversation".to_owned(),
            ));
        }
    }
    let declared_content_type = meta
        .content_type
        .as_deref()
        .or(blob_ref.content_type.as_deref());
    if let Some(mime_type) = declared_content_type.and_then(declared_mime_token) {
        match safe_preview_image_mime(mime_type) {
            Some(image_mime_type) if image_mime_type == declared_attachment_mime => {}
            _ => {
                return Err(invalid_payload(
                    "attachment media preview is only available for images".to_owned(),
                ));
            }
        }
    }
    let size_bytes = meta.size;
    if size_bytes > MAX_ATTACHMENT_BYTES {
        return Err(invalid_payload(
            "attachment image preview is too large".to_owned(),
        ));
    }

    let mut stream = blob_store
        .get(TenantId::SINGLE, blob_ref)
        .await
        .map_err(|_| {
            runtime_operation_failed("attachment image preview is unavailable".to_owned())
        })?;
    let mut bytes = Vec::with_capacity(size_bytes.min(MAX_ATTACHMENT_BYTES) as usize);
    while let Some(chunk) = stream.next().await {
        let next_len = bytes.len().saturating_add(chunk.len());
        if u64::try_from(next_len).unwrap_or(u64::MAX) > MAX_ATTACHMENT_BYTES {
            return Err(invalid_payload(
                "attachment image preview is too large".to_owned(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let detected_mime = detect_preview_image_mime(&bytes).ok_or_else(|| {
        invalid_payload("attachment media preview is only available for images".to_owned())
    })?;
    if detected_mime != declared_attachment_mime {
        return Err(invalid_payload(
            "attachment media preview is only available for images".to_owned(),
        ));
    }
    let (sanitized_bytes, mime_type) =
        sanitize_attachment_preview_image(&bytes, detected_mime, attachment_id)?;
    let size_bytes = sanitized_bytes.len() as u64;

    Ok(GetAttachmentMediaPreviewResponse {
        data_url: format!(
            "data:{mime_type};base64,{}",
            general_purpose::STANDARD.encode(sanitized_bytes)
        ),
        mime_type: mime_type.to_owned(),
        size_bytes,
    })
}

pub(crate) fn detect_preview_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1A\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let major_brand = &bytes[8..12];
        if major_brand == b"avif" || major_brand == b"avis" {
            return Some("image/avif");
        }
        if bytes
            .get(16..)
            .unwrap_or_default()
            .chunks_exact(4)
            .any(|brand| brand == b"avif" || brand == b"avis")
        {
            return Some("image/avif");
        }
    }
    None
}

pub(crate) fn safe_preview_image_mime(value: &str) -> Option<&'static str> {
    let mime = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        "image/avif" => Some("image/avif"),
        _ => None,
    }
}

pub(crate) fn ensure_preview_image_bytes_public(
    bytes: &[u8],
    attachment_id: &str,
) -> Result<(), CommandErrorPayload> {
    for text in printable_ascii_runs(bytes, 16) {
        if preview_text_contains_unsafe_payload(&text, attachment_id) {
            return Err(invalid_payload(
                "attachment image preview contains unsafe metadata".to_owned(),
            ));
        }
    }

    Ok(())
}

pub(crate) fn sanitize_attachment_preview_image(
    bytes: &[u8],
    detected_mime: &str,
    attachment_id: &str,
) -> Result<(Vec<u8>, &'static str), CommandErrorPayload> {
    match detected_mime {
        "image/png" => Ok((
            sanitize_attachment_preview_png(bytes, attachment_id)?,
            "image/png",
        )),
        "image/jpeg" | "image/gif" | "image/webp" => Ok((
            transcode_attachment_preview_to_png(bytes, detected_mime, attachment_id)?,
            "image/png",
        )),
        "image/avif" => Ok((
            sanitize_attachment_preview_avif(bytes, attachment_id)?,
            "image/avif",
        )),
        _ => Err(invalid_payload(
            "attachment media preview is only available for images".to_owned(),
        )),
    }
}

pub(crate) fn sanitize_artifact_preview_image(
    bytes: &[u8],
    detected_mime: &str,
    artifact_id: &str,
) -> Result<(Vec<u8>, &'static str), CommandErrorPayload> {
    sanitize_attachment_preview_image(bytes, detected_mime, artifact_id).map_err(|error| {
        let message = error
            .message
            .replace("attachment media preview", "artifact media preview")
            .replace("attachment image preview", "artifact image preview");
        CommandErrorPayload {
            code: error.code,
            message,
        }
    })
}

pub(crate) fn transcode_attachment_preview_to_png(
    bytes: &[u8],
    detected_mime: &str,
    attachment_id: &str,
) -> Result<Vec<u8>, CommandErrorPayload> {
    let format = preview_image_format(detected_mime).ok_or_else(|| {
        invalid_payload("attachment media preview is only available for images".to_owned())
    })?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(attachment_preview_decode_limits());
    let image = reader
        .decode()
        .map_err(|_| invalid_payload("attachment image preview is malformed".to_owned()))?;
    let mut encoded = Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|_| invalid_payload("attachment image preview is malformed".to_owned()))?;
    let sanitized = encoded.into_inner();
    if sanitized.len() as u64 > MAX_ATTACHMENT_BYTES {
        return Err(invalid_payload(
            "attachment image preview is too large".to_owned(),
        ));
    }
    ensure_preview_image_bytes_public(&sanitized, attachment_id)?;

    Ok(sanitized)
}

pub(crate) fn sanitize_attachment_preview_avif(
    bytes: &[u8],
    attachment_id: &str,
) -> Result<Vec<u8>, CommandErrorPayload> {
    let info = oxideav_avif::inspect(bytes)
        .map_err(|_| invalid_payload("attachment image preview is malformed".to_owned()))?;
    validate_attachment_preview_dimensions(info.width, info.height)?;
    if info.has_descriptive_metadata() {
        return Err(invalid_payload(
            "attachment image preview contains unsafe metadata".to_owned(),
        ));
    }
    // AVIF stays in its original container because this path uses pure Rust
    // container inspection rather than a system AV1 decoder. Descriptive
    // metadata and unsafe printable payloads fail closed before bytes return.
    ensure_preview_image_bytes_public(bytes, attachment_id)?;

    Ok(bytes.to_vec())
}

pub(crate) fn preview_image_format(mime_type: &str) -> Option<ImageFormat> {
    match mime_type {
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}

pub(crate) fn attachment_preview_decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_ATTACHMENT_PREVIEW_DIMENSION);
    limits.max_image_height = Some(MAX_ATTACHMENT_PREVIEW_DIMENSION);
    limits.max_alloc = Some(MAX_ATTACHMENT_PREVIEW_DECODED_BYTES);
    limits
}

pub(crate) fn sanitize_attachment_preview_png(
    bytes: &[u8],
    attachment_id: &str,
) -> Result<Vec<u8>, CommandErrorPayload> {
    let Some("image/png") = detect_preview_image_mime(bytes) else {
        return Err(invalid_payload(
            "attachment image preview is unavailable for this image type".to_owned(),
        ));
    };

    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1A\n";
    let mut cursor = PNG_SIGNATURE.len();
    let mut sanitized = Vec::with_capacity(bytes.len());
    sanitized.extend_from_slice(PNG_SIGNATURE);
    let mut saw_ihdr = false;
    let mut saw_idat = false;
    let mut saw_iend = false;

    while cursor < bytes.len() {
        let Some(length_bytes) = bytes.get(cursor..cursor + 4) else {
            return Err(invalid_payload(
                "attachment image preview is malformed".to_owned(),
            ));
        };
        let length = u32::from_be_bytes([
            length_bytes[0],
            length_bytes[1],
            length_bytes[2],
            length_bytes[3],
        ]) as usize;
        let chunk_start = cursor;
        let chunk_type_start = cursor + 4;
        let chunk_data_start = chunk_type_start + 4;
        let chunk_crc_start = chunk_data_start.saturating_add(length);
        let chunk_end = chunk_crc_start.saturating_add(4);
        let Some(chunk_type) = bytes.get(chunk_type_start..chunk_data_start) else {
            return Err(invalid_payload(
                "attachment image preview is malformed".to_owned(),
            ));
        };
        let Some(chunk) = bytes.get(chunk_start..chunk_end) else {
            return Err(invalid_payload(
                "attachment image preview is malformed".to_owned(),
            ));
        };

        match chunk_type {
            b"IHDR" if !saw_ihdr && cursor == PNG_SIGNATURE.len() && length == 13 => {
                let Some(dimensions) = bytes.get(chunk_data_start..chunk_data_start + 8) else {
                    return Err(invalid_payload(
                        "attachment image preview is malformed".to_owned(),
                    ));
                };
                let width = u32::from_be_bytes([
                    dimensions[0],
                    dimensions[1],
                    dimensions[2],
                    dimensions[3],
                ]);
                let height = u32::from_be_bytes([
                    dimensions[4],
                    dimensions[5],
                    dimensions[6],
                    dimensions[7],
                ]);
                validate_attachment_preview_dimensions(width, height)?;
                saw_ihdr = true;
                sanitized.extend_from_slice(chunk);
            }
            b"PLTE" if saw_ihdr && !saw_idat => {
                sanitized.extend_from_slice(chunk);
            }
            b"IDAT" if saw_ihdr && !saw_iend => {
                saw_idat = true;
                sanitized.extend_from_slice(chunk);
            }
            b"IEND" if saw_ihdr && saw_idat && !saw_iend && length == 0 => {
                saw_iend = true;
                sanitized.extend_from_slice(chunk);
                cursor = chunk_end;
                break;
            }
            _ if chunk_type.first().is_some_and(u8::is_ascii_lowercase) => {}
            _ => {
                return Err(invalid_payload(
                    "attachment image preview is malformed".to_owned(),
                ));
            }
        }

        cursor = chunk_end;
    }

    if !saw_iend || cursor != bytes.len() {
        return Err(invalid_payload(
            "attachment image preview is malformed".to_owned(),
        ));
    }
    if sanitized.len() as u64 > MAX_ATTACHMENT_BYTES {
        return Err(invalid_payload(
            "attachment image preview is too large".to_owned(),
        ));
    }
    ensure_preview_image_bytes_public(&sanitized, attachment_id)?;

    Ok(sanitized)
}

pub(crate) fn validate_attachment_preview_dimensions(
    width: u32,
    height: u32,
) -> Result<(), CommandErrorPayload> {
    if width == 0
        || height == 0
        || width > MAX_ATTACHMENT_PREVIEW_DIMENSION
        || height > MAX_ATTACHMENT_PREVIEW_DIMENSION
        || u64::from(width)
            .saturating_mul(u64::from(height))
            .saturating_mul(4)
            > MAX_ATTACHMENT_PREVIEW_DECODED_BYTES
    {
        return Err(invalid_payload(
            "attachment image preview is too large".to_owned(),
        ));
    }

    Ok(())
}

pub(crate) fn printable_ascii_runs(bytes: &[u8], min_len: usize) -> Vec<String> {
    let mut runs = Vec::new();
    let mut run = Vec::new();

    for byte in bytes {
        if byte.is_ascii_graphic() || *byte == b' ' || *byte == b'\t' {
            run.push(*byte);
            continue;
        }
        if run.len() >= min_len {
            runs.push(String::from_utf8_lossy(&run).into_owned());
        }
        run.clear();
    }
    if run.len() >= min_len {
        runs.push(String::from_utf8_lossy(&run).into_owned());
    }

    runs
}

pub(crate) fn preview_text_contains_unsafe_payload(value: &str, attachment_id: &str) -> bool {
    contains_obvious_secret(value)
        || redact_unsafe_display_text(value) != value
        || value.contains(attachment_id)
}

pub(crate) fn declared_mime_token(value: &str) -> Option<&str> {
    value
        .split(|character: char| character == ';' || character.is_whitespace())
        .find(|part| part.contains('/'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
}

pub(crate) fn is_preview_image_artifact_kind(value: Option<&str>) -> bool {
    value.is_some_and(|kind| kind == "image" || safe_preview_image_mime(kind).is_some())
}

pub(crate) fn project_artifact_event(
    event: Event,
    session_id: SessionId,
    redactor: &dyn Redactor,
    artifacts_by_id: &mut BTreeMap<String, ArtifactSummaryPayload>,
    order: &mut Vec<String>,
) {
    match event {
        Event::ArtifactCreated(event) => {
            if event.session_id != session_id {
                return;
            }
            let artifact_id = event.artifact_id;
            if !artifacts_by_id.contains_key(&artifact_id) {
                order.push(artifact_id.clone());
            }
            let title = public_text_display(event.title, redactor);
            let kind = public_text_display(event.kind, redactor);
            let preview = event.preview.map(|preview| {
                truncate_utf8(
                    public_text_display(preview, redactor),
                    MAX_ARTIFACT_PREVIEW_BYTES,
                )
            });
            let media = artifact_media_payload(event.blob_ref.as_ref(), &kind);
            artifacts_by_id.insert(
                artifact_id.clone(),
                ArtifactSummaryPayload {
                    action_label: "Open".to_owned(),
                    description: artifact_description_from_source(event.source),
                    id: artifact_id,
                    kind: kind.clone(),
                    preview: preview.clone(),
                    revisions: vec![artifact_revision_payload(
                        event.revision_id,
                        event.at,
                        kind.clone(),
                        artifact_status_label(event.status),
                        title.clone(),
                        preview.clone(),
                        media,
                    )],
                    source_message_id: event
                        .source_message_id
                        .map(|message_id| message_id.to_string()),
                    source_run_id: event.run_id.to_string(),
                    status: artifact_status_label(event.status),
                    title,
                    updated_at: Some(event.at.to_rfc3339()),
                },
            );
        }
        Event::ArtifactUpdated(event) => {
            if event.session_id != session_id {
                return;
            }
            let Some(artifact) = artifacts_by_id.get_mut(&event.artifact_id) else {
                return;
            };
            if let Some(kind) = event.kind {
                artifact.kind = public_text_display(kind, redactor);
            }
            let revision_kind = artifact.kind.clone();
            if let Some(preview) = event.preview {
                artifact.preview = Some(truncate_utf8(
                    public_text_display(preview, redactor),
                    MAX_ARTIFACT_PREVIEW_BYTES,
                ));
            }
            let revision_summary = artifact.preview.clone();
            if let Some(source_message_id) = event.source_message_id {
                artifact.source_message_id = Some(source_message_id.to_string());
            }
            artifact.source_run_id = event.run_id.to_string();
            if let Some(status) = event.status {
                artifact.status = artifact_status_label(status);
            }
            if let Some(title) = event.title {
                artifact.title = public_text_display(title, redactor);
            }
            let revision_status = artifact.status;
            let revision_title = artifact.title.clone();
            let revision_media = event
                .blob_ref
                .as_ref()
                .and_then(|blob_ref| artifact_media_payload(Some(blob_ref), &revision_kind));
            upsert_artifact_revision(
                artifact,
                event.revision_id,
                event.at,
                revision_kind,
                revision_status,
                revision_title,
                revision_summary,
                revision_media,
            );
            artifact.updated_at = Some(event.at.to_rfc3339());
        }
        _ => {}
    }
}

pub(crate) fn artifact_revision_payload(
    revision_id: harness_contracts::ArtifactRevisionId,
    updated_at: DateTime<Utc>,
    kind: String,
    status: &'static str,
    title: String,
    summary: Option<String>,
    media: Option<Value>,
) -> ArtifactRevisionPayload {
    ArtifactRevisionPayload {
        revision_id: revision_id.to_string(),
        content_ref: None,
        kind,
        media,
        preview_ref: None,
        status,
        summary,
        title,
        updated_at: updated_at.to_rfc3339(),
    }
}

pub(crate) fn upsert_artifact_revision(
    artifact: &mut ArtifactSummaryPayload,
    revision_id: harness_contracts::ArtifactRevisionId,
    updated_at: DateTime<Utc>,
    kind: String,
    status: &'static str,
    title: String,
    summary: Option<String>,
    media: Option<Value>,
) {
    let revision_id = revision_id.to_string();
    if let Some(revision) = artifact
        .revisions
        .iter_mut()
        .find(|revision| revision.revision_id == revision_id)
    {
        revision.updated_at = updated_at.to_rfc3339();
        revision.kind = kind;
        revision.status = status;
        revision.title = title;
        revision.summary = summary;
        if media.is_some() {
            revision.media = media;
        }
        return;
    }

    artifact.revisions.push(ArtifactRevisionPayload {
        revision_id,
        content_ref: None,
        kind,
        media,
        preview_ref: None,
        status,
        summary,
        title,
        updated_at: updated_at.to_rfc3339(),
    });
}

pub(crate) fn artifact_status_label(
    status: jyowo_harness_sdk::ext::ArtifactStatus,
) -> &'static str {
    match status {
        jyowo_harness_sdk::ext::ArtifactStatus::Pending => "pending",
        jyowo_harness_sdk::ext::ArtifactStatus::Running => "running",
        jyowo_harness_sdk::ext::ArtifactStatus::Ready => "ready",
        jyowo_harness_sdk::ext::ArtifactStatus::Failed => "failed",
        _ => "ready",
    }
}

pub(crate) fn artifact_source_label(
    source: jyowo_harness_sdk::ext::ArtifactSource,
) -> &'static str {
    match source {
        jyowo_harness_sdk::ext::ArtifactSource::Assistant => "assistant",
        jyowo_harness_sdk::ext::ArtifactSource::Tool => "tool",
        jyowo_harness_sdk::ext::ArtifactSource::File => "file",
        jyowo_harness_sdk::ext::ArtifactSource::ModelService => "model_service",
        _ => "assistant",
    }
}

pub(crate) fn artifact_media_payload(
    blob_ref: Option<&BlobRef>,
    artifact_kind: &str,
) -> Option<Value> {
    let blob_ref = blob_ref?;
    let safe_mime_type = blob_ref
        .content_type
        .as_deref()
        .and_then(safe_artifact_mime_type);
    let kind = artifact_media_kind_from_label(artifact_kind).or_else(|| {
        safe_mime_type
            .as_deref()
            .and_then(artifact_media_kind_from_mime)
    })?;
    let mime_type = safe_mime_type
        .filter(|mime_type| {
            kind == "file"
                || artifact_media_kind_from_mime(mime_type)
                    .is_some_and(|mime_kind| mime_kind == kind)
        })
        .unwrap_or_else(|| default_artifact_mime_type(kind).to_owned());
    Some(json!({
        "kind": kind,
        "mimeType": mime_type,
        "sizeBytes": blob_ref.size,
    }))
}

pub(crate) fn artifact_media_kind_from_label(value: &str) -> Option<&'static str> {
    match value {
        "image" => Some("image"),
        "video" => Some("video"),
        "audio" => Some("audio"),
        "file" => Some("file"),
        _ => safe_artifact_mime_type(value)
            .as_deref()
            .and_then(artifact_media_kind_from_mime),
    }
}

pub(crate) fn artifact_media_kind_from_mime(value: &str) -> Option<&'static str> {
    if safe_artifact_image_mime_type(value).is_some() {
        Some("image")
    } else if value.starts_with("video/") {
        Some("video")
    } else if value.starts_with("audio/") {
        Some("audio")
    } else if safe_artifact_mime_type(value).is_some() {
        Some("file")
    } else {
        None
    }
}

pub(crate) fn default_artifact_mime_type(kind: &str) -> &'static str {
    match kind {
        "image" => "image/png",
        "video" => "video/mp4",
        "audio" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

pub(crate) fn safe_artifact_mime_type(value: &str) -> Option<String> {
    let mime_type = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    match mime_type.as_str() {
        "image/png"
        | "image/jpeg"
        | "image/gif"
        | "image/webp"
        | "image/avif"
        | "video/mp4"
        | "video/webm"
        | "video/quicktime"
        | "audio/mpeg"
        | "audio/mp4"
        | "audio/ogg"
        | "audio/wav"
        | "audio/webm"
        | "text/plain"
        | "text/markdown"
        | "text/csv"
        | "application/json"
        | "application/pdf"
        | "application/zip"
        | "application/octet-stream" => Some(mime_type),
        _ => None,
    }
}

pub(crate) fn safe_artifact_image_mime_type(value: &str) -> Option<&'static str> {
    match value {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        "image/avif" => Some("image/avif"),
        _ => None,
    }
}

pub(crate) fn artifact_description_from_source(
    source: jyowo_harness_sdk::ext::ArtifactSource,
) -> String {
    match source {
        jyowo_harness_sdk::ext::ArtifactSource::Assistant => {
            "Generated by the assistant as a durable artifact.".to_owned()
        }
        jyowo_harness_sdk::ext::ArtifactSource::Tool => {
            "Generated by a tool as a durable artifact.".to_owned()
        }
        jyowo_harness_sdk::ext::ArtifactSource::File => {
            "Linked from the workspace as a durable artifact.".to_owned()
        }
        jyowo_harness_sdk::ext::ArtifactSource::ModelService => {
            "Generated by the model service as a durable artifact.".to_owned()
        }
        _ => "Generated as a durable artifact.".to_owned(),
    }
}
