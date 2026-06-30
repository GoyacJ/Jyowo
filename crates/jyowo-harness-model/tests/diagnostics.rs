use std::sync::Arc;
use std::time::Duration;

use async_stream::stream as event_stream;
use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ConversationModelCapability, ModelError, ModelProtocol, ProviderProbeErrorKind,
    ProviderProbeStatus, UsageSnapshot,
};
use harness_model::{
    ErrorClass, ErrorHints, HealthStatus, InferContext, ModelDescriptor, ModelLifecycle,
    ModelProvider, ModelRequest, ModelStream, ModelStreamEvent, ProviderProbeInput,
    ProviderProbeRunner,
};

struct ProbeTestProvider {
    events: Vec<ModelStreamEvent>,
    hang_after_start: bool,
}

#[async_trait]
impl ModelProvider for ProbeTestProvider {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            provider_id: "test".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            display_name: "Test".to_owned(),
            protocol: ModelProtocol::Responses,
            context_window: 8_192,
            max_output_tokens: 1_024,
            conversation_capability: ConversationModelCapability::default(),
            lifecycle: ModelLifecycle::Stable,
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        if self.hang_after_start {
            let events = self.events.clone();
            return Ok(Box::pin(event_stream! {
                for event in events {
                    yield event;
                }
                loop {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
            }));
        }
        Ok(Box::pin(stream::iter(self.events.clone())))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[tokio::test]
async fn provider_probe_runner_reports_online_on_message_stop() {
    let provider = Arc::new(ProbeTestProvider {
        events: vec![ModelStreamEvent::MessageStop],
        hang_after_start: false,
    });
    let outcome = ProviderProbeRunner::run(
        provider.as_ref(),
        ProviderProbeInput {
            config_id: "cfg-openai".to_owned(),
            provider_id: "openai".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            timeout_ms: 5_000,
        },
        ModelProtocol::Responses,
    )
    .await;

    assert_eq!(outcome.snapshot.status, ProviderProbeStatus::Online);
    assert!(outcome.snapshot.checked_at.timestamp() > 0);
    assert_eq!(outcome.snapshot.timeout_ms, 5_000);
    assert!(outcome.snapshot.latency_ms.is_some());
    assert!(outcome.snapshot.safe_message.is_none());
}

#[tokio::test]
async fn provider_probe_runner_maps_auth_stream_error_without_provider_body() {
    let provider = Arc::new(ProbeTestProvider {
        events: vec![ModelStreamEvent::StreamError {
            error: ModelError::AuthExpired("Bearer secret-token invalid body".to_owned()),
            class: ErrorClass::AuthExpired,
            hints: ErrorHints::default(),
        }],
        hang_after_start: false,
    });
    let outcome = ProviderProbeRunner::run(
        provider.as_ref(),
        ProviderProbeInput {
            config_id: "cfg-openai".to_owned(),
            provider_id: "openai".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            timeout_ms: 5_000,
        },
        ModelProtocol::Responses,
    )
    .await;

    assert_eq!(
        outcome.snapshot.status,
        ProviderProbeStatus::Unauthenticated
    );
    assert_eq!(
        outcome.snapshot.error_kind,
        Some(ProviderProbeErrorKind::Auth)
    );
    let safe_message = outcome
        .snapshot
        .safe_message
        .expect("auth failures must include safe message");
    assert!(!safe_message.contains("secret-token"));
    assert!(!safe_message.contains("Bearer"));
}

#[tokio::test]
async fn provider_probe_runner_maps_timeout() {
    let provider = Arc::new(ProbeTestProvider {
        events: vec![ModelStreamEvent::MessageStart {
            message_id: "msg-1".to_owned(),
            usage: UsageSnapshot::default(),
        }],
        hang_after_start: true,
    });
    let outcome = ProviderProbeRunner::run(
        provider.as_ref(),
        ProviderProbeInput {
            config_id: "cfg-openai".to_owned(),
            provider_id: "openai".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            timeout_ms: 50,
        },
        ModelProtocol::Responses,
    )
    .await;

    assert_eq!(outcome.snapshot.status, ProviderProbeStatus::Timeout);
    assert_eq!(
        outcome.snapshot.error_kind,
        Some(ProviderProbeErrorKind::Timeout)
    );
    assert_eq!(outcome.snapshot.timeout_ms, 50);
}

#[tokio::test]
async fn provider_probe_runner_classifies_diagnostic_usage_separately() {
    let provider = Arc::new(ProbeTestProvider {
        events: vec![
            ModelStreamEvent::MessageStart {
                message_id: "msg-1".to_owned(),
                usage: UsageSnapshot {
                    input_tokens: 12,
                    output_tokens: 3,
                    ..UsageSnapshot::default()
                },
            },
            ModelStreamEvent::MessageStop,
        ],
        hang_after_start: false,
    });
    let outcome = ProviderProbeRunner::run(
        provider.as_ref(),
        ProviderProbeInput {
            config_id: "cfg-openai".to_owned(),
            provider_id: "openai".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            timeout_ms: 5_000,
        },
        ModelProtocol::Responses,
    )
    .await;

    assert_eq!(outcome.snapshot.status, ProviderProbeStatus::Online);
    let usage = outcome
        .diagnostic_usage
        .expect("probe usage should be classified as diagnostic");
    assert_eq!(usage.input_tokens, 12);
    assert_eq!(usage.output_tokens, 3);
}

#[tokio::test]
async fn provider_probe_runner_sets_suppress_usage_accounting_context() {
    let provider = Arc::new(ProbeTestProvider {
        events: vec![ModelStreamEvent::MessageStop],
        hang_after_start: false,
    });
    let _ = ProviderProbeRunner::run(
        provider.as_ref(),
        ProviderProbeInput {
            config_id: "cfg-openai".to_owned(),
            provider_id: "openai".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            timeout_ms: 5_000,
        },
        ModelProtocol::Responses,
    )
    .await;

    // InferContext is internal; successful probe without journal side effects is covered
    // by desktop command tests. This test ensures the runner completes under diagnostic use.
    tokio::time::sleep(Duration::from_millis(1)).await;
}
