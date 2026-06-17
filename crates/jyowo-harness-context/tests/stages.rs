use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use futures::stream::{self, BoxStream};
use harness_context::{
    CollapseProvider, CompactHint, ContextBuffer, ContextEngine, ContextOutcome, ContextProvider,
    SnipProvider, ToolResultBudgetProvider,
};
use harness_contracts::{
    BlobError, BlobMeta, BlobRef, BlobStore, ContextError, ContextStageId, Message, MessageId,
    MessagePart, MessageRole, SessionId, TenantId, ToolResult, ToolUseId,
};

#[tokio::test]
async fn tool_result_budget_offloads_oversized_result_once() {
    let session_id = SessionId::new();
    let blob_store = Arc::new(RecordingBlobStore::default());
    let tool_use_id = ToolUseId::new();
    let result_message_id = MessageId::new();
    let mut buffer = ContextBuffer::new(TenantId::SINGLE, session_id);
    buffer.active.history = vec![tool_result_message(
        result_message_id,
        tool_use_id,
        "abcdefghijklmnopqrstuvwxyz",
    )];

    let engine = ContextEngine::builder()
        .with_provider(ToolResultBudgetProvider::new(10, blob_store.clone()))
        .build()
        .unwrap();

    let first = engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap();
    let second = engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap();

    assert_eq!(first, ContextOutcome::Modified { bytes_saved: 26 });
    assert_eq!(second, ContextOutcome::NoChange);
    assert!(buffer.bookkeeping.offloads.contains_key(&result_message_id));
    assert_eq!(blob_store.put_count().await, 1);
    assert_eq!(
        blob_store.last_meta().await.unwrap().retention_session(),
        session_id
    );
    assert_tool_result_text_contains(&buffer.active.history[0], "[TOOL_RESULT_TRUNCATED:");
}

#[tokio::test]
async fn tool_result_budget_returns_offload_error() {
    let mut buffer = ContextBuffer::new(TenantId::SINGLE, SessionId::new());
    buffer.active.history = vec![tool_result_message(
        MessageId::new(),
        ToolUseId::new(),
        "abcdefghijklmnopqrstuvwxyz",
    )];
    let engine = ContextEngine::builder()
        .with_provider(ToolResultBudgetProvider::new(
            10,
            Arc::new(FailingBlobStore),
        ))
        .build()
        .unwrap();

    let error = engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap_err();

    assert_eq!(error, ContextError::OffloadFailed("boom".to_owned()));
}

#[tokio::test]
async fn snip_drops_oldest_messages_and_preserves_recent_window() {
    let mut buffer = ContextBuffer::new(TenantId::SINGLE, SessionId::new());
    buffer.active.history = vec![
        text_message(MessageRole::User, "old one"),
        text_message(MessageRole::Assistant, "old two"),
        text_message(MessageRole::User, "recent one"),
        text_message(MessageRole::Assistant, "recent two"),
        text_message(MessageRole::User, "recent three"),
    ];
    buffer.bookkeeping.estimated_tokens = 500;
    let protected = ids(&buffer.active.history[2..]);
    let engine = ContextEngine::builder()
        .with_provider(SnipProvider::new(3))
        .build()
        .unwrap();

    let outcome = engine
        .compact(
            &mut buffer,
            CompactHint {
                estimated_tokens: 500,
                target_tokens: Some(4),
            },
        )
        .await
        .unwrap();

    assert!(matches!(outcome, ContextOutcome::Modified { .. }));
    assert_eq!(ids(&buffer.active.history), protected);
}

