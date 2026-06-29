#![cfg(feature = "minimax-tools")]

use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    CapabilityRegistry, CapabilityRouteKind, Decision, PermissionError, PermissionSubject,
    ProviderCredential, ProviderCredentialResolveContext, ProviderCredentialResolverCap,
    ToolCapability, ToolError,
};
use harness_permission::{
    PermissionBroker, PermissionCheck, PermissionContext, PermissionRequest, PersistedDecision,
};
use harness_tool::{
    BuiltinToolset, InterruptToken, MiniMaxResponsesTool, MiniMaxTextToImageTool,
    MiniMaxTextToSpeechTool, MiniMaxTextToVideoTool, Tool, ToolContext, ToolEvent,
    ToolRegistryBuilder,
};
use serde_json::json;
use std::{path::PathBuf, sync::{Arc, Mutex}};

#[tokio::test]
async fn minimax_tools_register_with_default_builtin_toolset() {
    let registry = ToolRegistryBuilder::new()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    assert!(registry.get("MiniMaxTextToImage").is_some());
    assert!(registry.get("MiniMaxTextToVideo").is_some());
    assert!(registry.get("MiniMaxVideoGenerationQuery").is_some());
    assert!(registry.get("MiniMaxTextToSpeech").is_some());
    assert!(registry.get("MiniMaxTextToSpeechWs").is_none());
    assert!(registry.get("MiniMaxTextToSpeechAsyncQuery").is_some());
    assert!(registry.get("MiniMaxMusicGeneration").is_some());
    assert!(registry.get("MiniMaxFileUpload").is_some());
    assert!(registry.get("MiniMaxFileList").is_some());
    assert!(registry.get("MiniMaxFileRetrieve").is_some());
    assert!(registry.get("MiniMaxFileDelete").is_some());
    assert!(registry.get("MiniMaxModelsList").is_some());
    assert!(registry.get("MiniMaxModelRetrieve").is_some());
    assert!(registry.get("MiniMaxResponses").is_some());
    assert!(registry.get("MiniMaxAnthropicMessages").is_some());
}

#[tokio::test]
async fn minimax_tool_fails_closed_when_credential_resolver_is_missing() {
    std::env::remove_var("MINIMAX_API_KEY");
    let tool = MiniMaxTextToImageTool::default();
    let error = execute_error(&tool, json!({"request": {"prompt": "x"}}), ctx()).await;

    assert!(matches!(
        error,
        ToolError::CapabilityMissing(ToolCapability::ProviderCredentialResolver)
    ));
    assert!(!error.to_string().contains("sk-"));
}

#[tokio::test]
async fn minimax_tool_uses_provider_credential_resolver() {
    std::env::remove_var("MINIMAX_API_KEY");
    let tool = MiniMaxTextToImageTool::default();
    let error = execute_error(
        &tool,
        json!({"request": {"prompt": "x"}}),
        ctx_with_resolver(Arc::new(WrongProviderResolver)),
    )
    .await;

    assert!(matches!(error, ToolError::PermissionDenied(_)));
    assert!(error.to_string().contains("does not match MiniMax"));
    assert!(!error.to_string().contains("sk-"));
}

#[tokio::test]
async fn minimax_permission_uses_configured_credential_base_url_host() {
    let tool = MiniMaxTextToImageTool::default();
    let check = tool
        .check_permission(
            &json!({"request": {"prompt": "x"}}),
            &ctx_with_resolver(Arc::new(MiniMaxResolver {
                api_key: "sk-redacted-test-key".to_owned(),
                base_url: Some("https://api.minimax.io".to_owned()),
            })),
        )
        .await;

    match check {
        PermissionCheck::AskUser {
            subject: PermissionSubject::NetworkAccess { host, port },
            ..
        } => {
            assert_eq!(host, "api.minimax.io");
            assert_eq!(port, None);
        }
        other => panic!("expected network permission request, got {other:?}"),
    }
}

#[tokio::test]
async fn minimax_permission_denies_when_credential_resolver_is_missing() {
    std::env::remove_var("MINIMAX_API_KEY");
    let tool = MiniMaxTextToImageTool::default();
    let check = tool
        .check_permission(&json!({"request": {"prompt": "x"}}), &ctx())
        .await;

    match check {
        PermissionCheck::Denied { reason } => {
            assert!(reason.contains("MiniMax provider credential resolver is missing"));
            assert!(!reason.contains("sk-"));
        }
        other => panic!("expected denied permission check, got {other:?}"),
    }
}

#[tokio::test]
async fn minimax_permission_denies_invalid_credential_base_url() {
    let tool = MiniMaxTextToImageTool::default();
    let check = tool
        .check_permission(
            &json!({"request": {"prompt": "x"}}),
            &ctx_with_resolver(Arc::new(MiniMaxResolver {
                api_key: "sk-redacted-test-key".to_owned(),
                base_url: Some("not a url".to_owned()),
            })),
        )
        .await;

    match check {
        PermissionCheck::Denied { reason } => {
            assert!(reason.contains("MiniMax provider base URL is invalid"));
            assert!(!reason.contains("sk-"));
        }
        other => panic!("expected denied permission check, got {other:?}"),
    }
}

