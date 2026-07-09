#![cfg(feature = "zhipu-tools")]

use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};

use bytes::Bytes;
use chrono::Utc;
use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    BlobMeta, BlobRef, BlobWriterCap, CapabilityRegistry, ModelModality, ProviderCredential,
    ProviderCredentialResolveContext, ProviderCredentialResolverCap, ToolActionPlan,
    ToolCapability, ToolError, ToolResult, ToolResultPart,
};
use harness_execution::ReqwestToolNetworkBroker;
use harness_tool::{
    AuthorizedTicketSummary, AuthorizedToolInput, InterruptToken, Tool, ToolContext, ToolEvent,
    ToolNetworkBrokerCap, ZhipuSpeechToTextTool, ZhipuTextToSpeechTool,
};
use serde_json::json;
use wiremock::{
    matchers::{body_string_contains, header, header_regex, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn zhipu_tts_posts_json_and_returns_audio_artifact() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/speech"))
        .and(header("authorization", "Bearer provider-test-token"))
        .and(header("content-type", "application/json"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "audio/wav")
                .set_body_bytes(Bytes::from_static(b"RIFF....WAVE")),
        )
        .mount(&server)
        .await;

    let events = execute_result(
        &ZhipuTextToSpeechTool::default(),
        json!({
            "request": {
                "model": "glm-tts",
                "input": "hello",
                "voice": "tongtong",
                "response_format": "wav"
            }
        }),
        ctx_with_broker(server.uri()),
    )
    .await
    .expect("tts should execute");

    let ToolEvent::Final(ToolResult::Mixed(parts)) = &events[0] else {
        panic!("expected mixed artifact result, got {:?}", events[0]);
    };
    assert!(parts.iter().any(|part| matches!(
        part,
        ToolResultPart::Artifact {
            artifact_kind: ModelModality::Audio,
            content_type,
            blob_ref,
            ..
        } if content_type == "audio/wav"
            && blob_ref.content_type.as_deref() == Some("audio/wav")
    )));
}

#[tokio::test]
async fn zhipu_stt_posts_multipart_and_returns_json() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .and(header("authorization", "Bearer provider-test-token"))
        .and(header_regex(
            "content-type",
            "multipart/form-data; boundary=jyowo-zhipu-[0-9a-f]{32}",
        ))
        .and(body_string_contains(
            "Content-Disposition: form-data; name=\"file_base64\"",
        ))
        .and(body_string_contains("abc123"))
        .and(body_string_contains(
            "Content-Disposition: form-data; name=\"stream\"",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({
                    "id": "asr_1",
                    "model": "glm-asr-2512",
                    "text": "hello"
                })),
        )
        .mount(&server)
        .await;

    let events = execute_result(
        &ZhipuSpeechToTextTool::default(),
        json!({
            "request": {
                "model": "glm-asr-2512",
                "file_base64": "abc123",
                "stream": false
            }
        }),
        ctx_with_broker(server.uri()),
    )
    .await
    .expect("stt should execute");

    let ToolEvent::Final(ToolResult::Structured(value)) = &events[0] else {
        panic!("expected structured result, got {:?}", events[0]);
    };
    assert_eq!(value["text"], "hello");
}

#[tokio::test]
async fn zhipu_stt_stream_response_returns_raw_event_stream() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .and(header("authorization", "Bearer provider-test-token"))
        .and(header_regex(
            "content-type",
            "multipart/form-data; boundary=jyowo-zhipu-[0-9a-f]{32}",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_bytes(Bytes::from_static(b"data: {\"text\":\"hello\"}\n\n")),
        )
        .mount(&server)
        .await;

    let events = execute_result(
        &ZhipuSpeechToTextTool::default(),
        json!({
            "request": {
                "model": "glm-asr-2512",
                "file_base64": "abc123",
                "stream": true
            }
        }),
        ctx_with_broker(server.uri()),
    )
    .await
    .expect("streaming stt should execute");

    let ToolEvent::Final(ToolResult::Structured(value)) = &events[0] else {
        panic!(
            "expected structured event stream result, got {:?}",
            events[0]
        );
    };
    assert_eq!(value["content_type"], "text/event-stream");
    assert_eq!(value["body"], "data: {\"text\":\"hello\"}\n\n");
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

fn ctx_with_broker(base_url: String) -> ToolContext {
    let mut cap_registry = CapabilityRegistry::default();
    let resolver: Arc<dyn ProviderCredentialResolverCap> = Arc::new(ZhipuResolver {
        base_url: Some(base_url),
    });
    cap_registry.install(ToolCapability::ProviderCredentialResolver, resolver);
    let writer: Arc<dyn BlobWriterCap> = Arc::new(CapturingBlobWriter);
    cap_registry.install(ToolCapability::BlobWriter, writer);
    cap_registry.install(ToolCapability::NetworkBroker, broker_for_test());
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

struct ZhipuResolver {
    base_url: Option<String>,
}

impl ProviderCredentialResolverCap for ZhipuResolver {
    fn resolve_provider_credential(
        &self,
        _context: ProviderCredentialResolveContext,
    ) -> BoxFuture<'_, Result<ProviderCredential, ToolError>> {
        let base_url = self.base_url.clone();
        Box::pin(async move {
            Ok(ProviderCredential {
                provider_id: "zhipu".to_owned(),
                config_id: "zhipu-test".to_owned(),
                api_key: "provider-test-token".to_owned(),
                base_url,
            })
        })
    }
}

struct CapturingBlobWriter;

impl BlobWriterCap for CapturingBlobWriter {
    fn write_blob(
        &self,
        _tenant_id: harness_contracts::TenantId,
        bytes: Bytes,
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
