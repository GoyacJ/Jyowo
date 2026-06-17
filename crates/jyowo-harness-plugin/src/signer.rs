use std::collections::BTreeMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::{stream, Stream};
use harness_contracts::TrustLevel;
use parking_lot::RwLock;
use ring::signature;
use serde_json::Value;

use crate::{ManifestSignature, PluginError, PluginManifest, SignatureAlgorithm, SignerStoreError};

pub type SignerStoreEventStream = Pin<Box<dyn Stream<Item = SignerStoreEvent> + Send>>;

#[async_trait]
pub trait TrustedSignerStore: Send + Sync + 'static {
    async fn list_active(&self) -> Result<Vec<TrustedSigner>, SignerStoreError>;

    async fn list_revoked(&self) -> Result<Vec<TrustedSigner>, SignerStoreError> {
        Ok(Vec::new())
    }

    async fn get(&self, id: &SignerId) -> Result<Option<TrustedSigner>, SignerStoreError>;

    async fn is_revoked(&self, id: &SignerId, at: DateTime<Utc>) -> Result<bool, SignerStoreError>;

    fn watch(&self) -> SignerStoreEventStream;
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SignerId(String);

impl SignerId {
    pub fn new(value: impl Into<String>) -> Result<Self, SignerStoreError> {
        let value = value.into();
        validate_signer_id(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SignerId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<SignerId> for String {
    fn from(value: SignerId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrustedSigner {
    pub id: SignerId,
    pub algorithm: SignatureAlgorithm,
    pub public_key: Vec<u8>,
    pub activated_at: DateTime<Utc>,
    pub retired_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub provenance: SignerProvenance,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SignerProvenance {
    BuiltinOfficial,
    BuilderInjected,
    PkiEndpoint { endpoint: String },
    PolicyFile { path: PathBuf },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SignerStoreEvent {
    Added(SignerId),
    Updated(SignerId),
    Retired(SignerId),
    Revoked(SignerId),
}

#[derive(Debug, Clone, Default)]
pub struct StaticTrustedSignerStore {
    signers: Arc<RwLock<BTreeMap<SignerId, TrustedSigner>>>,
}

impl StaticTrustedSignerStore {
    pub fn new(signers: Vec<TrustedSigner>) -> Result<Self, SignerStoreError> {
        let store = Self::default();
        for signer in signers {
            validate_signer(&signer)?;
            store.signers.write().insert(signer.id.clone(), signer);
        }
        Ok(store)
    }

    pub async fn list_active(&self) -> Result<Vec<TrustedSigner>, SignerStoreError> {
        <Self as TrustedSignerStore>::list_active(self).await
    }

    pub async fn list_revoked(&self) -> Result<Vec<TrustedSigner>, SignerStoreError> {
        <Self as TrustedSignerStore>::list_revoked(self).await
    }

    pub async fn get(&self, id: &SignerId) -> Result<Option<TrustedSigner>, SignerStoreError> {
        <Self as TrustedSignerStore>::get(self, id).await
    }

    pub async fn is_revoked(
        &self,
        id: &SignerId,
        at: DateTime<Utc>,
    ) -> Result<bool, SignerStoreError> {
        <Self as TrustedSignerStore>::is_revoked(self, id, at).await
    }
}

#[async_trait]
impl TrustedSignerStore for StaticTrustedSignerStore {
    async fn list_active(&self) -> Result<Vec<TrustedSigner>, SignerStoreError> {
        let now = Utc::now();
        Ok(self
            .signers
            .read()
            .values()
            .filter(|signer| signer.is_active_at(now))
            .cloned()
            .collect())
    }

    async fn list_revoked(&self) -> Result<Vec<TrustedSigner>, SignerStoreError> {
        Ok(self
            .signers
            .read()
            .values()
            .filter(|signer| signer.revoked_at.is_some())
            .cloned()
            .collect())
    }

    async fn get(&self, id: &SignerId) -> Result<Option<TrustedSigner>, SignerStoreError> {
        Ok(self.signers.read().get(id).cloned())
    }

    async fn is_revoked(&self, id: &SignerId, at: DateTime<Utc>) -> Result<bool, SignerStoreError> {
        Ok(self
            .signers
            .read()
            .get(id)
            .and_then(|signer| signer.revoked_at)
            .is_some_and(|revoked_at| revoked_at <= at))
    }

    fn watch(&self) -> SignerStoreEventStream {
        Box::pin(stream::empty())
    }
}

impl TrustedSigner {
    fn is_active_at(&self, at: DateTime<Utc>) -> bool {
        self.activated_at <= at
            && self.retired_at.map_or(true, |retired_at| at < retired_at)
            && self.revoked_at.map_or(true, |revoked_at| revoked_at > at)
    }

    fn signed_at_allowed(&self, signed_at: DateTime<Utc>) -> bool {
        self.activated_at <= signed_at
            && self
                .retired_at
                .map_or(true, |retired_at| signed_at < retired_at)
    }
}

#[derive(Clone)]
pub struct ManifestSigner {
    signer_store: Arc<dyn TrustedSignerStore>,
}

impl ManifestSigner {
    pub fn new(signer_store: Arc<dyn TrustedSignerStore>) -> Self {
        Self { signer_store }
    }

    pub fn canonical_payload(manifest: &PluginManifest) -> Result<Vec<u8>, PluginError> {
        let mut value =
            serde_json::to_value(manifest).map_err(|error| PluginError::SignatureInvalid {
                details: error.to_string(),
            })?;
        strip_signature(&mut value);
        let value = canonicalize_json(value);
        serde_json::to_vec(&value).map_err(|error| PluginError::SignatureInvalid {
            details: error.to_string(),
        })
    }

    pub async fn verify_manifest(&self, manifest: &PluginManifest) -> Result<(), PluginError> {
        if manifest.trust_level != TrustLevel::AdminTrusted {
            return Ok(());
        }

        let Some(manifest_signature) = &manifest.signature else {
            return Err(PluginError::SignatureInvalid {
                details: "missing signature".to_owned(),
            });
        };

        let signer_id = SignerId::new(manifest_signature.signer.clone())?;
        let Some(signer) = self.signer_store.get(&signer_id).await? else {
            return Err(PluginError::UnknownSigner(
                manifest_signature.signer.clone(),
            ));
        };

        if signer.algorithm != manifest_signature.algorithm {
            return Err(PluginError::SignatureInvalid {
                details: "algorithm mismatch".to_owned(),
            });
        }

        let signed_at = DateTime::parse_from_rfc3339(&manifest_signature.timestamp)
            .map_err(|error| PluginError::SignatureInvalid {
                details: format!("invalid signature timestamp: {error}"),
            })?
            .with_timezone(&Utc);

        if !signer.signed_at_allowed(signed_at) {
            return Err(PluginError::SignatureInvalid {
                details: "timestamp out of activation window".to_owned(),
            });
        }

        let now = Utc::now();
        if self.signer_store.is_revoked(&signer_id, now).await? {
            return Err(PluginError::SignerRevoked {
                signer: manifest_signature.signer.clone(),
                revoked_at: signer.revoked_at.unwrap_or(now),
            });
        }

        verify_signature(manifest, &signer, manifest_signature)
    }

    pub async fn signer_counts(&self) -> Result<(usize, usize), PluginError> {
        let active = self.signer_store.list_active().await?.len();
        let revoked = self.signer_store.list_revoked().await?.len();
        Ok((active, revoked))
    }
}

fn verify_signature(
    manifest: &PluginManifest,
    signer: &TrustedSigner,
    manifest_signature: &ManifestSignature,
) -> Result<(), PluginError> {
    let payload = ManifestSigner::canonical_payload(manifest)?;
    match signer.algorithm {
        SignatureAlgorithm::Ed25519 => {
            let public_key =
                signature::UnparsedPublicKey::new(&signature::ED25519, &signer.public_key);
            public_key
                .verify(&payload, &manifest_signature.signature)
                .map_err(|_| PluginError::SignatureInvalid {
                    details: "signature invalid".to_owned(),
                })
        }
        SignatureAlgorithm::RsaPkcs1Sha256 => {
            let public_key = signature::UnparsedPublicKey::new(
                &signature::RSA_PKCS1_2048_8192_SHA256,
                &signer.public_key,
            );
            public_key
                .verify(&payload, &manifest_signature.signature)
                .map_err(|_| PluginError::SignatureInvalid {
                    details: "signature invalid".to_owned(),
                })
        }
    }
}

fn strip_signature(value: &mut Value) {
    if let Value::Object(object) = value {
        object.remove("signature");
    }
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_json).collect()),
        Value::Object(object) => {
            let sorted = object
                .into_iter()
                .map(|(key, value)| (key, canonicalize_json(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(sorted.into_iter().collect())
        }
        other => other,
    }
}

fn validate_signer(signer: &TrustedSigner) -> Result<(), SignerStoreError> {
    validate_signer_id(signer.id.as_str())?;
    if matches!(signer.provenance, SignerProvenance::BuilderInjected)
        && (signer.id.as_str().starts_with("jyowo-") || signer.id.as_str().starts_with("harness-"))
    {
        return Err(SignerStoreError::InvalidId(signer.id.to_string()));
    }
    Ok(())
}

fn validate_signer_id(value: &str) -> Result<(), SignerStoreError> {
    let len = value.len();
    if !(1..=128).contains(&len) {
        return Err(SignerStoreError::InvalidId(value.to_owned()));
    }
    if value.ends_with('-') {
        return Err(SignerStoreError::InvalidId(value.to_owned()));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(SignerStoreError::InvalidId(value.to_owned()));
    }
    Ok(())
}
