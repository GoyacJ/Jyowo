#![cfg(feature = "server-adapter")]
#![allow(clippy::field_reassign_with_default)]

#[allow(dead_code)]
mod support;

use async_trait::async_trait;
use harness_contracts::{
    CapabilityRegistry, McpServerId, NetworkAccess, RunId, SessionId, TenantId, ToolActionPlan,
    ToolExecutionChannel, ToolUseId, TrustLevel, WorkspaceAccess,
};
use harness_mcp::{
    ExposedCapability, HarnessMcpBackend, HarnessMcpServer, IsolationMode, JsonRpcRequest,
    JsonRpcResponse, McpMetric, McpMetricOutcome, McpMetricsSink, McpPrompt, McpPromptMessages,
    McpReadResourceResult, McpResource, McpResourceContents, McpServerAdapter, McpServerAuditEvent,
    McpServerAuditSink, McpServerAuth, McpServerAuthValidator, McpServerError, McpServerPolicy,
    McpServerRateLimit, McpServerRequestContext, NoopMcpEventSink, PromptProvider,
    ResourceProvider, SamplingJsonRpcHandler, SamplingPolicy, SamplingProvider, SamplingRequest,
    SamplingResponse, StaticToolContextFactory, TenantMapping, TenantResolver, ToolContextFactory,
    MCP_SAMPLING_DENIED_CODE,
};
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, BuiltinToolset, InterruptToken, Tool,
    ToolContext, ToolRegistry,
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::net::TcpListener;

#[cfg(feature = "oauth")]
const TEST_RSA_PRIVATE_KEY: &str = r#"-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEAyRE6rHuNR0QbHO3H3Kt2pOKGVhQqGZXInOduQNxXzuKlvQTL
UTv4l4sggh5/CYYi/cvI+SXVT9kPWSKXxJXBXd/4LkvcPuUakBoAkfh+eiFVMh2V
rUyWyj3MFl0HTVF9KwRXLAcwkREiS3npThHRyIxuy0ZMeZfxVL5arMhw1SRELB8H
oGfG/AtH89BIE9jDBHZ9dLelK9a184zAf8LwoPLxvJb3Il5nncqPcSfKDDodMFBI
Mc4lQzDKL5gvmiXLXB1AGLm8KBjfE8s3L5xqi+yUod+j8MtvIj812dkS4QMiRVN/
by2h3ZY8LYVGrqZXZTcgn2ujn8uKjXLZVD5TdQIDAQABAoIBAHREk0I0O9DvECKd
WUpAmF3mY7oY9PNQiu44Yaf+AoSuyRpRUGTMIgc3u3eivOE8ALX0BmYUO5JtuRNZ
Dpvt4SAwqCnVUinIf6C+eH/wSurCpapSM0BAHp4aOA7igptyOMgMPYBHNA1e9A7j
E0dCxKWMl3DSWNyjQTk4zeRGEAEfbNjHrq6YCtjHSZSLmWiG80hnfnYos9hOr5Jn
LnyS7ZmFE/5P3XVrxLc/tQ5zum0R4cbrgzHiQP5RgfxGJaEi7XcgherCCOgurJSS
bYH29Gz8u5fFbS+Yg8s+OiCss3cs1rSgJ9/eHZuzGEdUZVARH6hVMjSuwvqVTFaE
8AgtleECgYEA+uLMn4kNqHlJS2A5uAnCkj90ZxEtNm3E8hAxUrhssktY5XSOAPBl
xyf5RuRGIImGtUVIr4HuJSa5TX48n3Vdt9MYCprO/iYl6moNRSPt5qowIIOJmIjY
2mqPDfDt/zw+fcDD3lmCJrFlzcnh0uea1CohxEbQnL3cypeLt+WbU6kCgYEAzSp1
9m1ajieFkqgoB0YTpt/OroDx38vvI5unInJlEeOjQ+oIAQdN2wpxBvTrRorMU6P0
7mFUbt1j+Co6CbNiw+X8HcCaqYLR5clbJOOWNR36PuzOpQLkfK8woupBxzW9B8gZ
mY8rB1mbJ+/WTPrEJy6YGmIEBkWylQ2VpW8O4O0CgYEApdbvvfFBlwD9YxbrcGz7
MeNCFbMz+MucqQntIKoKJ91ImPxvtc0y6e/Rhnv0oyNlaUOwJVu0yNgNG117w0g4
t/+Q38mvVC5xV7/cn7x9UMFk6MkqVir3dYGEqIl/OP1grY2Tq9HtB5iyG9L8NIam
QOLMyUqqMUILxdthHyFmiGkCgYEAn9+PjpjGMPHxL0gj8Q8VbzsFtou6b1deIRRA
2CHmSltltR1gYVTMwXxQeUhPMmgkMqUXzs4/WijgpthY44hK1TaZEKIuoxrS70nJ
4WQLf5a9k1065fDsFZD6yGjdGxvwEmlGMZgTwqV7t1I4X0Ilqhav5hcs5apYL7gn
PYPeRz0CgYALHCj/Ji8XSsDoF/MhVhnGdIs2P99NNdmo3R2Pv0CuZbDKMU559LJH
UvrKS8WkuWRDuKrz1W/EQKApFjDGpdqToZqriUFQzwy7mR3ayIiogzNtHcvbDHx8
oFnGY0OFksX/ye0/XGpy2SFxYRwGU98HPYeBvAQQrVjdkzfy7BmXQQ==
-----END RSA PRIVATE KEY-----"#;

