#![cfg(feature = "gemini-tools")]

use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    BlobMeta, BlobRef, BlobWriterCap, CapabilityRegistry, CapabilityRouteKind, ModelModality,
    ProviderCredential, ProviderCredentialResolveContext, ProviderCredentialResolverCap,
    ToolActionPlan, ToolCapability, ToolError, ToolResult, ToolResultPart,
};
use harness_execution::ReqwestToolNetworkBroker;
use harness_tool::{
    AuthorizedTicketSummary, AuthorizedToolInput, BuiltinToolset, GeminiFileUploadTool,
    GeminiImageGenerationTool, GeminiModelsListTool, GeminiTextToSpeechTool, GeminiTokensCountTool,
    GeminiVideoGenerationQueryTool, GeminiVideoGenerationTool, InterruptToken, Tool, ToolContext,
    ToolEvent, ToolNetworkBrokerCap, ToolRegistryBuilder,
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use wiremock::{
    matchers::{body_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

const PNG_1X1_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";
const WAV_HEADER_BYTES: &[u8] = b"RIFF$\x00\x00\x00WAVEfmt ";
const MP4_HEADER_BYTES: &[u8] = b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00\x00\x00\x00\x00";

#[tokio::test]
async fn gemini_tools_register_with_default_builtin_toolset() {
    let registry = ToolRegistryBuilder::new()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    let gemini_names = snapshot
        .iter_sorted()
        .map(|(name, _)| name.as_str())
        .filter(|name| name.starts_with("Gemini"))
        .collect::<Vec<_>>();

    assert_eq!(
        gemini_names,
        vec![
            "GeminiBatchCancel",
            "GeminiBatchCreate",
            "GeminiBatchGet",
            "GeminiBatchList",
            "GeminiCachedContentCreate",
            "GeminiCachedContentDelete",
            "GeminiCachedContentGet",
            "GeminiCachedContentList",
            "GeminiEmbedding",
            "GeminiEmbeddingBatch",
            "GeminiFileDelete",
            "GeminiFileGet",
            "GeminiFileList",
            "GeminiFileUpload",
            "GeminiImageGeneration",
            "GeminiModelGet",
            "GeminiModelsList",
            "GeminiTextToSpeech",
            "GeminiTokensCount",
            "GeminiVideoGeneration",
            "GeminiVideoGenerationQuery",
        ]
    );

    let availability =
        harness_tool::provider_service_adapter_availability_from_snapshot(&registry.snapshot());
    assert!(availability
        .bindings
        .iter()
        .any(|binding| binding.operation_id == "gemini.image_generation"));
    assert!(availability
        .bindings
        .iter()
        .any(|binding| binding.operation_id == "gemini.video_generation"));
    assert!(availability
        .bindings
        .iter()
        .any(|binding| binding.operation_id == "gemini.video_generation.query"));
    assert!(availability
        .bindings
        .iter()
        .any(|binding| binding.operation_id == "gemini.text_to_speech"));
    assert!(!availability
        .bindings
        .iter()
        .any(|binding| binding.operation_id == "gemini.live"));

    let image_descriptor = snapshot
        .iter_sorted()
        .find_map(|(name, tool)| {
            (name.as_str() == "GeminiImageGeneration").then_some(tool.descriptor())
        })
        .expect("image generation descriptor");
    assert!(image_descriptor
        .required_capabilities
        .contains(&ToolCapability::BlobWriter));
    let delete_descriptor = snapshot
        .iter_sorted()
        .find_map(|(name, tool)| (name.as_str() == "GeminiFileDelete").then_some(tool.descriptor()))
        .expect("file delete descriptor");
    assert!(delete_descriptor.properties.is_destructive);
}

#[tokio::test]
async fn gemini_model_list_uses_api_key_header_and_v1beta_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/models"))
        .and(header("x-goog-api-key", "provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": [{"name": "models/gemini-3.5-flash"}]
        })))
        .mount(&server)
        .await;

    let result = execute_final(
        &GeminiModelsListTool::default(),
        json!({"request": {}}),
        ctx_with_media(server.uri()),
    )
    .await;

    let ToolResult::Structured(value) = result else {
        panic!("expected structured result");
    };
    assert_eq!(value["models"][0]["name"], "models/gemini-3.5-flash");
}

#[tokio::test]
async fn gemini_count_tokens_posts_model_method_with_model_stripped_from_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-3.5-flash:countTokens"))
        .and(header("x-goog-api-key", "provider-test-token"))
        .and(body_json(json!({
            "contents": [{"parts": [{"text": "hello"}]}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"totalTokens": 3})))
        .mount(&server)
        .await;

    let result = execute_final(
        &GeminiTokensCountTool::default(),
        json!({"request": {
            "model": "gemini-3.5-flash",
            "contents": [{"parts": [{"text": "hello"}]}]
        }}),
        ctx_with_media(server.uri()),
    )
    .await;

    let ToolResult::Structured(value) = result else {
        panic!("expected structured result");
    };
    assert_eq!(value["totalTokens"], 3);
}

