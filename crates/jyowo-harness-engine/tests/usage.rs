use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_context::ContextEngine;
use harness_contracts::NoopRedactor;
use harness_contracts::{
    CapabilityRegistry, Decision, Event, Message, MessageId, MessagePart, MessageRole, ModelError,
    PermissionError, PricingSnapshotId, StopReason, TenantId, TurnInput, UsageSnapshot,
};
use harness_engine::{Engine, EngineId, EngineRunner, RunContext, SessionHandle};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::InMemoryEventStore;
use harness_model::{
    BillingMode, ContentDelta, ConversationModelCapability, Currency, HealthStatus, InferContext,
    ModelDescriptor, ModelPricing, ModelProtocol, ModelProvider, ModelRequest, ModelStream,
    ModelStreamEvent, PricingSnapshotResolveContext, PricingSnapshotResolver, PricingSource,
};
use harness_observability::{Observer, UsageScope};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_tool::ToolPool;
use parking_lot::Mutex;
use rust_decimal::Decimal;

mod authorization_support;
use authorization_support::test_authorization_service;

#[tokio::test]
async fn engine_records_stream_usage_into_observer_and_usage_events() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = harness_contracts::SessionId::new();
    let run_id = harness_contracts::RunId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let observer = Arc::new(Observer::builder().build().unwrap());
    let model = Arc::new(UsageModel);

    let engine = Engine::builder()
        .with_engine_id(EngineId::new("usage-test"))
        .with_event_store(store.clone())
        .with_context(ContextEngine::builder().build().unwrap())
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(model)
        .with_tools(ToolPool::default())
        .with_authorization_service(test_authorization_service(
            Arc::new(DenyBroker),
            store.clone(),
        ))
        .with_workspace_root(workspace.path())
        .with_model_id("usage-model")
        .with_protocol(ModelProtocol::Messages)
        .with_cap_registry(Arc::new(CapabilityRegistry::default()))
        .with_observer(observer.clone())
        .build()
        .unwrap();

    let events = engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("count usage"),
            RunContext::new(tenant_id, session_id, run_id),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    let snapshot = PricingSnapshotId {
        pricing_id: "test-pricing".to_owned(),
        version: 3,
    };
    assert!(events.iter().any(|event| matches!(
        event,
        Event::AssistantMessageCompleted(completed)
            if completed.pricing_snapshot_id.as_ref() == Some(&snapshot)
                && completed.usage.input_tokens == 11
                && completed.usage.output_tokens == 7
                && completed.usage.cost_micros == 260
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::UsageAccumulated(usage)
            if usage.session_id == session_id
                && usage.run_id == Some(run_id)
                && usage.model_ref.as_ref().is_some_and(|model| model.model_id == "usage-model")
                && usage.pricing_snapshot_id.as_ref() == Some(&snapshot)
                && usage.delta.input_tokens == 11
                && usage.delta.output_tokens == 7
                && usage.delta.cost_micros == 260
    )));

    assert_eq!(
        observer
            .usage
            .snapshot(UsageScope::Tenant(tenant_id))
            .input_tokens,
        11
    );
    assert_eq!(
        observer
            .usage
            .snapshot(UsageScope::Session(session_id))
            .output_tokens,
        7
    );
    assert_eq!(
        observer
            .usage
            .snapshot(UsageScope::Run(run_id))
            .input_tokens,
        11
    );
    assert_eq!(
        observer
            .usage
            .snapshot(UsageScope::Model("test/usage-model".to_owned()))
            .output_tokens,
        7
    );
    assert_eq!(observer.usage.snapshot(UsageScope::Global).input_tokens, 11);
    assert_eq!(observer.usage.snapshot(UsageScope::Global).cost_micros, 260);

    let metrics = observer.model_metrics.report();
    let model_metrics = metrics.models.get("test/usage-model").unwrap();
    assert_eq!(model_metrics.infer_total, 1);
    assert_eq!(model_metrics.input_tokens, 11);
    assert_eq!(model_metrics.output_tokens, 7);
    assert_eq!(model_metrics.cache_read_tokens, 1);
}

