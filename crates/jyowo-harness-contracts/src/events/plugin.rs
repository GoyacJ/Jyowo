use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginLoadedEvent {
    pub tenant_id: TenantId,
    pub plugin_id: PluginId,
    pub plugin_name: String,
    pub plugin_version: SemverString,
    pub trust_level: TrustLevel,
    pub capabilities: PluginCapabilitiesSummary,
    pub manifest_origin: ManifestOriginRef,
    pub manifest_hash: [u8; 32],
    pub from_state: PluginLifecycleStateDiscriminant,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginRejectedEvent {
    pub tenant_id: TenantId,
    pub plugin_id: PluginId,
    pub plugin_name: String,
    pub plugin_version: SemverString,
    pub trust_level: TrustLevel,
    pub manifest_origin: ManifestOriginRef,
    pub manifest_hash: [u8; 32],
    pub reason: RejectionReason,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ManifestValidationFailedEvent {
    pub tenant_id: TenantId,
    pub manifest_origin: ManifestOriginRef,
    pub partial_name: Option<String>,
    pub partial_version: Option<String>,
    pub raw_bytes_hash: [u8; 32],
    pub failure: ManifestValidationFailure,
    pub at: DateTime<Utc>,
}