#[cfg(feature = "oauth")]
const TEST_RSA_JWK_N: &str = "yRE6rHuNR0QbHO3H3Kt2pOKGVhQqGZXInOduQNxXzuKlvQTLUTv4l4sggh5_CYYi_cvI-SXVT9kPWSKXxJXBXd_4LkvcPuUakBoAkfh-eiFVMh2VrUyWyj3MFl0HTVF9KwRXLAcwkREiS3npThHRyIxuy0ZMeZfxVL5arMhw1SRELB8HoGfG_AtH89BIE9jDBHZ9dLelK9a184zAf8LwoPLxvJb3Il5nncqPcSfKDDodMFBIMc4lQzDKL5gvmiXLXB1AGLm8KBjfE8s3L5xqi-yUod-j8MtvIj812dkS4QMiRVN_by2h3ZY8LYVGrqZXZTcgn2ujn8uKjXLZVD5TdQ";

#[cfg(feature = "oauth")]
const TEST_RSA_JWK_E: &str = "AQAB";

#[tokio::test]
async fn server_lists_and_reads_adapter_resources() {
    let server = adapter_with(StaticResources);

    let listed = expect_result(
        server
            .handle_request(JsonRpcRequest::new(
                json!(1),
                "resources/list",
                Some(json!({})),
            ))
            .await,
    );
    assert_eq!(
        listed["resources"],
        json!([
            {
                "uri": "jyowo://sessions/active",
                "name": "active session",
                "description": "Current session metadata",
                "mimeType": "application/json"
            }
        ])
    );

    let read = expect_result(
        server
            .handle_request(JsonRpcRequest::new(
                json!(2),
                "resources/read",
                Some(json!({ "uri": "jyowo://sessions/active" })),
            ))
            .await,
    );
    assert_eq!(
        read,
        json!({
            "contents": [{
                "uri": "jyowo://sessions/active",
                "mimeType": "application/json",
                "text": "{\"session\":\"active\"}"
            }]
        })
    );
}

#[tokio::test]
async fn server_lists_and_gets_adapter_prompts() {
    let server = adapter_with(StaticResources);

    let listed = expect_result(
        server
            .handle_request(JsonRpcRequest::new(
                json!(3),
                "prompts/list",
                Some(json!({})),
            ))
            .await,
    );
    assert_eq!(
        listed["prompts"],
        json!([{ "name": "triage", "description": "Triage a session" }])
    );

    let prompt = expect_result(
        server
            .handle_request(JsonRpcRequest::new(
                json!(4),
                "prompts/get",
                Some(json!({
                    "name": "triage",
                    "arguments": { "focus": "runtime" }
                })),
            ))
            .await,
    );
    assert_eq!(
        prompt,
        json!({
            "messages": [{
                "role": "user",
                "content": {
                    "type": "text",
                    "text": "triage runtime"
                }
            }]
        })
    );
}

#[tokio::test]
async fn server_returns_jsonrpc_errors_for_missing_resources_and_prompts() {
    let server = adapter_with(StaticResources);

    let missing_resource = server
        .handle_request(JsonRpcRequest::new(
            json!(5),
            "resources/read",
            Some(json!({ "uri": "jyowo://missing" })),
        ))
        .await;
    assert_eq!(expect_error_code(missing_resource), -32602);

    let missing_prompt = server
        .handle_request(JsonRpcRequest::new(
            json!(6),
            "prompts/get",
            Some(json!({ "name": "missing", "arguments": {} })),
        ))
        .await;
    assert_eq!(expect_error_code(missing_prompt), -32602);
}

#[tokio::test]
async fn server_adapter_enforces_rate_limit_policy() {
    let mut policy = McpServerPolicy::default();
    policy.rate_limit = McpServerRateLimit {
        global_rps: 1,
        per_tenant_rps: 0,
        per_capability_rps: Default::default(),
        burst: 1,
        audit_throttle: true,
    };
    let server = adapter_with_policy(StaticResources, policy);

    let first = server
        .handle_request(JsonRpcRequest::new(json!(7), "ping", Some(json!({}))))
        .await;
    assert!(first.error.is_none());

    let second = server
        .handle_request(JsonRpcRequest::new(json!(8), "ping", Some(json!({}))))
        .await;
    assert_eq!(expect_error_code(second), -32029);
}

#[tokio::test]
async fn server_adapter_records_request_and_throttle_metrics() {
    let mut policy = McpServerPolicy::default();
    policy.rate_limit = McpServerRateLimit {
        global_rps: 1,
        per_tenant_rps: 0,
        per_capability_rps: Default::default(),
        burst: 1,
        audit_throttle: true,
    };
    let metrics = Arc::new(CollectingMetrics::default());
    let server = adapter_with_policy_and_metrics(StaticResources, policy, metrics.clone());

    let first = server
        .handle_request(JsonRpcRequest::new(json!(107), "ping", Some(json!({}))))
        .await;
    let second = server
        .handle_request(JsonRpcRequest::new(json!(108), "ping", Some(json!({}))))
        .await;

    assert!(first.error.is_none());
    assert_eq!(expect_error_code(second), -32029);
    let recorded = metrics.metrics();
    assert!(recorded.iter().any(|metric| {
        matches!(
            metric,
            McpMetric::ServerRequest {
                method,
                outcome: McpMetricOutcome::Success,
            } if *method == "ping"
        )
    }));
    assert!(recorded
        .iter()
        .any(|metric| matches!(metric, McpMetric::ServerThrottled { .. })));
}

#[tokio::test]
async fn server_adapter_records_tenant_isolation_rejection_metric() {
    let metrics = Arc::new(CollectingMetrics::default());
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .expect("registry");
    registry
        .register(Box::new(SchemaTool))
        .expect("schema tool");
    let mut policy = McpServerPolicy::default();
    policy.tenant_isolation.mode = IsolationMode::StrictTenant;
    let server = McpServerAdapter::builder(registry)
        .with_policy(policy)
        .with_tool_context_factory(ForeignTenantToolContextFactory)
        .with_metrics_sink(metrics.clone())
        .build()
        .expect("server adapter");

    let response = server
        .handle_request(JsonRpcRequest::new(
            json!(109),
            "tools/call",
            Some(json!({ "name": "schema_tool", "arguments": { "message": "x" } })),
        ))
        .await;

    assert_eq!(expect_error_code(response), -32603);
    assert!(metrics
        .metrics()
        .iter()
        .any(|metric| matches!(metric, McpMetric::ServerTenantIsolationRejected)));
}

