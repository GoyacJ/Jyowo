use harness_contracts::{ManifestValidationFailure as EventManifestValidationFailure, TrustLevel};

use crate::{CapabilitySlot, ManifestOrigin, PluginName};

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum PluginError {
    #[error("manifest loader: {0}")]
    ManifestLoader(#[from] ManifestLoaderError),
    #[error("runtime loader: {0}")]
    RuntimeLoader(#[from] RuntimeLoaderError),
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),
    #[error("admission denied: {policy}")]
    AdmissionDenied { policy: String },
    #[error("namespace conflict: {details}")]
    NamespaceConflict { details: String },
    #[error("trust mismatch: declared {declared:?}, source {source_label}")]
    TrustMismatch {
        declared: TrustLevel,
        source_label: String,
    },
    #[error("harness version mismatch: required {required}, actual {actual}")]
    HarnessVersionMismatch { required: String, actual: String },
    #[error("activation failed: {0}")]
    ActivateFailed(String),
    #[error("deactivation failed: {0}")]
    DeactivateFailed(String),
    #[error("registration: {0}")]
    Registration(#[from] RegistrationError),
    #[error("signer store: {0}")]
    SignerStore(#[from] SignerStoreError),
    #[error("signature invalid: {details}")]
    SignatureInvalid { details: String },
    #[error("unknown signer: {0}")]
    UnknownSigner(String),
    #[error("signer revoked: {signer} at {revoked_at}")]
    SignerRevoked {
        signer: String,
        revoked_at: chrono::DateTime<chrono::Utc>,
    },
    #[error("builder: {0}")]
    Builder(String),
    #[error("slot occupied: {slot:?} by {occupant:?}")]
    SlotOccupied {
        slot: CapabilitySlot,
        occupant: harness_contracts::PluginId,
    },
    #[error("dependency unsatisfied: {dependency} requires {requirement}")]
    DependencyUnsatisfied {
        dependency: String,
        requirement: String,
    },
    #[error("dependency cycle: {0:?}")]
    DependencyCycle(Vec<String>),
    #[error("active dependents: {0:?}")]
    ActiveDependents(Vec<harness_contracts::PluginId>),
}

#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum RegistrationError {
    #[error("undeclared tool: {name}")]
    UndeclaredTool { name: String },
    #[error("undeclared hook: {name}")]
    UndeclaredHook { name: String },
    #[error("undeclared mcp server: {name}")]
    UndeclaredMcp { name: String },
    #[error("undeclared skill: {name}")]
    UndeclaredSkill { name: String },
    #[error("undeclared activation result {kind}: {name}")]
    UndeclaredResult { kind: &'static str, name: String },
    #[error("duplicate slot registration: {slot:?}")]
    DuplicateSlot { slot: CapabilitySlot },
    #[error("descriptor mismatch for {name}: declared destructive={declared_destructive}, actual destructive={actual_destructive}")]
    DescriptorMismatch {
        name: String,
        declared_destructive: bool,
        actual_destructive: bool,
    },
    #[error("trust violation for {capability}: {details}")]
    TrustViolation {
        capability: &'static str,
        details: String,
    },
    #[error("{kind} owner registry rejected plugin capability: {details}")]
    OwnerRegistry { kind: &'static str, details: String },
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum ManifestLoaderError {
    #[error("unsupported source: {0}")]
    UnsupportedSource(String),
    #[error("validation failed: {0}")]
    Validation(ManifestValidationFailure),
    #[error("io: {0}")]
    Io(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManifestValidationFailure {
    pub origin: Option<ManifestOrigin>,
    pub partial_name: Option<String>,
    pub partial_version: Option<String>,
    pub raw_bytes_hash: [u8; 32],
    pub failure: EventManifestValidationFailure,
    pub details: String,
}

impl std::fmt::Display for ManifestValidationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.origin {
            Some(origin) => write!(formatter, "{}: {}", origin, self.details),
            None => formatter.write_str(&self.details),
        }
    }
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum RuntimeLoaderError {
    #[error("unsupported origin: {0}")]
    UnsupportedOrigin(String),
    #[error("plugin not found: {0}")]
    PluginNotFound(PluginName),
    #[error("load failed: {0}")]
    LoadFailed(String),
}

#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum SignerStoreError {
    #[error("invalid signer id: {0}")]
    InvalidId(String),
    #[error("policy file invalid: {0}")]
    PolicyFile(String),
    #[error("pki endpoint unreachable: {0}")]
    PkiEndpoint(String),
    #[error("io: {0}")]
    Io(String),
}
