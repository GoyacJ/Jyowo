use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use harness_contracts::PermissionError;
use parking_lot::RwLock;
use ring::hmac;
use serde_json::{Map, Value};
use zeroize::Zeroizing;

#[async_trait]
pub trait IntegritySigner: Send + Sync + 'static {
    fn algorithm(&self) -> IntegrityAlgorithm;

    fn key_id(&self) -> &str;

    async fn sign(&self, payload: &[u8]) -> Result<IntegritySignature, PermissionError>;

    async fn verify(
        &self,
        payload: &[u8],
        signature: &IntegritySignature,
    ) -> Result<(), IntegrityError>;
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum IntegrityAlgorithm {
    HmacSha256,
    HmacSha512,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntegritySignature {
    pub algorithm: IntegrityAlgorithm,
    pub key_id: String,
    pub mac: Bytes,
    pub signed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum IntegrityError {
    #[error("signature mismatch")]
    Mismatch,
    #[error("unknown key id: {0}")]
    UnknownKeyId(String),
    #[error("algorithm downgrade not allowed: stored={stored:?}, expected={expected:?}")]
    AlgorithmDowngrade {
        stored: IntegrityAlgorithm,
        expected: IntegrityAlgorithm,
    },
    #[error("missing signature")]
    Missing,
}

#[derive(Debug, Default, Clone)]
pub struct StaticSignerStore {
    keys: Arc<RwLock<BTreeMap<String, Zeroizing<Vec<u8>>>>>,
}

impl StaticSignerStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_key(&self, key_id: impl Into<String>, key: Vec<u8>) {
        self.keys.write().insert(key_id.into(), Zeroizing::new(key));
    }

    pub fn signer(
        &self,
        key_id: &str,
        algorithm: IntegrityAlgorithm,
    ) -> Result<Arc<dyn IntegritySigner>, PermissionError> {
        let keys = self.keys.read();
        let Some(key) = keys.get(key_id) else {
            return Err(PermissionError::Message(format!(
                "integrity key `{key_id}` is not registered"
            )));
        };

        Ok(Arc::new(DefaultHmacSigner {
            key_id: key_id.to_owned(),
            algorithm,
            key: Zeroizing::new(key.to_vec()),
        }))
    }

    pub fn from_key(
        key_id: impl Into<String>,
        key: Vec<u8>,
        algorithm: IntegrityAlgorithm,
    ) -> Result<Arc<dyn IntegritySigner>, PermissionError> {
        let key_id = key_id.into();
        let store = Self::new();
        store.insert_key(key_id.clone(), key);
        store.signer(&key_id, algorithm)
    }
}

#[derive(Debug)]
pub struct DefaultHmacSigner {
    key_id: String,
    algorithm: IntegrityAlgorithm,
    key: Zeroizing<Vec<u8>>,
}

#[async_trait]
impl IntegritySigner for DefaultHmacSigner {
    fn algorithm(&self) -> IntegrityAlgorithm {
        self.algorithm
    }

    fn key_id(&self) -> &str {
        &self.key_id
    }

    async fn sign(&self, payload: &[u8]) -> Result<IntegritySignature, PermissionError> {
        let key = hmac::Key::new(self.algorithm.ring_algorithm(), &self.key);
        let tag = hmac::sign(&key, payload);

        Ok(IntegritySignature {
            algorithm: self.algorithm,
            key_id: self.key_id.clone(),
            mac: Bytes::copy_from_slice(tag.as_ref()),
            signed_at: Utc::now(),
        })
    }

    async fn verify(
        &self,
        payload: &[u8],
        signature: &IntegritySignature,
    ) -> Result<(), IntegrityError> {
        if signature.key_id != self.key_id {
            return Err(IntegrityError::UnknownKeyId(signature.key_id.clone()));
        }

        if signature.algorithm != self.algorithm {
            if signature.algorithm.rank() < self.algorithm.rank() {
                return Err(IntegrityError::AlgorithmDowngrade {
                    stored: signature.algorithm,
                    expected: self.algorithm,
                });
            }

            return Err(IntegrityError::Mismatch);
        }

        let key = hmac::Key::new(self.algorithm.ring_algorithm(), &self.key);
        hmac::verify(&key, payload, &signature.mac).map_err(|_| IntegrityError::Mismatch)
    }
}

impl IntegrityAlgorithm {
    fn ring_algorithm(self) -> hmac::Algorithm {
        match self {
            Self::HmacSha256 => hmac::HMAC_SHA256,
            Self::HmacSha512 => hmac::HMAC_SHA512,
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::HmacSha256 => 1,
            Self::HmacSha512 => 2,
        }
    }
}

pub fn canonical_bytes(record: &Value) -> Result<Vec<u8>, IntegrityError> {
    let normalized = normalize_value(record, true);
    serde_json::to_vec(&normalized).map_err(|_| IntegrityError::Mismatch)
}

fn normalize_value(value: &Value, top_level: bool) -> Value {
    match value {
        Value::Object(object) => {
            let mut normalized = Map::new();
            for (key, value) in object {
                if top_level && key == "signature" {
                    continue;
                }
                normalized.insert(key.clone(), normalize_value(value, false));
            }
            Value::Object(normalized)
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| normalize_value(value, false))
                .collect(),
        ),
        _ => value.clone(),
    }
}