#[tokio::test]
async fn server_adapter_requires_static_bearer_when_policy_demands_it() {
    let mut policy = McpServerPolicy::default();
    policy.auth = McpServerAuth::StaticBearer("secret".to_owned());
    let server = adapter_with_policy(StaticResources, policy);

    let missing = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(9), "ping", Some(json!({}))),
            McpServerRequestContext::default(),
        )
        .await;
    assert_eq!(expect_error_code(missing), -32040);

    let allowed = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(10), "ping", Some(json!({}))),
            McpServerRequestContext::default().with_header("Authorization", "Bearer secret"),
        )
        .await;
    assert!(allowed.error.is_none());
}

#[tokio::test]
async fn server_adapter_accepts_custom_auth_validator() {
    let mut policy = McpServerPolicy::default();
    policy.auth = McpServerAuth::Custom(std::sync::Arc::new(HeaderAuth));
    let server = adapter_with_policy(StaticResources, policy);

    let rejected = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(11), "ping", Some(json!({}))),
            McpServerRequestContext::default(),
        )
        .await;
    assert_eq!(expect_error_code(rejected), -32040);

    let allowed = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(12), "ping", Some(json!({}))),
            McpServerRequestContext::default().with_header("x-test-auth", "yes"),
        )
        .await;
    assert!(allowed.error.is_none());
}

#[tokio::test]
async fn server_adapter_accepts_only_verified_external_mtls_identity() {
    let mut policy = McpServerPolicy::default();
    policy.auth = McpServerAuth::MutualTlsExternal {
        allowed_subjects: std::collections::BTreeSet::from(["CN=trusted-client".to_owned()]),
        allowed_sha256_fingerprints: std::collections::BTreeSet::new(),
    };
    let server = adapter_with_policy(StaticResources, policy);

    let missing = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(13), "ping", Some(json!({}))),
            McpServerRequestContext::default(),
        )
        .await;
    assert_eq!(expect_error_code(missing), -32040);

    let spoofed = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(14), "ping", Some(json!({}))),
            McpServerRequestContext::default()
                .with_header("x-client-cert-subject", "CN=trusted-client"),
        )
        .await;
    assert_eq!(expect_error_code(spoofed), -32040);

    let allowed = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(15), "ping", Some(json!({}))),
            McpServerRequestContext::default()
                .with_verified_client_cert_subject("CN=trusted-client"),
        )
        .await;
    assert!(allowed.error.is_none());
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn oauth_validator_populates_verified_claims_for_tenant_mapping() {
    let jwks = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/jwks"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(test_jwks("rsa01")))
        .mount(&jwks)
        .await;

    let mut policy = McpServerPolicy::default();
    policy.auth = McpServerAuth::OAuthValidator {
        issuer: "https://issuer.example.com".to_owned(),
        audience: "jyowo-mcp".to_owned(),
        jwks_url: format!("{}/jwks", jwks.uri()),
    };
    policy.tenant_mapping = TenantMapping::Claim("tenant_id".to_owned());
    let server = adapter_with_policy(StaticResources, policy);
    let token = test_jwt(
        "rsa01",
        "https://issuer.example.com",
        "jyowo-mcp",
        TenantId::SINGLE.to_string(),
        600,
    );

    let allowed = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(121), "ping", Some(json!({}))),
            McpServerRequestContext::default()
                .with_header("authorization", format!("Bearer {token}")),
        )
        .await;

    assert!(
        allowed.error.is_none(),
        "unexpected error: {:?}",
        allowed.error
    );
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn oauth_validator_rejects_invalid_issuer_audience_expiry_and_unknown_kid() {
    let jwks = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/jwks"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(test_jwks("rsa01")))
        .mount(&jwks)
        .await;

    let mut policy = McpServerPolicy::default();
    policy.auth = McpServerAuth::OAuthValidator {
        issuer: "https://issuer.example.com".to_owned(),
        audience: "jyowo-mcp".to_owned(),
        jwks_url: format!("{}/jwks", jwks.uri()),
    };
    policy.tenant_mapping = TenantMapping::Claim("tenant_id".to_owned());
    let server = adapter_with_policy(StaticResources, policy);
    let cases = [
        test_jwt(
            "rsa01",
            "https://wrong.example.com",
            "jyowo-mcp",
            TenantId::SINGLE.to_string(),
            600,
        ),
        test_jwt(
            "rsa01",
            "https://issuer.example.com",
            "wrong-audience",
            TenantId::SINGLE.to_string(),
            600,
        ),
        test_jwt(
            "rsa01",
            "https://issuer.example.com",
            "jyowo-mcp",
            TenantId::SINGLE.to_string(),
            -600,
        ),
        test_jwt(
            "rsa02",
            "https://issuer.example.com",
            "jyowo-mcp",
            TenantId::SINGLE.to_string(),
            600,
        ),
    ];

    for token in cases {
        let rejected = server
            .handle_request_with_context(
                JsonRpcRequest::new(json!(122), "ping", Some(json!({}))),
                McpServerRequestContext::default()
                    .with_header("authorization", format!("Bearer {token}")),
            )
            .await;
        assert_eq!(expect_error_code(rejected), -32040);
    }
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn oauth_validator_refreshes_jwks_when_cached_set_has_unknown_kid() {
    let jwks = wiremock::MockServer::start().await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let jwks_calls = calls.clone();
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/jwks"))
        .respond_with(move |_: &wiremock::Request| {
            let call = jwks_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let kid = if call == 0 { "rsa01" } else { "rsa02" };
            wiremock::ResponseTemplate::new(200).set_body_json(test_jwks(kid))
        })
        .expect(2)
        .mount(&jwks)
        .await;

    let mut policy = McpServerPolicy::default();
    policy.auth = McpServerAuth::OAuthValidator {
        issuer: "https://issuer.example.com".to_owned(),
        audience: "jyowo-mcp".to_owned(),
        jwks_url: format!("{}/jwks", jwks.uri()),
    };
    policy.tenant_mapping = TenantMapping::Claim("tenant_id".to_owned());
    let server = adapter_with_policy(StaticResources, policy);

    let first = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(123), "ping", Some(json!({}))),
            McpServerRequestContext::default().with_header(
                "authorization",
                format!(
                    "Bearer {}",
                    test_jwt(
                        "rsa01",
                        "https://issuer.example.com",
                        "jyowo-mcp",
                        TenantId::SINGLE.to_string(),
                        600,
                    )
                ),
            ),
        )
        .await;
    let rotated = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(124), "ping", Some(json!({}))),
            McpServerRequestContext::default().with_header(
                "authorization",
                format!(
                    "Bearer {}",
                    test_jwt(
                        "rsa02",
                        "https://issuer.example.com",
                        "jyowo-mcp",
                        TenantId::SINGLE.to_string(),
                        600,
                    )
                ),
            ),
        )
        .await;

    assert!(first.error.is_none());
    assert!(rotated.error.is_none());
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn oauth_validator_fails_closed_when_forced_jwks_refresh_fails() {
    let jwks = wiremock::MockServer::start().await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let jwks_calls = calls.clone();
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/jwks"))
        .respond_with(move |_: &wiremock::Request| {
            let call = jwks_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if call == 0 {
                return wiremock::ResponseTemplate::new(200).set_body_json(test_jwks("rsa01"));
            }
            wiremock::ResponseTemplate::new(500)
        })
        .expect(2)
        .mount(&jwks)
        .await;

    let mut policy = McpServerPolicy::default();
    policy.auth = McpServerAuth::OAuthValidator {
        issuer: "https://issuer.example.com".to_owned(),
        audience: "jyowo-mcp".to_owned(),
        jwks_url: format!("{}/jwks", jwks.uri()),
    };
    policy.tenant_mapping = TenantMapping::Claim("tenant_id".to_owned());
    let server = adapter_with_policy(StaticResources, policy);

    let first = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(125), "ping", Some(json!({}))),
            McpServerRequestContext::default().with_header(
                "authorization",
                format!(
                    "Bearer {}",
                    test_jwt(
                        "rsa01",
                        "https://issuer.example.com",
                        "jyowo-mcp",
                        TenantId::SINGLE.to_string(),
                        600,
                    )
                ),
            ),
        )
        .await;
    let rejected = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(126), "ping", Some(json!({}))),
            McpServerRequestContext::default().with_header(
                "authorization",
                format!(
                    "Bearer {}",
                    test_jwt(
                        "rsa02",
                        "https://issuer.example.com",
                        "jyowo-mcp",
                        TenantId::SINGLE.to_string(),
                        600,
                    )
                ),
            ),
        )
        .await;

    assert!(first.error.is_none());
    assert_eq!(expect_error_code(rejected), -32040);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn tenant_mapping_resolves_tenant_from_header_before_isolation() {
    let mut policy = McpServerPolicy::default();
    policy.tenant_mapping = TenantMapping::Header("x-tenant-id".to_owned());
    let server = adapter_with_policy(StaticResources, policy);

    let missing = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(13), "ping", Some(json!({}))),
            McpServerRequestContext::default(),
        )
        .await;
    assert_eq!(expect_error_code(missing), -32041);

    let allowed = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(14), "ping", Some(json!({}))),
            McpServerRequestContext::default()
                .with_header("x-tenant-id", TenantId::SINGLE.to_string()),
        )
        .await;
    assert!(allowed.error.is_none());
}

