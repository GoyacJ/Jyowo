use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{SkillConfigDocument, SkillId, SkillStatus};
use harness_skill::{
    ConfigResolveError, SkillConfigDecl, SkillConfigResolver, SkillParamType, SkillRegistrySnapshot,
};
use secrecy::ExposeSecret;
pub use secrecy::SecretString;
use serde_json::Value;

pub const JYOWO_KEYCHAIN_SERVICE: &str = "Jyowo";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SkillConfigStoreError {
    SecretStoreUnavailable,
    UnsupportedDocumentVersion(u32),
}

impl fmt::Display for SkillConfigStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SecretStoreUnavailable => formatter.write_str("skill secret store unavailable"),
            Self::UnsupportedDocumentVersion(version) => {
                write!(
                    formatter,
                    "unsupported skill config document version {version}"
                )
            }
        }
    }
}

impl std::error::Error for SkillConfigStoreError {}

pub trait SkillSecretStore: Send + Sync {
    fn get(&self, skill_id: &str, key: &str)
        -> Result<Option<SecretString>, SkillConfigStoreError>;
    fn set(
        &self,
        skill_id: &str,
        key: &str,
        value: SecretString,
    ) -> Result<(), SkillConfigStoreError>;
    fn delete(&self, skill_id: &str, key: &str) -> Result<(), SkillConfigStoreError>;
}

#[derive(Debug, Clone, Default)]
pub struct KeyringSkillSecretStore;

impl KeyringSkillSecretStore {
    #[must_use]
    pub fn account_name(skill_id: &str, key: &str) -> String {
        format!(
            "{}/{}",
            encode_account_component(skill_id),
            encode_account_component(key)
        )
    }

    fn entry(skill_id: &str, key: &str) -> Result<keyring::Entry, SkillConfigStoreError> {
        keyring::Entry::new(JYOWO_KEYCHAIN_SERVICE, &Self::account_name(skill_id, key))
            .map_err(|_| SkillConfigStoreError::SecretStoreUnavailable)
    }
}

fn encode_account_component(value: &str) -> String {
    value.replace('%', "%25").replace('/', "%2F")
}

