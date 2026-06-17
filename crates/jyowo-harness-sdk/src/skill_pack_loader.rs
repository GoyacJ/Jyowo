use std::fmt;
use std::path::PathBuf;

use harness_contracts::SkillId;
use harness_skill::{
    parse_skill_markdown_with_options, Skill, SkillCompatMode, SkillConfigDecl, SkillError,
    SkillParamType, SkillParameter, SkillPlatform, SkillSource,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedSkillVersionSnapshot {
    pub source: String,
    pub skill_id: String,
    pub skill_version_id: String,
    pub semantic_version: String,
    pub name: String,
    pub pack_hash: String,
    pub manifest_hash: String,
    pub permissions_hash: String,
    #[serde(default)]
    pub manifest: Value,
    #[serde(default)]
    pub permissions_summary: Value,
    #[serde(default)]
    pub files: Vec<LockedSkillPackFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedSkillPackFile {
    pub path: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub content_hash: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct SkillPackLoaderAdapter {
    runtime_platform: SkillPlatform,
    compat_mode: SkillCompatMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillPackLoaderError {
    MissingRequiredFile(String),
    ParseSkill(String),
    Registry(String),
}

impl Default for SkillPackLoaderAdapter {
    fn default() -> Self {
        Self {
            runtime_platform: current_platform(),
            compat_mode: SkillCompatMode::Lenient,
        }
    }
}

impl SkillPackLoaderAdapter {
    #[must_use]
    pub fn with_runtime_platform(mut self, platform: SkillPlatform) -> Self {
        self.runtime_platform = platform;
        self
    }

    #[must_use]
    pub fn with_compat_mode(mut self, compat_mode: SkillCompatMode) -> Self {
        self.compat_mode = compat_mode;
        self
    }

    pub fn load_skill(
        &self,
        snapshot: &LockedSkillVersionSnapshot,
    ) -> Result<Skill, SkillPackLoaderError> {
        require_pack_file(snapshot, "manifest.yaml")?;
        require_pack_file(snapshot, "permissions.yaml")?;
        let entry = manifest_entry(&snapshot.manifest).unwrap_or("SKILL.md");
        let skill_md = require_pack_file(snapshot, entry)?;
        let source_path = workspace_source_path(snapshot, entry);
        let source = SkillSource::Workspace(source_path.clone());
        let mut skill = parse_skill_markdown_with_options(
            &skill_md.content,
            source,
            Some(PathBuf::from(entry)),
            self.runtime_platform,
            self.compat_mode,
        )
        .map_err(skill_parse_error)?;

        apply_manifest_projection(&mut skill, snapshot);
        skill.id = SkillId(format!(
            "workspace:{}:{}",
            snapshot.skill_id, snapshot.skill_version_id
        ));
        skill.raw_path = Some(source_path);
        Ok(skill)
    }

    pub fn load_skills(
        &self,
        snapshots: &[LockedSkillVersionSnapshot],
    ) -> Result<Vec<Skill>, SkillPackLoaderError> {
        snapshots
            .iter()
            .map(|snapshot| self.load_skill(snapshot))
            .collect()
    }
}

impl fmt::Display for SkillPackLoaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequiredFile(path) => {
                write!(formatter, "missing required skill pack file `{path}`")
            }
            Self::ParseSkill(message) => write!(formatter, "parse locked skill pack: {message}"),
            Self::Registry(message) => write!(formatter, "register locked skill pack: {message}"),
        }
    }
}

impl std::error::Error for SkillPackLoaderError {}

fn require_pack_file<'a>(
    snapshot: &'a LockedSkillVersionSnapshot,
    path: &str,
) -> Result<&'a LockedSkillPackFile, SkillPackLoaderError> {
    snapshot
        .files
        .iter()
        .find(|file| file.path == path)
        .filter(|file| !file.content.trim().is_empty())
        .ok_or_else(|| SkillPackLoaderError::MissingRequiredFile(path.to_owned()))
}

fn manifest_entry(manifest: &Value) -> Option<&str> {
    manifest
        .get("entry")
        .and_then(Value::as_str)
        .filter(|entry| !entry.trim().is_empty())
}

fn workspace_source_path(snapshot: &LockedSkillVersionSnapshot, entry: &str) -> PathBuf {
    PathBuf::from("business-skill")
        .join(&snapshot.skill_id)
        .join(&snapshot.skill_version_id)
        .join(entry)
}

fn apply_manifest_projection(skill: &mut Skill, snapshot: &LockedSkillVersionSnapshot) {
    if !snapshot.name.trim().is_empty() {
        skill.name = snapshot.name.clone();
        skill.frontmatter.name = snapshot.name.clone();
    } else if let Some(name) = string_field(&snapshot.manifest, "name") {
        skill.name = name.to_owned();
        skill.frontmatter.name = name.to_owned();
    }

    if let Some(description) = string_field(&snapshot.manifest, "description") {
        skill.description = description.to_owned();
        skill.frontmatter.description = description.to_owned();
    }
    if let Some(category) = string_field(&snapshot.manifest, "category") {
        skill.frontmatter.category = Some(category.to_owned());
    }
    if let Some(tags) = string_array_field(&snapshot.manifest, "tags") {
        if !tags.is_empty() {
            skill.frontmatter.tags = tags;
        }
    }

    merge_manifest_parameters(skill, &snapshot.manifest);
    merge_manifest_config(skill, &snapshot.manifest);
    merge_manifest_metadata(skill, snapshot);
}