#[tokio::test]
async fn gemini_file_upload_rejects_multipart_header_injection_before_network() {
    let tool = GeminiFileUploadTool::default();
    let ctx = ctx_with_media("https://generativelanguage.googleapis.com".to_owned());
    let error = tool
        .validate(
            &json!({"request": {
                "file_name": "demo\r\nx.txt",
                "mime_type": "text/plain",
                "bytes_base64": general_purpose::STANDARD.encode(b"hello")
            }}),
            &ctx,
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("file_name is invalid"));
}

#[tokio::test]
async fn gemini_file_upload_uses_resumable_start_and_finalize() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/upload/v1beta/files"))
        .respond_with(ResponseTemplate::new(200).insert_header(
            "x-goog-upload-url",
            format!("{}/upload/session-1", server.uri()),
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/upload/session-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "file": {
                "name": "files/demo",
                "uri": "https://generativelanguage.googleapis.com/v1beta/files/demo"
            }
        })))
        .mount(&server)
        .await;

    let result = execute_final(
        &GeminiFileUploadTool::default(),
        json!({"request": {
            "file_name": "demo.txt",
            "mime_type": "text/plain",
            "bytes_base64": general_purpose::STANDARD.encode(b"hello")
        }}),
        ctx_with_media(server.uri()),
    )
    .await;

    let ToolResult::Structured(value) = result else {
        panic!("expected structured result");
    };
    assert_eq!(value["file"]["name"], "files/demo");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].url.path(), "/upload/v1beta/files");
    assert_eq!(requests[1].url.path(), "/upload/session-1");
    assert_eq!(
        requests[0]
            .headers
            .get("x-goog-upload-protocol")
            .and_then(|value| value.to_str().ok()),
        Some("resumable")
    );
    assert_eq!(
        requests[0]
            .headers
            .get("x-goog-upload-command")
            .and_then(|value| value.to_str().ok()),
        Some("start")
    );
    assert_eq!(
        requests[0]
            .headers
            .get("x-goog-upload-header-content-length")
            .and_then(|value| value.to_str().ok()),
        Some("5")
    );
    assert_eq!(
        requests[0]
            .headers
            .get("x-goog-upload-header-content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/plain")
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&requests[0].body).unwrap(),
        json!({ "file": { "displayName": "demo.txt" } })
    );
    assert_eq!(
        requests[1]
            .headers
            .get("x-goog-upload-command")
            .and_then(|value| value.to_str().ok()),
        Some("upload, finalize")
    );
    assert_eq!(requests[1].body, b"hello");
}

#[tokio::test]
async fn gemini_image_generation_returns_typed_image_artifact() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-3.1-flash-image:generateContent",
        ))
        .and(header("x-goog-api-key", "provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "inlineData": {
                            "mimeType": "image/png",
                            "data": PNG_1X1_BASE64
                        }
                    }]
                }
            }]
        })))
        .mount(&server)
        .await;

    let result = execute_final(
        &GeminiImageGenerationTool::default(),
        json!({"request": {
            "model": "gemini-3.1-flash-image",
            "contents": [{"parts": [{"text": "icon"}]}]
        }}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert_typed_artifact(
        &result,
        ModelModality::Image,
        "image/png",
        "Generated image",
    );
}

#[tokio::test]
async fn gemini_video_generation_returns_async_job_output() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/veo-3.1-generate-preview:predictLongRunning",
        ))
        .and(header("x-goog-api-key", "provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "operations/video-1",
            "done": false
        })))
        .mount(&server)
        .await;

    let result = execute_final(
        &GeminiVideoGenerationTool::default(),
        json!({"request": {
            "model": "veo-3.1-generate-preview",
            "instances": [{"prompt": "wave"}]
        }}),
        ctx_with_media(server.uri()),
    )
    .await;

    let ToolResult::Mixed(parts) = result else {
        panic!("expected mixed result");
    };
    let value = parts
        .iter()
        .find_map(|part| match part {
            ToolResultPart::Structured { value, .. } => Some(value),
            _ => None,
        })
        .expect("async job output");
    assert_eq!(value["jobId"], "operations/video-1");
    assert_eq!(value["pollOperationId"], "gemini.video_generation.query");
}