#[tokio::test]
async fn tenant_mapping_accepts_custom_resolver() {
    let mut policy = McpServerPolicy::default();
    policy.tenant_mapping = TenantMapping::Custom(std::sync::Arc::new(HeaderTenant));
    let server = adapter_with_policy(StaticResources, policy);

    let rejected = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(15), "ping", Some(json!({}))),
            McpServerRequestContext::default(),
        )
        .await;
    assert_eq!(expect_error_code(rejected), -32041);

    let allowed = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(16), "ping", Some(json!({}))),
            McpServerRequestContext::default().with_header("x-test-tenant", "single"),
        )
        .await;
    assert!(allowed.error.is_none());
}

#[tokio::test]
async fn tenant_mapping_resolves_tenant_from_verified_claim() {
    let mut policy = McpServerPolicy::default();
    policy.tenant_mapping = TenantMapping::Claim("workspace_id".to_owned());
    let server = adapter_with_policy(StaticResources, policy);
    let tenant = TenantId::new();

    let missing = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(24), "ping", Some(json!({}))),
            McpServerRequestContext::default(),
        )
        .await;
    assert_eq!(expect_error_code(missing), -32041);

    let header_only = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(37), "ping", Some(json!({}))),
            McpServerRequestContext::default().with_header("workspace_id", tenant.to_string()),
        )
        .await;
    assert_eq!(expect_error_code(header_only), -32041);

    let allowed = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(25), "ping", Some(json!({}))),
            McpServerRequestContext::default()
                .with_verified_claim("workspace_id", tenant.to_string()),
        )
        .await;
    assert!(allowed.error.is_none());
}

#[tokio::test]
async fn harness_mcp_server_public_serving_is_fail_closed_without_auth() {
    let server = HarnessMcpServer::new(std::sync::Arc::new(TestHarness))
        .build()
        .expect("server");

    let http = server
        .clone()
        .serve_http("127.0.0.1:0".parse().expect("addr"))
        .await;
    assert!(matches!(http, Err(McpServerError::UnsafeServing(_))));

    let websocket = server
        .serve_websocket("127.0.0.1:0".parse().expect("addr"))
        .await;
    assert!(matches!(websocket, Err(McpServerError::UnsafeServing(_))));
}

