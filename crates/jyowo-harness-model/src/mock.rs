use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::stream;
use harness_contracts::ModelError;
use secrecy::SecretString;
use tokio::sync::Mutex;

use crate::{
    CredentialError, CredentialKey, CredentialMetadata, CredentialSource, CredentialValue,
    HealthStatus, InferContext, ModelCapabilities, ModelDescriptor, ModelProvider, ModelRequest,
    ModelStream, ModelStreamEvent,
};

pub struct MockProvider {
    provider_id: String,
    descriptors: Vec<ModelDescriptor>,
    events: Vec<ModelStreamEvent>,
    requests: Mutex<Vec<ModelRequest>>,
    health: HealthStatus,
}

impl Default for MockProvider {
    fn default() -> Self {
        Self {
            provider_id: "mock".to_owned(),
            descriptors: vec![mock_descriptor()],
            events: vec![ModelStreamEvent::MessageStop],
            requests: Mutex::new(Vec::new()),
            health: HealthStatus::Healthy,
        }
    }
}

impl MockProvider {
    #[must_use]
    pub fn with_events(mut self, events: Vec<ModelStreamEvent>) -> Self {
        self.events = events;
        self
    }

    #[must_use]
    pub fn with_health(mut self, health: HealthStatus) -> Self {
        self.health = health;
        self
    }

    pub async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for MockProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        self.descriptors.clone()
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        if ctx.cancel.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        if let Some(deadline) = ctx.deadline {
            if Instant::now() >= deadline {
                return Err(ModelError::DeadlineExceeded(Duration::ZERO));
            }
        }
        self.requests.lock().await.push(req);
        Ok(Box::pin(stream::iter(self.events.clone())))
    }

    async fn health(&self) -> HealthStatus {
        self.health.clone()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScriptedResponse {
    Stream(Vec<ModelStreamEvent>),
    Error(ModelError),
    WaitForCancel,
}

pub struct ScriptedProvider {
    provider_id: String,
    descriptors: Vec<ModelDescriptor>,
    responses: Mutex<VecDeque<ScriptedResponse>>,
    requests: Mutex<Vec<ModelRequest>>,
    health: HealthStatus,
}

impl ScriptedProvider {
    pub fn new(responses: Vec<ScriptedResponse>) -> Self {
        Self {
            provider_id: "mock".to_owned(),
            descriptors: vec![mock_descriptor()],
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
            health: HealthStatus::Healthy,
        }
    }

    pub async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for ScriptedProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        self.descriptors.clone()
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);

        let response =
            self.responses.lock().await.pop_front().ok_or_else(|| {
                ModelError::InvalidRequest("scripted provider exhausted".to_owned())
            })?;

        match response {
            ScriptedResponse::Stream(events) => Ok(Box::pin(stream::iter(events))),
            ScriptedResponse::Error(error) => Err(error),
            ScriptedResponse::WaitForCancel => {
                ctx.cancel.cancelled().await;
                Err(ModelError::Cancelled)
            }
        }
    }

    async fn health(&self) -> HealthStatus {
        self.health.clone()
    }
}

#[derive(Default)]
pub struct MockCredentialSource {
    values: Mutex<HashMap<CredentialKey, CredentialValue>>,
    rotated: Mutex<Vec<CredentialKey>>,
}

impl MockCredentialSource {
    pub async fn insert(&self, key: CredentialKey, value: CredentialValue) {
        self.values.lock().await.insert(key, value);
    }

    pub async fn insert_secret(&self, key: CredentialKey, secret: impl Into<String>) {
        self.insert(
            key,
            CredentialValue {
                secret: SecretString::new(secret.into().into()),
                metadata: CredentialMetadata::default(),
            },
        )
        .await;
    }

    pub async fn rotated_keys(&self) -> Vec<CredentialKey> {
        self.rotated.lock().await.clone()
    }
}

#[async_trait]
impl CredentialSource for MockCredentialSource {
    async fn fetch(&self, key: CredentialKey) -> Result<CredentialValue, CredentialError> {
        self.values
            .lock()
            .await
            .get(&key)
            .cloned()
            .ok_or(CredentialError::Missing(key.key_label))
    }

    async fn rotate(&self, key: CredentialKey) -> Result<(), CredentialError> {
        if !self.values.lock().await.contains_key(&key) {
            return Err(CredentialError::Missing(key.key_label));
        }

        self.rotated.lock().await.push(key);
        Ok(())
    }
}

fn mock_descriptor() -> ModelDescriptor {
    ModelDescriptor {
        provider_id: "mock".to_owned(),
        model_id: "mock-model".to_owned(),
        display_name: "Mock model".to_owned(),
        context_window: 128_000,
        max_output_tokens: 8192,
        capabilities: ModelCapabilities::default(),
        pricing: None,
    }
}
