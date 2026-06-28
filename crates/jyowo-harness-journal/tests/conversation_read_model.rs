#![cfg(feature = "sqlite")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use harness_contracts::*;
use harness_journal::*;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "jyowo-conversation-read-model-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn envelope(
    tenant_id: TenantId,
    session_id: SessionId,
    offset: u64,
    payload: Event,
) -> EventEnvelope {
    EventEnvelope {
        offset: JournalOffset(offset),
        event_id: EventId::new(),
        session_id,
        tenant_id,
        run_id: None,
        correlation_id: CorrelationId::new(),
        causation_id: None,
        schema_version: SchemaVersion::CURRENT,
        recorded_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH
            + chrono::Duration::seconds(offset as i64),
        payload,
    }
}

fn user_message(run_id: RunId, message_id: MessageId, body: &str) -> Event {
    let mut labels = BTreeMap::new();
    labels.insert(
        "clientMessageId".to_owned(),
        "550e8400-e29b-41d4-a716-446655440000".to_owned(),
    );
    Event::UserMessageAppended(UserMessageAppendedEvent {
        run_id,
        message_id,
        content: MessageContent::Text(body.to_owned()),
        metadata: MessageMetadata {
            source: None,
            labels,
        },
        attachments: Vec::new(),
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
    })
}

fn user_message_with_attachment(run_id: RunId, message_id: MessageId, body: &str) -> Event {
    Event::UserMessageAppended(UserMessageAppendedEvent {
        run_id,
        message_id,
        content: MessageContent::Text(body.to_owned()),
        metadata: MessageMetadata::default(),
        attachments: vec![ConversationAttachmentReference {
            id: "attachment-001".to_owned(),
            name: "notes.txt".to_owned(),
            mime_type: "text/plain".to_owned(),
            size_bytes: 128,
            blob_ref: blob_ref_with_content_type(128, "text/plain"),
        }],
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
    })
}

fn user_message_with_unsafe_attachment(run_id: RunId, message_id: MessageId, body: &str) -> Event {
    Event::UserMessageAppended(UserMessageAppendedEvent {
        run_id,
        message_id,
        content: MessageContent::Text(body.to_owned()),
        metadata: MessageMetadata::default(),
        attachments: vec![ConversationAttachmentReference {
            id: "attachment-001".to_owned(),
            name: "/Users/alice/.ssh/id_rsa sk-secret-token".to_owned(),
            mime_type: "text/plain authorization bearer secret-token".to_owned(),
            size_bytes: 128,
            blob_ref: blob_ref_with_content_type(128, "file:///Users/alice/private.txt"),
        }],
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
    })
}

fn assistant_message(run_id: RunId, message_id: MessageId, body: &str) -> Event {
    Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
        run_id,
        message_id,
        content: MessageContent::Text(body.to_owned()),
        tool_uses: Vec::new(),
        usage: UsageSnapshot::default(),
        pricing_snapshot_id: None,
        stop_reason: StopReason::EndTurn,
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
    })
}

fn assistant_message_with_tool_use(
    run_id: RunId,
    message_id: MessageId,
    body: &str,
    tool_use_id: ToolUseId,
    tool_name: &str,
) -> Event {
    Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
        run_id,
        message_id,
        content: MessageContent::Text(body.to_owned()),
        tool_uses: vec![ToolUseSummary {
            tool_use_id,
            tool_name: tool_name.to_owned(),
        }],
        usage: UsageSnapshot::default(),
        pricing_snapshot_id: None,
        stop_reason: StopReason::ToolUse,
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
    })
}

fn image_blob_ref(size: u64) -> BlobRef {
    BlobRef {
        id: BlobId::new(),
        size,
        content_hash: [7; 32],
        content_type: Some("image/png".to_owned()),
    }
}

fn blob_ref_with_content_type(size: u64, content_type: &str) -> BlobRef {
    BlobRef {
        id: BlobId::new(),
        size,
        content_hash: [7; 32],
        content_type: Some(content_type.to_owned()),
    }
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_only_safe_command_preview() {
    let root = temp_root("safe-command-preview");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("read model opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let safe_tool_use_id = ToolUseId::new();
    let secret_tool_use_id = ToolUseId::new();
    let execute_code_tool_use_id = ToolUseId::new();
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            1,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id: safe_tool_use_id,
                tool_name: "Bash".to_owned(),
                input: serde_json::json!({ "command": "pnpm check:desktop" }),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            2,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id: secret_tool_use_id,
                tool_name: "Bash".to_owned(),
                input: serde_json::json!({ "command": "echo sk-abcdefghijklmnopqrstuvwxyz" }),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            3,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id: execute_code_tool_use_id,
                tool_name: "execute_code".to_owned(),
                input: serde_json::json!({ "code": "python ~/.ssh/config" }),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(3),
            }),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");

    assert_eq!(page.events[0].payload["command"], "pnpm check:desktop");
    assert!(page.events[1].payload.get("command").is_none());
    assert_eq!(page.events[2].payload["command"], "python [REDACTED]");
}