#[tokio::test]
async fn harness_mcp_server_serves_http_jsonrpc_with_request_headers() {
    let mut policy = McpServerPolicy::default();
    policy.auth = McpServerAuth::StaticBearer("secret".to_owned());
    policy.tenant_mapping = TenantMapping::Header("x-tenant-id".to_owned());
    let backend = Arc::new(TestHarness);
    let server = HarnessMcpServer::new(Arc::clone(&backend))
        .with_policy(policy)
        .build()
        .expect("server");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let task = tokio::spawn(server.serve_http_listener(listener));

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}"))
        .header("authorization", "Bearer secret")
        .header("x-tenant-id", TenantId::SINGLE.to_string())
        .json(&JsonRpcRequest::new(
            json!(31),
            "tools/list",
            Some(json!({})),
        ))
        .send()
        .await
        .expect("http response")
        .json::<JsonRpcResponse>()
        .await
        .expect("json-rpc response");

    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    let result = response.result.expect("result");
    let tools = result["tools"].as_array().expect("tools");
    assert!(tools.iter().any(|tool| tool["name"] == "sessions_list"));

    let call = client
        .post(format!("http://{addr}"))
        .header("authorization", "Bearer secret")
        .header("x-tenant-id", TenantId::SINGLE.to_string())
        .json(&JsonRpcRequest::new(
            json!(33),
            "tools/call",
            Some(json!({
                "name": "sessions_list",
                "arguments": {}
            })),
        ))
        .send()
        .await
        .expect("http tool response")
        .json::<JsonRpcResponse>()
        .await
        .expect("json-rpc tool response");
    task.abort();
    assert!(call.error.is_none(), "unexpected error: {:?}", call.error);
}

#[tokio::test]
#[cfg(feature = "websocket")]
async fn harness_mcp_server_serves_websocket_jsonrpc_with_request_headers() {
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::{
        connect_async,
        tungstenite::{client::IntoClientRequest, Message},
    };

    let mut policy = McpServerPolicy::default();
    policy.auth = McpServerAuth::StaticBearer("secret".to_owned());
    policy.tenant_mapping = TenantMapping::Header("x-tenant-id".to_owned());
    let backend = Arc::new(TestHarness);
    let server = HarnessMcpServer::new(Arc::clone(&backend))
        .with_policy(policy)
        .build()
        .expect("server");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let task = tokio::spawn(server.serve_websocket_listener(listener));

    let mut request = format!("ws://{addr}")
        .into_client_request()
        .expect("request");
    request
        .headers_mut()
        .insert("authorization", "Bearer secret".parse().expect("auth"));
    request.headers_mut().insert(
        "x-tenant-id",
        TenantId::SINGLE.to_string().parse().expect("tenant"),
    );
    let (mut socket, _) = connect_async(request).await.expect("connect");
    socket
        .send(Message::Text(
            serde_json::to_string(&JsonRpcRequest::new(
                json!(32),
                "initialize",
                Some(json!({})),
            ))
            .expect("request json"),
        ))
        .await
        .expect("send");
    let response = socket
        .next()
        .await
        .expect("message")
        .expect("message")
        .into_text()
        .expect("text");
    let response: JsonRpcResponse = serde_json::from_str(&response).expect("json");

    task.abort();
    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    assert_eq!(
        response.result.expect("result")["serverInfo"]["name"],
        "jyowo-harness-mcp"
    );
    socket
        .send(Message::Text(
            serde_json::to_string(&JsonRpcRequest::new(
                json!(34),
                "tools/list",
                Some(json!({})),
            ))
            .expect("request json"),
        ))
        .await
        .expect("send tools/list");
    let listed = socket
        .next()
        .await
        .expect("message")
        .expect("message")
        .into_text()
        .expect("text");
    let listed: JsonRpcResponse = serde_json::from_str(&listed).expect("json");
    assert!(
        listed.error.is_none(),
        "unexpected error: {:?}",
        listed.error
    );

    socket
        .send(Message::Text(
            serde_json::to_string(&JsonRpcRequest::new(
                json!(35),
                "tools/call",
                Some(json!({
                    "name": "sessions_list",
                    "arguments": {}
                })),
            ))
            .expect("request json"),
        ))
        .await
        .expect("send tools/call");
    let called = socket
        .next()
        .await
        .expect("message")
        .expect("message")
        .into_text()
        .expect("text");
    let called: JsonRpcResponse = serde_json::from_str(&called).expect("json");
    assert!(
        called.error.is_none(),
        "unexpected error: {:?}",
        called.error
    );
}

#[tokio::test]
async fn server_adapter_validates_tool_input_against_schema_before_execution() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(SchemaTool))
        .build()
        .expect("registry");
    let server = McpServerAdapter::builder(registry)
        .with_tool_context_factory(StaticToolContextFactory::new(tool_context()))
        .build()
        .expect("server adapter");

    let response = server
        .handle_request(JsonRpcRequest::new(
            json!(26),
            "tools/call",
            Some(json!({
                "name": "schema_tool",
                "arguments": { "extra": true }
            })),
        ))
        .await;

    assert_eq!(expect_error_code(response), -32602);
}

#[tokio::test]
async fn server_adapter_authorizes_tool_calls_from_authorization_context() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(SchemaTool))
        .build()
        .expect("registry");
    let server = McpServerAdapter::builder(registry)
        .with_tool_context_factory(StaticToolContextFactory::new(tool_context()))
        .with_authorization_context(support::mcp_authorization_context_allowing_tool(
            "schema_tool",
        ))
        .build()
        .expect("server adapter");

    let response = server
        .handle_request(JsonRpcRequest::new(
            json!(126),
            "tools/call",
            Some(json!({
                "name": "schema_tool",
                "arguments": { "message": "authorized" }
            })),
        ))
        .await;

    let result = expect_result(response);
    assert_eq!(result["content"][0]["text"], "ok");
}