#[tokio::test]
async fn credential_route_image_tool_passes_image_generation_operation_id() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let tool = MiniMaxTextToImageTool::default();
    let _ = tool
        .check_permission(
            &json!({"request": {"prompt": "x"}}),
            &ctx_with_resolver(Arc::new(ContextCapturingResolver {
                captured: Arc::clone(&captured),
            })),
        )
        .await;

    let context = captured.lock().unwrap().pop().expect("credential context captured");
    assert_eq!(context.operation_id.as_deref(), Some("minimax.image_generation"));
    assert_eq!(context.route_kind, Some(CapabilityRouteKind::ImageGeneration));
}

#[tokio::test]
async fn credential_route_video_tool_passes_video_generation_operation_id() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let tool = MiniMaxTextToVideoTool::default();
    let _ = tool
        .check_permission(
            &json!({"request": {}}),
            &ctx_with_resolver(Arc::new(ContextCapturingResolver {
                captured: Arc::clone(&captured),
            })),
        )
        .await;

    let context = captured.lock().unwrap().pop().expect("credential context captured");
    assert_eq!(context.operation_id.as_deref(), Some("minimax.video_generation"));
    assert_eq!(context.route_kind, Some(CapabilityRouteKind::VideoGeneration));
}

#[tokio::test]
async fn credential_route_tts_tool_passes_text_to_speech_operation_id() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let tool = MiniMaxTextToSpeechTool::default();
    let _ = tool
        .check_permission(
            &json!({"request": {}}),
            &ctx_with_resolver(Arc::new(ContextCapturingResolver {
                captured: Arc::clone(&captured),
            })),
        )
        .await;

    let context = captured.lock().unwrap().pop().expect("credential context captured");
    assert_eq!(
        context.operation_id.as_deref(),
        Some("minimax.text_to_speech.sync")
    );
    assert_eq!(context.route_kind, Some(CapabilityRouteKind::TextToSpeech));
}

#[tokio::test]
async fn credential_route_non_service_tool_uses_provider_only_context() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let tool = MiniMaxResponsesTool::default();
    let _ = tool
        .check_permission(
            &json!({"request": {}}),
            &ctx_with_resolver(Arc::new(ContextCapturingResolver {
                captured: Arc::clone(&captured),
            })),
        )
        .await;

    let context = captured.lock().unwrap().pop().expect("credential context captured");
    assert!(context.operation_id.is_none());
    assert!(context.route_kind.is_none());
}

async fn execute_error(tool: &dyn Tool, input: serde_json::Value, ctx: ToolContext) -> ToolError {
    tool.validate(&input, &ctx).await.unwrap();
    let stream = tool.execute(input, ctx).await.unwrap();
    let events = stream.collect::<Vec<_>>().await;
    events
        .into_iter()
        .find_map(|event| match event {
            ToolEvent::Error(error) => Some(error),
            _ => None,
        })
        .expect("tool should return error event")
}

fn ctx() -> ToolContext {
    ctx_with_cap_registry(CapabilityRegistry::default())
}

fn ctx_with_resolver(resolver: Arc<dyn ProviderCredentialResolverCap>) -> ToolContext {
    let mut cap_registry = CapabilityRegistry::default();
    cap_registry.install(ToolCapability::ProviderCredentialResolver, resolver);
    ctx_with_cap_registry(cap_registry)
}

fn ctx_with_cap_registry(cap_registry: CapabilityRegistry) -> ToolContext {
    ToolContext {
        tool_use_id: harness_contracts::ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: harness_contracts::TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::new(),
        subagent_depth: 0,
        workspace_root: PathBuf::from("/tmp"),
        sandbox: None,
        permission_broker: Arc::new(AllowBroker),
        cap_registry: Arc::new(cap_registry),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::new(),
        parent_run: None,
    }
}

struct WrongProviderResolver;

impl ProviderCredentialResolverCap for WrongProviderResolver {
    fn resolve_provider_credential(
        &self,
        _context: ProviderCredentialResolveContext,
    ) -> BoxFuture<'_, Result<ProviderCredential, ToolError>> {
        Box::pin(async {
            Ok(ProviderCredential {
                provider_id: "openai".to_owned(),
                config_id: "wrong-provider".to_owned(),
                api_key: "sk-redacted-test-key".to_owned(),
                base_url: None,
            })
        })
    }
}

struct MiniMaxResolver {
    api_key: String,
    base_url: Option<String>,
}

struct ContextCapturingResolver {
    captured: Arc<Mutex<Vec<ProviderCredentialResolveContext>>>,
}

impl ProviderCredentialResolverCap for ContextCapturingResolver {
    fn resolve_provider_credential(
        &self,
        context: ProviderCredentialResolveContext,
    ) -> BoxFuture<'_, Result<ProviderCredential, ToolError>> {
        self.captured.lock().unwrap().push(context);
        Box::pin(async {
            Ok(ProviderCredential {
                provider_id: "minimax".to_owned(),
                config_id: "minimax-test".to_owned(),
                api_key: "provider-test-token".to_owned(),
                base_url: None,
            })
        })
    }
}

impl ProviderCredentialResolverCap for MiniMaxResolver {
    fn resolve_provider_credential(
        &self,
        _context: ProviderCredentialResolveContext,
    ) -> BoxFuture<'_, Result<ProviderCredential, ToolError>> {
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        Box::pin(async move {
            Ok(ProviderCredential {
                provider_id: "minimax".to_owned(),
                config_id: "minimax-test".to_owned(),
                api_key,
                base_url,
            })
        })
    }
}

struct AllowBroker;

#[async_trait::async_trait]
impl PermissionBroker for AllowBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}
