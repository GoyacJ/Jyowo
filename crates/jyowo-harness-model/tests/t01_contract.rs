use std::sync::Arc;

use futures::stream;
use harness_contracts::{
    BudgetMetric, DeferPolicy, Message, ModelError, OverflowAction, ProviderRestriction,
    ResultBudget, TenantId, ToolDescriptor, ToolGroup, ToolOrigin, ToolProperties, TrustLevel,
};
use harness_model::*;

struct TestProvider;

#[async_trait::async_trait]
impl ModelProvider for TestProvider {
    fn provider_id(&self) -> &str {
        TEST_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "test".to_owned(),
            model_id: "test-model".to_owned(),
            display_name: "Test model".to_owned(),
            context_window: 128_000,
            max_output_tokens: 8192,
            conversation_capability: ConversationModelCapability::default(),
            pricing: None,
        }]
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::Responses
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> std::result::Result<ModelStream, ModelError> {
        Ok(Box::pin(stream::iter([ModelStreamEvent::MessageStop])))
    }
}

struct TestMiddleware;

#[async_trait::async_trait]
impl InferMiddleware for TestMiddleware {
    fn middleware_id(&self) -> &str {
        TEST_ID
    }
}

const TEST_ID: &str = "test";

#[test]
fn model_provider_is_dyn_safe_with_prompt_cache_default() {
    let provider: Arc<dyn ModelProvider> = Arc::new(TestProvider);

    assert_eq!(provider.provider_id(), "test");
    assert_eq!(provider.prompt_cache_style(), PromptCacheStyle::None);
    assert_eq!(
        provider.supported_models()[0].conversation_capability,
        ConversationModelCapability::default()
    );
}

#[test]
fn snapshot_for_model_uses_descriptor_protocol_and_default_fallback() {
    let provider = TestProvider;

    let known = provider.snapshot_for_model("test-model");
    assert_eq!(known.protocol, ModelProtocol::Messages);
    assert_eq!(known.lifecycle, ModelLifecycle::Stable);

    let fallback = provider.snapshot_for_model("unknown-model");
    assert_eq!(fallback.protocol, ModelProtocol::Responses);
    assert_eq!(fallback.lifecycle, ModelLifecycle::Stable);
}

#[test]
fn credential_key_is_tenant_scoped() {
    let first = CredentialKey {
        tenant_id: TenantId::SINGLE,
        provider_id: "anthropic".to_owned(),
        key_label: "primary".to_owned(),
    };
    let second = CredentialKey {
        tenant_id: TenantId::SHARED,
        provider_id: "anthropic".to_owned(),
        key_label: "primary".to_owned(),
    };

    assert_ne!(first, second);
}

#[test]
fn token_counter_and_middleware_defaults_are_noop() {
    struct Counter;
    impl TokenCounter for Counter {
        fn count_tokens(&self, text: &str, _model: &str) -> usize {
            text.len()
        }

        fn count_messages(&self, messages: &[Message], _model: &str) -> usize {
            messages.len()
        }
    }

    let image = ImageMeta {
        width: 100,
        height: 100,
        mime: "image/png".to_owned(),
        detail: ImageDetail::Auto,
    };
    assert_eq!(Counter.count_image(&image, "unknown"), None);

    let middleware = TestMiddleware;
    let input: ModelStream = Box::pin(stream::empty());
    let ctx = InferContext::for_test();
    let output = middleware.wrap_stream(input, &ctx);
    drop(output);
}

#[test]
fn model_request_accepts_contract_tool_descriptor() {
    let req = ModelRequest {
        model_id: "test-model".to_owned(),
        messages: Vec::new(),
        tools: Some(vec![ToolDescriptor {
            name: "read_file".to_owned(),
            display_name: "Read file".to_owned(),
            description: "Read a file".to_owned(),
            category: "filesystem".to_owned(),
            group: ToolGroup::FileSystem,
            version: "1.0.0".to_owned(),
            input_schema: serde_json::json!({ "type": "object" }),
            output_schema: None,
            dynamic_schema: false,
            properties: ToolProperties {
                is_concurrency_safe: true,
                is_read_only: true,
                is_destructive: false,
                long_running: None,
                defer_policy: DeferPolicy::AlwaysLoad,
            },
            trust_level: TrustLevel::AdminTrusted,
            required_capabilities: Vec::new(),
            budget: ResultBudget {
                metric: BudgetMetric::Chars,
                limit: 4096,
                on_overflow: OverflowAction::Offload,
                preview_head_chars: 512,
                preview_tail_chars: 512,
            },
            provider_restriction: ProviderRestriction::All,
            origin: ToolOrigin::Builtin,
            search_hint: None,
        }]),
        system: None,
        temperature: None,
        max_tokens: Some(1024),
        stream: true,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::Messages,
        extra: serde_json::Value::Null,
    };

    assert_eq!(req.tools.unwrap()[0].name, "read_file");
}
