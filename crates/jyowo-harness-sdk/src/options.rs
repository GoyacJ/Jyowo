use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

use crate::HarnessOptions;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum OptionsParseMode {
    Strict,
    Permissive,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ConfigSource {
    Primary { path: Option<PathBuf> },
    LastKnownGood { path: PathBuf },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ConfigWarning {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ParsedHarnessOptions {
    pub options: HarnessOptions,
    pub warnings: Vec<ConfigWarning>,
    pub source: ConfigSource,
    pub primary_error: Option<ConfigError>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LastKnownGoodConfig {
    pub path: PathBuf,
}

impl LastKnownGoodConfig {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ConfigError {
    Io(String),
    Parse(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) | Self::Parse(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ConfigError {}

impl HarnessOptions {
    pub fn parse(input: &str, mode: OptionsParseMode) -> Result<ParsedHarnessOptions, ConfigError> {
        parse_options(input, mode, ConfigSource::Primary { path: None }, None)
    }

    pub fn parse_file(
        path: impl AsRef<Path>,
        mode: OptionsParseMode,
    ) -> Result<ParsedHarnessOptions, ConfigError> {
        let path = path.as_ref();
        let input = std::fs::read_to_string(path)
            .map_err(|error| ConfigError::Io(format!("read config failed: {error}")))?;
        parse_options(
            &input,
            mode,
            ConfigSource::Primary {
                path: Some(path.to_path_buf()),
            },
            None,
        )
    }

    pub fn load_with_fallback(
        path: impl AsRef<Path>,
        lkg: LastKnownGoodConfig,
        mode: OptionsParseMode,
    ) -> Result<ParsedHarnessOptions, ConfigError> {
        match Self::parse_file(path, mode) {
            Ok(loaded) => Ok(loaded),
            Err(primary_error) => {
                let input = std::fs::read_to_string(&lkg.path).map_err(|error| {
                    ConfigError::Io(format!("read last-known-good config failed: {error}"))
                })?;
                parse_options(
                    &input,
                    mode,
                    ConfigSource::LastKnownGood { path: lkg.path },
                    Some(primary_error),
                )
            }
        }
    }
}

fn parse_options(
    input: &str,
    mode: OptionsParseMode,
    source: ConfigSource,
    primary_error: Option<ConfigError>,
) -> Result<ParsedHarnessOptions, ConfigError> {
    if mode == OptionsParseMode::Strict {
        reject_duplicate_fields(input)?;
    }
    let raw: Value = serde_json::from_str(input)
        .map_err(|error| ConfigError::Parse(format!("parse config failed: {error}")))?;
    let warnings = secret_warnings(&raw);
    let mut options: HarnessOptions = serde_json::from_value(raw)
        .map_err(|error| ConfigError::Parse(format!("parse config failed: {error}")))?;
    if options.default_session_options.workspace_root == PathBuf::from(".") {
        options.default_session_options.workspace_root = options.workspace_root.clone();
    }
    Ok(ParsedHarnessOptions {
        options,
        warnings,
        source,
        primary_error,
    })
}

fn reject_duplicate_fields(input: &str) -> Result<(), ConfigError> {
    let mut deserializer = serde_json::Deserializer::from_str(input);
    DuplicateDetector {
        path: String::new(),
    }
    .deserialize(&mut deserializer)
    .map_err(|error| ConfigError::Parse(format!("duplicate field check failed: {error}")))
}

struct DuplicateDetector {
    path: String,
}

impl<'de> DeserializeSeed<'de> for DuplicateDetector {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for DuplicateDetector {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut seen = BTreeSet::new();
        while let Some(key) = access.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                let path = join_path(&self.path, &key);
                return Err(de::Error::custom(format!("duplicate field `{path}`")));
            }
            access.next_value_seed(DuplicateDetector {
                path: join_path(&self.path, &key),
            })?;
        }
        Ok(())
    }

    fn visit_seq<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut index = 0_usize;
        while access
            .next_element_seed(DuplicateDetector {
                path: format!("{}[{index}]", self.path),
            })?
            .is_some()
        {
            index += 1;
        }
        Ok(())
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_string<E>(self, _value: String) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }
}

fn secret_warnings(value: &Value) -> Vec<ConfigWarning> {
    let mut warnings = Vec::new();
    collect_secret_warnings(value, "", &mut warnings);
    warnings
}

fn collect_secret_warnings(value: &Value, path: &str, warnings: &mut Vec<ConfigWarning>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = join_path(path, key);
                if is_sensitive_key(key)
                    && child
                        .as_str()
                        .is_some_and(|secret| !is_secret_reference(secret))
                {
                    warnings.push(ConfigWarning {
                        path: child_path.clone(),
                        message: "plaintext secret-like value should use ref:, env:, or vault:"
                            .to_owned(),
                    });
                }
                collect_secret_warnings(child, &child_path, warnings);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                collect_secret_warnings(child, &format!("{path}[{index}]"), warnings);
            }
        }
        _ => {}
    }
}

fn join_path(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}.{key}")
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("secret")
        || key.contains("token")
        || key.contains("password")
        || key.contains("key")
}

fn is_secret_reference(value: &str) -> bool {
    value.starts_with("ref:") || value.starts_with("env:") || value.starts_with("vault:")
}
