use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use harness_contracts::{HookEventKind, HookFailureMode, SkillId};
use serde_json::{Map, Number, Value};
use yaml_rust2::{Yaml, YamlLoader};

use crate::{
    skill_script_path_has_reserved_component, BuiltinHookKind, Skill, SkillConfigDecl, SkillError,
    SkillFrontmatter, SkillHookDecl, SkillHookExecSpec, SkillHookHttpSecuritySpec,
    SkillHookHttpSpec, SkillHookTransport, SkillParamType, SkillParameter, SkillPlatform,
    SkillPrerequisites, SkillScriptDecl, SkillScriptEnvDecl, SkillScriptNetworkPolicy, SkillSource,
    DEFAULT_SKILL_SCRIPT_ARTIFACT_BYTES, DEFAULT_SKILL_SCRIPT_ARTIFACT_COUNT,
    DEFAULT_SKILL_SCRIPT_OUTPUT_BYTES, DEFAULT_SKILL_SCRIPT_STDERR_BYTES,
    DEFAULT_SKILL_SCRIPT_STDOUT_BYTES, DEFAULT_SKILL_SCRIPT_TIMEOUT_SECONDS,
    MAX_SKILL_SCRIPT_ARTIFACT_BYTES, MAX_SKILL_SCRIPT_ARTIFACT_COUNT,
    MAX_SKILL_SCRIPT_OUTPUT_BYTES, MAX_SKILL_SCRIPT_STREAM_BYTES, MAX_SKILL_SCRIPT_TIMEOUT_SECONDS,
};

pub fn parse_skill_markdown(
    markdown: &str,
    source: SkillSource,
    raw_path: Option<PathBuf>,
    runtime_platform: SkillPlatform,
) -> Result<Skill, SkillError> {
    let (frontmatter_yaml, body) = split_frontmatter(markdown)?;
    let docs = YamlLoader::load_from_str(frontmatter_yaml)
        .map_err(|error| SkillError::ParseFrontmatter(error.to_string()))?;
    let yaml = docs.first().unwrap_or(&Yaml::BadValue);
    let frontmatter = parse_frontmatter(yaml)?;

    if frontmatter.name.chars().count() > 64 {
        return Err(SkillError::NameTooLong(frontmatter.name.chars().count()));
    }
    if frontmatter.description.chars().count() > 1024 {
        return Err(SkillError::DescriptionTooLong(
            frontmatter.description.chars().count(),
        ));
    }
    if !frontmatter.platforms.is_empty() && !frontmatter.platforms.contains(&runtime_platform) {
        return Err(SkillError::PlatformMismatch {
            required: frontmatter.platforms.clone(),
        });
    }

    let name = frontmatter.name.clone();
    let description = frontmatter.description.clone();
    Ok(Skill {
        id: SkillId(format!("{}:{name}", source_label(&source))),
        name,
        description,
        source,
        frontmatter,
        body: body.trim_start_matches('\n').to_owned(),
        raw_path,
    })
}

fn split_frontmatter(markdown: &str) -> Result<(&str, &str), SkillError> {
    let markdown = markdown.strip_prefix("---\n").ok_or_else(|| {
        SkillError::ParseFrontmatter("missing opening frontmatter delimiter".to_owned())
    })?;
    let Some((frontmatter, body)) = markdown.split_once("\n---") else {
        return Err(SkillError::ParseFrontmatter(
            "missing closing frontmatter delimiter".to_owned(),
        ));
    };
    Ok((
        frontmatter,
        body.trim_start_matches("\r\n").trim_start_matches('\n'),
    ))
}

