use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use harness_contracts::ManifestValidationFailure as EventManifestValidationFailure;
use ring::digest;
use serde_json::{Map, Number, Value};
use yaml_rust2::{Yaml, YamlLoader};

use crate::{
    DiscoverySource, ManifestLoadReport, ManifestLoaderError, ManifestOrigin, ManifestRecord,
    ManifestSigner, ManifestValidationFailure, PluginManifest, PluginManifestLoader,
};

#[derive(Debug, Default, Clone)]
pub struct FileManifestLoader;

impl FileManifestLoader {
    pub async fn load_package_report(
        &self,
        plugin_dir: &Path,
    ) -> Result<ManifestLoadReport, ManifestLoaderError> {
        let metadata = secure_plugin_directory(plugin_dir)?;
        if !metadata.is_dir() {
            return Ok(ManifestLoadReport::default());
        }
        let Some(manifest_path) = manifest_path(plugin_dir)? else {
            return Ok(ManifestLoadReport::default());
        };
        match read_manifest(&manifest_path) {
            Ok(record) => Ok(ManifestLoadReport {
                records: vec![record],
                failures: Vec::new(),
            }),
            Err(ManifestLoaderError::Validation(failure)) => Ok(ManifestLoadReport {
                records: Vec::new(),
                failures: vec![failure],
            }),
            Err(error) => Err(error),
        }
    }

    pub fn load_source_report(
        &self,
        source: &DiscoverySource,
    ) -> Result<ManifestLoadReport, ManifestLoaderError> {
        let Some(root) = source_root(source) else {
            return Ok(ManifestLoadReport::default());
        };

        let plugin_root = plugin_root(source, root);
        if !plugin_root.exists() {
            return Ok(ManifestLoadReport::default());
        }
        let metadata = secure_plugin_directory(&plugin_root)?;
        if !metadata.is_dir() {
            return Ok(ManifestLoadReport::default());
        }

        let mut entries = fs::read_dir(&plugin_root)
            .map_err(|error| ManifestLoaderError::Io(error.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| ManifestLoaderError::Io(error.to_string()))?;
        entries.sort_by_key(|entry| entry.path());

        let mut records = Vec::new();
        let mut failures = Vec::new();
        for entry in entries {
            let path = entry.path();
            let metadata = secure_plugin_directory(&path)?;
            if !metadata.is_dir() {
                continue;
            }
            let Some(manifest_path) = manifest_path(&path)? else {
                continue;
            };
            match read_manifest(&manifest_path) {
                Ok(record) => records.push(record),
                Err(ManifestLoaderError::Validation(failure)) => failures.push(failure),
                Err(error) => return Err(error),
            }
        }

        Ok(ManifestLoadReport { records, failures })
    }
}

#[async_trait]
impl PluginManifestLoader for FileManifestLoader {
    async fn enumerate(
        &self,
        source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        let report = self.load_report(source).await?;
        if let Some(failure) = report.failures.into_iter().next() {
            return Err(ManifestLoaderError::Validation(failure));
        }
        Ok(report.records)
    }

