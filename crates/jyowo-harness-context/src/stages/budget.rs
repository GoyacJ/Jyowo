use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use harness_contracts::{
    BlobError, BlobMeta, BlobRetention, BlobStore, ContextError, ContextStageId, MessagePart,
    ToolResult,
};

use crate::{CompactHint, ContextBuffer, ContextOutcome, ContextProvider};

use super::tool_result_text;

const TRUNCATED_CONTENT_TYPE: &str = "text/plain; charset=utf-8";

pub struct ToolResultBudgetProvider {
    per_tool_max_chars: u64,
    blob_offload: Arc<dyn BlobStore>,
}

impl ToolResultBudgetProvider {
    pub fn new(per_tool_max_chars: u64, blob_offload: Arc<dyn BlobStore>) -> Self {
        Self {
            per_tool_max_chars,
            blob_offload,
        }
    }
}

#[async_trait]
impl ContextProvider for ToolResultBudgetProvider {
    fn provider_id(&self) -> &'static str {
        "tool-result-budget"
    }

    fn stage(&self) -> ContextStageId {
        ContextStageId::ToolResultBudget
    }

    async fn apply(
        &self,
        ctx: &mut ContextBuffer,
        _hint: &CompactHint,
    ) -> Result<ContextOutcome, ContextError> {
        let mut bytes_saved = 0_u64;

        for message in &mut ctx.active.history {
            if ctx.bookkeeping.offloads.contains_key(&message.id) {
                continue;
            }

            for part in &mut message.parts {
                let MessagePart::ToolResult { content, .. } = part else {
                    continue;
                };
                let Some(text) = tool_result_text(content) else {
                    continue;
                };
                if text.chars().count() as u64 <= self.per_tool_max_chars {
                    continue;
                }

                let bytes = Bytes::from(text.clone());
                let meta = BlobMeta {
                    content_type: Some(TRUNCATED_CONTENT_TYPE.to_owned()),
                    size: bytes.len() as u64,
                    content_hash: *blake3::hash(&bytes).as_bytes(),
                    created_at: Utc::now(),
                    retention: BlobRetention::SessionScoped(ctx.identity.session_id),
                };
                let blob_ref = self
                    .blob_offload
                    .put(ctx.identity.tenant_id, bytes, meta)
                    .await
                    .map_err(offload_error)?;

                *content = ToolResult::Text(format!(
                    "[TOOL_RESULT_TRUNCATED: see blob {} size={}]",
                    blob_ref.id,
                    text.len()
                ));
                ctx.bookkeeping.offloads.insert(message.id, blob_ref);
                bytes_saved = bytes_saved.saturating_add(text.len() as u64);
            }
        }

        if bytes_saved == 0 {
            Ok(ContextOutcome::NoChange)
        } else {
            Ok(ContextOutcome::Modified { bytes_saved })
        }
    }
}

fn offload_error(error: BlobError) -> ContextError {
    match error {
        BlobError::Backend(detail) => ContextError::OffloadFailed(detail),
        other => ContextError::OffloadFailed(other.to_string()),
    }
}