fn parse_frontmatter(yaml: &Yaml) -> Result<SkillFrontmatter, SkillError> {
    reject_unknown_top_level_fields(yaml)?;
    let name = required_string(yaml, "name")?;
    let description = required_string(yaml, "description")?;
    let metadata = yaml_to_map(yaml_hash_get(yaml, "metadata").unwrap_or(&Yaml::BadValue));
    let jyowo_meta = yaml_hash_get(
        yaml_hash_get(yaml, "metadata").unwrap_or(&Yaml::BadValue),
        "jyowo",
    );

    let tags = string_vec(yaml_hash_get(yaml, "tags"))
        .or_else(|| jyowo_meta.and_then(|meta| string_vec(yaml_hash_get(meta, "tags"))))
        .unwrap_or_default();
    let category = optional_string(yaml_hash_get(yaml, "category"))
        .or_else(|| jyowo_meta.and_then(|meta| optional_string(yaml_hash_get(meta, "category"))));

    let config = parse_config(yaml_hash_get(yaml, "config"))?;
    let scripts = parse_scripts(yaml_hash_get(yaml, "scripts"), &config)?;

    Ok(SkillFrontmatter {
        name,
        description,
        allowlist_agents: string_vec(yaml_hash_get(yaml, "allowlist_agents")),
        parameters: parse_parameters(yaml_hash_get(yaml, "parameters"))?,
        config,
        scripts,
        platforms: parse_platforms(yaml_hash_get(yaml, "platforms"))?,
        prerequisites: parse_prerequisites(yaml_hash_get(yaml, "prerequisites")),
        hooks: parse_hooks(yaml_hash_get(yaml, "hooks"))?,
        tags,
        category,
        metadata,
    })
}

fn parse_scripts(
    yaml: Option<&Yaml>,
    config: &[SkillConfigDecl],
) -> Result<Vec<SkillScriptDecl>, SkillError> {
    let Some(yaml) = yaml else {
        return Ok(Vec::new());
    };
    let Yaml::Array(items) = yaml else {
        return Err(script_error("scripts must be a list"));
    };
    let config_by_key = config
        .iter()
        .map(|decl| (decl.key.as_str(), decl))
        .collect::<HashMap<_, _>>();
    let mut ids = HashSet::new();
    let mut scripts = Vec::with_capacity(items.len());

    for item in items {
        reject_unknown_fields(
            item,
            &[
                "id",
                "path",
                "timeoutSeconds",
                "network",
                "env",
                "maxStdoutBytes",
                "maxStderrBytes",
                "maxOutputBytes",
                "maxArtifactCount",
                "maxArtifactBytes",
            ],
            "script",
        )?;
        let id = required_script_string(item, "id")?;
        if id.trim().is_empty() {
            return Err(script_error("script id must not be empty"));
        }
        if !ids.insert(id.clone()) {
            return Err(script_error(format!("duplicate script id: {id}")));
        }
        let path_value = required_script_string(item, "path")?;
        let path = relative_package_path(&path_value)?;
        let network = parse_script_network(yaml_hash_get(item, "network"))?;
        let env = parse_script_env(yaml_hash_get(item, "env"), &id, &config_by_key)?;

        scripts.push(SkillScriptDecl {
            id,
            path,
            timeout_seconds: bounded_script_u64(
                item,
                "timeoutSeconds",
                DEFAULT_SKILL_SCRIPT_TIMEOUT_SECONDS,
                MAX_SKILL_SCRIPT_TIMEOUT_SECONDS,
            )?,
            network,
            env,
            max_stdout_bytes: bounded_script_u64(
                item,
                "maxStdoutBytes",
                DEFAULT_SKILL_SCRIPT_STDOUT_BYTES,
                MAX_SKILL_SCRIPT_STREAM_BYTES,
            )?,
            max_stderr_bytes: bounded_script_u64(
                item,
                "maxStderrBytes",
                DEFAULT_SKILL_SCRIPT_STDERR_BYTES,
                MAX_SKILL_SCRIPT_STREAM_BYTES,
            )?,
            max_output_bytes: bounded_script_u64(
                item,
                "maxOutputBytes",
                DEFAULT_SKILL_SCRIPT_OUTPUT_BYTES,
                MAX_SKILL_SCRIPT_OUTPUT_BYTES,
            )?,
            max_artifact_count: bounded_script_u64(
                item,
                "maxArtifactCount",
                DEFAULT_SKILL_SCRIPT_ARTIFACT_COUNT,
                MAX_SKILL_SCRIPT_ARTIFACT_COUNT,
            )?,
            max_artifact_bytes: bounded_script_u64(
                item,
                "maxArtifactBytes",
                DEFAULT_SKILL_SCRIPT_ARTIFACT_BYTES,
                MAX_SKILL_SCRIPT_ARTIFACT_BYTES,
            )?,
        });
    }
    Ok(scripts)
}

