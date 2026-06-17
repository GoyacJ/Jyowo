use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use harness_context::{CompactHint, ContextBuffer, ContextEngine, ContextOutcome, ContextProvider};
use harness_contracts::{
    ContextError, ContextStageId, Message, MessageId, MessagePart, MessageRole, SessionId,
    ToolUseId,
};

#[test]
fn compact_stage_order_is_fixed_contract() {
    assert_eq!(
        ContextEngine::compact_stage_order(),
        [
            ContextStageId::ToolResultBudget,
            ContextStageId::Snip,
            ContextStageId::Microcompact,
            ContextStageId::Collapse,
            ContextStageId::Autocompact,
        ]
    );
}

#[tokio::test]
async fn provider_cannot_mutate_frozen_context() {
    let engine = ContextEngine::builder()
        .with_provider(FrozenMutatingProvider)
        .build()
        .unwrap();
    let mut buffer = ContextBuffer::default();

    let error = engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ContextError::Internal("context provider mutated frozen context".to_owned())
    );
}

#[tokio::test]
async fn active_history_modifications_refresh_pairs_and_token_estimate() {
    let tool_use_id = ToolUseId::new();
    let mut buffer = ContextBuffer::new(harness_contracts::TenantId::SINGLE, SessionId::new());
    buffer.active.history = vec![tool_use_message(tool_use_id)];
    buffer.rebuild_tool_use_pairs();
    assert_eq!(buffer.active.tool_use_pairs[0].tool_result_message_id, None);
    assert_eq!(buffer.bookkeeping.estimated_tokens, 0);

    let engine = ContextEngine::builder()
        .with_provider(AppendToolResultProvider { tool_use_id })
        .build()
        .unwrap();

    engine
        .compact(&mut buffer, CompactHint::default())
        .await
        .unwrap();

    assert_eq!(buffer.active.tool_use_pairs.len(), 1);
    assert!(buffer.active.tool_use_pairs[0]
        .tool_result_message_id
        .is_some());
    assert!(buffer.bookkeeping.estimated_tokens > 0);
}

#[test]
fn context_crate_contract_dependency_boundary() {
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
        "frozen-mutator-contract"
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

struct AppendToolResultProvider {
    tool_use_id: ToolUseId,
}

#[async_trait]
impl ContextProvider for AppendToolResultProvider {
    fn provider_id(&self) -> &'static str {
        "append-tool-result"
    }

    fn stage(&self) -> ContextStageId {
        ContextStageId::Snip
    }

    async fn apply(
        &self,
        ctx: &mut ContextBuffer,
        _hint: &CompactHint,
    ) -> Result<ContextOutcome, ContextError> {
        ctx.active.history.push(Message {
            id: MessageId::new(),
            role: MessageRole::Tool,
            parts: vec![MessagePart::ToolResult {
                tool_use_id: self.tool_use_id,
                content: harness_contracts::ToolResult::Text("result".to_owned()),
            }],
            created_at: Utc::now(),
        });
        Ok(ContextOutcome::Modified { bytes_saved: 0 })
    }
}

fn tool_use_message(tool_use_id: ToolUseId) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::Assistant,
        parts: vec![MessagePart::ToolUse {
            id: tool_use_id,
            name: "grep".to_owned(),
            input: serde_json::json!({}),
        }],
        created_at: Utc::now(),
    }
}
