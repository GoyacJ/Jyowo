#![cfg(feature = "seedance-tools")]

use base64::{engine::general_purpose, Engine as _};
use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    BlobMeta, BlobRef, BlobWriterCap, CapabilityRegistry, CapabilityRouteKind, Decision,
    ModelModality, PermissionError, ProviderCredential, ProviderCredentialResolveContext,
    ProviderCredentialResolverCap, ToolCapability, ToolError, ToolResult, ToolResultPart,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest, PersistedDecision};
use harness_tool::{
    BuiltinToolset, InterruptToken, SeedanceImageToVideo, SeedanceTextToVideo,
    SeedanceVideoGenerationQueryTool, Tool, ToolContext, ToolEvent, ToolRegistryBuilder,
};
use serde_json::json;
use std::{path::PathBuf, sync::{Arc, Mutex}};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

const MP4_HEADER_BYTES: &[u8] = &[
    0x00, 0x00, 0x00, 0x20, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6F, 0x6D,
];

#[tokio::test]
async fn seedance_tools_register_with_default_builtin_toolset() {
    let registry = ToolRegistryBuilder::new()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    assert!(registry.get("SeedanceTextToVideo").is_some());
    assert!(registry.get("SeedanceImageToVideo").is_some());
    assert!(registry.get("SeedanceVideoGenerationQuery").is_some());
}

#[tokio::test]
async fn seedance_text_to_video_returns_async_job_output() {
    let server = MockServer::start().await;
    mount_seedance_post(
        &server,
        "/contents/generations/tasks",
        json!({
            "model": "doubao-seedance-2-0-260128",
            "content": [{"type": "text", "text": "wave"}]
        }),
        json!({"id": "cgt-video-1"}),
    )
    .await;

    let tool = SeedanceTextToVideo::default();
    let result = execute_final(
        &tool,
        json!({"request": {
            "model": "doubao-seedance-2-0-260128",
            "content": [{"type": "text", "text": "wave"}]
        }}),
        ctx_with_media(server.uri()),
    )
    .await;

    let ToolResult::Mixed(parts) = result else {
        panic!("expected mixed result, got {result:?}");
    };
    let part = parts
        .iter()
        .find_map(|part| match part {
            ToolResultPart::Structured { value, schema_ref } => Some((value, schema_ref.as_deref())),
            _ => None,
        })
        .expect("expected structured async job part");
    assert_eq!(part.1.as_deref(), Some("provider_service_async_job.v1"));
    assert_eq!(part.0["kind"], "async_job");
    assert_eq!(part.0["jobId"], "cgt-video-1");
    assert_eq!(part.0["pollOperationId"], "seedance.video_generation.query");
    assert_eq!(part.0["artifactKind"], "video");
}

#[tokio::test]
async fn seedance_image_to_video_returns_async_job_output() {
    let server = MockServer::start().await;
    mount_seedance_post(
        &server,
        "/contents/generations/tasks",
        json!({
            "model": "doubao-seedance-2-0-260128",
            "content": [
                {"type": "text", "text": "turn toward camera"},
                {"type": "image_url", "image_url": {"url": "https://example.com/portrait.jpg"}}
            ]
        }),
        json!({"id": "cgt-video-2"}),
    )
    .await;

    let tool = SeedanceImageToVideo::default();
    let result = execute_final(
        &tool,
        json!({"request": {
            "model": "doubao-seedance-2-0-260128",
            "content": [
                {"type": "text", "text": "turn toward camera"},
                {"type": "image_url", "image_url": {"url": "https://example.com/portrait.jpg"}}
            ]
        }}),
        ctx_with_media(server.uri()),
    )
    .await;

    let ToolResult::Mixed(parts) = result else {
        panic!("expected mixed result, got {result:?}");
    };
    assert!(parts.iter().any(|part| matches!(
        part,
        ToolResultPart::Structured { value, .. }
            if value["kind"] == "async_job" && value["jobId"] == "cgt-video-2"
    )));
}

