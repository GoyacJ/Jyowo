use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ConversationModelCapability, ConversationTurnInput, DeferPolicy, ModelError, ModelProtocol,
    NoopRedactor, ProviderRestriction, SessionId, TenantId, ToolDescriptor, ToolError, ToolGroup,
    ToolOrigin, ToolProperties, TrustLevel, UsageSnapshot,
};
use harness_journal::InMemoryEventStore;
use harness_model::{
    ContentDelta, InferContext, ModelDescriptor, ModelLifecycle, ModelProvider, ModelRequest,
    ModelRuntimeSemantics, ModelStream, ModelStreamEvent,
};
use harness_provider_state::{
    ProviderContinuationKind, ProviderContinuationQuery, ProviderContinuationRecord,
    ProviderContinuationStore, ProviderContinuationStoreError,
};
use harness_sandbox::NoopSandbox;
use harness_tool::{
    default_result_budget, PermissionCheck, SchemaResolverContext, Tool, ToolContext, ToolRegistry,
    ToolStream, ValidationError,
};
use jyowo_harness_sdk::{ConversationRunOptions, ConversationTurnRequest, Harness, SessionOptions};
use serde_json::{json, Value};

#[tokio::test]
async fn sdk_passes_provider_continuation_store_to_engine() {
    let workspace = unique_workspace("sdk-provider-continuation-store");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = SessionId::new();
    let store = Arc::new(RecordingProviderContinuationStore::default());
    let model = DeepSeekSemanticsProvider::new(vec![
        ModelStreamEvent::MessageStart {
            message_id: "provider-assistant".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload: json!({ "private": "sdk-store-sentinel" }),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("done".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ]);

    let harness = Harness::builder()
        .with_model_arc(model)
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(NoopSandbox::new())
        .with_provider_continuation_store_arc(store.clone())
        .build()
        .await
        .expect("harness should build");

    let options = SessionOptions::new(&workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .expect("session should open");

    harness
        .submit_conversation_turn(conversation_turn_request(
            options,
            ConversationTurnInput::ask("capture private continuation"),
        ))
        .await
        .expect("turn should run");

    let appended = store.appended.lock().unwrap();
    assert_eq!(appended.len(), 1);
    assert_eq!(appended[0].tenant_id, TenantId::SINGLE);
    assert_eq!(appended[0].session_id, session_id);
    assert_eq!(appended[0].kind, ProviderContinuationKind::ReasoningReplay);
    assert_eq!(appended[0].payload["private"], "sdk-store-sentinel");
}

#[tokio::test]
async fn runtime_with_deepseek_semantics_requires_provider_continuation_store() {
    let workspace = unique_workspace("sdk-provider-continuation-required");
    std::fs::create_dir_all(&workspace).unwrap();
    let model = DeepSeekSemanticsProvider::new(vec![ModelStreamEvent::MessageStop]);

    let harness = Harness::builder()
        .with_model_arc(model.clone())
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(NoopSandbox::new())
        .with_tool_registry(
            ToolRegistry::builder()
                .with_tool(Box::new(TestTool::new("required_tool")))
                .build()
                .expect("tool registry"),
        )
        .build()
        .await
        .expect("harness should build");

    let options = SessionOptions::new(&workspace);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .expect("session should open");

    let error = harness
        .submit_conversation_turn(conversation_turn_request(
            options,
            ConversationTurnInput::ask("tool-capable deepseek turn"),
        ))
        .await
        .expect_err("DeepSeek tool-capable runtime must require a continuation store");

    assert!(error
        .to_string()
        .contains("provider continuation required for assistant tool replay but missing"));
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn sdk_delete_conversation_session_prunes_provider_continuations() {
    let workspace = unique_workspace("sdk-provider-continuation-prune");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = SessionId::new();
    let store = Arc::new(RecordingProviderContinuationStore::default());
    let harness = Harness::builder()
        .with_model_arc(DeepSeekSemanticsProvider::new(vec![
            ModelStreamEvent::MessageStop,
        ]))
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(NoopSandbox::new())
        .with_provider_continuation_store_arc(store.clone())
        .build()
        .await
        .expect("harness should build");

    harness
        .open_or_create_conversation_session(
            SessionOptions::new(&workspace).with_session_id(session_id),
        )
        .await
        .expect("session should open");

    let deleted = harness
        .delete_conversation_session(SessionOptions::new(&workspace).with_session_id(session_id))
        .await
        .expect("delete should prune continuation store");

    assert!(deleted);
    assert_eq!(
        store.pruned.lock().unwrap().as_slice(),
        &[(TenantId::SINGLE, session_id)]
    );
}

#[tokio::test]
async fn sdk_delete_conversation_session_returns_safe_error_when_prune_fails() {
    let workspace = unique_workspace("sdk-provider-continuation-prune-error");
    std::fs::create_dir_all(&workspace).unwrap();
    let session_id = SessionId::new();
    let store = Arc::new(RecordingProviderContinuationStore {
        fail_prune: true,
        ..Default::default()
    });
    let harness = Harness::builder()
        .with_model_arc(DeepSeekSemanticsProvider::new(vec![
            ModelStreamEvent::MessageStop,
        ]))
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(NoopSandbox::new())
        .with_provider_continuation_store_arc(store)
        .build()
        .await
        .expect("harness should build");

    harness
        .open_or_create_conversation_session(
            SessionOptions::new(&workspace).with_session_id(session_id),
        )
        .await
        .expect("session should open");

    let error = harness
        .delete_conversation_session(SessionOptions::new(&workspace).with_session_id(session_id))
        .await
        .expect_err("prune failure should fail the delete");
    let message = error.to_string();

    assert!(message.contains("provider continuation pruning failed"));
    assert!(!message.contains("secret-continuation-payload"));
}

fn conversation_turn_request(
    options: SessionOptions,
    input: ConversationTurnInput,
) -> ConversationTurnRequest {
    ConversationTurnRequest {
        run_options: ConversationRunOptions::from_session_options(&options),
        options,
        input,
        permission_actor_source: None,
    }
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-{name}-{}-{}",
        std::process::id(),
        SessionId::new()
    ))
}

#[derive(Default)]
struct RecordingProviderContinuationStore {
    appended: Mutex<Vec<ProviderContinuationRecord>>,
    loaded: Mutex<Vec<ProviderContinuationQuery>>,
    pruned: Mutex<Vec<(TenantId, SessionId)>>,
    fail_prune: bool,
}

#[async_trait]
impl ProviderContinuationStore for RecordingProviderContinuationStore {
    async fn load_for_messages(
        &self,
        query: ProviderContinuationQuery,
    ) -> Result<Vec<ProviderContinuationRecord>, ProviderContinuationStoreError> {
        self.loaded.lock().unwrap().push(query);
        Ok(Vec::new())
    }

    async fn append_batch(
        &self,
        records: Vec<ProviderContinuationRecord>,
    ) -> Result<(), ProviderContinuationStoreError> {
        self.appended.lock().unwrap().extend(records);
        Ok(())
    }

    async fn prune_session(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<(), ProviderContinuationStoreError> {
        self.pruned.lock().unwrap().push((tenant_id, session_id));
        if self.fail_prune {
            return Err(ProviderContinuationStoreError::CorruptRecord {
                line: 1,
                details: "secret-continuation-payload".to_owned(),
            });
        }
        Ok(())
    }
}

struct DeepSeekSemanticsProvider {
    events: Mutex<Vec<ModelStreamEvent>>,
    calls: AtomicUsize,
}

impl DeepSeekSemanticsProvider {
    fn new(events: Vec<ModelStreamEvent>) -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(events),
            calls: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl ModelProvider for DeepSeekSemanticsProvider {
    fn provider_id(&self) -> &str {
        "deepseek"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            provider_id: "deepseek".to_owned(),
            model_id: "deepseek-chat".to_owned(),
            display_name: "DeepSeek Chat".to_owned(),
            protocol: ModelProtocol::ChatCompletions,
            context_window: 128_000,
            max_output_tokens: 8_192,
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: ModelRuntimeSemantics::openai_chat_deepseek(),
            lifecycle: ModelLifecycle::Stable,
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(stream::iter(self.events.lock().unwrap().clone())))
    }
}

struct TestTool {
    descriptor: ToolDescriptor,
}

impl TestTool {
    fn new(name: &str) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: name.to_owned(),
                description: "test tool".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.1.0".to_owned(),
                input_schema: json!({ "type": "object" }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::UserControlled,
                required_capabilities: Vec::new(),
                budget: default_result_budget(),
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
            },
        }
    }
}

#[async_trait]
impl Tool for TestTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn resolve_schema(&self, _ctx: &SchemaResolverContext) -> Result<Value, ToolError> {
        Ok(self.descriptor.input_schema.clone())
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn check_permission(&self, _input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        PermissionCheck::Allowed
    }

    async fn execute(&self, _input: Value, _ctx: ToolContext) -> Result<ToolStream, ToolError> {
        Ok(Box::pin(stream::empty()))
    }
}