    async fn load_report(
        &self,
        source: &DiscoverySource,
    ) -> Result<ManifestLoadReport, ManifestLoaderError> {
        self.load_source_report(source)
    }
}

#[derive(Debug, Clone, Default)]
pub struct InlineManifestLoader {
    records: Vec<ManifestRecord>,
}

impl InlineManifestLoader {
    pub fn new(records: Vec<ManifestRecord>) -> Self {
        Self { records }
    }
}

#[async_trait]
impl PluginManifestLoader for InlineManifestLoader {
    async fn enumerate(
        &self,
        source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        if matches!(source, DiscoverySource::Inline) {
            Ok(self.records.clone())
        } else {
            Ok(Vec::new())
        }
    }
}

fn source_root(source: &DiscoverySource) -> Option<&Path> {
    match source {
        DiscoverySource::Workspace(path)
        | DiscoverySource::User(path)
        | DiscoverySource::Project(path) => Some(path.as_path()),
        DiscoverySource::CargoExtension | DiscoverySource::Inline => None,
    }
}

fn plugin_root(source: &DiscoverySource, root: &Path) -> PathBuf {
    match source {
        DiscoverySource::Workspace(_) => {
            let standard = root.join("data/plugins");
            if standard.exists() {
                standard
            } else {
                root.to_path_buf()
            }
        }
        DiscoverySource::User(_) | DiscoverySource::Project(_) => {
            let standard = root.join(".jyowo/plugins");
            if standard.exists() {
                standard
            } else {
                root.to_path_buf()
            }
        }
        DiscoverySource::CargoExtension | DiscoverySource::Inline => root.to_path_buf(),
    }
}

fn manifest_path(plugin_dir: &Path) -> Result<Option<PathBuf>, ManifestLoaderError> {
    for path in ["plugin.json", "plugin.yaml", "plugin.yml"]
        .into_iter()
        .map(|name| plugin_dir.join(name))
    {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(ManifestLoaderError::Io(error.to_string())),
        };
        if metadata.file_type().is_symlink() {
            return Err(ManifestLoaderError::Io(
                "plugin manifest must not be a symlink".to_owned(),
            ));
        }
        if metadata.is_file() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn read_manifest(path: &Path) -> Result<ManifestRecord, ManifestLoaderError> {
    let bytes = read_regular_file_no_follow(path)?;
    let raw_hash = sha256(&bytes);
    let file_origin = ManifestOrigin::File {
        path: path.to_path_buf(),
    };
    let manifest = parse_manifest(path, &bytes, raw_hash, file_origin.clone())?;
    let origin = match path
        .parent()
        .map(|plugin_dir| local_sidecar_binary(plugin_dir, manifest.name.as_str()))
        .transpose()?
        .flatten()
    {
        Some(binary) => ManifestOrigin::CargoExtension {
            binary,
            package_metadata: BTreeMap::new(),
        },
        None => file_origin,
    };
    let canonical_hash = canonical_manifest_hash(&manifest, raw_hash, &origin)?;

    ManifestRecord::new(manifest.clone(), origin, canonical_hash).map_err(|error| {
        validation_error(
            None,
            Some(manifest.name.to_string()),
            Some(manifest.version.to_string()),
            raw_hash,
            EventManifestValidationFailure::SchemaViolation {
                json_pointer: String::new(),
                details: format!("manifest basic validation failed: {error}"),
            },
            format!("manifest basic validation failed: {error}"),
        )
    })
}

#[cfg(unix)]
fn read_regular_file_no_follow(path: &Path) -> Result<Vec<u8>, ManifestLoaderError> {
    let mut components = Vec::new();
    let mut absolute = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                return Err(ManifestLoaderError::Io(
                    "plugin manifest path has unsupported prefix".to_owned(),
                ));
            }
            Component::RootDir => absolute = true,
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(ManifestLoaderError::Io(
                    "plugin manifest path must not use parent directory components".to_owned(),
                ));
            }
            Component::Normal(value) => components.push(value.to_os_string()),
        }
    }
    let file_name = components.pop().ok_or_else(|| {
        ManifestLoaderError::Io("plugin manifest path has no file name".to_owned())
    })?;
    let mut directory = if absolute {
        fs::File::open(Path::new("/"))
    } else {
        fs::File::open(Path::new("."))
    }
    .map_err(|error| ManifestLoaderError::Io(error.to_string()))?;

    for component in components {
        let fd = match rustix::fs::openat(
            &directory,
            Path::new(&component),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0),
        ) {
            Ok(fd) => fd,
            Err(rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) => {
                return Err(ManifestLoaderError::Io(
                    "plugin manifest must not use symlinks".to_owned(),
                ));
            }
            Err(error) => return Err(ManifestLoaderError::Io(error.to_string())),
        };
        directory = fs::File::from(fd);
    }

    let fd = match rustix::fs::openat(
        &directory,
        Path::new(&file_name),
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::from_raw_mode(0),
    ) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) => {
            return Err(ManifestLoaderError::Io(
                "plugin manifest must not be a symlink".to_owned(),
            ));
        }
        Err(error) => return Err(ManifestLoaderError::Io(error.to_string())),
    };
    let mut file = fs::File::from(fd);
    let metadata = file
        .metadata()
        .map_err(|error| ManifestLoaderError::Io(error.to_string()))?;
    if !metadata.is_file() {
        return Err(ManifestLoaderError::Io(
            "plugin manifest must be a file".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| ManifestLoaderError::Io(error.to_string()))?;
    Ok(bytes)
}

#[cfg(not(unix))]
fn read_regular_file_no_follow(path: &Path) -> Result<Vec<u8>, ManifestLoaderError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| ManifestLoaderError::Io(error.to_string()))?;
    if metadata.file_type().is_symlink() {
        return Err(ManifestLoaderError::Io(
            "plugin manifest must not be a symlink".to_owned(),
        ));
    }
    if !metadata.is_file() {
        return Err(ManifestLoaderError::Io(
            "plugin manifest must be a file".to_owned(),
        ));
    }
    fs::read(path).map_err(|error| ManifestLoaderError::Io(error.to_string()))
}