impl SkillSecretStore for KeyringSkillSecretStore {
    fn get(
        &self,
        skill_id: &str,
        key: &str,
    ) -> Result<Option<SecretString>, SkillConfigStoreError> {
        match Self::entry(skill_id, key)?.get_password() {
            Ok(secret) => Ok(Some(SecretString::from(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(SkillConfigStoreError::SecretStoreUnavailable),
        }
    }

    fn set(
        &self,
        skill_id: &str,
        key: &str,
        value: SecretString,
    ) -> Result<(), SkillConfigStoreError> {
        Self::entry(skill_id, key)?
            .set_password(value.expose_secret())
            .map_err(|_| SkillConfigStoreError::SecretStoreUnavailable)
    }

    fn delete(&self, skill_id: &str, key: &str) -> Result<(), SkillConfigStoreError> {
        match Self::entry(skill_id, key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(SkillConfigStoreError::SecretStoreUnavailable),
        }
    }
}

#[derive(Clone, Default)]
pub struct SkillConfigSnapshot {
    skills: BTreeMap<String, ScopedSkillConfig>,
    secret_store: Option<Arc<dyn SkillSecretStore>>,
}

#[derive(Clone, Default)]
struct ScopedSkillConfig {
    values: BTreeMap<String, Value>,
    configured_secrets: BTreeSet<String>,
}

impl SkillConfigSnapshot {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_document(
        document: SkillConfigDocument,
        secret_store: Arc<dyn SkillSecretStore>,
    ) -> Result<Self, SkillConfigStoreError> {
        if document.version != SkillConfigDocument::CURRENT_VERSION {
            return Err(SkillConfigStoreError::UnsupportedDocumentVersion(
                document.version,
            ));
        }
        let skills = document
            .skills
            .into_iter()
            .map(|(skill_id, entry)| {
                let configured_secrets = entry
                    .secrets
                    .into_iter()
                    .filter_map(|(key, metadata)| metadata.configured.then_some(key))
                    .collect();
                (
                    skill_id,
                    ScopedSkillConfig {
                        values: entry.values,
                        configured_secrets,
                    },
                )
            })
            .collect();
        Ok(Self {
            skills,
            secret_store: Some(secret_store),
        })
    }

    #[must_use]
    pub fn with_skill_value(
        mut self,
        skill_id: impl Into<String>,
        key: impl Into<String>,
        value: Value,
    ) -> Self {
        self.skills
            .entry(skill_id.into())
            .or_default()
            .values
            .insert(key.into(), value);
        self
    }

    #[must_use]
    pub fn with_skill_secret_presence(
        mut self,
        skill_id: impl Into<String>,
        key: impl Into<String>,
    ) -> Self {
        self.skills
            .entry(skill_id.into())
            .or_default()
            .configured_secrets
            .insert(key.into());
        self
    }

    #[must_use]
    pub fn with_secret_store(mut self, secret_store: Arc<dyn SkillSecretStore>) -> Self {
        self.secret_store = Some(secret_store);
        self
    }

    #[must_use]
    pub fn value_for(&self, skill_id: &str, key: &str) -> Option<&Value> {
        self.skills.get(skill_id)?.values.get(key)
    }

    #[must_use]
    pub fn contains_secret_for(&self, skill_id: &str, key: &str) -> bool {
        self.skills
            .get(skill_id)
            .is_some_and(|config| config.configured_secrets.contains(key))
    }

    pub fn secret_is_available_for(
        &self,
        skill_id: &str,
        key: &str,
    ) -> Result<bool, SkillConfigStoreError> {
        let Some(store) = &self.secret_store else {
            return Ok(false);
        };
        store.get(skill_id, key).map(|secret| secret.is_some())
    }

    pub fn secret_for_script(
        &self,
        skill_id: &str,
        key: &str,
    ) -> Result<Option<SecretString>, SkillConfigStoreError> {
        self.secret_store
            .as_ref()
            .ok_or(SkillConfigStoreError::SecretStoreUnavailable)?
            .get(skill_id, key)
    }
}

impl fmt::Debug for SkillConfigSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let summary = self
            .skills
            .iter()
            .map(|(skill_id, config)| {
                (
                    skill_id,
                    (
                        config.values.keys().collect::<Vec<_>>(),
                        config.configured_secrets.iter().collect::<Vec<_>>(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        formatter
            .debug_struct("SkillConfigSnapshot")
            .field("skills", &summary)
            .finish()
    }
}

#[derive(Debug)]
pub enum SkillConfigError {
    MissingRequired {
        skill_id: String,
        key: String,
    },
    InvalidType {
        skill_id: String,
        key: String,
        expected: &'static str,
    },
    Store(SkillConfigStoreError),
}

impl fmt::Display for SkillConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequired { skill_id, key } => {
                write!(
                    formatter,
                    "skill `{skill_id}` is missing required config `{key}`"
                )
            }
            Self::InvalidType {
                skill_id,
                key,
                expected,
            } => write!(
                formatter,
                "invalid skill config `{key}` for `{skill_id}`: expected {expected}"
            ),
            Self::Store(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SkillConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::MissingRequired { .. } | Self::InvalidType { .. } => None,
        }
    }
}

impl From<SkillConfigStoreError> for SkillConfigError {
    fn from(error: SkillConfigStoreError) -> Self {
        Self::Store(error)
    }
}

#[derive(Clone, Debug)]
pub struct SkillConfigSnapshotResolver {
    snapshot: SkillConfigSnapshot,
    selected_skill_id: String,
    declarations: BTreeMap<String, SkillConfigDecl>,
}

impl SkillConfigSnapshotResolver {
    #[must_use]
    pub fn for_skill(
        skill_id: impl Into<String>,
        snapshot: SkillConfigSnapshot,
        declarations: impl IntoIterator<Item = SkillConfigDecl>,
    ) -> Self {
        let skill_id = skill_id.into();
        let declarations = declarations
            .into_iter()
            .map(|declaration| (declaration.key.clone(), declaration))
            .collect();
        Self {
            snapshot,
            selected_skill_id: skill_id,
            declarations,
        }
    }

    fn selected_id(&self) -> &str {
        &self.selected_skill_id
    }

    fn declaration(
        &self,
        skill_id: &str,
        key: &str,
    ) -> Result<&SkillConfigDecl, ConfigResolveError> {
        if skill_id != self.selected_skill_id {
            return Err(ConfigResolveError::SkillIdentityMismatch {
                expected_skill_id: self.selected_skill_id.clone(),
                actual_skill_id: skill_id.to_owned(),
            });
        }
        self.declarations
            .get(key)
            .ok_or_else(|| ConfigResolveError::UnknownKey(key.to_owned()))
    }

    fn resolve_value(&self, skill_id: &str, key: &str) -> Result<Value, ConfigResolveError> {
        let declaration = self.declaration(skill_id, key)?;
        if declaration.secret {
            return Err(ConfigResolveError::SecretInterpolationForbidden {
                skill_id: skill_id.to_owned(),
                key: key.to_owned(),
            });
        }
        let value = self
            .snapshot
            .value_for(skill_id, key)
            .cloned()
            .or_else(|| declaration.default.clone())
            .ok_or_else(|| ConfigResolveError::MissingRequiredConfig {
                skill_id: skill_id.to_owned(),
                key: key.to_owned(),
            })?;
        if !validate_type(&value, declaration.value_type) {
            return Err(ConfigResolveError::InvalidType {
                skill_id: skill_id.to_owned(),
                key: key.to_owned(),
                expected: skill_param_type_name(declaration.value_type),
            });
        }
        Ok(value)
    }
}

#[async_trait]
impl SkillConfigResolver for SkillConfigSnapshotResolver {
    async fn resolve(&self, key: &str) -> Result<Value, ConfigResolveError> {
        self.resolve_value(self.selected_id(), key)
    }

    async fn resolve_secret(&self, key: &str) -> Result<SecretString, ConfigResolveError> {
        let _ = self.declaration(self.selected_id(), key)?;
        Err(ConfigResolveError::SecretInterpolationForbidden {
            skill_id: self.selected_id().to_owned(),
            key: key.to_owned(),
        })
    }

    async fn resolve_for(
        &self,
        skill_id: &SkillId,
        key: &str,
    ) -> Result<Value, ConfigResolveError> {
        self.resolve_value(&skill_id.0, key)
    }

    async fn resolve_secret_for(
        &self,
        skill_id: &SkillId,
        key: &str,
    ) -> Result<SecretString, ConfigResolveError> {
        let _ = self.declaration(&skill_id.0, key)?;
        Err(ConfigResolveError::SecretInterpolationForbidden {
            skill_id: skill_id.0.clone(),
            key: key.to_owned(),
        })
    }

    async fn resolve_secret_for_script(
        &self,
        skill_id: &SkillId,
        key: &str,
    ) -> Result<SecretString, ConfigResolveError> {
        let declaration = self.declaration(&skill_id.0, key)?;
        if !declaration.secret {
            return Err(ConfigResolveError::InvalidType {
                skill_id: skill_id.0.clone(),
                key: key.to_owned(),
                expected: "secret config",
            });
        }
        self.snapshot
            .secret_for_script(&skill_id.0, key)
            .map_err(|error| ConfigResolveError::Message(error.to_string()))?
            .ok_or_else(|| ConfigResolveError::MissingRequiredConfig {
                skill_id: skill_id.0.clone(),
                key: key.to_owned(),
            })
    }
}

pub fn apply_skill_config_statuses(
    registry_snapshot: &mut SkillRegistrySnapshot,
    config_snapshot: &SkillConfigSnapshot,
) -> Result<(), SkillConfigStoreError> {
    for skill in registry_snapshot.entries.values() {
        let missing = missing_required_config(skill, config_snapshot)?;
        if missing.is_empty() {
            continue;
        }
        let env_vars = match registry_snapshot.status.get(&skill.id) {
            Some(SkillStatus::PrerequisiteMissing { env_vars, .. }) => env_vars.clone(),
            Some(SkillStatus::Ready) | None => Vec::new(),
        };
        registry_snapshot.status.insert(
            skill.id.clone(),
            SkillStatus::PrerequisiteMissing {
                env_vars,
                config_keys: missing,
            },
        );
    }
    Ok(())
}

pub fn validate_required_skill_config(
    registry_snapshot: &SkillRegistrySnapshot,
    config_snapshot: &SkillConfigSnapshot,
) -> Result<(), SkillConfigError> {
    for skill in registry_snapshot.entries.values() {
        for declaration in &skill.frontmatter.config {
            if !declaration.secret {
                if let Some(value) = config_snapshot.value_for(&skill.id.0, &declaration.key) {
                    if !validate_type(value, declaration.value_type) {
                        return Err(SkillConfigError::InvalidType {
                            skill_id: skill.id.0.clone(),
                            key: declaration.key.clone(),
                            expected: skill_param_type_name(declaration.value_type),
                        });
                    }
                }
            }
            if declaration.required && !is_configured(skill, declaration, config_snapshot)? {
                return Err(SkillConfigError::MissingRequired {
                    skill_id: skill.id.0.clone(),
                    key: declaration.key.clone(),
                });
            }
        }
    }
    Ok(())
}

fn missing_required_config(
    skill: &harness_skill::Skill,
    config_snapshot: &SkillConfigSnapshot,
) -> Result<Vec<String>, SkillConfigStoreError> {
    let mut missing = Vec::new();
    for declaration in &skill.frontmatter.config {
        if declaration.required && !is_configured(skill, declaration, config_snapshot)? {
            missing.push(declaration.key.clone());
        }
    }
    missing.sort();
    Ok(missing)
}

fn is_configured(
    skill: &harness_skill::Skill,
    declaration: &SkillConfigDecl,
    config_snapshot: &SkillConfigSnapshot,
) -> Result<bool, SkillConfigStoreError> {
    if declaration.secret {
        config_snapshot.secret_is_available_for(&skill.id.0, &declaration.key)
    } else {
        Ok(config_snapshot
            .value_for(&skill.id.0, &declaration.key)
            .or(declaration.default.as_ref())
            .is_some_and(|value| validate_type(value, declaration.value_type)))
    }
}

fn validate_type(value: &Value, expected: SkillParamType) -> bool {
    match expected {
        SkillParamType::String | SkillParamType::Path | SkillParamType::Url => value.is_string(),
        SkillParamType::Number => value.is_number(),
        SkillParamType::Boolean => value.is_boolean(),
    }
}

fn skill_param_type_name(value_type: SkillParamType) -> &'static str {
    match value_type {
        SkillParamType::String => "string",
        SkillParamType::Number => "number",
        SkillParamType::Boolean => "boolean",
        SkillParamType::Path => "path",
        SkillParamType::Url => "url",
    }
}