#[tokio::test]
async fn server_adapter_authorization_uses_tool_context_identity() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(SchemaTool))
        .build()
        .expect("registry");
    let authorization_events = Arc::new(support::RecordingAuthorizationEventSink::default());
    let event_sink: Arc<dyn harness_execution::AuthorizationEventSink> =
        authorization_events.clone();
    let mut context = tool_context();
    context.session_id = SessionId::from_u128(10);
    context.run_id = RunId::from_u128(11);
    let server = McpServerAdapter::builder(registry)
        .with_tool_context_factory(StaticToolContextFactory::new(context))
        .with_authorization_context(support::mcp_authorization_context_allowing_tool_with_sink(
            "schema_tool",
            event_sink,
        ))
        .build()
        .expect("server adapter");

    let response = server
        .handle_request(JsonRpcRequest::new(
            json!(128),
            "tools/call",
            Some(json!({
                "name": "schema_tool",
                "arguments": { "message": "authorized" }
            })),
        ))
        .await;

    let result = expect_result(response);
    assert_eq!(result["content"][0]["text"], "ok");
    assert!(authorization_events.events().iter().any(|event| {
        matches!(
            event,
            harness_contracts::Event::PermissionRequested(permission)
                if permission.session_id == SessionId::from_u128(10)
                    && permission.run_id == RunId::from_u128(11)
        )
    }));
}

#[tokio::test]
async fn server_adapter_denies_tool_calls_from_authorization_context_without_execution() {
    let executed = Arc::new(Mutex::new(false));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(ExecutionProbeTool {
            executed: Arc::clone(&executed),
        }))
        .build()
        .expect("registry");
    let server = McpServerAdapter::builder(registry)
        .with_tool_context_factory(StaticToolContextFactory::new(tool_context()))
        .with_authorization_context(support::mcp_authorization_context_allowing_tool(
            "other_tool",
        ))
        .build()
        .expect("server adapter");

    let response = server
        .handle_request(JsonRpcRequest::new(
            json!(127),
            "tools/call",
            Some(json!({
                "name": "schema_tool",
                "arguments": { "message": "denied" }
            })),
        ))
        .await;

    let result = expect_result(response);
    assert_eq!(result["isError"], true);
    assert_eq!(*executed.lock(), false);
}

#[tokio::test]
async fn server_adapter_emits_audit_events_for_tenant_mapping_and_throttle_rejections() {
    let audit = Arc::new(RecordingAudit::default());
    let mut policy = McpServerPolicy::default();
    policy.tenant_mapping = TenantMapping::Header("x-tenant-id".to_owned());
    policy.rate_limit = McpServerRateLimit {
        global_rps: 1,
        per_tenant_rps: 0,
        per_capability_rps: Default::default(),
        burst: 1,
        audit_throttle: true,
    };
    let server = McpServerAdapter::builder(
        ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .build()
            .expect("registry"),
    )
    .with_policy(policy)
    .with_audit_sink(Arc::clone(&audit))
    .with_tool_context_factory(StaticToolContextFactory::new(tool_context()))
    .build()
    .expect("server adapter");

    let missing_tenant = server
        .handle_request(JsonRpcRequest::new(json!(27), "ping", Some(json!({}))))
        .await;
    assert_eq!(expect_error_code(missing_tenant), -32041);

    let tenant = TenantId::new();
    let context = McpServerRequestContext::default().with_header("x-tenant-id", tenant.to_string());
    assert!(server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(28), "ping", Some(json!({}))),
            context.clone(),
        )
        .await
        .error
        .is_none());
    let throttled = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(29), "ping", Some(json!({}))),
            context,
        )
        .await;
    assert_eq!(expect_error_code(throttled), -32029);

    let events = audit.events.lock().clone();
    assert!(events
        .iter()
        .any(|event| matches!(event, McpServerAuditEvent::TenantMappingRejected { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event, McpServerAuditEvent::RateLimited { .. })));
}

#[tokio::test]
async fn harness_server_validates_9_plus_1_tool_input_against_schema() {
    let backend = std::sync::Arc::new(TestHarness);
    let server = HarnessMcpServer::new(std::sync::Arc::clone(&backend))
        .build()
        .expect("server");

    let response = server
        .handle_request(JsonRpcRequest::new(
            json!(30),
            "tools/call",
            Some(json!({
                "name": "messages_send",
                "arguments": { "session_id": "01HF7YAT00TEST000000000000", "unknown": true }
            })),
        ))
        .await;

    assert_eq!(expect_error_code(response), -32602);
}

#[tokio::test]
async fn harness_server_sanitizes_internal_jsonrpc_errors() {
    let server = HarnessMcpServer::new(Arc::new(FailingHarness))
        .build()
        .expect("server");

    let response = server
        .handle_request(JsonRpcRequest::new(
            json!(36),
            "tools/call",
            Some(json!({
                "name": "sessions_list",
                "arguments": {}
            })),
        ))
        .await;
    let error = response.error.expect("json-rpc error");

    assert_eq!(error.code, -32603);
    assert_eq!(error.message, "internal error");
    assert!(!error.message.contains("postgres://secret"));
}

#[tokio::test]
async fn server_adapter_routes_sampling_create_message_to_fail_closed_handler() {
    let server = adapter_with(StaticResources);

    let response = server
        .handle_request(JsonRpcRequest::new(
            json!(17),
            "sampling/createMessage",
            Some(json!({
                "request_id": harness_contracts::RequestId::from_u128(3),
                "model": "claude-3-5-sonnet",
                "input_tokens": 1,
                "max_tokens": 2,
                "messages": []
            })),
        ))
        .await;

    assert!(matches!(
        response.error,
        Some(error)
            if error.code == MCP_SAMPLING_DENIED_CODE
                && error.message == "sampling/createMessage denied"
    ));
}