fn secure_plugin_directory(path: &Path) -> Result<fs::Metadata, ManifestLoaderError> {
    ensure_no_world_writable_ancestors(path, "plugin directory")?;
    let metadata =
        fs::symlink_metadata(path).map_err(|error| ManifestLoaderError::Io(error.to_string()))?;
    if metadata.file_type().is_symlink() {
        return Err(ManifestLoaderError::Io(
            "plugin directory must not be a symlink".to_owned(),
        ));
    }
    if metadata.is_dir() && is_world_writable(&metadata) {
        return Err(ManifestLoaderError::Io(
            "plugin directory must not be world-writable".to_owned(),
        ));
    }
    Ok(metadata)
}

fn local_sidecar_binary(
    plugin_dir: &Path,
    plugin_name: &str,
) -> Result<Option<PathBuf>, ManifestLoaderError> {
    let entries = fs::read_dir(plugin_dir)
        .map_err(|error| ManifestLoaderError::Io(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ManifestLoaderError::Io(error.to_string()))?;
    let mut candidates = entries
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(is_cargo_extension_name)
                && is_executable_regular_file(path)
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(None);
    }
    candidates.sort();
    let expected_name = format!("jyowo-plugin-{plugin_name}");
    let Some(selected) = candidates
        .iter()
        .find(|path| path.file_name().and_then(OsStr::to_str) == Some(expected_name.as_str()))
    else {
        return Err(ManifestLoaderError::Io(format!(
            "plugin sidecar binary must be named {expected_name}"
        )));
    };
    selected.canonicalize().map(Some).map_err(|error| {
        ManifestLoaderError::Io(format!("plugin sidecar path unavailable: {error}"))
    })
}

fn is_cargo_extension_name(name: &OsStr) -> bool {
    name.to_string_lossy().starts_with("jyowo-plugin-")
}

#[cfg(unix)]
fn ensure_no_world_writable_ancestors(path: &Path, label: &str) -> Result<(), ManifestLoaderError> {
    use std::os::unix::fs::PermissionsExt;

    for ancestor in path.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|error| ManifestLoaderError::Io(error.to_string()))?;
        let mode = metadata.permissions().mode();
        if mode & 0o002 != 0 && mode & 0o1000 == 0 {
            return Err(ManifestLoaderError::Io(format!(
                "{label} ancestors must not be world-writable"
            )));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_no_world_writable_ancestors(
    _path: &Path,
    _label: &str,
) -> Result<(), ManifestLoaderError> {
    Ok(())
}

#[cfg(unix)]
fn is_executable_regular_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::symlink_metadata(path)
        .map(|metadata| {
            !metadata.file_type().is_symlink()
                && metadata.is_file()
                && metadata.permissions().mode() & 0o111 != 0
        })
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| !metadata.file_type().is_symlink() && metadata.is_file())
        .unwrap_or(false)
}

#[cfg(unix)]
fn is_world_writable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o002 != 0
}

#[cfg(not(unix))]
fn is_world_writable(_metadata: &fs::Metadata) -> bool {
    false
}

fn canonical_manifest_hash(
    manifest: &PluginManifest,
    raw_hash: [u8; 32],
    origin: &ManifestOrigin,
) -> Result<[u8; 32], ManifestLoaderError> {
    let canonical = ManifestSigner::canonical_payload(manifest).map_err(|error| {
        validation_error(
            Some(origin.clone()),
            Some(manifest.name.to_string()),
            Some(manifest.version.to_string()),
            raw_hash,
            EventManifestValidationFailure::SchemaViolation {
                json_pointer: String::new(),
                details: format!("manifest canonicalization failed: {error}"),
            },
            format!("manifest canonicalization failed: {error}"),
        )
    })?;
    Ok(sha256(&canonical))
}