#[tokio::test]
async fn seedance_query_completed_task_returns_typed_video_artifact() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/contents/generations/tasks/cgt-video-3"))
        .and(header("authorization", "Bearer provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "succeeded",
            "content": {
                "video_base64": general_purpose::STANDARD.encode(MP4_HEADER_BYTES)
            }
        })))
        .mount(&server)
        .await;

    let tool = SeedanceVideoGenerationQueryTool::default();
    let result = execute_final(
        &tool,
        json!({"request": {"task_id": "cgt-video-3"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert_typed_artifact(&result, ModelModality::Video, "video/mp4", "Generated video");
}

#[tokio::test]
async fn seedance_query_rejects_unsafe_output_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/contents/generations/tasks/cgt-video-4"))
        .and(header("authorization", "Bearer provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "succeeded",
            "content": {
                "video_url": "https://example.invalid/private.mp4"
            }
        })))
        .mount(&server)
        .await;

    let tool = SeedanceVideoGenerationQueryTool::default();
    let error = execute_error(
        &tool,
        json!({"request": {"task_id": "cgt-video-4"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert!(matches!(error, ToolError::PermissionDenied(_)));
}

#[tokio::test]
async fn seedance_tool_rejects_wrong_credential_provider() {
    let tool = SeedanceTextToVideo::default();
    let error = execute_error(
        &tool,
        json!({"request": {"model": "doubao-seedance-2-0-260128"}}),
        ctx_with_resolver(Arc::new(WrongProviderResolver)),
    )
    .await;

    assert!(matches!(error, ToolError::PermissionDenied(_)));
    assert!(error.to_string().contains("does not match Seedance"));
    assert!(!error.to_string().contains("sk-"));
}

#[tokio::test]
async fn credential_route_video_tool_passes_seedance_operation_id() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let tool = SeedanceTextToVideo::default();
    let _ = tool
        .check_permission(
            &json!({"request": {}}),
            &ctx_with_resolver(Arc::new(CapturingResolver {
                captured: Arc::clone(&captured),
                base_url: "https://127.0.0.1".to_owned(),
            })),
        )
        .await;

    let context = captured
        .lock()
        .expect("captured contexts")
        .pop()
        .expect("credential context captured");
    assert_eq!(context.provider_id, "doubao");
    assert_eq!(
        context.operation_id.as_deref(),
        Some("seedance.video_generation")
    );
    assert_eq!(
        context.route_kind,
        Some(CapabilityRouteKind::VideoGeneration)
    );
}

async fn mount_seedance_post(
    server: &MockServer,
    endpoint: &str,
    expected_body: serde_json::Value,
    response_body: serde_json::Value,
) {
    Mock::given(method("POST"))
        .and(path(endpoint))
        .and(header("authorization", "Bearer provider-test-token"))
        .and(wiremock::matchers::body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .mount(server)
        .await;
}

async fn execute_final(tool: &dyn Tool, input: serde_json::Value, ctx: ToolContext) -> ToolResult {
    tool.validate(&input, &ctx).await.unwrap();
    let stream = tool.execute(input, ctx).await.unwrap();
    let events = stream.collect::<Vec<_>>().await;
    events
        .into_iter()
        .find_map(|event| match event {
            ToolEvent::Final(result) => Some(result),
            ToolEvent::Error(error) => panic!("unexpected tool error: {error:?}"),
            _ => None,
        })
        .expect("tool should return final event")
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

fn assert_typed_artifact(
    result: &ToolResult,
    artifact_kind: ModelModality,
    content_type: &str,
    title: &str,
) {
    let ToolResult::Mixed(parts) = result else {
        panic!("expected mixed result, got {result:?}");
    };
    assert!(parts.iter().any(|part| matches!(
        part,
        ToolResultPart::Artifact {
            artifact_kind: kind,
            content_type: mime,
            blob_ref,
            title: artifact_title,
            ..
        } if *kind == artifact_kind
            && mime == content_type
            && blob_ref.content_type.as_deref() == Some(content_type)
            && artifact_title == title
    )));
}

fn ctx_with_media(base_url: String) -> ToolContext {
    let mut cap_registry = CapabilityRegistry::default();
    let resolver: Arc<dyn ProviderCredentialResolverCap> = Arc::new(DoubaoResolver {
        api_key: "provider-test-token".to_owned(),
        base_url: Some(base_url),
    });
    cap_registry.install(ToolCapability::ProviderCredentialResolver, resolver);
    let writer: Arc<dyn BlobWriterCap> = Arc::new(CapturingBlobWriter);
    cap_registry.install(ToolCapability::BlobWriter, writer);
    ctx_with_cap_registry(cap_registry)
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

struct DoubaoResolver {
    api_key: String,
    base_url: Option<String>,
}

impl ProviderCredentialResolverCap for DoubaoResolver {
    fn resolve_provider_credential(
        &self,
        context: ProviderCredentialResolveContext,
    ) -> BoxFuture<'_, Result<ProviderCredential, ToolError>> {
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        Box::pin(async move {
            Ok(ProviderCredential {
                provider_id: context.provider_id,
                config_id: "doubao-main".to_owned(),
                api_key,
                base_url,
            })
        })
    }
}

struct WrongProviderResolver;

impl ProviderCredentialResolverCap for WrongProviderResolver {
    fn resolve_provider_credential(
        &self,
        _context: ProviderCredentialResolveContext,
    ) -> BoxFuture<'_, Result<ProviderCredential, ToolError>> {
        Box::pin(async move {
            Ok(ProviderCredential {
                provider_id: "minimax".to_owned(),
                config_id: "minimax-main".to_owned(),
                api_key: "provider-test-token".to_owned(),
                base_url: None,
            })
        })
    }
}

struct CapturingResolver {
    captured: Arc<Mutex<Vec<ProviderCredentialResolveContext>>>,
    base_url: String,
}

impl ProviderCredentialResolverCap for CapturingResolver {
    fn resolve_provider_credential(
        &self,
        context: ProviderCredentialResolveContext,
    ) -> BoxFuture<'_, Result<ProviderCredential, ToolError>> {
        let captured = self.captured.clone();
        let base_url = self.base_url.clone();
        Box::pin(async move {
            captured.lock().expect("capture lock").push(context.clone());
            Ok(ProviderCredential {
                provider_id: context.provider_id,
                config_id: "doubao-main".to_owned(),
                api_key: "provider-test-token".to_owned(),
                base_url: Some(base_url),
            })
        })
    }
}

struct CapturingBlobWriter;

impl BlobWriterCap for CapturingBlobWriter {
    fn write_blob(
        &self,
        _tenant_id: harness_contracts::TenantId,
        bytes: bytes::Bytes,
        meta: BlobMeta,
    ) -> BoxFuture<'_, Result<BlobRef, ToolError>> {
        Box::pin(async move {
            assert!(!bytes.is_empty());
            Ok(BlobRef {
                id: harness_contracts::BlobId::new(),
                size: meta.size,
                content_hash: meta.content_hash,
                content_type: meta.content_type,
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