#[tokio::test]
async fn safe_tool_process_extracts_only_allowlisted_projection_fields() {
    let root = temp_root("safe-tool-process");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("read model opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let grep_tool_use_id = ToolUseId::new();
    let bash_tool_use_id = ToolUseId::new();
    let edit_tool_use_id = ToolUseId::new();
    let generic_tool_use_id = ToolUseId::new();
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            1,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id: grep_tool_use_id,
                tool_name: "grep".to_owned(),
                input: serde_json::json!({
                    "path": "crates/jyowo-harness-journal/src/lib.rs",
                    "pattern": "needle",
                    "provider_payload": {
                        "signed_url": "https://example.invalid/signed/private"
                    }
                }),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            2,
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id: grep_tool_use_id,
                result: ToolResult::Structured(serde_json::json!([
                    {
                        "path": "crates/jyowo-harness-journal/src/lib.rs",
                        "line": 7,
                        "text": "needle"
                    },
                    {
                        "path": "/Users/goya/private/secret.txt",
                        "line": 1,
                        "text": "secret"
                    },
                    {
                        "file": "/Users/goya/private/other-secret.txt",
                        "line": 2,
                        "text": "secret"
                    },
                    {
                        "path": ".jyowo/runtime/blobs/private-image",
                        "line": 3,
                        "text": "secret"
                    }
                ])),
                usage: None,
                duration_ms: 10,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            3,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id: bash_tool_use_id,
                tool_name: "Bash".to_owned(),
                input: serde_json::json!({ "command": "pnpm check:desktop" }),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(3),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            4,
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id: bash_tool_use_id,
                result: ToolResult::Structured(serde_json::json!({
                    "exit_code": 0,
                    "stdout": "passed\n/Users/goya/private/build.log\n/tmp/jyowo/.jyowo/blob\n.jyowo/runtime/blobs/private-image\nblob:.jyowo/runtime/blobs/private-image\nhttps://example.invalid/signed/private\nerror:/Users/goya/.ssh/config\nurl:<https://example.invalid/signed/private>\nhome:~/.ssh/config",
                    "stderr": ""
                })),
                usage: None,
                duration_ms: 25,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(4),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            5,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id: edit_tool_use_id,
                tool_name: "apply_patch".to_owned(),
                input: serde_json::json!({ "patch": "*** Begin Patch\n*** End Patch" }),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(5),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            6,
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id: edit_tool_use_id,
                result: ToolResult::Structured(serde_json::json!({
                    "diff": {
                        "files": [
                            {
                                "path": "apps/desktop/src/features/conversation/timeline/process-panel.tsx",
                                "addedLines": 2,
                                "removedLines": 1,
                                "preview": "@@\n-old\n+new"
                            },
                            {
                                "path": "/Users/goya/private/secret.ts",
                                "addedLines": 1,
                                "removedLines": 0,
                                "preview": "+secret"
                            },
                            {
                                "path": "~/.ssh/config",
                                "addedLines": 1,
                                "removedLines": 0,
                                "preview": "+secret"
                            }
                        ]
                    }
                })),
                usage: None,
                duration_ms: 5,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(6),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            7,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id: generic_tool_use_id,
                tool_name: "custom_provider_tool".to_owned(),
                input: serde_json::json!({
                    "path": "safe-looking/provider-path.txt",
                    "query": "safe-looking-query",
                    "provider_payload": {
                        "signed_url": "https://example.invalid/provider-native"
                    }
                }),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(7),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            8,
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id: generic_tool_use_id,
                result: ToolResult::Text(
                    "provider payload https://example.invalid/native /tmp/native-path".to_owned(),
                ),
                usage: None,
                duration_ms: 15,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(8),
            }),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let serialized = serde_json::to_string(&page.events).unwrap();
    let grep_requested = &page.events[0].payload;
    let grep_completed = &page.events[1].payload;
    let bash_completed = &page.events[3].payload;
    let edit_completed = &page.events[5].payload;
    let generic_completed = &page.events[7].payload;

    assert!(grep_requested.get("targetPath").is_none());
    assert_eq!(grep_requested["query"], "needle");
    assert_eq!(grep_completed["itemCount"], 1);
    assert_eq!(bash_completed["exitCode"], 0);
    assert_eq!(
        bash_completed["outputSummary"],
        "passed\n[REDACTED]\n[REDACTED]\n[REDACTED]\n[REDACTED]\n[REDACTED]\nerror:[REDACTED]\nurl:<[REDACTED]>\nhome:[REDACTED]"
    );
    assert_eq!(
        edit_completed["diff"]["files"][0]["path"],
        "apps/desktop/src/features/conversation/timeline/process-panel.tsx"
    );
    assert_eq!(edit_completed["diff"]["files"].as_array().unwrap().len(), 1);
    assert_eq!(
        generic_completed["outputSummary"],
        "Output withheld from conversation timeline."
    );
    assert!(page.events[6].payload.get("targetPath").is_none());
    assert!(page.events[6].payload.get("query").is_none());
    assert!(!serialized.contains("provider_payload"));
    assert!(!serialized.contains("signed_url"));
    assert!(!serialized.contains("/Users/goya/private"));
    assert!(!serialized.contains("/tmp/jyowo"));
    assert!(!serialized.contains(".jyowo/runtime/blobs"));
    assert!(!serialized.contains("~/.ssh"));
    assert!(!serialized.contains("example.invalid"));
}