fn parse_manifest(
    path: &Path,
    bytes: &[u8],
    raw_hash: [u8; 32],
    origin: ManifestOrigin,
) -> Result<PluginManifest, ManifestLoaderError> {
    let extension = path.extension().and_then(std::ffi::OsStr::to_str);
    match extension {
        Some("json") => {
            let value = serde_json::from_slice(bytes).map_err(|error| {
                validation_error(
                    Some(origin.clone()),
                    partial_json_string(bytes, "name"),
                    partial_json_string(bytes, "version"),
                    raw_hash,
                    EventManifestValidationFailure::SyntaxError {
                        details: format!("json parse failed: {error}"),
                    },
                    format!("json parse failed: {error}"),
                )
            })?;
            decode_manifest_value(
                value,
                raw_hash,
                origin,
                partial_json_string(bytes, "name"),
                partial_json_string(bytes, "version"),
            )
        }
        Some("yaml" | "yml") => {
            let text = std::str::from_utf8(bytes).map_err(|error| {
                validation_error(
                    Some(origin.clone()),
                    None,
                    None,
                    raw_hash,
                    EventManifestValidationFailure::SyntaxError {
                        details: format!("yaml utf8 failed: {error}"),
                    },
                    format!("yaml utf8 failed: {error}"),
                )
            })?;
            let docs = YamlLoader::load_from_str(text).map_err(|error| {
                validation_error(
                    Some(origin.clone()),
                    partial_yaml_string(text, "name"),
                    partial_yaml_string(text, "version"),
                    raw_hash,
                    EventManifestValidationFailure::SyntaxError {
                        details: format!("yaml parse failed: {error}"),
                    },
                    format!("yaml parse failed: {error}"),
                )
            })?;
            let Some(document) = docs.first() else {
                return Err(validation_error(
                    Some(origin),
                    None,
                    None,
                    raw_hash,
                    EventManifestValidationFailure::SyntaxError {
                        details: "yaml document is empty".to_owned(),
                    },
                    "yaml document is empty".to_owned(),
                ));
            };
            let value = yaml_to_json(document).map_err(|details| {
                validation_error(
                    Some(origin.clone()),
                    None,
                    None,
                    raw_hash,
                    EventManifestValidationFailure::SchemaViolation {
                        json_pointer: String::new(),
                        details: format!("yaml convert failed: {details}"),
                    },
                    format!("yaml convert failed: {details}"),
                )
            })?;
            decode_manifest_value(
                value,
                raw_hash,
                origin,
                partial_yaml_string(text, "name"),
                partial_yaml_string(text, "version"),
            )
        }
        _ => Err(validation_error(
            Some(origin),
            None,
            None,
            raw_hash,
            EventManifestValidationFailure::SchemaViolation {
                json_pointer: String::new(),
                details: "unsupported manifest extension".to_owned(),
            },
            "unsupported manifest extension".to_owned(),
        )),
    }
}

fn decode_manifest_value(
    value: Value,
    raw_hash: [u8; 32],
    origin: ManifestOrigin,
    partial_name: Option<String>,
    partial_version: Option<String>,
) -> Result<PluginManifest, ManifestLoaderError> {
    validate_manifest_schema(
        &value,
        &origin,
        partial_name.as_ref(),
        partial_version.as_ref(),
        raw_hash,
    )?;
    serde_json::from_value(value).map_err(|error| {
        validation_error(
            Some(origin),
            partial_name,
            partial_version,
            raw_hash,
            EventManifestValidationFailure::SchemaViolation {
                json_pointer: String::new(),
                details: format!("manifest decode failed: {error}"),
            },
            format!("manifest decode failed: {error}"),
        )
    })
}

pub(crate) fn validate_manifest_schema(
    value: &Value,
    origin: &ManifestOrigin,
    partial_name: Option<&String>,
    partial_version: Option<&String>,
    raw_hash: [u8; 32],
) -> Result<(), ManifestLoaderError> {
    let schema = manifest_schema();
    let validator = jsonschema::validator_for(&schema).map_err(|error| {
        validation_error(
            Some(origin.clone()),
            partial_name.cloned(),
            partial_version.cloned(),
            raw_hash,
            EventManifestValidationFailure::SchemaViolation {
                json_pointer: String::new(),
                details: format!("manifest schema cannot compile: {error}"),
            },
            format!("manifest schema cannot compile: {error}"),
        )
    })?;
    if validator.is_valid(value) {
        return Ok(());
    }
    let details = validator.iter_errors(value).next().map_or_else(
        || "manifest schema violation".to_owned(),
        |error| error.to_string(),
    );
    Err(validation_error(
        Some(origin.clone()),
        partial_name.cloned(),
        partial_version.cloned(),
        raw_hash,
        EventManifestValidationFailure::SchemaViolation {
            json_pointer: String::new(),
            details: details.clone(),
        },
        details,
    ))
}

