#![cfg(feature = "auto-mode")]

use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    Decision, DecisionScope, FallbackPolicy, InteractivityLevel, ModelError, PermissionMode,
    PermissionSubject, RequestId, Severity, TenantId, ToolUseId,
};
use harness_model::{
    AuxModelProvider, AuxOptions, AuxTask, ConversationModelCapability, HealthStatus, InferContext,
    ModelDescriptor, ModelProvider, ModelRequest, ModelStream,
};
use harness_permission::{AuxLlmBroker, PermissionBroker, PermissionContext, PermissionRequest};
use tokio::sync::Mutex;

#[tokio::test]
async fn aux_llm_broker_calls_permission_advisory_task() {
    let aux = Arc::new(RecordingAuxProvider::new(Ok("APPROVE".to_owned())));
    let broker = AuxLlmBroker::new(aux.clone());

    let decision = broker
        .decide(
            permission_request(),
            permission_context(PermissionMode::Auto),
        )
        .await;

    assert_eq!(decision, Decision::AllowOnce);
    assert_eq!(aux.tasks().await, vec![AuxTask::PermissionAdvisory]);
}

#[tokio::test]
async fn aux_llm_broker_fails_closed_on_aux_error() {
    let aux = Arc::new(RecordingAuxProvider::new(Err(
        ModelError::ProviderUnavailable("down".to_owned()),
    )));
    let broker = AuxLlmBroker::new(aux);

    let decision = broker
        .decide(
            permission_request(),
            permission_context(PermissionMode::Auto),
        )
        .await;

    assert_eq!(decision, Decision::DenyOnce);
}

#[tokio::test]
async fn aux_llm_broker_escalates_outside_auto_mode() {
    let aux = Arc::new(RecordingAuxProvider::new(Ok("APPROVE".to_owned())));
    let broker = AuxLlmBroker::new(aux.clone());

    let decision = broker
        .decide(
            permission_request(),
            permission_context(PermissionMode::Default),
        )
        .await;

    assert_eq!(decision, Decision::Escalate);
    assert!(aux.tasks().await.is_empty());
}

struct RecordingAuxProvider {
    result: Mutex<Result<String, ModelError>>,
    tasks: Mutex<Vec<AuxTask>>,
}

impl RecordingAuxProvider {
    fn new(result: Result<String, ModelError>) -> Self {
        Self {
            result: Mutex::new(result),
            tasks: Mutex::new(Vec::new()),
        }
    }

    async fn tasks(&self) -> Vec<AuxTask> {
        self.tasks.lock().await.clone()
    }
}

#[async_trait]
impl AuxModelProvider for RecordingAuxProvider {
    fn inner(&self) -> Arc<dyn ModelProvider> {
        Arc::new(DummyModelProvider)
    }

    fn aux_options(&self) -> AuxOptions {
        AuxOptions {
            fail_open: true,
            ..AuxOptions::default()
        }
    }

    async fn call_aux(&self, task: AuxTask, _req: ModelRequest) -> Result<String, ModelError> {
        self.tasks.lock().await.push(task);
        self.result.lock().await.clone()
    }
}

struct DummyModelProvider;

#[async_trait]
impl ModelProvider for DummyModelProvider {
    fn provider_id(&self) -> &str {
        "dummy"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "dummy".to_owned(),
            model_id: "dummy".to_owned(),
            display_name: "Dummy".to_owned(),
            context_window: 1_000,
            max_output_tokens: 100,
            conversation_capability: ConversationModelCapability::default(),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(futures::stream::empty()))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

fn permission_request() -> PermissionRequest {
    PermissionRequest {
        request_id: RequestId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: harness_contracts::SessionId::new(),
        tool_use_id: ToolUseId::new(),
        tool_name: "shell".to_owned(),
        subject: PermissionSubject::ToolInvocation {
            tool: "shell".to_owned(),
            input: serde_json::json!({ "command": "echo ok" }),
        },
        severity: Severity::Low,
        scope_hint: DecisionScope::ToolName("shell".to_owned()),
        created_at: harness_contracts::now(),
    }
}

fn permission_context(mode: PermissionMode) -> PermissionContext {
    PermissionContext {
        permission_mode: mode,
        previous_mode: None,
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        run_id: None,
        interactivity: InteractivityLevel::FullyInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::AskUser,
        hook_overrides: Vec::new(),
    }
}