#[tokio::test]
async fn safe_tool_process_rejects_opaque_url_and_runtime_paths() {
    let root = temp_root("safe-tool-process-unsafe-paths");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("read model opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let read_tool_use_id = ToolUseId::new();
    let edit_tool_use_id = ToolUseId::new();
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            1,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id: read_tool_use_id,
                tool_name: "ReadFile".to_owned(),
                input: serde_json::json!({ "path": "safe/data:text/plain,secret" }),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            2,
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id: read_tool_use_id,
                result: ToolResult::Structured(serde_json::json!([
                    "src/lib.rs",
                    "blob:null/provider-output",
                    "file:relative/path",
                    "javascript:alert(1)",
                    "mailto:user@example.com",
                    "safe/data:text/plain,secret",
                    ".JYOWO/runtime/blobs/blob-001"
                ])),
                usage: None,
                duration_ms: 10,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            3,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id: edit_tool_use_id,
                tool_name: "apply_patch".to_owned(),
                input: serde_json::json!({ "targetPath": "file:relative/path" }),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(3),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            4,
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id: edit_tool_use_id,
                result: ToolResult::Structured(serde_json::json!({
                    "diff": {
                        "files": [
                            {
                                "path": "src/lib.rs",
                                "addedLines": 1,
                                "removedLines": 0,
                                "preview": "+safe"
                            },
                            {
                                "path": "blob:null/provider-output",
                                "addedLines": 1,
                                "removedLines": 0,
                                "preview": "+unsafe"
                            },
                            {
                                "path": "file:relative/path",
                                "addedLines": 1,
                                "removedLines": 0,
                                "preview": "+unsafe"
                            },
                            {
                                "path": "javascript:alert(1)",
                                "addedLines": 1,
                                "removedLines": 0,
                                "preview": "+unsafe"
                            },
                            {
                                "path": "mailto:user@example.com",
                                "addedLines": 1,
                                "removedLines": 0,
                                "preview": "+unsafe"
                            },
                            {
                                "path": "safe/data:text/plain,secret",
                                "addedLines": 1,
                                "removedLines": 0,
                                "preview": "+unsafe"
                            },
                            {
                                "path": ".JYOWO/runtime/blobs/blob-001",
                                "addedLines": 1,
                                "removedLines": 0,
                                "preview": "+unsafe"
                            }
                        ]
                    }
                })),
                usage: None,
                duration_ms: 5,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(4),
            }),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let serialized = serde_json::to_string(&page.events).unwrap();
    let read_requested = &page.events[0].payload;
    let read_completed = &page.events[1].payload;
    let edit_requested = &page.events[2].payload;
    let edit_completed = &page.events[3].payload;

    assert!(read_requested.get("targetPath").is_none());
    assert_eq!(read_completed["itemCount"], 1);
    assert!(edit_requested.get("targetPath").is_none());
    assert_eq!(edit_completed["diff"]["files"].as_array().unwrap().len(), 1);
    assert_eq!(edit_completed["diff"]["files"][0]["path"], "src/lib.rs");
    for unsafe_fragment in [
        "data:text",
        "blob:null",
        "file:relative",
        "javascript:",
        "mailto:",
        ".JYOWO/runtime/blobs",
    ] {
        assert!(!serialized.contains(unsafe_fragment));
    }
}

