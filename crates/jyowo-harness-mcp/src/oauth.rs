use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::McpError;

const DEVICE_CODE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthTokenSet {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkceAuthRequest {
    pub authorize_url: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub state: String,
    pub code_verifier: String,
    pub code_challenge: String,
}

impl PkceAuthRequest {
    pub fn new(
        authorize_url: impl Into<String>,
        client_id: impl Into<String>,
        redirect_uri: impl Into<String>,
        scopes: impl IntoIterator<Item = String>,
        state: impl Into<String>,
        code_verifier: impl Into<String>,
    ) -> Result<Self, McpError> {
        let code_verifier = code_verifier.into();
        let digest = ring::digest::digest(&ring::digest::SHA256, code_verifier.as_bytes());
        Ok(Self {
            authorize_url: authorize_url.into(),
            client_id: client_id.into(),
            redirect_uri: redirect_uri.into(),
            scopes: scopes.into_iter().collect(),
            state: state.into(),
            code_challenge: base64_url_no_pad(digest.as_ref()),
            code_verifier,
        })
    }

    pub fn authorization_url(&self) -> String {
        let mut url = Url::parse(&self.authorize_url).expect("valid authorize_url");
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("response_type", "code");
            query.append_pair("client_id", &self.client_id);
            query.append_pair("redirect_uri", &self.redirect_uri);
            query.append_pair("scope", &self.scopes.join(" "));
            query.append_pair("state", &self.state);
            query.append_pair("code_challenge", &self.code_challenge);
            query.append_pair("code_challenge_method", "S256");
        }
        url.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAuthorization {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: Option<u64>,
}

#[derive(Clone)]
pub struct OAuthClient {
    http: reqwest::Client,
    token_url: String,
}

impl OAuthClient {
    pub fn new(token_url: impl Into<String>) -> Self {
        Self {
            http: oauth_http_client(),
            token_url: token_url.into(),
        }
    }

    pub async fn exchange_code(
        &self,
        client_id: &str,
        client_secret: Option<&str>,
        code: &str,
        redirect_uri: &str,
        code_verifier: &str,
    ) -> Result<OAuthTokenSet, McpError> {
        let mut body = json!({
            "grant_type": "authorization_code",
            "client_id": client_id,
            "code": code,
            "redirect_uri": redirect_uri,
            "code_verifier": code_verifier,
        });
        insert_secret(&mut body, client_secret);
        self.post_token(body).await
    }

    pub async fn refresh_token(
        &self,
        client_id: &str,
        client_secret: Option<&str>,
        refresh_token: &str,
    ) -> Result<OAuthTokenSet, McpError> {
        let mut body = json!({
            "grant_type": "refresh_token",
            "client_id": client_id,
            "refresh_token": refresh_token,
        });
        insert_secret(&mut body, client_secret);
        self.post_token(body).await
    }

    async fn post_token(&self, body: Value) -> Result<OAuthTokenSet, McpError> {
        let response = self
            .http
            .post(&self.token_url)
            .json(&body)
            .send()
            .await
            .map_err(|error| McpError::OAuth(error.to_string()))?;
        parse_token_response(response).await
    }
}

#[derive(Clone)]
pub struct DeviceTokenPoller {
    http: reqwest::Client,
    token_url: String,
    client_id: String,
    device_code: String,
    interval: Duration,
    max_attempts: u32,
}

impl DeviceTokenPoller {
    pub fn new(
        token_url: impl Into<String>,
        client_id: impl Into<String>,
        device_code: impl Into<String>,
    ) -> Self {
        Self {
            http: oauth_http_client(),
            token_url: token_url.into(),
            client_id: client_id.into(),
            device_code: device_code.into(),
            interval: Duration::from_secs(5),
            max_attempts: 120,
        }
    }

    #[must_use]
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    #[must_use]
    pub fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    pub async fn poll(&self) -> Result<OAuthTokenSet, McpError> {
        for attempt in 0..self.max_attempts {
            if attempt > 0 && !self.interval.is_zero() {
                tokio::time::sleep(self.interval).await;
            }
            let body = json!({
                "grant_type": DEVICE_CODE_GRANT,
                "client_id": self.client_id,
                "device_code": self.device_code,
            });
            let response = self
                .http
                .post(&self.token_url)
                .json(&body)
                .send()
                .await
                .map_err(|error| McpError::OAuth(error.to_string()))?;
            if response.status().is_success() {
                return response
                    .json::<OAuthTokenSet>()
                    .await
                    .map_err(|error| McpError::OAuth(error.to_string()));
            }

            let error = oauth_error(response).await?;
            if error == "authorization_pending" {
                continue;
            }
            return Err(McpError::OAuth(error));
        }
        Err(McpError::OAuth(
            "device authorization polling attempts exhausted".to_owned(),
        ))
    }
}

async fn parse_token_response(response: reqwest::Response) -> Result<OAuthTokenSet, McpError> {
    if response.status().is_success() {
        return response
            .json::<OAuthTokenSet>()
            .await
            .map_err(|error| McpError::OAuth(error.to_string()));
    }
    Err(McpError::OAuth(oauth_error(response).await?))
}

async fn oauth_error(response: reqwest::Response) -> Result<String, McpError> {
    let status = response.status();
    let value = response
        .json::<Value>()
        .await
        .map_err(|error| McpError::OAuth(error.to_string()))?;
    Ok(value
        .get("error")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("oauth endpoint returned {status}")))
}

fn insert_secret(body: &mut Value, client_secret: Option<&str>) {
    if let (Some(secret), Some(object)) = (client_secret, body.as_object_mut()) {
        object.insert("client_secret".to_owned(), Value::String(secret.to_owned()));
    }
}

fn oauth_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()
        .expect("default OAuth HTTP client should build")
}

fn base64_url_no_pad(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        }
    }
    out
}
