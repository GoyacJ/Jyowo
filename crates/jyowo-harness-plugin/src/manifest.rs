use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use harness_contracts::{HookEventKind, PluginId, TrustLevel};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PluginError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub name: PluginName,
    #[serde(with = "semver_version_serde")]
    pub version: Version,
    pub trust_level: TrustLevel,
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    pub repository: Option<String>,
    pub signature: Option<ManifestSignature>,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub dependencies: Vec<PluginDependency>,
    #[serde(with = "semver_version_req_serde")]
    pub min_harness_version: VersionReq,
}

impl PluginManifest {
    pub fn plugin_id(&self) -> PluginId {
        PluginId(format!("{}@{}", self.name, self.version))
    }

    pub fn validate_basic(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCapabilities {
    #[serde(default, skip_serializing_if = "is_false")]
    pub steering: bool,
    #[serde(default)]
    pub tools: Vec<ToolManifestEntry>,
    #[serde(default)]
    pub skills: Vec<SkillManifestEntry>,
    #[serde(default)]
    pub hooks: Vec<HookManifestEntry>,
    #[serde(default)]
    pub mcp_servers: Vec<McpManifestEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_toolsets: Vec<CustomToolsetManifestEntry>,
    pub memory_provider: Option<MemoryProviderManifestEntry>,
    pub coordinator_strategy: Option<CoordinatorStrategyManifestEntry>,
    pub configuration_schema: Option<Value>,
}

impl PluginCapabilities {
    pub fn is_empty(&self) -> bool {
        !self.steering
            && self.tools.is_empty()
            && self.skills.is_empty()
            && self.hooks.is_empty()
            && self.mcp_servers.is_empty()
            && self.custom_toolsets.is_empty()
            && self.memory_provider.is_none()
            && self.coordinator_strategy.is_none()
            && self.configuration_schema.is_none()
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

fn default_tool_input_schema() -> Value {
    serde_json::json!({ "type": "object" })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolManifestEntry {
    pub name: String,
    #[serde(default)]
    pub destructive: bool,
    #[serde(default = "default_tool_input_schema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillManifestEntry {
    pub name: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookManifestEntry {
    pub name: String,
    #[serde(default)]
    pub events: Vec<HookEventKind>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpManifestEntry {
    pub name: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomToolsetManifestEntry {
    pub name: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryProviderManifestEntry {
    pub name: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorStrategyManifestEntry {
    pub name: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDependency {
    pub name: PluginName,
    #[serde(default = "default_version_req", with = "semver_version_req_serde")]
    pub version_req: VersionReq,
    #[serde(default)]
    pub kind: PluginDependencyKind,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginDependencyKind {
    #[default]
    Required,
    Optional,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSignature {
    pub algorithm: SignatureAlgorithm,
    pub signer: String,
    pub signature: Vec<u8>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureAlgorithm {
    Ed25519,
    RsaPkcs1Sha256,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PluginName(String);

impl PluginName {
    pub fn new(value: impl Into<String>) -> Result<Self, PluginError> {
        let value = value.into();
        validate_plugin_name(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PluginName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for PluginName {
    type Error = PluginError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PluginName> for String {
    fn from(value: PluginName) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestRecord {
    pub manifest: PluginManifest,
    pub origin: ManifestOrigin,
    pub manifest_hash: [u8; 32],
}

impl ManifestRecord {
    pub fn new(
        manifest: PluginManifest,
        origin: ManifestOrigin,
        manifest_hash: [u8; 32],
    ) -> Result<Self, PluginError> {
        manifest.validate_basic()?;
        Ok(Self {
            manifest,
            origin,
            manifest_hash,
        })
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestOrigin {
    File {
        path: PathBuf,
    },
    CargoExtension {
        binary: PathBuf,
        package_metadata: BTreeMap<String, Value>,
    },
    RemoteRegistry {
        endpoint: String,
        etag: Option<String>,
    },
}

impl fmt::Display for ManifestOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { path } => write!(formatter, "file:{}", path.display()),
            Self::CargoExtension { binary, .. } => {
                write!(formatter, "cargo_extension:{}", binary.display())
            }
            Self::RemoteRegistry { endpoint, .. } => write!(formatter, "remote:{endpoint}"),
        }
    }
}

fn validate_plugin_name(value: &str) -> Result<(), PluginError> {
    let len = value.len();
    if !(1..=64).contains(&len) {
        return Err(PluginError::InvalidManifest(
            "plugin name length must be 1..=64".to_owned(),
        ));
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(PluginError::InvalidManifest(
            "plugin name must not be empty".to_owned(),
        ));
    };
    if !first.is_ascii_lowercase() {
        return Err(PluginError::InvalidManifest(
            "plugin name must start with a lowercase ASCII letter".to_owned(),
        ));
    }
    if value.ends_with('-') {
        return Err(PluginError::InvalidManifest(
            "plugin name must not end with '-'".to_owned(),
        ));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(PluginError::InvalidManifest(
            "plugin name may only contain lowercase ASCII letters, digits, and '-'".to_owned(),
        ));
    }
    Ok(())
}

fn default_version_req() -> VersionReq {
    VersionReq::parse(">=0.0.0").unwrap_or(VersionReq::STAR)
}

mod semver_version_serde {
    use semver::Version;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(version: &Version, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&version.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Version, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Version::parse(&value).map_err(serde::de::Error::custom)
    }
}

mod semver_version_req_serde {
    use semver::VersionReq;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(requirement: &VersionReq, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&requirement.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<VersionReq, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        VersionReq::parse(&value).map_err(serde::de::Error::custom)
    }
}