#[tokio::test]
async fn usage_uses_session_pricing_snapshot() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = harness_contracts::SessionId::new();
    let run_id = harness_contracts::RunId::new();
    let model = Arc::new(MutablePricingModel::new(1));
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

    let engine = Engine::builder()
        .with_engine_id(EngineId::new("usage-pricing-snapshot-test"))
        .with_event_store(store.clone())
        .with_context(ContextEngine::builder().build().unwrap())
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(model.clone())
        .with_tools(ToolPool::default())
        .with_authorization_service(test_authorization_service(
            Arc::new(DenyBroker),
            store.clone(),
        ))
        .with_workspace_root(workspace.path())
        .with_model_id("usage-model")
        .with_protocol(ModelProtocol::Messages)
        .with_cap_registry(Arc::new(CapabilityRegistry::default()))
        .build()
        .unwrap();
    model.set_pricing_version(2).await;

    let events = engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("count usage"),
            RunContext::new(tenant_id, session_id, run_id),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    let snapshot = PricingSnapshotId {
        pricing_id: "mutable-pricing".to_owned(),
        version: 1,
    };
    assert!(events.iter().any(|event| matches!(
        event,
        Event::UsageAccumulated(usage)
            if usage.run_id == Some(run_id)
                && usage.pricing_snapshot_id.as_ref() == Some(&snapshot)
    )));
}

#[tokio::test]
async fn usage_uses_pricing_snapshot_resolver() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = harness_contracts::SessionId::new();
    let run_id = harness_contracts::RunId::new();
    let resolver = Arc::new(StaticPricingSnapshotResolver {
        snapshot: PricingSnapshotId {
            pricing_id: "resolver-pricing".to_owned(),
            version: 9,
        },
        calls: Mutex::new(Vec::new()),
    });
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

    let engine = Engine::builder()
        .with_engine_id(EngineId::new("usage-pricing-resolver-test"))
        .with_event_store(store.clone())
        .with_context(ContextEngine::builder().build().unwrap())
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(Arc::new(UsageModel))
        .with_tools(ToolPool::default())
        .with_authorization_service(test_authorization_service(
            Arc::new(DenyBroker),
            store.clone(),
        ))
        .with_workspace_root(workspace.path())
        .with_model_id("usage-model")
        .with_protocol(ModelProtocol::Messages)
        .with_cap_registry(Arc::new(CapabilityRegistry::default()))
        .with_pricing_snapshot_resolver(resolver.clone())
        .build()
        .unwrap();

    let events = engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("count usage"),
            RunContext::new(tenant_id, session_id, run_id),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert_eq!(resolver.calls.lock().len(), 1);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::UsageAccumulated(usage)
            if usage.run_id == Some(run_id)
                && usage.pricing_snapshot_id.as_ref().is_some_and(|snapshot|
                    snapshot.pricing_id == "resolver-pricing" && snapshot.version == 9)
    )));
}

#[tokio::test]
async fn pricing_snapshot_resolver_miss_is_reported_without_fallback_snapshot() {
    let workspace = tempfile::tempdir().unwrap();
    let tenant_id = TenantId::SINGLE;
    let session_id = harness_contracts::SessionId::new();
    let run_id = harness_contracts::RunId::new();
    let resolver = Arc::new(MissingPricingSnapshotResolver::default());
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

    let engine = Engine::builder()
        .with_engine_id(EngineId::new("usage-pricing-miss-test"))
        .with_event_store(store.clone())
        .with_context(ContextEngine::builder().build().unwrap())
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(Arc::new(UsageModel))
        .with_tools(ToolPool::default())
        .with_authorization_service(test_authorization_service(
            Arc::new(DenyBroker),
            store.clone(),
        ))
        .with_workspace_root(workspace.path())
        .with_model_id("usage-model")
        .with_protocol(ModelProtocol::Messages)
        .with_cap_registry(Arc::new(CapabilityRegistry::default()))
        .with_pricing_snapshot_resolver(resolver.clone())
        .build()
        .unwrap();

    let events = engine
        .run(
            SessionHandle {
                tenant_id,
                session_id,
            },
            turn_input("count usage"),
            RunContext::new(tenant_id, session_id, run_id),
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert_eq!(resolver.misses.lock().len(), 1);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::UsageAccumulated(usage)
            if usage.run_id == Some(run_id) && usage.pricing_snapshot_id.is_none()
    )));
}

