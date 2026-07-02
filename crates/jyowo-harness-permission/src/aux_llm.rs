use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{Decision, Message, MessagePart, MessageRole, PermissionError};
use harness_model::{
    AuxExecutor, AuxModelProvider, AuxOptions, AuxTask, ModelProtocol, ModelRequest,
};
use serde_json::json;

use crate::{
    DecisionPersistence, NoopDecisionPersistence, PermissionBroker, PermissionContext,
    PermissionRequest, PersistedDecision,
};

pub struct AuxLlmBroker {
    executor: AuxExecutor,
    persistence: Arc<dyn DecisionPersistence>,
}

impl AuxLlmBroker {
    #[must_use]
    pub fn new(aux_provider: Arc<dyn AuxModelProvider>) -> Self {
        let mut options = aux_provider.aux_options();
        options.fail_open = false;
        Self::with_options(aux_provider, options)
    }

    #[must_use]
    pub fn with_options(aux_provider: Arc<dyn AuxModelProvider>, mut options: AuxOptions) -> Self {
        options.fail_open = false;
        Self {
            executor: AuxExecutor::with_options(aux_provider, options),
            persistence: Arc::new(NoopDecisionPersistence),
        }
    }

    #[must_use]
    pub fn with_persistence(mut self, persistence: Arc<dyn DecisionPersistence>) -> Self {
        self.persistence = persistence;
        self
    }

    async fn advise(&self, request: &PermissionRequest, ctx: &PermissionContext) -> Decision {
        if ctx.permission_mode != harness_contracts::PermissionMode::Auto {
            return Decision::Escalate;
        }

        let req = advisory_request(request, ctx);
        match self.executor.call(AuxTask::PermissionAdvisory, req).await {
            Ok(Some(output)) => parse_advisory(&output),
            Ok(None) => Decision::DenyOnce,
            Err(_error) => Decision::DenyOnce,
        }
    }
}

#[async_trait]
impl PermissionBroker for AuxLlmBroker {
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        self.advise(&request, &ctx).await
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.persistence.persist(decision).await
    }
}

fn advisory_request(request: &PermissionRequest, ctx: &PermissionContext) -> ModelRequest {
    let payload = json!({
        "tool_name": request.tool_name,
        "subject": format!("{:?}", request.subject),
        "severity": format!("{:?}", request.severity),
        "scope_hint": format!("{:?}", request.scope_hint),
        "permission_mode": format!("{:?}", ctx.permission_mode),
        "fallback_policy": format!("{:?}", ctx.fallback_policy),
    });

    ModelRequest {
        model_id: "aux-permission-advisory".to_owned(),
        messages: vec![Message {
            id: harness_contracts::MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(payload.to_string())],
            created_at: harness_contracts::now(),
        }],
        tools: None,
        system: Some(
            "Decide whether this permission request should be APPROVE, DENY, or ESCALATE. \
             Return exactly one of those words."
                .to_owned(),
        ),
        temperature: Some(0.0),
        max_tokens: Some(8),
        stream: false,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::Responses,
        extra: serde_json::Value::Null,
        provider_context: harness_model::ProviderRequestContext::default(),
    }
}

fn parse_advisory(output: &str) -> Decision {
    let normalized = output.trim().to_ascii_uppercase();
    if normalized.starts_with("APPROVE") {
        Decision::AllowOnce
    } else if normalized.starts_with("DENY") {
        Decision::DenyOnce
    } else {
        Decision::Escalate
    }
}
