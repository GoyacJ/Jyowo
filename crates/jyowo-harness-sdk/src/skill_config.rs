use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use async_trait::async_trait;
use harness_skill::{
    ConfigResolveError, SkillConfigResolver, SkillParamType, SkillRegistrySnapshot,
};
use secrecy::SecretString;
use serde_json::Value;

#[derive(Clone, Default)]
pub struct SkillConfigSnapshot {
    values: BTreeMap<String, Value>,
    secrets: BTreeMap<String, SecretString>,
}

impl SkillConfigSnapshot {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_value(mut self, key: impl Into<String>, value: Value) -> Self {
        self.values.insert(key.into(), value);
        self
    }

    #[must_use]
    pub fn with_secret(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.secrets
            .insert(key.into(), SecretString::from(value.into()));
        self
    }

    #[must_use]
    pub fn contains_value(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    #[must_use]
    pub fn contains_secret(&self, key: &str) -> bool {
        self.secrets.contains_key(key)
    }
}

impl fmt::Debug for SkillConfigSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SkillConfigSnapshot")
            .field("value_keys", &self.values.keys().collect::<Vec<_>>())
            .field("secret_keys", &self.secrets.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Debug)]
pub enum SkillConfigError {
    MissingRequired(String),
    InvalidType { key: String, expected: &'static str },
}

impl fmt::Display for SkillConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequired(key) => {
                write!(formatter, "missing required skill config `{key}`")
            }
            Self::InvalidType { key, expected } => {
                write!(
                    formatter,
                    "invalid skill config `{key}`: expected {expected}"
                )
            }
        }
    }
}

impl std::error::Error for SkillConfigError {}

#[derive(Clone, Debug)]
pub struct SkillConfigSnapshotResolver {
    snapshot: SkillConfigSnapshot,
    approved_keys: BTreeSet<String>,
    defaults: BTreeMap<String, Value>,
}

impl SkillConfigSnapshotResolver {
    #[must_use]
    pub fn new(
        snapshot: SkillConfigSnapshot,
        approved_keys: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            snapshot,
            approved_keys: approved_keys.into_iter().collect(),
            defaults: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn from_registry_snapshot(
        registry_snapshot: &SkillRegistrySnapshot,
        snapshot: SkillConfigSnapshot,
    ) -> Self {
        let mut approved_keys = BTreeSet::new();
        let mut defaults = BTreeMap::new();
        for skill in registry_snapshot.entries.values() {
            for config in &skill.frontmatter.config {
                approved_keys.insert(config.key.clone());
                if let Some(default) = &config.default {
                    defaults.insert(config.key.clone(), default.clone());
                }
            }
        }
        Self {
            snapshot,
            approved_keys,
            defaults,
        }
    }
}

#[async_trait]
impl SkillConfigResolver for SkillConfigSnapshotResolver {
    async fn resolve(&self, key: &str) -> Result<Value, ConfigResolveError> {
        if !self.approved_keys.contains(key) {
            return Err(ConfigResolveError::UnknownKey(key.to_owned()));
        }
        self.snapshot
            .values
            .get(key)
            .cloned()
            .or_else(|| self.defaults.get(key).cloned())
            .ok_or_else(|| ConfigResolveError::UnknownKey(key.to_owned()))
    }

    async fn resolve_secret(&self, key: &str) -> Result<SecretString, ConfigResolveError> {
        if !self.approved_keys.contains(key) {
            return Err(ConfigResolveError::UnknownKey(key.to_owned()));
        }
        self.snapshot
            .secrets
            .get(key)
            .cloned()
            .ok_or_else(|| ConfigResolveError::UnknownKey(key.to_owned()))
    }
}

pub fn validate_required_skill_config(
    registry_snapshot: &SkillRegistrySnapshot,
    config_snapshot: &SkillConfigSnapshot,
) -> Result<(), SkillConfigError> {
    for skill in registry_snapshot.entries.values() {
        for config in &skill.frontmatter.config {
            if config.secret {
                if config_snapshot.contains_secret(&config.key) {
                    continue;
                }
            } else if config_snapshot.contains_value(&config.key) {
                validate_type(
                    &config.key,
                    config_snapshot.values.get(&config.key),
                    config.value_type,
                )?;
                continue;
            } else if config.default.is_some() {
                continue;
            }

            if config.required {
                return Err(SkillConfigError::MissingRequired(config.key.clone()));
            }
        }
    }
    Ok(())
}

fn validate_type(
    key: &str,
    value: Option<&Value>,
    expected: SkillParamType,
) -> Result<(), SkillConfigError> {
    let Some(value) = value else {
        return Ok(());
    };
    let valid = match expected {
        SkillParamType::String | SkillParamType::Path | SkillParamType::Url => value.is_string(),
        SkillParamType::Number => value.is_number(),
        SkillParamType::Boolean => value.is_boolean(),
    };
    if valid {
        Ok(())
    } else {
        Err(SkillConfigError::InvalidType {
            key: key.to_owned(),
            expected: expected_name(expected),
        })
    }
}

fn expected_name(expected: SkillParamType) -> &'static str {
    match expected {
        SkillParamType::String => "string",
        SkillParamType::Number => "number",
        SkillParamType::Boolean => "boolean",
        SkillParamType::Path => "path",
        SkillParamType::Url => "url",
    }
}