#[tokio::test]
async fn snip_drops_completed_tool_pair_together() {
    let tool_use_id = ToolUseId::new();
    let tool_use_message = Message {
        id: MessageId::new(),
        role: MessageRole::Assistant,
        parts: vec![MessagePart::ToolUse {
            id: tool_use_id,
            name: "grep".to_owned(),
            input: serde_json::json!({ "query": "needle" }),
        }],
        created_at: Utc::now(),
    };
    let tool_result_message = tool_result_message(MessageId::new(), tool_use_id, "result");
    let mut buffer = ContextBuffer::new(TenantId::SINGLE, SessionId::new());
    buffer.active.history = vec![
        tool_use_message.clone(),
        text_message(MessageRole::User, "middle"),
        tool_result_message.clone(),
        text_message(MessageRole::User, "recent one"),
        text_message(MessageRole::Assistant, "recent two"),
        text_message(MessageRole::User, "recent three"),
    ];
    let engine = ContextEngine::builder()
        .with_provider(SnipProvider::new(3))
        .build()
        .unwrap();

    engine
        .compact(
            &mut buffer,
            CompactHint {
                estimated_tokens: 500,
                target_tokens: Some(4),
            },
        )
        .await
        .unwrap();

    let remaining = ids(&buffer.active.history);
    assert!(!remaining.contains(&tool_use_message.id));
    assert!(!remaining.contains(&tool_result_message.id));
    assert!(buffer.active.tool_use_pairs.is_empty());
}

#[tokio::test]
async fn snip_keeps_pending_tool_use_pair() {
    let tool_use_id = ToolUseId::new();
    let old_message = text_message(MessageRole::User, "old");
    let tool_use_message = Message {
        id: MessageId::new(),
        role: MessageRole::Assistant,
        parts: vec![MessagePart::ToolUse {
            id: tool_use_id,
            name: "grep".to_owned(),
            input: serde_json::json!({}),
        }],
        created_at: Utc::now(),
    };
    let mut buffer = ContextBuffer::new(TenantId::SINGLE, SessionId::new());
    buffer.active.history = vec![
        tool_use_message.clone(),
        old_message.clone(),
        text_message(MessageRole::User, "recent one"),
        text_message(MessageRole::Assistant, "recent two"),
        text_message(MessageRole::User, "recent three"),
    ];
    let engine = ContextEngine::builder()
        .with_provider(SnipProvider::new(3))
        .build()
        .unwrap();

    engine
        .compact(
            &mut buffer,
            CompactHint {
                estimated_tokens: 500,
                target_tokens: Some(4),
            },
        )
        .await
        .unwrap();

    assert!(ids(&buffer.active.history).contains(&tool_use_message.id));
    assert!(!ids(&buffer.active.history).contains(&old_message.id));
    assert_eq!(buffer.active.tool_use_pairs.len(), 1);
    assert_eq!(buffer.active.tool_use_pairs[0].tool_result_message_id, None);
}

#[tokio::test]
async fn snip_does_not_drop_pair_that_crosses_recent_window() {
    let tool_use_id = ToolUseId::new();
    let tool_use_message = tool_use_message(tool_use_id, "grep");
    let tool_result_message = tool_result_message(MessageId::new(), tool_use_id, "protected");
    let mut buffer = ContextBuffer::new(TenantId::SINGLE, SessionId::new());
    buffer.active.history = vec![
        tool_use_message.clone(),
        text_message(MessageRole::User, "drop me"),
        tool_result_message.clone(),
        text_message(MessageRole::Assistant, "recent two"),
        text_message(MessageRole::User, "recent three"),
    ];
    let engine = ContextEngine::builder()
        .with_provider(SnipProvider::new(3))
        .build()
        .unwrap();

    engine
        .compact(
            &mut buffer,
            CompactHint {
                estimated_tokens: 500,
                target_tokens: Some(4),
            },
        )
        .await
        .unwrap();

    let remaining = ids(&buffer.active.history);
    assert!(remaining.contains(&tool_use_message.id));
    assert!(remaining.contains(&tool_result_message.id));
}

#[tokio::test]
async fn snip_cleans_offloads_for_dropped_messages() {
    let dropped = text_message(MessageRole::User, "old offloaded");
    let blob_ref = blob_ref(20);
    let mut buffer = ContextBuffer::new(TenantId::SINGLE, SessionId::new());
    buffer.bookkeeping.offloads.insert(dropped.id, blob_ref);
    buffer.active.history = vec![
        dropped.clone(),
        text_message(MessageRole::User, "recent one"),
        text_message(MessageRole::Assistant, "recent two"),
        text_message(MessageRole::User, "recent three"),
    ];
    let engine = ContextEngine::builder()
        .with_provider(SnipProvider::new(3))
        .build()
        .unwrap();

    engine
        .compact(
            &mut buffer,
            CompactHint {
                estimated_tokens: 500,
                target_tokens: Some(4),
            },
        )
        .await
        .unwrap();

    assert!(!buffer.bookkeeping.offloads.contains_key(&dropped.id));
}

