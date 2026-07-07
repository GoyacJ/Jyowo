#![cfg(feature = "doubao")]

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_contracts::ModelError;
use harness_model::{SeedanceApiClient, SeedanceHttpTransport};
use serde_json::{json, Value};
use wiremock::{
    matchers::{body_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

struct ReqwestSeedanceTransport {
    client: reqwest::Client,
}

impl ReqwestSeedanceTransport {
    fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .no_proxy()
                .pool_max_idle_per_host(0)
                .build()
                .expect("test reqwest client should build"),
        }
    }
}

#[async_trait::async_trait]
impl SeedanceHttpTransport for ReqwestSeedanceTransport {
    async fn post_json(
        &self,
        url: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> Result<(u16, Vec<u8>), ModelError> {
        let mut request = self.client.post(url).body(body);
        for (key, value) in headers {
            request = request.header(key.as_str(), value.as_str());
        }
        response_parts(request.send().await).await
    }

    async fn get_json(
        &self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> Result<(u16, Vec<u8>), ModelError> {
        let mut request = self.client.get(url);
        for (key, value) in headers {
            request = request.header(key.as_str(), value.as_str());
        }
        response_parts(request.send().await).await
    }
}

async fn response_parts(
    response: Result<reqwest::Response, reqwest::Error>,
) -> Result<(u16, Vec<u8>), ModelError> {
    let response = response.map_err(|error| ModelError::ProviderUnavailable(error.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .bytes()
        .await
        .map_err(|error| ModelError::ProviderUnavailable(error.to_string()))?
        .to_vec();
    Ok((status, body))
}

fn seedance_client(api_key: &str, server: &MockServer) -> SeedanceApiClient {
    SeedanceApiClient::from_transport(Arc::new(ReqwestSeedanceTransport::new()), api_key)
        .with_base_url(server.uri())
}

#[tokio::test]
async fn seedance_create_video_task_uses_official_endpoint_and_auth() {
    let server = MockServer::start().await;
    let client = seedance_client("provider-key", &server);

    let request = json!({
        "model": "doubao-seedance-2-0-260128",
        "content": [{
            "type": "text",
            "text": "A golden retriever running through a sunlit wheat field"
        }],
        "resolution": "1080p",
        "ratio": "16:9",
        "duration": 5,
        "watermark": false
    });

    Mock::given(method("POST"))
        .and(path("/contents/generations/tasks"))
        .and(header("authorization", "Bearer provider-key"))
        .and(body_json(request.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "cgt-task-1"})))
        .mount(&server)
        .await;

    let response = client
        .create_video_generation_task(request)
        .await
        .expect("create task should succeed");

    assert_eq!(response["id"], "cgt-task-1");
}

#[tokio::test]
async fn seedance_query_running_task_returns_status() {
    let server = MockServer::start().await;
    let client = seedance_client("provider-key", &server);

    Mock::given(method("GET"))
        .and(path("/contents/generations/tasks/cgt-task-1"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "running"})))
        .mount(&server)
        .await;

    let response = client
        .query_video_generation_task("cgt-task-1")
        .await
        .expect("query task should succeed");

    assert_eq!(response["status"], "running");
}

#[tokio::test]
async fn seedance_query_task_percent_encodes_task_id_path_segment() {
    let server = MockServer::start().await;
    let client = seedance_client("provider-key", &server);

    Mock::given(method("GET"))
        .and(path(
            "/contents/generations/tasks/cgt%2Ftask%3Fwith%23reserved%25chars",
        ))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "running"})))
        .mount(&server)
        .await;

    let response = client
        .query_video_generation_task("cgt/task?with#reserved%chars")
        .await
        .expect("query task should percent-encode path segment");

    assert_eq!(response["status"], "running");
}

#[tokio::test]
async fn seedance_query_completed_task_returns_video_url() {
    let server = MockServer::start().await;
    let client = seedance_client("provider-key", &server);

    Mock::given(method("GET"))
        .and(path("/contents/generations/tasks/cgt-task-2"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "succeeded",
            "content": {
                "video_url": "https://ark.cn-beijing.volces.com/generated/video.mp4"
            }
        })))
        .mount(&server)
        .await;

    let response = client
        .query_video_generation_task("cgt-task-2")
        .await
        .expect("query task should succeed");

    assert_eq!(response["status"], "succeeded");
    assert_eq!(
        response["content"]["video_url"],
        "https://ark.cn-beijing.volces.com/generated/video.mp4"
    );
}

#[tokio::test]
async fn seedance_provider_error_does_not_leak_api_key() {
    let server = MockServer::start().await;
    let client = seedance_client("super-secret-key", &server);

    Mock::given(method("POST"))
        .and(path("/contents/generations/tasks"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"error": {"message": "invalid auth super-secret-key"}})),
        )
        .mount(&server)
        .await;

    let error = client
        .create_video_generation_task(json!({"model": "doubao-seedance-2-0-260128"}))
        .await
        .expect_err("invalid auth should fail");

    let message = error.to_string();
    assert!(message.contains("401"));
    assert!(!message.contains("super-secret-key"));
}

#[tokio::test]
async fn seedance_auth_header_uses_bearer_shape() {
    let server = MockServer::start().await;
    let client = seedance_client("provider-key", &server);

    Mock::given(method("GET"))
        .and(path("/contents/generations/tasks/cgt-task-3"))
        .and(header("authorization", "Bearer provider-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "queued"})))
        .mount(&server)
        .await;

    client
        .query_video_generation_task("cgt-task-3")
        .await
        .expect("query should succeed with bearer auth");
}

#[allow(dead_code)]
async fn assert_post(
    server: &MockServer,
    endpoint: &str,
    expected_body: Value,
    response_body: Value,
) {
    Mock::given(method("POST"))
        .and(path(endpoint))
        .and(header("authorization", "Bearer provider-key"))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .mount(server)
        .await;
}