fn parse_script_network(yaml: Option<&Yaml>) -> Result<SkillScriptNetworkPolicy, SkillError> {
    let Some(yaml) = yaml else {
        return Ok(SkillScriptNetworkPolicy::Deny);
    };
    let Some(value) = yaml.as_str() else {
        return Err(script_error("script network policy must be a string"));
    };
    match value {
        "deny" => Ok(SkillScriptNetworkPolicy::Deny),
        _ => Err(script_error(format!(
            "unsupported script network policy: {value}"
        ))),
    }
}

fn parse_script_env(
    yaml: Option<&Yaml>,
    script_id: &str,
    config_by_key: &HashMap<&str, &SkillConfigDecl>,
) -> Result<BTreeMap<String, SkillScriptEnvDecl>, SkillError> {
    let Some(yaml) = yaml else {
        return Ok(BTreeMap::new());
    };
    let Yaml::Hash(entries) = yaml else {
        return Err(script_error(format!(
            "script `{script_id}` env must be a mapping"
        )));
    };
    let mut env = BTreeMap::new();
    for (env_name, value) in entries {
        let Some(env_name) = env_name.as_str() else {
            return Err(script_error(format!(
                "script `{script_id}` env name must be a string"
            )));
        };
        reject_unknown_fields(value, &["config", "secret"], "script env")?;
        let config_key = required_script_string(value, "config")?;
        let config = config_by_key.get(config_key.as_str()).ok_or_else(|| {
            script_error(format!(
                "script `{script_id}` env `{env_name}` references unknown config `{config_key}`"
            ))
        })?;
        let secret = optional_bool(yaml_hash_get(value, "secret")).unwrap_or(false);
        if secret && !config.secret {
            return Err(script_error(format!(
                "script `{script_id}` env `{env_name}` must reference config declared secret"
            )));
        }
        if config.secret && !secret {
            return Err(script_error(format!(
                "script `{script_id}` env `{env_name}` must set secret: true"
            )));
        }
        if env
            .insert(
                env_name.to_owned(),
                SkillScriptEnvDecl {
                    config: config_key,
                    secret,
                },
            )
            .is_some()
        {
            return Err(script_error(format!(
                "script `{script_id}` has duplicate env `{env_name}`"
            )));
        }
    }
    Ok(env)
}

fn bounded_script_u64(yaml: &Yaml, field: &str, default: u64, max: u64) -> Result<u64, SkillError> {
    let Some(value) = yaml_hash_get(yaml, field) else {
        return Ok(default);
    };
    let value = optional_u64(Some(value)).ok_or_else(|| {
        script_error(format!(
            "script {field} must be an integer between 1 and {max}"
        ))
    })?;
    if !(1..=max).contains(&value) {
        return Err(script_error(format!(
            "script {field} must be between 1 and {max}"
        )));
    }
    Ok(value)
}