fn turn_input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
        },
        metadata: serde_json::json!({}),
    }
}

struct MutablePricingModel {
    pricing_version: Mutex<u32>,
}

impl MutablePricingModel {
    fn new(pricing_version: u32) -> Self {
        Self {
            pricing_version: Mutex::new(pricing_version),
        }
    }

    async fn set_pricing_version(&self, pricing_version: u32) {
        *self.pricing_version.lock() = pricing_version;
    }
}

#[async_trait]
impl ModelProvider for MutablePricingModel {
    fn provider_id(&self) -> &'static str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        let pricing_version = self.pricing_version.lock();
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            supported_parameters: Vec::new(),
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "test".to_owned(),
            model_id: "usage-model".to_owned(),
            display_name: "Usage model".to_owned(),
            context_window: 8_000,
            max_output_tokens: 1_000,
            provider_declared_capability: ConversationModelCapability::default(),
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
                harness_model::ModelProtocol::Messages,
            ),
            pricing: Some(model_pricing("mutable-pricing", *pricing_version)),
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(stream::iter(usage_events())))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct UsageModel;

#[async_trait]
impl ModelProvider for UsageModel {
    fn provider_id(&self) -> &'static str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            supported_parameters: Vec::new(),
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "test".to_owned(),
            model_id: "usage-model".to_owned(),
            display_name: "Usage model".to_owned(),
            context_window: 8_000,
            max_output_tokens: 1_000,
            provider_declared_capability: ConversationModelCapability::default(),
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
                harness_model::ModelProtocol::Messages,
            ),
            pricing: Some(model_pricing("test-pricing", 3)),
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(stream::iter(usage_events())))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct StaticPricingSnapshotResolver {
    snapshot: PricingSnapshotId,
    calls: Mutex<Vec<PricingSnapshotResolveContext>>,
}

#[derive(Default)]
struct MissingPricingSnapshotResolver {
    misses: Mutex<Vec<PricingSnapshotResolveContext>>,
}

#[async_trait]
impl PricingSnapshotResolver for MissingPricingSnapshotResolver {
    async fn resolve(&self, _context: PricingSnapshotResolveContext) -> Option<PricingSnapshotId> {
        None
    }

    async fn record_miss(&self, context: PricingSnapshotResolveContext) {
        self.misses.lock().push(context);
    }
}

#[async_trait]
impl PricingSnapshotResolver for StaticPricingSnapshotResolver {
    async fn resolve(&self, context: PricingSnapshotResolveContext) -> Option<PricingSnapshotId> {
        self.calls.lock().push(context);
        Some(self.snapshot.clone())
    }
}

fn model_pricing(pricing_id: &str, pricing_version: u32) -> ModelPricing {
    ModelPricing {
        pricing_id: pricing_id.to_owned(),
        pricing_version,
        currency: Currency::Usd,
        input_per_million: Decimal::new(10, 0),
        output_per_million: Decimal::new(20, 0),
        cache_creation_per_million: None,
        cache_read_per_million: None,
        image_per_image: None,
        last_updated: harness_contracts::now(),
        source: PricingSource::BusinessProvided,
        billing_mode: BillingMode::Standard,
    }
}

fn usage_events() -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-1".to_owned(),
            usage: UsageSnapshot {
                input_tokens: 10,
                output_tokens: 0,
                cache_read_tokens: 1,
                cache_write_tokens: 0,
                cost_micros: 0,
                tool_calls: 0,
            },
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("done".to_owned()),
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            usage_delta: UsageSnapshot {
                input_tokens: 1,
                output_tokens: 7,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost_micros: 0,
                tool_calls: 0,
            },
        },
        ModelStreamEvent::MessageStop,
    ]
}

#[derive(Default)]
struct DenyBroker;

#[async_trait]
impl PermissionBroker for DenyBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::DenyOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}