#[tokio::test]
async fn collapse_merges_adjacent_same_tool_results() {
    let first_tool_use = ToolUseId::new();
    let second_tool_use = ToolUseId::new();
    let mut buffer = ContextBuffer::new(TenantId::SINGLE, SessionId::new());
    buffer.active.history = vec![
        tool_use_message(first_tool_use, "grep"),
        tool_result_message(MessageId::new(), first_tool_use, "alpha"),
        tool_use_message(second_tool_use, "grep"),
        tool_result_message(MessageId::new(), second_tool_use, "beta"),
        text_message(MessageRole::User, "recent"),
    ];
    let engine = ContextEngine::builder()
        .with_provider(CollapseProvider::new(100))
        .build()
        .unwrap();

    let outcome = engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap();

    assert!(matches!(outcome, ContextOutcome::Modified { .. }));
    let tool_results = buffer
        .active
        .history
        .iter()
        .flat_map(|message| &message.parts)
        .filter(|part| matches!(part, MessagePart::ToolResult { .. }))
        .count();
    assert_eq!(tool_results, 2);
    assert_eq!(buffer.active.tool_use_pairs.len(), 2);
    assert_eq!(
        buffer.active.tool_use_pairs[0].tool_result_message_id,
        buffer.active.tool_use_pairs[1].tool_result_message_id
    );
}

#[tokio::test]
async fn collapse_skips_different_non_adjacent_or_large_results() {
    let first = ToolUseId::new();
    let second = ToolUseId::new();
    let third = ToolUseId::new();
    let fourth = ToolUseId::new();
    let mut buffer = ContextBuffer::new(TenantId::SINGLE, SessionId::new());
    buffer.active.history = vec![
        tool_use_message(first, "grep"),
        tool_result_message(MessageId::new(), first, "alpha"),
        text_message(MessageRole::User, "separator"),
        tool_use_message(second, "grep"),
        tool_result_message(MessageId::new(), second, "beta"),
        tool_use_message(third, "cat"),
        tool_result_message(MessageId::new(), third, "gamma"),
        tool_use_message(fourth, "cat"),
        tool_result_message(MessageId::new(), fourth, "this result is too large"),
    ];
    let engine = ContextEngine::builder()
        .with_provider(CollapseProvider::new(8))
        .build()
        .unwrap();

    let outcome = engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap();

    assert_eq!(outcome, ContextOutcome::NoChange);
}

#[tokio::test]
async fn engine_rejects_provider_that_mutates_frozen_context() {
    let mut buffer = ContextBuffer::new(TenantId::SINGLE, SessionId::new());
    let engine = ContextEngine::builder()
        .with_provider(FrozenMutatingProvider)
        .build()
        .unwrap();

    let error = engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ContextError::Internal("context provider mutated frozen context".to_owned())
    );
}

#[test]
fn context_crate_keeps_stage_dependency_boundary() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();

    assert!(!manifest.contains("jyowo-harness-tool"));
    assert!(!manifest.contains("jyowo-harness-session"));
    assert!(!manifest.contains("jyowo-harness-engine"));
    assert!(!manifest.contains("jyowo-harness-hook"));
    assert!(!manifest.contains("jyowo-harness-observability"));
}

struct FrozenMutatingProvider;

#[async_trait]
impl ContextProvider for FrozenMutatingProvider {
    fn provider_id(&self) -> &'static str {
        "frozen-mutator"
    }

    fn stage(&self) -> ContextStageId {
        ContextStageId::Snip
    }

    async fn apply(
        &self,
        ctx: &mut ContextBuffer,
        _hint: &CompactHint,
    ) -> Result<ContextOutcome, ContextError> {
        ctx.frozen.system_header = Some(Arc::from("mutated"));
        Ok(ContextOutcome::Modified { bytes_saved: 1 })
    }
}