fn relative_package_path(value: &str) -> Result<PathBuf, SkillError> {
    let normalized_value = value.replace('\\', "/");
    let path = Path::new(&normalized_value);
    let windows_absolute = normalized_value
        .as_bytes()
        .get(1)
        .is_some_and(|value| *value == b':');
    if path.is_absolute() || windows_absolute || normalized_value.starts_with("//") {
        return Err(script_error(format!(
            "script path `{value}` must be a relative package path"
        )));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(script_error(format!(
                    "script path `{value}` must be a relative package path"
                )));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(script_error(format!(
            "script path `{value}` must be a relative package path"
        )));
    }
    if skill_script_path_has_reserved_component(&normalized) {
        return Err(script_error(format!(
            "script path `{value}` uses reserved .jyowo- path component"
        )));
    }
    Ok(normalized)
}

fn reject_unknown_fields(yaml: &Yaml, allowed: &[&str], subject: &str) -> Result<(), SkillError> {
    let Yaml::Hash(hash) = yaml else {
        return Err(script_error(format!("{subject} must be a mapping")));
    };
    for (key, _) in hash {
        let Some(key) = key.as_str() else {
            return Err(script_error(format!(
                "{subject} field name must be a string"
            )));
        };
        if !allowed.contains(&key) {
            return Err(script_error(format!("unknown {subject} field: {key}")));
        }
    }
    Ok(())
}

fn required_script_string(yaml: &Yaml, key: &str) -> Result<String, SkillError> {
    optional_string(yaml_hash_get(yaml, key))
        .ok_or_else(|| script_error(format!("missing required script field: {key}")))
}

fn script_error(message: impl Into<String>) -> SkillError {
    SkillError::InvalidScriptDeclaration(message.into())
}

fn parse_parameters(yaml: Option<&Yaml>) -> Result<Vec<SkillParameter>, SkillError> {
    let Some(Yaml::Array(items)) = yaml else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|item| {
            let param_type = optional_string(yaml_hash_get(item, "type"))
                .as_deref()
                .and_then(SkillParamType::parse)
                .unwrap_or(SkillParamType::String);
            let default = yaml_hash_get(item, "default").map(yaml_to_json);
            if let Some(value) = &default {
                validate_json_type(value, param_type).map_err(|expected| {
                    SkillError::ParseFrontmatter(format!(
                        "parameter `{}` default must be {expected}",
                        required_string(item, "name").unwrap_or_else(|_| "<unknown>".to_owned())
                    ))
                })?;
            }
            Ok(SkillParameter {
                name: required_string(item, "name")?,
                param_type,
                required: optional_bool(yaml_hash_get(item, "required")).unwrap_or(false),
                default,
                description: optional_string(yaml_hash_get(item, "description")),
            })
        })
        .collect()
}

fn parse_config(yaml: Option<&Yaml>) -> Result<Vec<SkillConfigDecl>, SkillError> {
    let Some(Yaml::Array(items)) = yaml else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|item| {
            let value_type = optional_string(yaml_hash_get(item, "type"))
                .as_deref()
                .and_then(SkillParamType::parse)
                .unwrap_or(SkillParamType::String);
            let key = required_string(item, "key")?;
            let secret = optional_bool(yaml_hash_get(item, "secret")).unwrap_or(false);
            let default = yaml_hash_get(item, "default").map(yaml_to_json);
            if secret && default.is_some() {
                return Err(SkillError::ParseFrontmatter(format!(
                    "secret config `{key}` cannot declare a default"
                )));
            }
            if let Some(value) = &default {
                validate_json_type(value, value_type).map_err(|expected| {
                    SkillError::ParseFrontmatter(format!(
                        "config `{key}` default must be {expected}"
                    ))
                })?;
            }
            Ok(SkillConfigDecl {
                key,
                value_type,
                secret,
                required: optional_bool(yaml_hash_get(item, "required")).unwrap_or(false),
                default,
                description: optional_string(yaml_hash_get(item, "description")),
            })
        })
        .collect()
}

fn parse_platforms(yaml: Option<&Yaml>) -> Result<Vec<SkillPlatform>, SkillError> {
    let Some(Yaml::Array(items)) = yaml else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .filter_map(|item| item.as_str())
        .map(|value| {
            SkillPlatform::parse(value)
                .ok_or_else(|| SkillError::ParseFrontmatter(format!("unknown platform: {value}")))
        })
        .collect()
}