fn merge_manifest_parameters(skill: &mut Skill, manifest: &Value) {
    let Some(items) = manifest.get("parameters").and_then(Value::as_array) else {
        return;
    };
    let existing = skill
        .frontmatter
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect::<Vec<_>>();
    for item in items {
        let Some(parameter) = parameter_from_manifest(item) else {
            continue;
        };
        if !existing.iter().any(|name| name == &parameter.name) {
            skill.frontmatter.parameters.push(parameter);
        }
    }
}

fn merge_manifest_config(skill: &mut Skill, manifest: &Value) {
    let Some(items) = manifest.get("config").and_then(Value::as_array) else {
        return;
    };
    let existing = skill
        .frontmatter
        .config
        .iter()
        .map(|config| config.key.clone())
        .collect::<Vec<_>>();
    for item in items {
        let Some(config) = config_from_manifest(item) else {
            continue;
        };
        if !existing.iter().any(|key| key == &config.key) {
            skill.frontmatter.config.push(config);
        }
    }
}

fn merge_manifest_metadata(skill: &mut Skill, snapshot: &LockedSkillVersionSnapshot) {
    if let Some(metadata) = snapshot.manifest.get("metadata").and_then(Value::as_object) {
        for (key, value) in metadata {
            skill
                .frontmatter
                .metadata
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }

    let mut jyowo = match skill.frontmatter.metadata.remove("jyowo") {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    jyowo.insert("source".to_owned(), json!(snapshot.source));
    jyowo.insert("skill_id".to_owned(), json!(snapshot.skill_id));
    jyowo.insert(
        "skill_version_id".to_owned(),
        json!(snapshot.skill_version_id),
    );
    jyowo.insert(
        "semantic_version".to_owned(),
        json!(snapshot.semantic_version),
    );
    jyowo.insert("pack_hash".to_owned(), json!(snapshot.pack_hash));
    jyowo.insert("manifest_hash".to_owned(), json!(snapshot.manifest_hash));
    jyowo.insert(
        "permissions_hash".to_owned(),
        json!(snapshot.permissions_hash),
    );
    jyowo.insert("manifest".to_owned(), snapshot.manifest.clone());
    jyowo.insert(
        "permissions_summary".to_owned(),
        snapshot.permissions_summary.clone(),
    );
    skill
        .frontmatter
        .metadata
        .insert("jyowo".to_owned(), Value::Object(jyowo));
}

fn parameter_from_manifest(value: &Value) -> Option<SkillParameter> {
    let name = string_field(value, "name")?;
    let param_type = param_type_from_value(value.get("type")).unwrap_or(SkillParamType::String);
    let default = value.get("default").cloned();
    if default
        .as_ref()
        .is_some_and(|default| !value_matches_type(default, param_type))
    {
        return None;
    }
    Some(SkillParameter {
        name: name.to_owned(),
        param_type,
        required: bool_field(value, "required"),
        default,
        description: string_field(value, "description").map(ToOwned::to_owned),
    })
}

fn config_from_manifest(value: &Value) -> Option<SkillConfigDecl> {
    let key = string_field(value, "key")?;
    let value_type = param_type_from_value(value.get("type")).unwrap_or(SkillParamType::String);
    let secret = bool_field(value, "secret");
    let default = value.get("default").cloned();
    if secret && default.is_some() {
        return None;
    }
    if default
        .as_ref()
        .is_some_and(|default| !value_matches_type(default, value_type))
    {
        return None;
    }
    Some(SkillConfigDecl {
        key: key.to_owned(),
        value_type,
        secret,
        required: bool_field(value, "required"),
        default,
        description: string_field(value, "description").map(ToOwned::to_owned),
    })
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn string_array_field(value: &Value, key: &str) -> Option<Vec<String>> {
    value.get(key).and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .filter(|item| !item.trim().is_empty())
            .map(ToOwned::to_owned)
            .collect()
    })
}

fn param_type_from_value(value: Option<&Value>) -> Option<SkillParamType> {
    match value.and_then(Value::as_str).unwrap_or("string") {
        "string" => Some(SkillParamType::String),
        "number" => Some(SkillParamType::Number),
        "boolean" => Some(SkillParamType::Boolean),
        "path" => Some(SkillParamType::Path),
        "url" => Some(SkillParamType::Url),
        _ => None,
    }
}

fn value_matches_type(value: &Value, param_type: SkillParamType) -> bool {
    match param_type {
        SkillParamType::String | SkillParamType::Path | SkillParamType::Url => value.is_string(),
        SkillParamType::Number => value.is_number(),
        SkillParamType::Boolean => value.is_boolean(),
    }
}

fn skill_parse_error(error: SkillError) -> SkillPackLoaderError {
    SkillPackLoaderError::ParseSkill(error.to_string())
}

fn current_platform() -> SkillPlatform {
    if cfg!(target_os = "macos") {
        SkillPlatform::Macos
    } else if cfg!(target_os = "windows") {
        SkillPlatform::Windows
    } else {
        SkillPlatform::Linux
    }
}
