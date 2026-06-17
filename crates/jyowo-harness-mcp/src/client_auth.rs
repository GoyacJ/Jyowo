#![allow(dead_code)]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

use harness_contracts::{
    now, Event, McpOAuthRefreshEvent, McpOAuthRefreshOutcome, McpOAuthRefreshPhase, McpServerId,
};

use crate::{
    McpClientAuth, McpError, McpEventSink, McpMetric, McpMetricOutcome, McpMetricsSink,
    NoopMcpEventSink, NoopMcpMetricsSink,
};

const OAUTH_REFRESH_SKEW: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct McpClientAuthProvider {
    auth: McpClientAuth,
    oauth_state: Option<Arc<Mutex<OAuthState>>>,
    metrics_sink: Arc<dyn McpMetricsSink>,
    event_sink: Arc<dyn McpEventSink>,
    server_id: McpServerId,
    transport: String,
}

#[derive(Debug)]
struct OAuthState {
    access_token: Option<String>,
    expires_at: Option<Instant>,
    refresh_token: String,
}

#[derive(Debug)]
struct RefreshedOAuthToken {
    access_token: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
}

impl McpClientAuthProvider {
    pub fn new(auth: &McpClientAuth) -> Self {
        let oauth_state = match auth {
            McpClientAuth::OAuth { refresh_token, .. } => {
                refresh_token.clone().map(|refresh_token| {
                    Arc::new(Mutex::new(OAuthState {
                        access_token: None,
                        expires_at: None,
                        refresh_token,
                    }))
                })
            }
            _ => None,
        };
        Self {
            auth: auth.clone(),
            oauth_state,
            metrics_sink: Arc::new(NoopMcpMetricsSink),
            event_sink: Arc::new(NoopMcpEventSink),
            server_id: McpServerId("unknown".to_owned()),
            transport: "unknown".to_owned(),
        }
    }

    pub fn with_metrics_sink(mut self, metrics_sink: Arc<dyn McpMetricsSink>) -> Self {
        self.metrics_sink = metrics_sink;
        self
    }

    pub fn with_lifecycle_events(
        mut self,
        server_id: McpServerId,
        transport: impl Into<String>,
        event_sink: Arc<dyn McpEventSink>,
    ) -> Self {
        self.server_id = server_id;
        self.transport = transport.into();
        self.event_sink = event_sink;
        self
    }

    pub async fn authorization_header(&self) -> Result<Option<String>, McpError> {
        match &self.auth {
            McpClientAuth::None => Ok(None),
            McpClientAuth::Bearer(token) => Ok(Some(format!("Bearer {token}"))),
            McpClientAuth::OAuth { .. } => self.oauth_authorization_header(false).await,
            McpClientAuth::Xaa { .. } => Err(McpError::Unsupported(
                "XAA client auth requires an external request signer boundary".into(),
            )),
        }
    }

    pub async fn force_refresh_authorization_header(&self) -> Result<Option<String>, McpError> {
        match &self.auth {
            McpClientAuth::OAuth { .. } => self.oauth_authorization_header(true).await,
            _ => self.authorization_header().await,
        }
    }

    pub fn can_refresh(&self) -> bool {
        matches!(self.auth, McpClientAuth::OAuth { .. }) && self.oauth_state.is_some()
    }

    async fn oauth_authorization_header(
        &self,
        force_refresh: bool,
    ) -> Result<Option<String>, McpError> {
        let McpClientAuth::OAuth {
            token_url,
            client_id,
            client_secret,
            ..
        } = &self.auth
        else {
            return Ok(None);
        };
        let Some(state) = &self.oauth_state else {
            return Err(McpError::Unsupported(
                "OAuth client auth requires refresh_token for transport lifecycle".into(),
            ));
        };

        let mut state = state.lock().await;
        if !force_refresh && !state.needs_refresh(Instant::now()) {
            if let Some(access_token) = &state.access_token {
                return Ok(Some(format!("Bearer {access_token}")));
            }
        }

        self.emit_refresh_event(
            McpOAuthRefreshPhase::Started,
            McpOAuthRefreshOutcome::Started,
            None,
        );
        let token = match refresh_oauth_access_token(
            token_url,
            client_id,
            client_secret,
            &state.refresh_token,
        )
        .await
        {
            Ok(token) => {
                self.record_refresh(McpMetricOutcome::Success);
                self.emit_refresh_event(
                    McpOAuthRefreshPhase::Completed,
                    McpOAuthRefreshOutcome::Success,
                    None,
                );
                token
            }
            Err(error) => {
                self.record_refresh(McpMetricOutcome::Error);
                self.emit_refresh_event(
                    McpOAuthRefreshPhase::Completed,
                    McpOAuthRefreshOutcome::Error,
                    Some(oauth_error_reason(&error).to_owned()),
                );
                return Err(error);
            }
        };
        if let Some(refresh_token) = token.refresh_token {
            state.refresh_token = refresh_token;
        }
        state.expires_at = token
            .expires_in
            .map(|expires_in| Instant::now() + Duration::from_secs(expires_in));
        state.access_token = Some(token.access_token.clone());
        Ok(Some(format!("Bearer {}", token.access_token)))
    }

    fn record_refresh(&self, outcome: McpMetricOutcome) {
        self.metrics_sink
            .record(McpMetric::OAuthRefresh { outcome });
    }

    fn emit_refresh_event(
        &self,
        phase: McpOAuthRefreshPhase,
        outcome: McpOAuthRefreshOutcome,
        reason: Option<String>,
    ) {
        self.event_sink
            .emit(Event::McpOAuthRefresh(McpOAuthRefreshEvent {
                server_id: self.server_id.clone(),
                transport: self.transport.clone(),
                phase,
                outcome,
                reason,
                at: now(),
            }));
    }
}

impl OAuthState {
    fn needs_refresh(&self, now: Instant) -> bool {
        match (self.access_token.as_ref(), self.expires_at) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(_), Some(expires_at)) => now + OAUTH_REFRESH_SKEW >= expires_at,
        }
    }
}

fn oauth_error_reason(error: &McpError) -> &'static str {
    match error {
        McpError::OAuth(_) => "oauth_error",
        McpError::Transport(_) => "transport_error",
        McpError::InvalidResponse(_) => "invalid_response",
        McpError::Unsupported(_) => "unsupported",
        _ => "error",
    }
}

#[cfg(feature = "oauth")]
async fn refresh_oauth_access_token(
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<RefreshedOAuthToken, McpError> {
    let token = crate::OAuthClient::new(token_url.to_owned())
        .refresh_token(client_id, Some(client_secret), refresh_token)
        .await?;
    if !token.token_type.eq_ignore_ascii_case("bearer") {
        return Err(McpError::OAuth(format!(
            "unsupported oauth token_type: {}",
            token.token_type
        )));
    }
    Ok(RefreshedOAuthToken {
        access_token: token.access_token,
        expires_in: token.expires_in,
        refresh_token: token.refresh_token,
    })
}

#[cfg(not(feature = "oauth"))]
async fn refresh_oauth_access_token(
    _token_url: &str,
    _client_id: &str,
    _client_secret: &str,
    _refresh_token: &str,
) -> Result<RefreshedOAuthToken, McpError> {
    Err(McpError::Unsupported(
        "OAuth client auth requires the oauth feature".into(),
    ))
}
