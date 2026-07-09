use super::{openai_descriptor_record, provider_settings_store_for_workspace, unique_workspace};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{ConversationModelCapability, ModelError, ModelProtocol};
use harness_model::{ErrorClass, ErrorHints, ModelStreamEvent};
use jyowo_desktop_shell::commands::{
    list_provider_probe_snapshots_with_runtime_state, probe_provider_config_with_provider,
    probe_provider_config_with_runtime_state, DesktopProviderDiagnosticsStore,
    DesktopProviderSettingsStore, DesktopRuntimeState, ProbeProviderConfigRequest,
    ProviderConfigRecord, ProviderDiagnosticsStore, ProviderProbeErrorKindPayload,
    ProviderProbeStatusPayload, ProviderSettingsRecord, ProviderSettingsStore,
};
use jyowo_harness_sdk::ext::{
    HealthStatus, InferContext, ModelDescriptor, ModelLifecycle, ModelProvider, ModelRequest,
    ModelStream,
};

struct ProbeCountingProvider {
    events: Vec<ModelStreamEvent>,
    infer_calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for ProbeCountingProvider {
    fn provider_id(&self) -> &str {
        "openai"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            provider_id: "openai".to_owned(),
            model_id: "gpt-5.4-mini".to_owned(),
            display_name: "OpenAI".to_owned(),
            protocol: ModelProtocol::Responses,
            context_window: 128_000,
            max_output_tokens: 16_384,
            provider_declared_capability: ConversationModelCapability::default(),
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: jyowo_harness_sdk::ext::ModelRuntimeSemantics::messages_default(
                ModelProtocol::Responses,
            ),
            lifecycle: ModelLifecycle::Stable,
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.infer_calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        Ok(Box::pin(stream::iter(self.events.clone())))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

fn sample_provider_config(api_key: &str) -> ProviderConfigRecord {
    ProviderConfigRecord {
        api_key: api_key.to_owned(),
        protocol: ModelProtocol::Responses,
        base_url: Some("https://gateway.example.com".to_owned()),
        display_name: "OpenAI Work".to_owned(),
        id: "openai-work".to_owned(),
        model_id: "gpt-5.4-mini".to_owned(),
        official_quota_api_key: None,
        provider_id: "openai".to_owned(),
        provider_defaults: None,
        model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
    }
}

fn prepare_workspace(name: &str) -> std::path::PathBuf {
    let workspace = unique_workspace(name);
    std::fs::create_dir_all(&workspace).unwrap();
    workspace.canonicalize().unwrap()
}

#[tokio::test]
async fn provider_probe_rejects_unknown_config_id() {
    let workspace = prepare_workspace("provider-probe-unknown-config");
    provider_settings_store_for_workspace(&workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![sample_provider_config("provider-test-token")],
        })
        .unwrap();
    let runtime = DesktopRuntimeState::with_workspace_for_test(workspace).unwrap();
    let error = probe_provider_config_with_runtime_state(
        ProbeProviderConfigRequest {
            config_id: "missing".to_owned(),
            timeout_ms: None,
        },
        &runtime,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn provider_probe_rejects_config_without_api_key() {
    let workspace = prepare_workspace("provider-probe-no-key");
    let config = sample_provider_config("");
    let provider: Arc<dyn ModelProvider> = Arc::new(ProbeCountingProvider {
        events: vec![ModelStreamEvent::MessageStop],
        infer_calls: Arc::new(AtomicUsize::new(0)),
    });
    let diagnostics_store = DesktopProviderDiagnosticsStore::new(workspace);
    let flights = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let error = probe_provider_config_with_provider(
        ProbeProviderConfigRequest {
            config_id: "openai-work".to_owned(),
            timeout_ms: None,
        },
        &config,
        provider,
        ModelProtocol::Responses,
        &diagnostics_store,
        &flights,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn provider_probe_persists_snapshot_to_diagnostics_store() {
    let workspace = prepare_workspace("provider-probe-persist");
    let config = sample_provider_config("provider-test-token");
    let infer_calls = Arc::new(AtomicUsize::new(0));
    let provider: Arc<dyn ModelProvider> = Arc::new(ProbeCountingProvider {
        events: vec![ModelStreamEvent::MessageStop],
        infer_calls: Arc::clone(&infer_calls),
    });
    let diagnostics_store = DesktopProviderDiagnosticsStore::new(workspace.clone());
    let flights = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

    let response = probe_provider_config_with_provider(
        ProbeProviderConfigRequest {
            config_id: "openai-work".to_owned(),
            timeout_ms: Some(5_000),
        },
        &config,
        provider.clone(),
        ModelProtocol::Responses,
        &diagnostics_store,
        &flights,
    )
    .await
    .unwrap();

    assert_eq!(response.snapshot.status, ProviderProbeStatusPayload::Online);
    assert!(!response.snapshot.checked_at.is_empty());
    let persisted = diagnostics_store.load_record().unwrap();
    assert_eq!(persisted.snapshots.len(), 1);
    assert_eq!(persisted.snapshots[0].config_id, "openai-work");
    assert_eq!(infer_calls.load(Ordering::SeqCst), 1);

    let runtime = DesktopRuntimeState::with_workspace_for_test(workspace).unwrap();
    let listed = list_provider_probe_snapshots_with_runtime_state(&runtime).unwrap();
    assert_eq!(listed.snapshots.len(), 1);
    assert_eq!(listed.snapshots[0].config_id, "openai-work");
}

#[tokio::test]
async fn provider_probe_maps_auth_failure_without_provider_native_body() {
    let workspace = prepare_workspace("provider-probe-auth");
    let config = sample_provider_config("provider-test-token");
    let infer_calls = Arc::new(AtomicUsize::new(0));
    let provider: Arc<dyn ModelProvider> = Arc::new(ProbeCountingProvider {
        events: vec![ModelStreamEvent::StreamError {
            error: ModelError::AuthExpired("Bearer leaked-secret".to_owned()),
            class: ErrorClass::AuthExpired,
            hints: ErrorHints::default(),
        }],
        infer_calls: Arc::clone(&infer_calls),
    });
    let diagnostics_store = DesktopProviderDiagnosticsStore::new(workspace);
    let flights = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

    let response = probe_provider_config_with_provider(
        ProbeProviderConfigRequest {
            config_id: "openai-work".to_owned(),
            timeout_ms: None,
        },
        &config,
        provider,
        ModelProtocol::Responses,
        &diagnostics_store,
        &flights,
    )
    .await
    .unwrap();

    assert_eq!(
        response.snapshot.status,
        ProviderProbeStatusPayload::Unauthenticated
    );
    assert_eq!(
        response.snapshot.error_kind,
        Some(ProviderProbeErrorKindPayload::Auth)
    );
    let message = response.snapshot.safe_message.unwrap();
    assert!(!message.contains("leaked-secret"));
}

#[tokio::test]
async fn provider_probe_single_flight_deduplicates_same_config_id() {
    let workspace = prepare_workspace("provider-probe-single-flight");
    let config = sample_provider_config("provider-test-token");
    let infer_calls = Arc::new(AtomicUsize::new(0));
    let provider: Arc<dyn ModelProvider> = Arc::new(ProbeCountingProvider {
        events: vec![ModelStreamEvent::MessageStop],
        infer_calls: Arc::clone(&infer_calls),
    });
    let diagnostics_store = DesktopProviderDiagnosticsStore::new(workspace);
    let flights = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let request = ProbeProviderConfigRequest {
        config_id: "openai-work".to_owned(),
        timeout_ms: Some(5_000),
    };

    let first = probe_provider_config_with_provider(
        request.clone(),
        &config,
        Arc::clone(&provider),
        ModelProtocol::Responses,
        &diagnostics_store,
        &flights,
    );
    let second = probe_provider_config_with_provider(
        request,
        &config,
        Arc::clone(&provider),
        ModelProtocol::Responses,
        &diagnostics_store,
        &flights,
    );
    let (_, _) = tokio::join!(first, second);
    assert_eq!(infer_calls.load(Ordering::SeqCst), 1);
}

#[cfg(unix)]
#[test]
fn desktop_provider_diagnostics_store_rejects_symlink_file() {
    let workspace = prepare_workspace("provider-diagnostics-symlink");
    std::fs::create_dir_all(workspace.join(".jyowo/runtime")).unwrap();
    let external = workspace.join("external");
    std::fs::create_dir_all(&external).unwrap();
    let settings_path = workspace
        .join(".jyowo")
        .join("runtime")
        .join("provider-diagnostics.json");
    std::fs::write(
        external.join("provider-diagnostics.json"),
        r#"{"snapshots":[]}"#,
    )
    .unwrap();
    std::os::unix::fs::symlink(external.join("provider-diagnostics.json"), &settings_path).unwrap();

    let error = DesktopProviderDiagnosticsStore::new(workspace)
        .load_record()
        .unwrap_err();
    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
}

#[test]
fn provider_diagnostics_store_recovers_from_invalid_json() {
    let workspace = prepare_workspace("provider-diagnostics-invalid-json");
    let runtime_dir = workspace.join(".jyowo").join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let diagnostics_path = runtime_dir.join("provider-diagnostics.json");
    std::fs::write(&diagnostics_path, b"{not-json").unwrap();

    let record = DesktopProviderDiagnosticsStore::new(workspace)
        .load_record()
        .unwrap();

    assert!(record.snapshots.is_empty());
    assert!(!diagnostics_path.exists());
}
