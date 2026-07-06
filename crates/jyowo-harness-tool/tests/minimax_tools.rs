#![cfg(feature = "minimax-tools")]

use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    BlobMeta, BlobRef, BlobWriterCap, CapabilityRegistry, CapabilityRouteKind, ModelModality,
    PermissionSubject, ProviderCredential, ProviderCredentialResolveContext,
    ProviderCredentialResolverCap, ToolActionPlan, ToolCapability, ToolError, ToolResult,
    ToolResultPart,
};
use harness_contracts::{HostRule, RunId, SessionId, ToolUseId};
use harness_execution::ReqwestToolNetworkBroker;
use harness_tool::provider_media::MAX_MINIMAX_MEDIA_BYTES;
use harness_tool::{
    AuthorizedNetworkPermit, AuthorizedTicketSummary, AuthorizedToolInput, BuiltinToolset,
    InterruptToken, MiniMaxImageToImageTool, MiniMaxMusicGenerationTool, MiniMaxResponsesTool,
    MiniMaxTextToImageTool, MiniMaxTextToSpeechAsyncQueryTool, MiniMaxTextToSpeechAsyncTool,
    MiniMaxTextToSpeechTool, MiniMaxTextToVideoTool, MiniMaxVideoGenerationQueryTool,
    MiniMaxVideoTemplateQueryTool, Tool, ToolContext, ToolEvent, ToolNetworkBrokerCap,
    ToolRegistryBuilder,
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use wiremock::{
    matchers::{header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

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

    match error {
        ToolError::PermissionDenied(reason) => {
            assert!(reason.contains("MiniMax provider credential resolver is missing"));
            assert!(!reason.contains("sk-"));
        }
        other => panic!("expected denied permission error, got {other:?}"),
    }
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
    let plan = tool
        .plan(
            &json!({"request": {"prompt": "x"}}),
            &ctx_with_resolver(Arc::new(MiniMaxResolver {
                api_key: "sk-redacted-test-key".to_owned(),
                base_url: Some("https://api.minimax.io".to_owned()),
            })),
        )
        .await;

    match plan.unwrap().subject {
        PermissionSubject::NetworkAccess { host, port } => {
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
    let error = tool
        .plan(&json!({"request": {"prompt": "x"}}), &ctx())
        .await
        .unwrap_err();

    match error {
        ToolError::PermissionDenied(reason) => {
            assert!(reason.contains("MiniMax provider credential resolver is missing"));
            assert!(!reason.contains("sk-"));
        }
        other => panic!("expected denied permission error, got {other:?}"),
    }
}

#[tokio::test]
async fn minimax_permission_denies_invalid_credential_base_url() {
    let tool = MiniMaxTextToImageTool::default();
    let error = tool
        .plan(
            &json!({"request": {"prompt": "x"}}),
            &ctx_with_resolver(Arc::new(MiniMaxResolver {
                api_key: "sk-redacted-test-key".to_owned(),
                base_url: Some("not a url".to_owned()),
            })),
        )
        .await
        .unwrap_err();

    match error {
        ToolError::PermissionDenied(reason) => {
            assert!(reason.contains("MiniMax provider base URL is invalid"));
            assert!(!reason.contains("sk-"));
        }
        other => panic!("expected denied permission error, got {other:?}"),
    }
}

#[tokio::test]
async fn credential_route_image_tool_passes_image_generation_operation_id() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let tool = MiniMaxTextToImageTool::default();
    let _ = tool
        .plan(
            &json!({"request": {"prompt": "x"}}),
            &ctx_with_resolver(Arc::new(ContextCapturingResolver {
                captured: Arc::clone(&captured),
            })),
        )
        .await;

    let context = captured
        .lock()
        .unwrap()
        .pop()
        .expect("credential context captured");
    assert_eq!(
        context.operation_id.as_deref(),
        Some("minimax.image_generation")
    );
    assert_eq!(
        context.route_kind,
        Some(CapabilityRouteKind::ImageGeneration)
    );
}

#[tokio::test]
async fn credential_route_video_tool_passes_video_generation_operation_id() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let tool = MiniMaxTextToVideoTool::default();
    let _ = tool
        .plan(
            &json!({"request": {}}),
            &ctx_with_resolver(Arc::new(ContextCapturingResolver {
                captured: Arc::clone(&captured),
            })),
        )
        .await;

    let context = captured
        .lock()
        .unwrap()
        .pop()
        .expect("credential context captured");
    assert_eq!(
        context.operation_id.as_deref(),
        Some("minimax.video_generation")
    );
    assert_eq!(
        context.route_kind,
        Some(CapabilityRouteKind::VideoGeneration)
    );
}

#[tokio::test]
async fn credential_route_tts_tool_passes_text_to_speech_operation_id() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let tool = MiniMaxTextToSpeechTool::default();
    let _ = tool
        .plan(
            &json!({"request": {}}),
            &ctx_with_resolver(Arc::new(ContextCapturingResolver {
                captured: Arc::clone(&captured),
            })),
        )
        .await;

    let context = captured
        .lock()
        .unwrap()
        .pop()
        .expect("credential context captured");
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
        .plan(
            &json!({"request": {}}),
            &ctx_with_resolver(Arc::new(ContextCapturingResolver {
                captured: Arc::clone(&captured),
            })),
        )
        .await;

    let context = captured
        .lock()
        .unwrap()
        .pop()
        .expect("credential context captured");
    assert!(context.operation_id.is_none());
    assert!(context.route_kind.is_none());
}

const PNG_1X1_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";
const MP3_HEADER_BYTES: &[u8] = b"ID3\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
const MP4_HEADER_BYTES: &[u8] = b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00\x00\x00\x00\x00";

#[tokio::test]
async fn minimax_service_artifact_image_generation_returns_typed_image_artifact() {
    let server = MockServer::start().await;
    mount_minimax_post(
        &server,
        "/v1/image_generation",
        json!({"model": "image-01", "prompt": "tiny icon"}),
        json!({"data": {"image_base64": PNG_1X1_BASE64}}),
    )
    .await;

    let tool = MiniMaxTextToImageTool::default();
    let result = execute_final(
        &tool,
        json!({"request": {"model": "image-01", "prompt": "tiny icon"}}),
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
async fn minimax_service_artifact_image_to_image_returns_typed_image_artifact() {
    let server = MockServer::start().await;
    mount_minimax_post(
        &server,
        "/v1/image_generation",
        json!({"model": "image-01", "prompt": "edit"}),
        json!({"data": {"image_base64": PNG_1X1_BASE64}}),
    )
    .await;

    let tool = MiniMaxImageToImageTool::default();
    let result = execute_final(
        &tool,
        json!({"request": {"model": "image-01", "prompt": "edit"}}),
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
async fn minimax_service_artifact_video_generation_returns_async_job_output() {
    let server = MockServer::start().await;
    mount_minimax_post(
        &server,
        "/v1/video_generation",
        json!({"model": "MiniMax-Hailuo-2.3-Fast", "prompt": "wave"}),
        json!({"task_id": "video-task-1"}),
    )
    .await;

    let tool = MiniMaxTextToVideoTool::default();
    let result = execute_final(
        &tool,
        json!({"request": {"model": "MiniMax-Hailuo-2.3-Fast", "prompt": "wave"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    let ToolResult::Mixed(parts) = result else {
        panic!("expected mixed result, got {result:?}");
    };
    let part = parts
        .iter()
        .find_map(|part| match part {
            ToolResultPart::Structured { value, schema_ref } => Some((value, schema_ref)),
            _ => None,
        })
        .expect("async job structured output");
    assert_eq!(part.1.as_deref(), Some("provider_service_async_job.v1"));
    assert_eq!(part.0["kind"], "async_job");
    assert_eq!(part.0["jobId"], "video-task-1");
    assert_eq!(part.0["pollOperationId"], "minimax.video_generation.query");
    assert_eq!(part.0["artifactKind"], "video");
}

#[tokio::test]
async fn minimax_service_artifact_video_query_returns_typed_video_artifact() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/query/video_generation"))
        .and(query_param("task_id", "video-task-1"))
        .and(header("authorization", "Bearer provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "Success",
            "video_base64": general_purpose::STANDARD.encode(MP4_HEADER_BYTES)
        })))
        .mount(&server)
        .await;

    let tool = MiniMaxVideoGenerationQueryTool::default();
    let result = execute_final(
        &tool,
        json!({"request": {"task_id": "video-task-1"}}),
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
async fn minimax_service_artifact_video_query_downloads_allowed_https_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/query/video_generation"))
        .and(query_param("task_id", "video-task-url"))
        .and(header("authorization", "Bearer provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "video_url": format!("{}/generated/private-video.mp4", server.uri())
        })))
        .mount(&server)
        .await;
    mount_media_download(
        &server,
        "/generated/private-video.mp4",
        "video/mp4",
        MP4_HEADER_BYTES,
    )
    .await;

    let tool = MiniMaxVideoGenerationQueryTool::default();
    let result = execute_final(
        &tool,
        json!({"request": {"task_id": "video-task-url"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert_typed_artifact(
        &result,
        ModelModality::Video,
        "video/mp4",
        "Generated video",
    );
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("private-video.mp4"));
}

#[tokio::test]
async fn minimax_service_artifact_tts_sync_returns_typed_audio_artifact() {
    let server = MockServer::start().await;
    mount_minimax_post(
        &server,
        "/v1/t2a_v2",
        json!({"model": "speech-2.8-turbo", "text": "hi"}),
        json!({"data": {"audio": general_purpose::STANDARD.encode(MP3_HEADER_BYTES)}}),
    )
    .await;

    let tool = MiniMaxTextToSpeechTool::default();
    let result = execute_final(
        &tool,
        json!({"request": {"model": "speech-2.8-turbo", "text": "hi"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert_typed_artifact(
        &result,
        ModelModality::Audio,
        "audio/mpeg",
        "Generated speech",
    );
}

#[tokio::test]
async fn minimax_service_artifact_tts_async_returns_async_job_output() {
    let server = MockServer::start().await;
    mount_minimax_post(
        &server,
        "/v1/t2a_async_v2",
        json!({"model": "speech-2.8-turbo", "text": "long"}),
        json!({"task_id": "tts-task-1"}),
    )
    .await;

    let tool = MiniMaxTextToSpeechAsyncTool::default();
    let result = execute_final(
        &tool,
        json!({"request": {"model": "speech-2.8-turbo", "text": "long"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    let ToolResult::Mixed(parts) = result else {
        panic!("expected mixed result, got {result:?}");
    };
    let value = parts
        .iter()
        .find_map(|part| match part {
            ToolResultPart::Structured { value, .. } => Some(value),
            _ => None,
        })
        .expect("async job output");
    assert_eq!(value["kind"], "async_job");
    assert_eq!(value["jobId"], "tts-task-1");
    assert_eq!(
        value["pollOperationId"],
        "minimax.text_to_speech.async.query"
    );
    assert_eq!(value["artifactKind"], "audio");
}

#[tokio::test]
async fn minimax_service_artifact_tts_async_query_returns_typed_audio_artifact() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/query/t2a_async_query_v2"))
        .and(query_param("task_id", "tts-task-1"))
        .and(header("authorization", "Bearer provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "Success",
            "audio": general_purpose::STANDARD.encode(MP3_HEADER_BYTES)
        })))
        .mount(&server)
        .await;

    let tool = MiniMaxTextToSpeechAsyncQueryTool::default();
    let result = execute_final(
        &tool,
        json!({"request": {"task_id": "tts-task-1"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert_typed_artifact(
        &result,
        ModelModality::Audio,
        "audio/mpeg",
        "Generated speech",
    );
}

#[tokio::test]
async fn minimax_service_artifact_music_generation_returns_typed_audio_artifact() {
    let server = MockServer::start().await;
    mount_minimax_post(
        &server,
        "/v1/music_generation",
        json!({"model": "music-2.6", "prompt": "short"}),
        json!({"audio": general_purpose::STANDARD.encode(MP3_HEADER_BYTES)}),
    )
    .await;

    let tool = MiniMaxMusicGenerationTool::default();
    let result = execute_final(
        &tool,
        json!({"request": {"model": "music-2.6", "prompt": "short"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert_typed_artifact(
        &result,
        ModelModality::Audio,
        "audio/mpeg",
        "Generated music",
    );
}

#[tokio::test]
async fn minimax_service_artifact_rejects_disallowed_media_host() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/query/video_template_generation"))
        .and(query_param("task_id", "template-task"))
        .and(header("authorization", "Bearer provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "video_url": "https://example.invalid/private.mp4"
        })))
        .mount(&server)
        .await;

    let tool = MiniMaxVideoTemplateQueryTool::default();
    let error = execute_error(
        &tool,
        json!({"request": {"task_id": "template-task"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert!(error
        .to_string()
        .contains("provider media asset host is not allowed"));
    assert!(!error.to_string().contains("private.mp4"));
}

#[tokio::test]
async fn minimax_service_artifact_rejects_redirect_to_untrusted_host() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/query/video_generation"))
        .and(query_param("task_id", "video-task-redirect"))
        .and(header("authorization", "Bearer provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "video_url": format!("{}/redirect/video.mp4", server.uri())
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/redirect/video.mp4"))
        .respond_with(
            ResponseTemplate::new(302)
                .append_header("Location", "https://example.invalid/evil.mp4"),
        )
        .mount(&server)
        .await;

    let tool = MiniMaxVideoGenerationQueryTool::default();
    let error = execute_error(
        &tool,
        json!({"request": {"task_id": "video-task-redirect"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert!(
        error.to_string().contains("provider media download failed")
            || error.to_string().contains("redirect")
    );
    assert!(!error.to_string().contains("evil.mp4"));
}

#[tokio::test]
async fn minimax_service_artifact_rejects_excessive_content_length() {
    let server = MockServer::start().await;
    let oversized = usize::try_from(MAX_MINIMAX_MEDIA_BYTES + 1).unwrap();
    let oversized_body = vec![0u8; oversized];
    Mock::given(method("GET"))
        .and(path("/v1/query/video_generation"))
        .and(query_param("task_id", "video-task-length"))
        .and(header("authorization", "Bearer provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "video_url": format!("{}/generated/oversized.mp4", server.uri())
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/generated/oversized.mp4"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "video/mp4")
                .insert_header("content-length", oversized.to_string())
                .set_body_bytes(oversized_body),
        )
        .mount(&server)
        .await;

    let tool = MiniMaxVideoGenerationQueryTool::default();
    let error = execute_error(
        &tool,
        json!({"request": {"task_id": "video-task-length"}}),
        ctx_with_media(server.uri()),
    )
    .await;

    assert!(matches!(error, ToolError::ResultTooLarge { .. }));
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

async fn mount_minimax_post(
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

async fn mount_media_download(
    server: &MockServer,
    endpoint: &str,
    content_type: &str,
    bytes: &[u8],
) {
    Mock::given(method("GET"))
        .and(path(endpoint))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", content_type)
                .insert_header("content-length", bytes.len().to_string())
                .set_body_bytes(bytes.to_vec()),
        )
        .mount(server)
        .await;
}

// ── Broker integration tests (Task 7) ──

#[tokio::test]
async fn minimax_brokered_request_succeeds_against_approved_loopback() {
    let server = MockServer::start().await;

    // Mock the MiniMax image generation endpoint.
    Mock::given(method("POST"))
        .and(path("/v1/image_generation"))
        .and(header("authorization", "Bearer provider-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "image_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=",
            }
        })))
        .mount(&server)
        .await;

    let broker = broker_for_test();
    let ctx = ctx_with_broker(broker, server.uri());

    let tool = MiniMaxTextToImageTool::default();
    let result = execute_result(&tool, json!({"request": {"prompt": "test"}}), ctx).await;
    // Should succeed — the broker validates the host and dispatches the request.
    assert!(
        result.is_ok(),
        "brokered minimax request should succeed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn minimax_brokered_request_fails_when_broker_is_missing() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/image_generation"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": { "image_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=" }
        })))
        .mount(&server)
        .await;

    // Context WITHOUT the broker registered — the tool must fail when trying
    // to obtain `ToolNetworkBrokerCap` during execute_authorized.
    let ctx = ctx_with_media(server.uri());

    let tool = MiniMaxTextToImageTool::default();
    let error = execute_error(&tool, json!({"request": {"prompt": "test"}}), ctx).await;
    let msg = error.to_string();
    assert!(
        msg.contains("NetworkBroker") || msg.contains("capability"),
        "error must identify missing NetworkBroker capability: {msg}"
    );
}

fn broker_for_test() -> Arc<dyn ToolNetworkBrokerCap> {
    Arc::new(
        ReqwestToolNetworkBroker::new(
            std::time::Duration::from_secs(10),
            1_048_576,
            std::sync::Arc::new(harness_contracts::NoopRedactor),
        )
        .expect("broker construction"),
    )
}

fn permit_for_server(server: &MockServer) -> AuthorizedNetworkPermit {
    let addr = server.address();
    let host_rule = HostRule {
        pattern: addr.ip().to_string(),
        ports: Some(vec![addr.port()]),
    };
    AuthorizedNetworkPermit::for_test(
        "MiniMaxTextToImage",
        ToolUseId::new(),
        SessionId::new(),
        RunId::new(),
        vec![host_rule],
    )
}

fn ctx_with_broker(broker: Arc<dyn ToolNetworkBrokerCap>, base_url: String) -> ToolContext {
    let mut cap_registry = CapabilityRegistry::default();
    // Provider credential resolver
    let resolver: Arc<dyn ProviderCredentialResolverCap> = Arc::new(MiniMaxResolver {
        api_key: "provider-test-token".to_owned(),
        base_url: Some(base_url),
    });
    cap_registry.install(ToolCapability::ProviderCredentialResolver, resolver);
    // Blob writer
    let writer: Arc<dyn BlobWriterCap> = Arc::new(CapturingBlobWriter);
    cap_registry.install(ToolCapability::BlobWriter, writer);
    // Network broker
    cap_registry.install(ToolCapability::NetworkBroker, broker);
    ctx_with_cap_registry(cap_registry)
}

async fn execute_result(
    tool: &dyn Tool,
    input: serde_json::Value,
    ctx: ToolContext,
) -> Result<Vec<ToolEvent>, ToolError> {
    tool.validate(&input, &ctx).await.unwrap();
    let plan = tool.plan(&input, &ctx).await?;
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();
    let stream = tool.execute_authorized(authorized, ctx).await?;
    Ok(stream.collect::<Vec<_>>().await)
}

fn ctx_with_media(base_url: String) -> ToolContext {
    let mut cap_registry = CapabilityRegistry::default();
    let resolver: Arc<dyn ProviderCredentialResolverCap> = Arc::new(MiniMaxResolver {
        api_key: "provider-test-token".to_owned(),
        base_url: Some(base_url),
    });
    cap_registry.install(ToolCapability::ProviderCredentialResolver, resolver);
    let writer: Arc<dyn BlobWriterCap> = Arc::new(CapturingBlobWriter);
    cap_registry.install(ToolCapability::BlobWriter, writer);
    ctx_with_cap_registry(cap_registry)
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

async fn execute_error(tool: &dyn Tool, input: serde_json::Value, ctx: ToolContext) -> ToolError {
    tool.validate(&input, &ctx).await.unwrap();
    let plan = match tool.plan(&input, &ctx).await {
        Ok(plan) => plan,
        Err(error) => return error,
    };
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();
    let stream = tool.execute_authorized(authorized, ctx).await.unwrap();
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
        cap_registry: Arc::new(cap_registry),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::new(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    AuthorizedTicketSummary {
        ticket_id: harness_contracts::AuthorizationTicketId::new(),
        tenant_id: harness_contracts::TenantId::SINGLE,
        session_id: harness_contracts::SessionId::new(),
        run_id: harness_contracts::RunId::new(),
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
        consumed_at: Utc::now(),
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