#[tokio::test]
async fn server_adapter_injects_authorization_context_into_sampling_handler() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .expect("registry");
    let server = McpServerAdapter::builder(registry)
        .with_tool_context_factory(StaticToolContextFactory::new(tool_context()))
        .with_sampling_handler(
            SamplingJsonRpcHandler::new(SamplingPolicy::allow_auto(), Arc::new(NoopMcpEventSink))
                .with_session_id(SessionId::from_u128(1))
                .with_run_id(Some(RunId::from_u128(2)))
                .with_server_id(McpServerId("github".to_owned()))
                .with_server_trust(TrustLevel::AdminTrusted)
                .with_provider(Arc::new(EchoSamplingProvider)),
        )
        .with_authorization_context(support::mcp_authorization_context())
        .build()
        .expect("server");

    let response = server
        .handle_request(JsonRpcRequest::new(
            json!(18),
            "sampling/createMessage",
            Some(json!({
                "request_id": harness_contracts::RequestId::from_u128(4),
                "model": "claude-3-5-sonnet",
                "input_tokens": 1,
                "max_tokens": 2,
                "messages": [{ "role": "user", "content": { "type": "text", "text": "hello" } }]
            })),
        ))
        .await;

    assert!(
        response.error.is_none(),
        "unexpected response: {response:?}"
    );
    assert_eq!(
        response.result,
        Some(json!({
            "model": "test",
            "role": "assistant",
            "content": { "type": "text", "text": "ok" },
            "stopReason": "endTurn"
        }))
    );
}

#[derive(Default)]
struct RecordingAudit {
    events: Mutex<Vec<McpServerAuditEvent>>,
}

struct EchoSamplingProvider;

#[async_trait]
impl SamplingProvider for EchoSamplingProvider {
    async fn create_message(
        &self,
        _request: SamplingRequest,
    ) -> Result<SamplingResponse, harness_mcp::McpError> {
        Ok(SamplingResponse {
            model_id: "test".to_owned(),
            content: json!({ "type": "text", "text": "ok" }),
            input_tokens: 1,
            output_tokens: 1,
        })
    }
}

impl McpServerAuditSink for RecordingAudit {
    fn record(&self, event: McpServerAuditEvent) {
        self.events.lock().push(event);
    }
}

#[derive(Default)]
struct CollectingMetrics {
    metrics: Mutex<Vec<McpMetric>>,
}

impl CollectingMetrics {
    fn metrics(&self) -> Vec<McpMetric> {
        self.metrics.lock().clone()
    }
}

impl McpMetricsSink for CollectingMetrics {
    fn record(&self, metric: McpMetric) {
        self.metrics.lock().push(metric);
    }
}

struct ForeignTenantToolContextFactory;

#[async_trait]
impl ToolContextFactory for ForeignTenantToolContextFactory {
    async fn create_tool_context(
        &self,
        _tool_name: &str,
        _arguments: &Value,
    ) -> Result<ToolContext, McpServerError> {
        let mut context = tool_context();
        context.tenant_id = TenantId::from_u128(2);
        Ok(context)
    }
}

struct SchemaTool;

#[async_trait]
impl Tool for SchemaTool {
    fn descriptor(&self) -> &harness_contracts::ToolDescriptor {
        static DESCRIPTOR: std::sync::OnceLock<harness_contracts::ToolDescriptor> =
            std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| harness_contracts::ToolDescriptor {
            name: "schema_tool".to_owned(),
            display_name: "schema_tool".to_owned(),
            description: "schema tool".to_owned(),
            category: "test".to_owned(),
            group: harness_contracts::ToolGroup::Custom("test".to_owned()),
            version: harness_contracts::SemverString::from("0.1.0"),
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["message"],
                "properties": {
                    "message": { "type": "string" }
                }
            }),
            output_schema: None,
            dynamic_schema: false,
            properties: harness_contracts::ToolProperties {
                is_concurrency_safe: true,
                is_read_only: true,
                is_destructive: false,
                long_running: None,
                defer_policy: harness_contracts::DeferPolicy::AlwaysLoad,
            },
            trust_level: harness_contracts::TrustLevel::AdminTrusted,
            required_capabilities: Vec::new(),
            budget: harness_contracts::ResultBudget {
                metric: harness_contracts::BudgetMetric::Chars,
                limit: 10_000,
                on_overflow: harness_contracts::OverflowAction::Truncate,
                preview_head_chars: 1_000,
                preview_tail_chars: 200,
            },
            provider_restriction: harness_contracts::ProviderRestriction::All,
            origin: harness_contracts::ToolOrigin::Builtin,
            search_hint: None,
            service_binding: None,
            metadata: Default::default(),
        })
    }

    async fn validate(
        &self,
        _input: &Value,
        _ctx: &ToolContext,
    ) -> Result<(), harness_tool::ValidationError> {
        Ok(())
    }

    async fn plan(
        &self,
        input: &Value,
        ctx: &ToolContext,
    ) -> Result<ToolActionPlan, harness_contracts::ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            harness_tool::PermissionCheck::Allowed,
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<harness_tool::ToolStream, harness_contracts::ToolError> {
        Ok(Box::pin(futures::stream::iter([
            harness_tool::ToolEvent::Final(harness_contracts::ToolResult::Text("ok".to_owned())),
        ])))
    }
}

struct ExecutionProbeTool {
    executed: Arc<Mutex<bool>>,
}

#[async_trait]
impl Tool for ExecutionProbeTool {
    fn descriptor(&self) -> &harness_contracts::ToolDescriptor {
        SchemaTool.descriptor()
    }

    async fn validate(
        &self,
        input: &Value,
        ctx: &ToolContext,
    ) -> Result<(), harness_tool::ValidationError> {
        SchemaTool.validate(input, ctx).await
    }