#[derive(Default)]
struct RecordingBlobStore {
    writes: tokio::sync::Mutex<Vec<BlobMeta>>,
}

impl RecordingBlobStore {
    async fn put_count(&self) -> usize {
        self.writes.lock().await.len()
    }

    async fn last_meta(&self) -> Option<BlobMeta> {
        self.writes.lock().await.last().cloned()
    }
}

#[async_trait]
impl BlobStore for RecordingBlobStore {
    fn store_id(&self) -> &'static str {
        "recording"
    }

    async fn put(
        &self,
        _tenant: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        self.writes.lock().await.push(meta.clone());
        Ok(BlobRef {
            id: harness_contracts::BlobId::new(),
            size: bytes.len() as u64,
            content_hash: *blake3::hash(&bytes).as_bytes(),
            content_type: meta.content_type,
        })
    }

    async fn get(
        &self,
        _tenant: TenantId,
        blob: &BlobRef,
    ) -> Result<BoxStream<'static, Bytes>, BlobError> {
        Err(BlobError::NotFound(blob.id))
    }

    async fn head(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<Option<BlobMeta>, BlobError> {
        Ok(None)
    }

    async fn delete(&self, _tenant: TenantId, _blob: &BlobRef) -> Result<(), BlobError> {
        Ok(())
    }
}

struct FailingBlobStore;

#[async_trait]
impl BlobStore for FailingBlobStore {
    fn store_id(&self) -> &'static str {
        "failing"
    }

    async fn put(
        &self,
        _tenant: TenantId,
        _bytes: Bytes,
        _meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        Err(BlobError::Backend("boom".to_owned()))
    }

    async fn get(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<BoxStream<'static, Bytes>, BlobError> {
        Ok(Box::pin(stream::empty()))
    }

    async fn head(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<Option<BlobMeta>, BlobError> {
        Ok(None)
    }

    async fn delete(&self, _tenant: TenantId, _blob: &BlobRef) -> Result<(), BlobError> {
        Ok(())
    }
}

trait RetentionSession {
    fn retention_session(&self) -> SessionId;
}

impl RetentionSession for BlobMeta {
    fn retention_session(&self) -> SessionId {
        match self.retention {
            harness_contracts::BlobRetention::SessionScoped(session_id) => session_id,
            _ => panic!("expected session scoped retention"),
        }
    }
}

fn text_message(role: MessageRole, text: &str) -> Message {
    Message {
        id: MessageId::new(),
        role,
        parts: vec![MessagePart::Text(text.to_owned())],
        created_at: Utc::now(),
    }
}

fn tool_use_message(tool_use_id: ToolUseId, name: &str) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::Assistant,
        parts: vec![MessagePart::ToolUse {
            id: tool_use_id,
            name: name.to_owned(),
            input: serde_json::json!({}),
        }],
        created_at: Utc::now(),
    }
}

fn tool_result_message(id: MessageId, tool_use_id: ToolUseId, text: &str) -> Message {
    Message {
        id,
        role: MessageRole::Tool,
        parts: vec![MessagePart::ToolResult {
            tool_use_id,
            content: ToolResult::Text(text.to_owned()),
        }],
        created_at: Utc::now(),
    }
}

fn assert_tool_result_text_contains(message: &Message, needle: &str) {
    let text = match &message.parts[0] {
        MessagePart::ToolResult {
            content: ToolResult::Text(text),
            ..
        } => text,
        other => panic!("unexpected tool result part: {other:?}"),
    };
    assert!(text.contains(needle), "{text}");
}

fn ids(messages: &[Message]) -> Vec<MessageId> {
    messages.iter().map(|message| message.id).collect()
}

fn blob_ref(size: u64) -> BlobRef {
    BlobRef {
        id: harness_contracts::BlobId::new(),
        size,
        content_hash: [7; 32],
        content_type: Some("text/plain".to_owned()),
    }
}