fn parse_prerequisites(yaml: Option<&Yaml>) -> SkillPrerequisites {
    let Some(yaml) = yaml else {
        return SkillPrerequisites::default();
    };
    SkillPrerequisites {
        env_vars: string_vec(yaml_hash_get(yaml, "env_vars")).unwrap_or_default(),
        commands: string_vec(yaml_hash_get(yaml, "commands")).unwrap_or_default(),
    }
}

fn parse_hooks(yaml: Option<&Yaml>) -> Result<Vec<SkillHookDecl>, SkillError> {
    let Some(Yaml::Array(items)) = yaml else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|item| {
            let events = parse_hook_events(yaml_hash_get(item, "events"))?;
            if events.is_empty() {
                return Err(SkillError::ParseFrontmatter(format!(
                    "hook `{}` must declare at least one event",
                    required_string(item, "id").unwrap_or_else(|_| "<unknown>".to_owned())
                )));
            }
            let id = required_string(item, "id")?;
            let transport = parse_hook_transport(yaml_hash_get(item, "transport"))?;
            Ok(SkillHookDecl {
                id,
                events,
                transport,
            })
        })
        .collect()
}

fn parse_hook_transport(yaml: Option<&Yaml>) -> Result<SkillHookTransport, SkillError> {
    let Some(yaml) = yaml else {
        return Err(SkillError::ParseFrontmatter(
            "hook requires transport".to_owned(),
        ));
    };
    let transport_type = optional_string(yaml_hash_get(yaml, "type"))
        .ok_or_else(|| SkillError::ParseFrontmatter("hook transport missing type".to_owned()))?;
    match transport_type.as_str() {
        "builtin" => {
            let kind = optional_string(yaml_hash_get(yaml, "kind"))
                .unwrap_or_else(|| "AuditLog".to_owned());
            let kind = match kind.as_str() {
                "AuditLog" | "audit_log" => BuiltinHookKind::AuditLog,
                _ => {
                    return Err(SkillError::ParseFrontmatter(format!(
                        "unknown builtin hook kind: {kind}"
                    )));
                }
            };
            Ok(SkillHookTransport::Builtin(kind))
        }
        "exec" => Ok(SkillHookTransport::Exec(SkillHookExecSpec {
            command: PathBuf::from(required_string(yaml, "command")?),
            args: string_vec(yaml_hash_get(yaml, "args")).unwrap_or_default(),
            timeout_ms: optional_u64(yaml_hash_get(yaml, "timeout_ms")).unwrap_or(5_000),
            failure_mode: parse_failure_mode(yaml_hash_get(yaml, "failure_mode"))?,
        })),
        "http" => {
            let security = parse_http_security(yaml)?;
            Ok(SkillHookTransport::Http(SkillHookHttpSpec {
                url: required_string(yaml, "url")?,
                timeout_ms: optional_u64(yaml_hash_get(yaml, "timeout_ms")).unwrap_or(5_000),
                allowlist: security.allowlist.clone(),
                security,
                failure_mode: parse_failure_mode(yaml_hash_get(yaml, "failure_mode"))?,
            }))
        }
        _ => Err(SkillError::ParseFrontmatter(format!(
            "unknown hook transport: {transport_type}"
        ))),
    }
}

fn parse_http_security(yaml: &Yaml) -> Result<SkillHookHttpSecuritySpec, SkillError> {
    let security_yaml = yaml_hash_get(yaml, "security").ok_or_else(|| {
        SkillError::ParseFrontmatter("http hook requires transport.security".to_owned())
    })?;
    let mut security = SkillHookHttpSecuritySpec::default();
    security.allowlist = string_vec(yaml_hash_get(security_yaml, "allowlist")).unwrap_or_default();
    security.ssrf_guard = optional_bool(yaml_hash_get(security_yaml, "ssrf_guard")).unwrap_or(true);
    security.max_redirects =
        optional_u64(yaml_hash_get(security_yaml, "max_redirects")).unwrap_or(0) as usize;
    security.max_body_bytes =
        optional_u64(yaml_hash_get(security_yaml, "max_body_bytes")).unwrap_or(1024 * 1024);
    security.mtls_required =
        optional_bool(yaml_hash_get(security_yaml, "mtls_required")).unwrap_or(false);
    Ok(security)
}