    async fn plan(
        &self,
        input: &Value,
        ctx: &ToolContext,
    ) -> Result<ToolActionPlan, harness_contracts::ToolError> {
        SchemaTool.plan(input, ctx).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<harness_tool::ToolStream, harness_contracts::ToolError> {
        *self.executed.lock() = true;
        SchemaTool.execute_authorized(authorized, ctx).await
    }
}

fn adapter_with(provider: StaticResources) -> McpServerAdapter {
    adapter_with_policy(provider, McpServerPolicy::default())
}

fn adapter_with_policy(provider: StaticResources, policy: McpServerPolicy) -> McpServerAdapter {
    adapter_with_policy_and_metrics(provider, policy, Arc::new(CollectingMetrics::default()))
}

fn adapter_with_policy_and_metrics(
    provider: StaticResources,
    policy: McpServerPolicy,
    metrics: Arc<CollectingMetrics>,
) -> McpServerAdapter {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .expect("registry");
    McpServerAdapter::builder(registry)
        .with_policy(policy)
        .with_tool_context_factory(StaticToolContextFactory::new(tool_context()))
        .with_resource_provider(provider.clone())
        .with_prompt_provider(provider)
        .with_metrics_sink(metrics)
        .build()
        .expect("server adapter")
}

fn expect_result(response: JsonRpcResponse) -> Value {
    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    response.result.expect("result")
}

fn expect_error_code(response: JsonRpcResponse) -> i32 {
    response.error.expect("error").code
}

#[cfg(feature = "oauth")]
fn test_jwks(kid: &str) -> Value {
    json!({
        "keys": [{
            "kty": "RSA",
            "kid": kid,
            "alg": "RS256",
            "use": "sig",
            "n": TEST_RSA_JWK_N,
            "e": TEST_RSA_JWK_E
        }]
    })
}

#[cfg(feature = "oauth")]
fn test_jwt(
    kid: &str,
    issuer: &str,
    audience: &str,
    tenant_id: String,
    expiry_offset_seconds: i64,
) -> String {
    #[derive(serde::Serialize)]
    struct Claims {
        iss: String,
        aud: String,
        sub: String,
        exp: i64,
        tenant_id: String,
    }

    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(kid.to_owned());
    let claims = Claims {
        iss: issuer.to_owned(),
        aud: audience.to_owned(),
        sub: "subject-1".to_owned(),
        exp: chrono::Utc::now().timestamp() + expiry_offset_seconds,
        tenant_id,
    };
    jsonwebtoken::encode(
        &header,
        &claims,
        &jsonwebtoken::EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_KEY.as_bytes())
            .expect("test key"),
    )
    .expect("test token")
}

fn tool_context() -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::path::PathBuf::from("."),
        project_workspace_root: None,
        sandbox: None,
        cap_registry: std::sync::Arc::new(CapabilityRegistry::default()),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::new(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

struct HeaderAuth;

#[async_trait]
impl McpServerAuthValidator for HeaderAuth {
    async fn validate(&self, context: &mut McpServerRequestContext) -> Result<(), McpServerError> {
        if context.header("x-test-auth") == Some("yes") {
            Ok(())
        } else {
            Err(McpServerError::Unauthorized(
                "missing x-test-auth".to_owned(),
            ))
        }
    }
}

struct HeaderTenant;

#[async_trait]
impl TenantResolver for HeaderTenant {
    async fn resolve_tenant(
        &self,
        context: &McpServerRequestContext,
    ) -> Result<TenantId, McpServerError> {
        if context.header("x-test-tenant") == Some("single") {
            Ok(TenantId::SINGLE)
        } else {
            Err(McpServerError::TenantMapping(
                "missing x-test-tenant".to_owned(),
            ))
        }
    }
}

#[derive(Clone)]
struct TestHarness;

#[async_trait]
impl HarnessMcpBackend for TestHarness {
    async fn call_harness_tool(
        &self,
        _context: &McpServerRequestContext,
        _capability: ExposedCapability,
        _arguments: Value,
    ) -> Result<Value, McpServerError> {
        Ok(json!({}))
    }
}

#[derive(Clone)]
struct FailingHarness;

#[async_trait]
impl HarnessMcpBackend for FailingHarness {
    async fn call_harness_tool(
        &self,
        _context: &McpServerRequestContext,
        _capability: ExposedCapability,
        _arguments: Value,
    ) -> Result<Value, McpServerError> {
        Err(McpServerError::Internal("postgres://secret".to_owned()))
    }
}

#[derive(Clone)]
struct StaticResources;

#[async_trait]
impl ResourceProvider for StaticResources {
    async fn list_resources(&self) -> Result<Vec<McpResource>, McpServerError> {
        Ok(vec![McpResource {
            uri: "jyowo://sessions/active".into(),
            name: "active session".into(),
            title: None,
            description: Some("Current session metadata".into()),
            mime_type: Some("application/json".into()),
            icons: None,
            annotations: None,
            size: None,
            meta: Default::default(),
        }])
    }

    async fn read_resource(&self, uri: &str) -> Result<McpReadResourceResult, McpServerError> {
        if uri != "jyowo://sessions/active" {
            return Err(McpServerError::InvalidParams(format!(
                "unknown resource: {uri}"
            )));
        }
        Ok(McpReadResourceResult {
            contents: vec![McpResourceContents {
                uri: uri.into(),
                mime_type: Some("application/json".into()),
                text: Some("{\"session\":\"active\"}".into()),
                blob: None,
                meta: Default::default(),
            }],
            meta: Default::default(),
        })
    }
}

#[async_trait]
impl PromptProvider for StaticResources {
    async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpServerError> {
        Ok(vec![McpPrompt {
            name: "triage".into(),
            title: None,
            description: Some("Triage a session".into()),
            icons: None,
            arguments: None,
            meta: Default::default(),
        }])
    }

    async fn get_prompt(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<McpPromptMessages, McpServerError> {
        if name != "triage" {
            return Err(McpServerError::InvalidParams(format!(
                "unknown prompt: {name}"
            )));
        }
        let focus = arguments
            .get("focus")
            .and_then(Value::as_str)
            .unwrap_or("session");
        Ok(McpPromptMessages {
            description: None,
            messages: vec![json!({
                "role": "user",
                "content": {
                    "type": "text",
                    "text": format!("triage {focus}")
                }
            })],
            meta: Default::default(),
        })
    }
}