fn manifest_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "name": { "type": "string" },
            "version": { "type": "string" },
            "trust_level": { "type": "string", "enum": ["admin_trusted", "user_controlled"] },
            "description": { "type": ["string", "null"] },
            "authors": { "type": "array", "items": { "type": "string" } },
            "repository": { "type": ["string", "null"] },
            "signature": {
                "type": ["object", "null"],
                "additionalProperties": false,
                "properties": {
                    "algorithm": { "type": "string", "enum": ["ed25519", "rsa_pkcs1_sha256"] },
                    "signer": { "type": "string" },
                    "signature": { "type": "array", "items": { "type": "integer", "minimum": 0, "maximum": 255 } },
                    "timestamp": { "type": "string" }
                },
                "required": ["algorithm", "signer", "signature", "timestamp"]
            },
            "capabilities": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "tools": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "name": { "type": "string" },
                                "destructive": { "type": "boolean" },
                                "input_schema": {}
                            },
                            "required": ["name"]
                        }
                    },
                    "skills": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": { "name": { "type": "string" } },
                            "required": ["name"]
                        }
                    },
                    "hooks": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "name": { "type": "string" },
                                "events": { "type": "array", "items": { "type": "string" } }
                            },
                            "required": ["name"]
                        }
                    },
                    "mcp_servers": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": { "name": { "type": "string" } },
                            "required": ["name"]
                        }
                    },
                    "steering": { "type": "boolean" },
                    "custom_toolsets": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": { "name": { "type": "string" } },
                            "required": ["name"]
                        }
                    },
                    "memory_provider": {
                        "type": ["object", "null"],
                        "additionalProperties": false,
                        "properties": { "name": { "type": "string" } },
                        "required": ["name"]
                    },
                    "coordinator_strategy": {
                        "type": ["object", "null"],
                        "additionalProperties": false,
                        "properties": { "name": { "type": "string" } },
                        "required": ["name"]
                    },
                    "configuration_schema": {}
                }
            },
            "dependencies": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "name": { "type": "string" },
                        "version_req": { "type": "string" },
                        "kind": { "type": "string", "enum": ["required", "optional"] }
                    },
                    "required": ["name"]
                }
            },
            "min_harness_version": { "type": "string" }
        },
        "required": ["name", "version", "trust_level", "min_harness_version"]
    })
}

fn yaml_to_json(yaml: &Yaml) -> Result<Value, String> {
    match yaml {
        Yaml::Real(value) => value
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| format!("invalid real value: {value}")),
        Yaml::Integer(value) => Ok(Value::Number(Number::from(*value))),
        Yaml::String(value) => Ok(Value::String(value.clone())),
        Yaml::Boolean(value) => Ok(Value::Bool(*value)),
        Yaml::Array(values) => values
            .iter()
            .map(yaml_to_json)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Yaml::Hash(hash) => {
            let mut object = Map::new();
            for (key, value) in hash {
                let Yaml::String(key) = key else {
                    return Err("yaml object keys must be strings".to_owned());
                };
                object.insert(key.clone(), yaml_to_json(value)?);
            }
            Ok(Value::Object(object))
        }
        Yaml::Null => Ok(Value::Null),
        Yaml::BadValue | Yaml::Alias(_) => Err("unsupported yaml value".to_owned()),
    }
}

fn validation_error(
    origin: Option<ManifestOrigin>,
    partial_name: Option<String>,
    partial_version: Option<String>,
    raw_bytes_hash: [u8; 32],
    failure: EventManifestValidationFailure,
    details: String,
) -> ManifestLoaderError {
    ManifestLoaderError::Validation(ManifestValidationFailure {
        origin,
        partial_name,
        partial_version,
        raw_bytes_hash,
        failure,
        details,
    })
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = digest::digest(&digest::SHA256, bytes);
    let mut hash = [0_u8; 32];
    hash.copy_from_slice(digest.as_ref());
    hash
}

fn partial_json_string(bytes: &[u8], key: &str) -> Option<String> {
    let value = serde_json::from_slice::<Value>(bytes).ok()?;
    value.get(key)?.as_str().map(str::to_owned)
}

fn partial_yaml_string(text: &str, key: &str) -> Option<String> {
    let docs = YamlLoader::load_from_str(text).ok()?;
    let document = docs.first()?;
    let Yaml::Hash(hash) = document else {
        return None;
    };
    hash.iter().find_map(|(candidate, value)| {
        let Yaml::String(candidate) = candidate else {
            return None;
        };
        if candidate != key {
            return None;
        }
        let Yaml::String(value) = value else {
            return None;
        };
        Some(value.clone())
    })
}