#[tokio::test]
async fn sqlite_conversation_read_model_redacts_urls_and_blob_paths_from_public_text() {
    let root = temp_root("public-text-redaction");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let message_id = MessageId::new();
    let review_request_id = RequestId::new();
    let clarification_request_id = RequestId::new();
    let notice_id = RequestId::new();
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            1,
            Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                run_id,
                message_id,
                delta: DeltaChunk::ReasoningSummary(ReasoningSummaryChunk {
                    text:
                        "Checked https://provider.example/image，链接https://provider.example/tight and 路径：.jyowo/runtime/blobs/blob-001 log/tmp/provider-output"
                            .to_owned(),
                    provider_id: "test".to_owned(),
                    provider_native: None,
                }),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            2,
            Event::ArtifactCreated(ArtifactCreatedEvent {
                session_id,
                run_id,
                artifact_id: "artifact-unsafe-text".to_owned(),
                title: "Image at https://provider.example/image data:image/svg+xml,<svg onload=alert(1)>。"
                    .to_owned(),
                kind: "image javascript:alert(1)".to_owned(),
                status: ArtifactStatus::Ready,
                source: ArtifactSource::Tool,
                source_message_id: None,
                source_tool_use_id: None,
                blob_ref: None,
                preview: Some("Blob path .jyowo/runtime/blobs/blob-001 blob:null/provider".to_owned()),
                content_hash: None,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            3,
            Event::ArtifactUpdated(ArtifactUpdatedEvent {
                session_id,
                run_id,
                artifact_id: "artifact-unsafe-text".to_owned(),
                title: None,
                kind: Some("image/png file:/tmp/provider-output".to_owned()),
                status: Some(ArtifactStatus::Ready),
                source: ArtifactSource::Tool,
                source_message_id: None,
                source_tool_use_id: None,
                blob_ref: None,
                preview: None,
                content_hash: None,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(3),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            4,
            Event::AssistantReviewRequested(AssistantReviewRequestedEvent {
                run_id,
                request_id: review_request_id,
                title: UiSafeText::from_trusted_redacted("Review https://provider.example/review"),
                body: Some(UiSafeText::from_trusted_redacted(
                    "Confirm blob:.jyowo/runtime/blobs/blob-001",
                )),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(3),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            5,
            Event::AssistantClarificationRequested(AssistantClarificationRequestedEvent {
                run_id,
                request_id: clarification_request_id,
                prompt: UiSafeText::from_trusted_redacted("Use链接https://provider.example/prompt"),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(4),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            6,
            Event::AssistantNotice(AssistantNoticeEvent {
                run_id,
                notice_id,
                body: UiSafeText::from_trusted_redacted("Read 路径：.jyowo/runtime/blobs/blob-001"),
                code: None,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(5),
            }),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let serialized = serde_json::to_string(&page.events).unwrap();

    assert_eq!(
        page.events[0].payload["safeSummaryDelta"],
        "Checked [REDACTED]，链接[REDACTED] and 路径：[REDACTED] log[REDACTED]"
    );
    assert_eq!(
        page.events[1].payload["title"],
        "Image at [REDACTED] [REDACTED]。"
    );
    assert_eq!(page.events[1].payload["kind"], "image [REDACTED]");
    assert_eq!(
        page.events[1].payload["summary"],
        "Blob path [REDACTED] [REDACTED]"
    );
    assert_eq!(page.events[2].payload["kind"], "image/png [REDACTED]");
    assert_eq!(page.events[3].payload["title"], "Review [REDACTED]");
    assert_eq!(page.events[3].payload["body"], "Confirm [REDACTED]");
    assert_eq!(page.events[4].payload["prompt"], "Use链接[REDACTED]");
    assert_eq!(page.events[5].payload["body"], "Read 路径：[REDACTED]");
    assert!(!serialized.contains("provider.example"));
    assert!(!serialized.contains(".jyowo/runtime/blobs"));
    assert!(!serialized.contains("/tmp/provider-output"));
    assert!(!serialized.contains("data:image"));
    assert!(!serialized.contains("blob:null"));
    assert!(!serialized.contains("javascript:"));
    assert!(!serialized.contains("file:"));
}

#[tokio::test]
async fn sqlite_conversation_read_model_clears_cached_projection_on_version_mismatch() {
    let root = temp_root("version-mismatch");
    let path = root.join("read-model.sqlite");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let envelopes = vec![envelope(
        tenant_id,
        session_id,
        0,
        user_message(run_id, MessageId::new(), "stale"),
    )];
    {
        let store = SqliteConversationReadModelStore::open(&path)
            .await
            .expect("store opens");
        store
            .apply_envelopes(tenant_id, session_id, &envelopes, None)
            .await
            .expect("projection applies");
        assert_eq!(
            store
                .list_summaries(tenant_id, 10)
                .await
                .expect("summaries load")
                .len(),
            1
        );
    }
    {
        let connection = rusqlite::Connection::open(&path).expect("sqlite opens");
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS conversation_read_model_meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                 ) STRICT;",
            )
            .expect("meta table exists");
        connection
            .execute(
                "INSERT INTO conversation_read_model_meta (key, value)
                 VALUES ('conversation_read_model_projection_version', '1')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )
            .expect("old projection version is written");
    }

    let reopened = SqliteConversationReadModelStore::open(&path)
        .await
        .expect("store reopens");

    assert!(
        reopened
            .list_summaries(tenant_id, 10)
            .await
            .expect("summaries load")
            .is_empty(),
        "version mismatch must clear cached read model rows"
    );
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_assistant_delta_message_id() {
    let root = temp_root("assistant-delta-message-id");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let message_id = MessageId::new();
    let envelopes = vec![envelope(
        tenant_id,
        session_id,
        0,
        Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
            run_id,
            message_id,
            delta: DeltaChunk::Text("hello".to_owned()),
            at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
        }),
    )];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let delta = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.delta")
        .expect("assistant delta is projected");

    assert_eq!(delta.payload["text"], "hello");
    assert_eq!(delta.payload["messageId"], message_id.to_string());
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_user_message_attachments() {
    let root = temp_root("user-message-attachments");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let message_id = MessageId::new();
    let envelopes = vec![envelope(
        tenant_id,
        session_id,
        1,
        user_message_with_attachment(run_id, message_id, "Summarize this attachment"),
    )];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let user = page
        .events
        .iter()
        .find(|event| event.event_type == "user.message.appended")
        .expect("user message is projected");

    assert_eq!(user.payload["attachments"][0]["name"], "notes.txt");
    assert_eq!(user.payload["attachments"][0]["mimeType"], "text/plain");
    assert_eq!(user.payload["attachments"][0]["sizeBytes"], 128);
}

#[tokio::test]
async fn sqlite_conversation_read_model_redacts_unsafe_attachment_metadata() {
    let root = temp_root("unsafe-user-message-attachments");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let message_id = MessageId::new();
    let envelopes = vec![envelope(
        tenant_id,
        session_id,
        1,
        user_message_with_unsafe_attachment(run_id, message_id, "Summarize this attachment"),
    )];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let user = page
        .events
        .iter()
        .find(|event| event.event_type == "user.message.appended")
        .expect("user message is projected");
    let attachment = &user.payload["attachments"][0];

    assert_eq!(attachment["name"], "[REDACTED]");
    assert_eq!(attachment["mimeType"], "application/octet-stream");
    assert_eq!(
        attachment["blobRef"]["contentType"],
        serde_json::Value::Null
    );
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_safe_reasoning_summary_without_raw_thought_text() {
    let root = temp_root("safe-reasoning-summary");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let message_id = MessageId::new();
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            0,
            user_message(run_id, MessageId::new(), "think"),
        ),
        envelope(
            tenant_id,
            session_id,
            1,
            Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                run_id,
                message_id,
                delta: DeltaChunk::Thought(ThoughtChunk {
                    text: Some("raw private chain".to_owned()),
                    provider_id: "test".to_owned(),
                    provider_native: None,
                    signature: None,
                }),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            2,
            Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                run_id,
                message_id,
                delta: DeltaChunk::ReasoningSummary(ReasoningSummaryChunk {
                    text: "Checked project context.".to_owned(),
                    provider_id: "test".to_owned(),
                    provider_native: None,
                }),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
            }),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let thinking_events = page
        .events
        .iter()
        .filter(|event| event.event_type == "assistant.thinking.delta")
        .collect::<Vec<_>>();

    assert_eq!(thinking_events.len(), 2);
    assert_eq!(thinking_events[0].payload["status"], "running");
    assert!(thinking_events[0].payload.get("text").is_none());
    assert!(thinking_events[0].payload.get("safeSummaryDelta").is_none());
    assert_eq!(
        thinking_events[1].payload["safeSummaryDelta"],
        "Checked project context."
    );
    assert!(!serde_json::to_string(&page.events)
        .unwrap()
        .contains("raw private chain"));
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_completed_tool_uses() {
    let root = temp_root("assistant-completed-tool-uses");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let message_id = MessageId::new();
    let tool_use_id = ToolUseId::new();
    let envelopes = vec![envelope(
        tenant_id,
        session_id,
        0,
        assistant_message_with_tool_use(
            run_id,
            message_id,
            "I need to inspect files.",
            tool_use_id,
            "read_file",
        ),
    )];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let completed = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.completed")
        .expect("assistant completed is projected");

    assert_eq!(completed.payload["messageId"], message_id.to_string());
    assert_eq!(completed.payload["body"], "I need to inspect files.");
    assert_eq!(
        completed.payload["toolUses"][0]["toolUseId"],
        tool_use_id.to_string()
    );
    assert_eq!(completed.payload["toolUses"][0]["toolName"], "read_file");
}

#[tokio::test]
async fn sqlite_conversation_read_model_redacts_secret_like_tool_names() {
    let root = temp_root("secret-like-tool-name");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    let message_id = MessageId::new();
    let tool_name = "sk-abcdefghijklmnopqrstuvwxyz";
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            0,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id,
                tool_name: tool_name.to_owned(),
                input: serde_json::json!({}),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            1,
            Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                run_id,
                message_id,
                content: MessageContent::Text("Tool requested.".to_owned()),
                tool_uses: vec![ToolUseSummary {
                    tool_use_id,
                    tool_name: tool_name.to_owned(),
                }],
                usage: UsageSnapshot::default(),
                pricing_snapshot_id: None,
                stop_reason: StopReason::ToolUse,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
            }),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let serialized = serde_json::to_string(&page).unwrap();

    assert!(!serialized.contains(tool_name));
    assert!(serialized.contains("[REDACTED]"));
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_image_artifact_media_metadata() {
    let root = temp_root("image-artifact-media");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let blob_ref = image_blob_ref(512);
    let envelopes = vec![envelope(
        tenant_id,
        session_id,
        0,
        Event::ArtifactCreated(ArtifactCreatedEvent {
            session_id,
            run_id,
            artifact_id: "artifact-image".to_owned(),
            title: "生成的图片".to_owned(),
            kind: "image".to_owned(),
            status: ArtifactStatus::Ready,
            source: ArtifactSource::Tool,
            source_message_id: None,
            source_tool_use_id: Some(ToolUseId::new()),
            blob_ref: Some(blob_ref),
            preview: Some("图片已生成。".to_owned()),
            content_hash: Some(vec![1, 2, 3]),
            at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
        }),
    )];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let artifact = page
        .events
        .iter()
        .find(|event| event.event_type == "artifact.created")
        .expect("artifact created is projected");
    let serialized = serde_json::to_string(&artifact.payload).unwrap();

    assert_eq!(artifact.payload["artifactId"], "artifact-image");
    assert_eq!(artifact.payload["kind"], "image");
    assert_eq!(artifact.payload["status"], "ready");
    assert_eq!(artifact.payload["source"], "tool");
    assert_eq!(artifact.payload["media"]["kind"], "image");
    assert_eq!(artifact.payload["media"]["mimeType"], "image/png");
    assert_eq!(artifact.payload["media"]["sizeBytes"], 512);
    assert!(!serialized.contains("contentHash"));
    assert!(!serialized.contains("blobRef"));
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_updated_image_media_without_kind() {
    let root = temp_root("updated-image-artifact-media");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let blob_ref = image_blob_ref(1024);
    let artifact_id = "artifact-image".to_owned();
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            0,
            Event::ArtifactCreated(ArtifactCreatedEvent {
                session_id,
                run_id,
                artifact_id: artifact_id.clone(),
                title: "生成的图片".to_owned(),
                kind: "image".to_owned(),
                status: ArtifactStatus::Pending,
                source: ArtifactSource::Tool,
                source_message_id: None,
                source_tool_use_id: Some(ToolUseId::new()),
                blob_ref: None,
                preview: Some("图片生成中。".to_owned()),
                content_hash: None,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            1,
            Event::ArtifactUpdated(ArtifactUpdatedEvent {
                session_id,
                run_id,
                artifact_id: artifact_id.clone(),
                title: None,
                kind: None,
                status: Some(ArtifactStatus::Ready),
                source: ArtifactSource::Tool,
                source_message_id: None,
                source_tool_use_id: Some(ToolUseId::new()),
                blob_ref: Some(blob_ref),
                preview: None,
                content_hash: Some(vec![1, 2, 3]),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
            }),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let artifact = page
        .events
        .iter()
        .find(|event| event.event_type == "artifact.updated")
        .expect("artifact update is projected");

    assert_eq!(artifact.payload["status"], "ready");
    assert_eq!(artifact.payload["media"]["kind"], "image");
    assert_eq!(artifact.payload["media"]["mimeType"], "image/png");
    assert_eq!(artifact.payload["media"]["sizeBytes"], 1024);
}

#[tokio::test]
async fn sqlite_conversation_read_model_redacts_unsafe_artifact_media_mime_type() {
    let root = temp_root("unsafe-artifact-media-mime-type");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let blob_ref = blob_ref_with_content_type(
        1024,
        "image/png /tmp/provider-output https://provider.example/blob",
    );
    let envelopes = vec![envelope(
        tenant_id,
        session_id,
        0,
        Event::ArtifactCreated(ArtifactCreatedEvent {
            session_id,
            run_id,
            artifact_id: "artifact-image".to_owned(),
            title: "生成的图片".to_owned(),
            kind: "image".to_owned(),
            status: ArtifactStatus::Ready,
            source: ArtifactSource::Tool,
            source_message_id: None,
            source_tool_use_id: Some(ToolUseId::new()),
            blob_ref: Some(blob_ref),
            preview: Some("图片已生成。".to_owned()),
            content_hash: Some(vec![1, 2, 3]),
            at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
        }),
    )];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let artifact = page
        .events
        .iter()
        .find(|event| event.event_type == "artifact.created")
        .expect("artifact created is projected");
    let serialized = serde_json::to_string(&artifact.payload).unwrap();

    assert_eq!(artifact.payload["media"]["kind"], "image");
    assert_eq!(artifact.payload["media"]["mimeType"], "image/png");
    assert_eq!(artifact.payload["media"]["sizeBytes"], 1024);
    assert!(!serialized.contains("/tmp/provider-output"));
    assert!(!serialized.contains("provider.example"));
}

#[tokio::test]
async fn sqlite_conversation_read_model_does_not_project_secret_like_mime_token() {
    let root = temp_root("secret-like-artifact-media-mime-token");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let blob_ref =
        blob_ref_with_content_type(1024, "video/sk-abcdefghijklmnopqrstuvwxyz0123456789");
    let envelopes = vec![envelope(
        tenant_id,
        session_id,
        0,
        Event::ArtifactCreated(ArtifactCreatedEvent {
            session_id,
            run_id,
            artifact_id: "artifact-video".to_owned(),
            title: "生成的视频".to_owned(),
            kind: "video".to_owned(),
            status: ArtifactStatus::Ready,
            source: ArtifactSource::Tool,
            source_message_id: None,
            source_tool_use_id: Some(ToolUseId::new()),
            blob_ref: Some(blob_ref),
            preview: Some("视频已生成。".to_owned()),
            content_hash: Some(vec![1, 2, 3]),
            at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
        }),
    )];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let artifact = page
        .events
        .iter()
        .find(|event| event.event_type == "artifact.created")
        .expect("artifact created is projected");
    let serialized = serde_json::to_string(&artifact.payload).unwrap();

    assert_eq!(artifact.payload["media"]["kind"], "video");
    assert_eq!(artifact.payload["media"]["mimeType"], "video/mp4");
    assert!(!serialized.contains("sk-abcdefghijklmnopqrstuvwxyz0123456789"));
}

#[tokio::test]
async fn sqlite_conversation_read_model_preserves_allowlisted_file_mime_type() {
    let root = temp_root("safe-file-artifact-media-mime-type");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let blob_ref = blob_ref_with_content_type(1024, "text/plain");
    let envelopes = vec![envelope(
        tenant_id,
        session_id,
        0,
        Event::ArtifactCreated(ArtifactCreatedEvent {
            session_id,
            run_id,
            artifact_id: "artifact-notes".to_owned(),
            title: "Notes".to_owned(),
            kind: "file".to_owned(),
            status: ArtifactStatus::Ready,
            source: ArtifactSource::Tool,
            source_message_id: None,
            source_tool_use_id: Some(ToolUseId::new()),
            blob_ref: Some(blob_ref),
            preview: Some("Notes ready.".to_owned()),
            content_hash: Some(vec![1, 2, 3]),
            at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
        }),
    )];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let artifact = page
        .events
        .iter()
        .find(|event| event.event_type == "artifact.created")
        .expect("artifact created is projected");

    assert_eq!(artifact.payload["media"]["kind"], "file");
    assert_eq!(artifact.payload["media"]["mimeType"], "text/plain");
}

fn tool_properties() -> ToolProperties {
    ToolProperties {
        is_concurrency_safe: true,
        is_read_only: true,
        is_destructive: false,
        long_running: None,
        defer_policy: DeferPolicy::AlwaysLoad,
    }
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_summary_snapshot_and_timeline_idempotently() {
    let root = temp_root("sqlite");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let user_message_id = MessageId::new();
    let assistant_message_id = MessageId::new();
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            0,
            user_message(run_id, user_message_id, "open /Users/goya/.ssh/config"),
        ),
        envelope(
            tenant_id,
            session_id,
            1,
            assistant_message(run_id, assistant_message_id, "Done"),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");
    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection is idempotent");

    let summaries = store
        .list_summaries(tenant_id, 10)
        .await
        .expect("summaries load");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, session_id.to_string());
    assert_eq!(summaries[0].title.as_str(), "open [REDACTED]");
    assert_eq!(
        summaries[0].last_message_preview.as_ref().unwrap().as_str(),
        "Done"
    );
    assert_eq!(summaries[0].cursor.unwrap().conversation_sequence, 2);

    let snapshot = store
        .snapshot(tenant_id, session_id, 200)
        .await
        .expect("snapshot loads")
        .expect("snapshot exists");
    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(snapshot.messages[0].body.as_str(), "open [REDACTED]");
    assert_eq!(
        snapshot.messages[0].client_message_id.as_deref(),
        Some("550e8400-e29b-41d4-a716-446655440000")
    );
    assert_eq!(snapshot.messages[1].body.as_str(), "Done");

    let page = store
        .page_timeline(tenant_id, session_id, None, 1)
        .await
        .expect("first page loads");
    assert_eq!(page.events.len(), 1);
    let next = store
        .page_timeline(tenant_id, session_id, page.cursor, 10)
        .await
        .expect("second page loads");
    assert_eq!(next.events.len(), 1);
    assert_eq!(next.events[0].cursor.conversation_sequence, 2);
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_worktree_by_complete_turns() {
    let root = temp_root("worktree-page");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_1 = RunId::new();
    let run_2 = RunId::new();
    let events = vec![
        envelope(
            tenant_id,
            session_id,
            0,
            user_message(run_1, MessageId::new(), "first"),
        ),
        envelope(
            tenant_id,
            session_id,
            1,
            assistant_message(run_1, MessageId::new(), "first answer"),
        ),
        envelope(
            tenant_id,
            session_id,
            2,
            assistant_message(run_1, MessageId::new(), "first final"),
        ),
        envelope(
            tenant_id,
            session_id,
            3,
            user_message(run_2, MessageId::new(), "second"),
        ),
        envelope(
            tenant_id,
            session_id,
            4,
            assistant_message(run_2, MessageId::new(), "second answer"),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &events, None)
        .await
        .expect("projection applies");

    let first = store
        .page_worktree(
            tenant_id,
            session_id,
            None,
            ConversationTurnPageDirection::After,
            1,
        )
        .await
        .expect("first worktree page loads");

    assert_eq!(first.turns.len(), 1);
    assert_eq!(first.turns[0].user.body.as_str(), "first");
    assert_eq!(
        first.turns[0].assistant.as_ref().unwrap().segments.len(),
        2,
        "limit counts complete turns, not raw timeline events"
    );
    assert_eq!(first.event_cursor.unwrap().conversation_sequence, 5);
    assert!(first.has_more_after);

    let second = store
        .page_worktree(
            tenant_id,
            session_id,
            first.page_cursor.clone(),
            ConversationTurnPageDirection::After,
            1,
        )
        .await
        .expect("second worktree page loads");

    assert_eq!(second.turns.len(), 1);
    assert_eq!(second.turns[0].user.body.as_str(), "second");
    assert!(second.has_more_before);
    assert!(!second.has_more_after);
}

#[tokio::test]
async fn sqlite_conversation_read_model_worktree_replays_complete_timeline_before_slicing() {
    let root = temp_root("worktree-complete-replay");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_1 = RunId::new();
    let run_2 = RunId::new();
    let run_3 = RunId::new();
    let run_4 = RunId::new();
    let events = vec![
        envelope(
            tenant_id,
            session_id,
            0,
            user_message(run_1, MessageId::new(), "first"),
        ),
        envelope(
            tenant_id,
            session_id,
            1,
            assistant_message(run_1, MessageId::new(), "first answer"),
        ),
        envelope(
            tenant_id,
            session_id,
            2,
            user_message(run_2, MessageId::new(), "second"),
        ),
        envelope(
            tenant_id,
            session_id,
            3,
            assistant_message(run_2, MessageId::new(), "second answer"),
        ),
        envelope(
            tenant_id,
            session_id,
            4,
            user_message(run_3, MessageId::new(), "third"),
        ),
        envelope(
            tenant_id,
            session_id,
            5,
            assistant_message(run_3, MessageId::new(), "third answer"),
        ),
        envelope(
            tenant_id,
            session_id,
            6,
            user_message(run_4, MessageId::new(), "fourth"),
        ),
        envelope(
            tenant_id,
            session_id,
            7,
            assistant_message(run_4, MessageId::new(), "fourth answer"),
        ),
        envelope(
            tenant_id,
            session_id,
            8,
            Event::ArtifactCreated(ArtifactCreatedEvent {
                session_id,
                run_id: run_2,
                artifact_id: "artifact-late".to_owned(),
                title: "Late artifact".to_owned(),
                kind: "markdown".to_owned(),
                status: ArtifactStatus::Ready,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                blob_ref: None,
                preview: Some("Late update for an older turn".to_owned()),
                content_hash: None,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(8),
            }),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &events, None)
        .await
        .expect("projection applies");

    let after_first = store
        .page_worktree(
            tenant_id,
            session_id,
            None,
            ConversationTurnPageDirection::After,
            2,
        )
        .await
        .expect("after page loads");
    assert_eq!(
        after_first
            .turns
            .iter()
            .map(|turn| turn.user.body.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );
    assert_eq!(
        after_first.page_cursor.as_ref().unwrap().position,
        after_first.turns[1].position
    );
    assert_eq!(
        after_first.event_cursor.unwrap().conversation_sequence,
        9,
        "event cursor comes from complete replay, not the selected turn page"
    );
    assert!(!after_first.gap);

    let after_second = store
        .page_worktree(
            tenant_id,
            session_id,
            after_first.page_cursor.clone(),
            ConversationTurnPageDirection::After,
            2,
        )
        .await
        .expect("second after page loads");
    assert_eq!(
        after_second
            .turns
            .iter()
            .map(|turn| turn.user.body.as_str())
            .collect::<Vec<_>>(),
        vec!["third", "fourth"]
    );

    let before_cursor = ConversationTurnCursor {
        turn_id: after_second.turns[1].id.clone(),
        position: after_second.turns[1].position,
    };
    let before_first = store
        .page_worktree(
            tenant_id,
            session_id,
            Some(before_cursor),
            ConversationTurnPageDirection::Before,
            2,
        )
        .await
        .expect("before page loads");
    assert_eq!(
        before_first
            .turns
            .iter()
            .map(|turn| turn.user.body.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "third"],
        "Before pages stay in ascending conversation order"
    );
    assert_eq!(
        before_first.page_cursor.as_ref().unwrap().position,
        before_first.turns[0].position,
        "Before cursor points at the first returned turn"
    );
    assert_eq!(before_first.event_cursor.unwrap().conversation_sequence, 9);
    assert!(!before_first.gap);
    let second_turn = &before_first.turns[0];
    let artifact_count = second_turn
        .assistant
        .as_ref()
        .unwrap()
        .segments
        .iter()
        .filter(|segment| matches!(segment, AssistantSegment::Artifact(_)))
        .count();
    assert_eq!(
        artifact_count, 1,
        "late events for older turns are applied before page slicing"
    );

    let before_second = store
        .page_worktree(
            tenant_id,
            session_id,
            before_first.page_cursor.clone(),
            ConversationTurnPageDirection::Before,
            2,
        )
        .await
        .expect("second before page loads");
    assert_eq!(
        before_second
            .turns
            .iter()
            .map(|turn| turn.user.body.as_str())
            .collect::<Vec<_>>(),
        vec!["first"],
        "repeated Before requests do not overlap at the cursor boundary"
    );

    let stale_cursor = ConversationTurnCursor {
        turn_id: after_first.turns[0].id.clone(),
        position: after_first.turns[1].position,
    };
    let error = store
        .page_worktree(
            tenant_id,
            session_id,
            Some(stale_cursor),
            ConversationTurnPageDirection::After,
            2,
        )
        .await
        .expect_err("mismatched worktree cursor is rejected");
    assert!(
        error.to_string().contains("conversation cursor is unknown"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn sqlite_conversation_read_model_projects_assistant_review_requested_tool_permission_and_artifact_events(
) {
    let root = temp_root("timeline-events");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    let review_request_id = RequestId::new();
    let clarification_request_id = RequestId::new();
    let notice_id = RequestId::new();
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            0,
            Event::RunStarted(RunStartedEvent {
                run_id,
                session_id,
                tenant_id,
                parent_run_id: None,
                input: TurnInput {
                    message: Message {
                        id: MessageId::new(),
                        role: MessageRole::User,
                        parts: vec![MessagePart::Text("run".to_owned())],
                        created_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
                    },
                    metadata: serde_json::Value::Null,
                },
                snapshot_id: SnapshotId::new(),
                effective_config_hash: ConfigHash([0; 32]),
                started_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
                correlation_id: CorrelationId::new(),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            1,
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id,
                tool_use_id,
                tool_name: "shell".to_owned(),
                input: serde_json::json!({ "secret": "sk-abcdefghijklmnopqrstuvwxyz" }),
                properties: tool_properties(),
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            2,
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id,
                result: ToolResult::Text("wrote /Users/goya/.ssh/config".to_owned()),
                usage: None,
                duration_ms: 12,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(2),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            3,
            Event::PermissionRequested(PermissionRequestedEvent {
                request_id,
                run_id,
                session_id,
                tenant_id,
                tool_use_id,
                tool_name: "shell".to_owned(),
                subject: PermissionSubject::CommandExec {
                    command: "/bin/rm -rf target".to_owned(),
                    argv: vec!["rm".to_owned()],
                    cwd: None,
                    fingerprint: None,
                },
                severity: Severity::High,
                scope_hint: DecisionScope::Any,
                fingerprint: None,
                presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
                interactivity: InteractivityLevel::FullyInteractive,
                causation_id: EventId::new(),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(3),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            4,
            Event::PermissionResolved(PermissionResolvedEvent {
                request_id,
                decision: Decision::DenyOnce,
                decided_by: DecidedBy::User,
                scope: DecisionScope::Any,
                fingerprint: None,
                rationale: None,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(4),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            5,
            Event::ArtifactCreated(ArtifactCreatedEvent {
                session_id,
                run_id,
                artifact_id: "artifact-001".to_owned(),
                title: "Report".to_owned(),
                kind: "markdown".to_owned(),
                status: ArtifactStatus::Ready,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                blob_ref: None,
                preview: Some("Image artifact ready".to_owned()),
                content_hash: None,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(5),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            6,
            Event::AssistantReviewRequested(AssistantReviewRequestedEvent {
                run_id,
                request_id: review_request_id,
                title: UiSafeText::from_trusted_redacted(
                    "Review Authorization: Bearer synthetic-token",
                ),
                body: Some(UiSafeText::from_trusted_redacted(
                    "Confirm before applying /Users/example/private.",
                )),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(6),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            7,
            Event::AssistantClarificationRequested(AssistantClarificationRequestedEvent {
                run_id,
                request_id: clarification_request_id,
                prompt: UiSafeText::from_trusted_redacted(
                    "Which style should I use with sk-synthetic?",
                ),
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(7),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            8,
            Event::AssistantNotice(AssistantNoticeEvent {
                run_id,
                notice_id,
                body: UiSafeText::from_trusted_redacted(
                    "Tool output was summarized from /home/example/private.",
                ),
                code: None,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(8),
            }),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let event_types = page
        .events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        event_types,
        vec![
            "run.started",
            "tool.requested",
            "tool.completed",
            "permission.requested",
            "permission.resolved",
            "artifact.created",
            "assistant.review.requested",
            "assistant.clarification.requested",
            "assistant.notice"
        ]
    );
    assert_eq!(
        page.events[1].payload["argumentsSummary"],
        "Input withheld from conversation timeline."
    );
    assert_eq!(page.events[2].payload["outputSummary"], "wrote [REDACTED]");
    assert!(!page.events[2]
        .payload
        .to_string()
        .contains("/Users/goya/.ssh/config"));
    assert_eq!(page.events[3].payload["operation"], "Execute command");
    assert_eq!(page.events[3].payload["target"], "rm");
    assert_eq!(page.events[3].payload["toolUseId"], tool_use_id.to_string());
    assert_eq!(page.events[4].payload["decision"], "deny");
    assert_eq!(page.events[5].payload["artifactId"], "artifact-001");
    assert_eq!(page.events[5].payload["title"], "Report");
    assert_eq!(page.events[5].payload["summary"], "Image artifact ready");
    assert!(page.events[5].payload.get("blobRef").is_none());
    assert!(page.events[5].payload.get("contentHash").is_none());
    assert_eq!(
        page.events[6].payload["requestId"],
        review_request_id.to_string()
    );
    assert_eq!(
        page.events[6].payload["title"],
        "Review [REDACTED] [REDACTED] [REDACTED]"
    );
    assert_eq!(
        page.events[6].payload["body"],
        "Confirm before applying [REDACTED]"
    );
    assert_eq!(
        page.events[7].payload["requestId"],
        clarification_request_id.to_string()
    );
    assert_eq!(
        page.events[7].payload["prompt"],
        "Which style should I use with [REDACTED]"
    );
    assert_eq!(page.events[8].payload["noticeId"], notice_id.to_string());
    assert_eq!(
        page.events[8].payload["body"],
        "Tool output was summarized from [REDACTED]"
    );
}

#[tokio::test]
async fn sqlite_conversation_read_model_omits_empty_assistant_review_body() {
    let root = temp_root("review-body-omitted");
    let store = SqliteConversationReadModelStore::open(root.join("read-model.sqlite"))
        .await
        .expect("store opens");
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let envelopes = vec![
        envelope(
            tenant_id,
            session_id,
            0,
            Event::RunStarted(RunStartedEvent {
                run_id,
                session_id,
                tenant_id,
                parent_run_id: None,
                input: TurnInput {
                    message: Message {
                        id: MessageId::new(),
                        role: MessageRole::User,
                        parts: vec![MessagePart::Text("run".to_owned())],
                        created_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
                    },
                    metadata: serde_json::Value::Null,
                },
                snapshot_id: SnapshotId::new(),
                effective_config_hash: ConfigHash([0; 32]),
                started_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
                correlation_id: CorrelationId::new(),
            }),
        ),
        envelope(
            tenant_id,
            session_id,
            1,
            Event::AssistantReviewRequested(AssistantReviewRequestedEvent {
                run_id,
                request_id,
                title: UiSafeText::from_trusted_redacted("Review changes"),
                body: None,
                at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::seconds(1),
            }),
        ),
    ];

    store
        .apply_envelopes(tenant_id, session_id, &envelopes, None)
        .await
        .expect("projection applies");

    let page = store
        .page_timeline(tenant_id, session_id, None, 20)
        .await
        .expect("timeline loads");
    let review = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.review.requested")
        .expect("review event is projected");

    assert_eq!(review.payload["requestId"], request_id.to_string());
    assert_eq!(review.payload["title"], "Review changes");
    assert!(review.payload.get("body").is_none());
}