#[tokio::test]
async fn gemini_video_query_downloads_allowed_media_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/operations/video-1"))
        .and(header("x-goog-api-key", "provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "operations/video-1",
            "done": true,
            "response": {
                "generatedVideos": [{
                    "video": { "uri": format!("{}/generated/video.mp4", server.uri()) }
                }]
            }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/generated/video.mp4"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "video/mp4")
                .insert_header("content-length", MP4_HEADER_BYTES.len().to_string())
                .set_body_bytes(MP4_HEADER_BYTES.to_vec()),
        )
        .mount(&server)
        .await;

    let result = execute_final(
        &GeminiVideoGenerationQueryTool::default(),
        json!({"request": {"name": "operations/video-1"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert_typed_artifact(
        &result,
        ModelModality::Video,
        "video/mp4",
        "Generated video",
    );
}

#[tokio::test]
async fn gemini_text_to_speech_returns_typed_audio_artifact() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-3.1-flash-tts-preview:generateContent",
        ))
        .and(header("x-goog-api-key", "provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "inlineData": {
                            "mimeType": "audio/wav",
                            "data": general_purpose::STANDARD.encode(WAV_HEADER_BYTES)
                        }
                    }]
                }
            }]
        })))
        .mount(&server)
        .await;

    let result = execute_final(
        &GeminiTextToSpeechTool::default(),
        json!({"request": {
            "model": "gemini-3.1-flash-tts-preview",
            "contents": [{"parts": [{"text": "hello"}]}]
        }}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert_typed_artifact(
        &result,
        ModelModality::Audio,
        "audio/wav",
        "Generated speech",
    );
}

#[tokio::test]
async fn gemini_image_tool_passes_route_context_to_credential_resolver() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let tool = GeminiImageGenerationTool::default();
    let _ = tool
        .plan(
            &json!({"request": {"model": "gemini-3.1-flash-image"}}),
            &ctx_with_resolver(Arc::new(ContextCapturingResolver {
                captured: Arc::clone(&captured),
            })),
        )
        .await;

    let context = captured.lock().unwrap().pop().unwrap();
    assert_eq!(
        context.operation_id.as_deref(),
        Some("gemini.image_generation")
    );
    assert_eq!(
        context.route_kind,
        Some(CapabilityRouteKind::ImageGeneration)
    );
}

async fn execute_final(tool: &dyn Tool, input: serde_json::Value, ctx: ToolContext) -> ToolResult {
    tool.validate(&input, &ctx).await.unwrap();
    let plan = tool.plan(&input, &ctx).await.unwrap();
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();
    let stream = tool.execute_authorized(authorized, ctx).await.unwrap();
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
    let resolver: Arc<dyn ProviderCredentialResolverCap> = Arc::new(GeminiResolver {
        api_key: "provider-test-token".to_owned(),
        base_url: Some(base_url),
    });
    cap_registry.install(ToolCapability::ProviderCredentialResolver, resolver);
    let writer: Arc<dyn BlobWriterCap> = Arc::new(CapturingBlobWriter);
    cap_registry.install(ToolCapability::BlobWriter, writer);
    cap_registry.install(ToolCapability::NetworkBroker, broker_for_test());
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
        project_workspace_root: None,
        sandbox: None,
        cap_registry: Arc::new(cap_registry),
        redactor: Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::new(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

fn broker_for_test() -> Arc<dyn ToolNetworkBrokerCap> {
    Arc::new(
        ReqwestToolNetworkBroker::new_with_ticket_authority(
            Duration::from_secs(10),
            1_048_576,
            Arc::new(harness_contracts::NoopRedactor),
            test_ticket_authority(),
        )
        .expect("broker construction"),
    )
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

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    let ledger = harness_tool::TicketLedger::with_authority_key(
        Duration::from_secs(300),
        test_ticket_authority(),
    );
    let claims = harness_tool::AuthorizationTicketClaims {
        tenant_id: harness_contracts::TenantId::SINGLE,
        session_id: harness_contracts::SessionId::new(),
        run_id: harness_contracts::RunId::new(),
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
    };
    let ticket = ledger
        .mint(claims.clone(), Utc::now())
        .expect("test ticket should mint");
    ledger
        .consume(ticket.id, &claims, Utc::now())
        .expect("test ticket should consume")
}

fn test_ticket_authority() -> harness_tool::AuthorizationTicketKey {
    static KEY: OnceLock<harness_tool::AuthorizationTicketKey> = OnceLock::new();
    KEY.get_or_init(harness_tool::AuthorizationTicketKey::generate)
        .clone()
}

struct GeminiResolver {
    api_key: String,
    base_url: Option<String>,
}

impl ProviderCredentialResolverCap for GeminiResolver {
    fn resolve_provider_credential(
        &self,
        _context: ProviderCredentialResolveContext,
    ) -> BoxFuture<'_, Result<ProviderCredential, ToolError>> {
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        Box::pin(async move {
            Ok(ProviderCredential {
                provider_id: "gemini".to_owned(),
                config_id: "gemini-test".to_owned(),
                api_key,
                base_url,
            })
        })
    }
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
                provider_id: "gemini".to_owned(),
                config_id: "gemini-test".to_owned(),
                api_key: "provider-test-token".to_owned(),
                base_url: None,
            })
        })
    }
}