fn parse_failure_mode(yaml: Option<&Yaml>) -> Result<HookFailureMode, SkillError> {
    let Some(value) = optional_string(yaml) else {
        return Ok(HookFailureMode::FailOpen);
    };
    match value.as_str() {
        "fail_open" | "FailOpen" => Ok(HookFailureMode::FailOpen),
        "fail_closed" | "FailClosed" => Ok(HookFailureMode::FailClosed),
        _ => Err(SkillError::ParseFrontmatter(format!(
            "unknown hook failure mode: {value}"
        ))),
    }
}

fn parse_hook_events(yaml: Option<&Yaml>) -> Result<Vec<HookEventKind>, SkillError> {
    let Some(Yaml::Array(items)) = yaml else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|item| {
            let value = item.as_str().ok_or_else(|| {
                SkillError::ParseFrontmatter("hook event must be a string".to_owned())
            })?;
            parse_hook_event_kind(value)
        })
        .collect()
}

fn parse_hook_event_kind(value: &str) -> Result<HookEventKind, SkillError> {
    match value {
        "UserPromptSubmit" | "user_prompt_submit" => Ok(HookEventKind::UserPromptSubmit),
        "PreToolUse" | "pre_tool_use" => Ok(HookEventKind::PreToolUse),
        "PostToolUse" | "post_tool_use" => Ok(HookEventKind::PostToolUse),
        "PostToolUseFailure" | "post_tool_use_failure" => Ok(HookEventKind::PostToolUseFailure),
        "PermissionRequest" | "permission_request" => Ok(HookEventKind::PermissionRequest),
        "SessionStart" | "session_start" => Ok(HookEventKind::SessionStart),
        "Setup" | "setup" => Ok(HookEventKind::Setup),
        "SessionEnd" | "session_end" => Ok(HookEventKind::SessionEnd),
        "SubagentStart" | "subagent_start" => Ok(HookEventKind::SubagentStart),
        "SubagentStop" | "subagent_stop" => Ok(HookEventKind::SubagentStop),
        "Notification" | "notification" => Ok(HookEventKind::Notification),
        "PreLlmCall" | "pre_llm_call" => Ok(HookEventKind::PreLlmCall),
        "PostLlmCall" | "post_llm_call" => Ok(HookEventKind::PostLlmCall),
        "PreApiRequest" | "pre_api_request" => Ok(HookEventKind::PreApiRequest),
        "PostApiRequest" | "post_api_request" => Ok(HookEventKind::PostApiRequest),
        "TransformToolResult" | "transform_tool_result" => Ok(HookEventKind::TransformToolResult),
        "TransformTerminalOutput" | "transform_terminal_output" => {
            Ok(HookEventKind::TransformTerminalOutput)
        }
        "Elicitation" | "elicitation" => Ok(HookEventKind::Elicitation),
        "PreToolSearch" | "pre_tool_search" => Ok(HookEventKind::PreToolSearch),
        "PostToolSearchMaterialize" | "post_tool_search_materialize" => {
            Ok(HookEventKind::PostToolSearchMaterialize)
        }
        _ => Err(SkillError::ParseFrontmatter(format!(
            "unknown hook event: {value}"
        ))),
    }
}

fn yaml_hash_get<'a>(yaml: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    let Yaml::Hash(hash) = yaml else {
        return None;
    };
    hash.get(&Yaml::String(key.to_owned()))
}

