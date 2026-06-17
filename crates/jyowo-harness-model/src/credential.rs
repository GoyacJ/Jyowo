use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_contracts::{ModelError, TenantId};
use secrecy::SecretString;
use serde_json::Value;

#[async_trait]
pub trait CredentialSource: Send + Sync + 'static {
    async fn fetch(&self, key: CredentialKey) -> Result<CredentialValue, CredentialError>;
    async fn rotate(&self, key: CredentialKey) -> Result<(), CredentialError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CredentialKey {
    pub tenant_id: TenantId,
    pub provider_id: String,
    pub key_label: String,
}

#[derive(Debug, Clone)]
pub struct CredentialValue {
    pub secret: SecretString,
    pub metadata: CredentialMetadata,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CredentialMetadata {
    pub fields: BTreeMap<String, Value>,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    #[error("credential missing: {0}")]
    Missing(String),
    #[error("credential rotation failed: {0}")]
    RotationFailed(String),
    #[error("model: {0}")]
    Model(ModelError),
}

impl From<ModelError> for CredentialError {
    fn from(value: ModelError) -> Self {
        Self::Model(value)
    }
}

impl CredentialError {
    #[must_use]
    pub fn into_model_error(self) -> ModelError {
        match self {
            Self::Missing(message) => ModelError::AuthExpired(message),
            Self::RotationFailed(message) => ModelError::AuthExpired(message),
            Self::Model(error) => error,
        }
    }
}
