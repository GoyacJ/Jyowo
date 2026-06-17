use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::{CacheImpact, HarnessError, RunId, SessionId};
use harness_model::ModelCapabilities;
use harness_tool_search::{
    AnthropicToolReferenceBackend, DefaultBackendSelector, InlineReinjectionBackend,
    MaterializationCoalescer, MaterializeOutcome, ReloadHandle, ToolLoadingBackend,
    ToolLoadingBackendSelector, ToolLoadingContext, ToolLoadingError,
};

#[tokio::test]
async fn anthropic_backend_emits_tool_references() {
    let backend = AnthropicToolReferenceBackend;
    let outcome = backend
        .materialize(&ctx(None, true), &["Read".to_owned(), "Write".to_owned()])
        .await
        .unwrap();

    assert_eq!(backend.backend_name(), "anthropic_tool_reference");
    assert_eq!(
        outcome,
        MaterializeOutcome::ToolReferenceEmitted {
            refs: vec![
                harness_tool_search::ToolReference {
                    tool_name: "Read".to_owned()
                },
                harness_tool_search::ToolReference {
                    tool_name: "Write".to_owned()
                }
            ]
        }
    );
}

#[tokio::test]
async fn inline_backend_requires_reload_handle() {
    let backend = InlineReinjectionBackend::new(MaterializationCoalescer::new(Duration::ZERO, 32));
    let error = backend
        .materialize(&ctx(None, false), &["Read".to_owned()])
        .await
        .unwrap_err();

    assert!(matches!(error, ToolLoadingError::ReloadHandleMissing));
}

#[tokio::test]
async fn inline_backend_reloads_with_requested_tools() {
    let handle = Arc::new(RecordingReload::default());
    let backend = InlineReinjectionBackend::new(MaterializationCoalescer::new(Duration::ZERO, 32));
    let outcome = backend
        .materialize(&ctx(Some(handle.clone()), false), &["Read".to_owned()])
        .await
        .unwrap();

    assert_eq!(handle.calls().await, vec![vec!["Read".to_owned()]]);
    assert_eq!(
        outcome,
        MaterializeOutcome::InlineReinjected {
            tools: vec!["Read".to_owned()],
            cache_impact: CacheImpact {
                prompt_cache_invalidated: true,
                reason: Some("test reload".to_owned()),
            },
        }
    );
}

#[tokio::test]
async fn default_selector_prefers_anthropic_when_model_supports_references() {
    let selector = DefaultBackendSelector::new(
        Arc::new(AnthropicToolReferenceBackend),
        Arc::new(InlineReinjectionBackend::new(
            MaterializationCoalescer::new(Duration::ZERO, 32),
        )),
    );

    assert_eq!(
        selector.select(&ctx(None, true)).await.backend_name(),
        "anthropic_tool_reference"
    );
    assert_eq!(
        selector
            .select(&ctx(Some(Arc::new(RecordingReload::default())), false))
            .await
            .backend_name(),
        "inline_reinjection"
    );
}

fn ctx(
    reload_handle: Option<Arc<dyn ReloadHandle>>,
    supports_tool_reference: bool,
) -> ToolLoadingContext {
    let caps = ModelCapabilities {
        supports_tool_reference,
        ..Default::default()
    };
    ToolLoadingContext {
        session_id: SessionId::new(),
        run_id: RunId::new(),
        model_caps: Arc::new(caps),
        reload_handle,
    }
}

#[derive(Default)]
struct RecordingReload {
    calls: tokio::sync::Mutex<Vec<Vec<String>>>,
}

impl RecordingReload {
    async fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().await.clone()
    }
}

#[async_trait]
impl ReloadHandle for RecordingReload {
    async fn reload_with_add_tools(&self, tools: Vec<String>) -> Result<CacheImpact, HarnessError> {
        self.calls.lock().await.push(tools);
        Ok(CacheImpact {
            prompt_cache_invalidated: true,
            reason: Some("test reload".to_owned()),
        })
    }
}