fn required_string(yaml: &Yaml, key: &str) -> Result<String, SkillError> {
    optional_string(yaml_hash_get(yaml, key))
        .ok_or_else(|| SkillError::ParseFrontmatter(format!("missing required field: {key}")))
}

fn optional_string(yaml: Option<&Yaml>) -> Option<String> {
    yaml.and_then(Yaml::as_str).map(ToOwned::to_owned)
}

fn optional_bool(yaml: Option<&Yaml>) -> Option<bool> {
    yaml.and_then(Yaml::as_bool)
}

fn optional_u64(yaml: Option<&Yaml>) -> Option<u64> {
    match yaml? {
        Yaml::Integer(value) => u64::try_from(*value).ok(),
        Yaml::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn string_vec(yaml: Option<&Yaml>) -> Option<Vec<String>> {
    match yaml? {
        Yaml::Array(values) => Some(
            values
                .iter()
                .filter_map(Yaml::as_str)
                .map(ToOwned::to_owned)
                .collect(),
        ),
        Yaml::String(value) => Some(vec![value.clone()]),
        _ => None,
    }
}

fn yaml_to_map(yaml: &Yaml) -> HashMap<String, Value> {
    let Value::Object(map) = yaml_to_json(yaml) else {
        return HashMap::new();
    };
    map.into_iter().collect()
}

fn yaml_to_json(yaml: &Yaml) -> Value {
    match yaml {
        Yaml::Real(value) => value
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Yaml::Integer(value) => Value::Number(Number::from(*value)),
        Yaml::String(value) => Value::String(value.clone()),
        Yaml::Boolean(value) => Value::Bool(*value),
        Yaml::Array(values) => Value::Array(values.iter().map(yaml_to_json).collect()),
        Yaml::Hash(hash) => {
            let mut map = Map::new();
            for (key, value) in hash {
                if let Some(key) = key.as_str() {
                    map.insert(key.to_owned(), yaml_to_json(value));
                }
            }
            Value::Object(map)
        }
        Yaml::Null | Yaml::BadValue | Yaml::Alias(_) => Value::Null,
    }
}

fn validate_json_type(value: &Value, param_type: SkillParamType) -> Result<(), &'static str> {
    match param_type {
        SkillParamType::String | SkillParamType::Path | SkillParamType::Url => value
            .as_str()
            .map(|_| ())
            .ok_or(param_type_name(param_type)),
        SkillParamType::Number => value.as_f64().map(|_| ()).ok_or("number"),
        SkillParamType::Boolean => value.as_bool().map(|_| ()).ok_or("boolean"),
    }
}

fn param_type_name(param_type: SkillParamType) -> &'static str {
    match param_type {
        SkillParamType::String => "string",
        SkillParamType::Number => "number",
        SkillParamType::Boolean => "boolean",
        SkillParamType::Path => "path string",
        SkillParamType::Url => "url string",
    }
}

fn reject_unknown_top_level_fields(yaml: &Yaml) -> Result<(), SkillError> {
    const ALLOWED: &[&str] = &[
        "name",
        "description",
        "allowlist_agents",
        "parameters",
        "config",
        "scripts",
        "platforms",
        "prerequisites",
        "hooks",
        "tags",
        "category",
        "metadata",
    ];
    let Yaml::Hash(hash) = yaml else {
        return Ok(());
    };
    for (key, _) in hash {
        let Some(key) = key.as_str() else {
            continue;
        };
        if !ALLOWED.contains(&key) {
            return Err(SkillError::ParseFrontmatter(format!(
                "unknown top-level frontmatter field: {key}"
            )));
        }
    }
    Ok(())
}

fn source_label(source: &SkillSource) -> &'static str {
    match source {
        SkillSource::Bundled => "bundled",
        SkillSource::Workspace(_) => "workspace",
        SkillSource::User(_) => "user",
        SkillSource::Plugin { .. } => "plugin",
        SkillSource::Mcp(_) => "mcp",
    }
}
