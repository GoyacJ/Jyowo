#![cfg(feature = "oauth")]

use harness_mcp::{DeviceTokenPoller, OAuthClient, PkceAuthRequest};
use serde_json::json;
use wiremock::{
    matchers::{body_partial_json, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[test]
fn pkce_authorization_url_contains_required_parameters() {
    let request = PkceAuthRequest::new(
        "https://auth.example/authorize",
        "client-1",
        "http://localhost/callback",
        ["mcp:tools".to_owned(), "offline_access".to_owned()],
        "state-1",
        "verifier-1",
    )
    .expect("pkce request");

    let url = request.authorization_url();
    assert!(url.contains("client_id=client-1"));
    assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%2Fcallback"));
    assert!(url.contains("scope=mcp%3Atools+offline_access"));
    assert!(url.contains("state=state-1"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("code_challenge="));
}

#[tokio::test]
async fn exchanges_authorization_code_and_refreshes_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_partial_json(json!({
            "grant_type": "authorization_code",
            "code": "code-1",
            "client_id": "client-1",
            "code_verifier": "verifier-1"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-1",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "refresh-1",
            "scope": "mcp:tools"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_partial_json(json!({
            "grant_type": "refresh_token",
            "refresh_token": "refresh-1",
            "client_id": "client-1"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-2",
            "token_type": "Bearer",
            "expires_in": 7200,
            "refresh_token": "refresh-2"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = OAuthClient::new(format!("{}/token", server.uri()));
    let token = client
        .exchange_code(
            "client-1",
            None,
            "code-1",
            "http://localhost/callback",
            "verifier-1",
        )
        .await
        .expect("code exchange");
    assert_eq!(token.access_token, "access-1");
    assert_eq!(token.refresh_token.as_deref(), Some("refresh-1"));

    let refreshed = client
        .refresh_token("client-1", None, "refresh-1")
        .await
        .expect("refresh");
    assert_eq!(refreshed.access_token, "access-2");
    assert_eq!(refreshed.refresh_token.as_deref(), Some("refresh-2"));
}

#[tokio::test]
async fn device_flow_polling_waits_through_pending_and_stops_on_denied() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_partial_json(json!({
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            "device_code": "device-1"
        })))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "authorization_pending"
        })))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_partial_json(json!({
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            "device_code": "device-1"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-device",
            "token_type": "Bearer"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let poller = DeviceTokenPoller::new(format!("{}/token", server.uri()), "client-1", "device-1")
        .with_max_attempts(2)
        .with_interval(std::time::Duration::ZERO);
    let token = poller.poll().await.expect("device token");
    assert_eq!(token.access_token, "access-device");

    let denied_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "access_denied"
        })))
        .expect(1)
        .mount(&denied_server)
        .await;
    let error = DeviceTokenPoller::new(
        format!("{}/token", denied_server.uri()),
        "client-1",
        "device-2",
    )
    .with_interval(std::time::Duration::ZERO)
    .poll()
    .await
    .expect_err("access denied");
    assert!(error.to_string().contains("access_denied"));
}
