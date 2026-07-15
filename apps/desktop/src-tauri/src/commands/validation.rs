#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
use super::contracts::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::plugins::*;
#[allow(unused_imports)]
use super::providers::*;
#[allow(unused_imports)]
use super::runtime::*;
#[allow(unused_imports)]
use super::skills::*;
#[allow(unused_imports)]
use super::stores::*;
use super::*;

pub(crate) fn ensure_non_empty(
    field: &'static str,
    value: &str,
) -> Result<(), CommandErrorPayload> {
    if value.trim().is_empty() {
        return Err(invalid_payload(format!("{field} must not be empty")));
    }

    Ok(())
}

pub(crate) fn ensure_max_bytes(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), CommandErrorPayload> {
    if value.len() > max_bytes {
        return Err(invalid_payload(format!(
            "{field} must be at most {max_bytes} bytes"
        )));
    }

    Ok(())
}

pub(crate) fn ensure_optional(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), CommandErrorPayload> {
    if let Some(value) = value {
        ensure_non_empty(field, value)?;
    }

    Ok(())
}

pub(crate) fn effective_execution_settings_permission_mode(
    permission_mode: PermissionMode,
) -> PermissionMode {
    if permission_mode == PermissionMode::Auto && !auto_mode_available() {
        PermissionMode::Default
    } else {
        permission_mode
    }
}

pub(crate) fn ensure_provider_settings(
    request: &ProviderSettingsRequest,
) -> Result<(), CommandErrorPayload> {
    ensure_provider_metadata_shape(&request.provider_id, &request.model_id)?;
    if let Some(config_id) = &request.config_id {
        ensure_provider_config_id(config_id)?;
    }
    if let Some(display_name) = &request.display_name {
        ensure_optional("displayName", Some(display_name))?;
    }
    let _ = normalized_provider_base_url(&request.provider_id, request.base_url.as_deref())?;
    validate_provider_defaults(&request.provider_id, request.provider_defaults.as_ref())?;

    Ok(())
}

pub(crate) fn ensure_provider_metadata_shape(
    provider_id: &str,
    model_id: &str,
) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("providerId", provider_id)?;
    ensure_non_empty("modelId", model_id)
}

pub(crate) fn ensure_provider_config_id(value: &str) -> Result<(), CommandErrorPayload> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid_payload("configId must not be empty".to_owned()));
    }
    if trimmed.len() > 64
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(invalid_payload(
            "configId must contain only letters, numbers, dot, dash, or underscore".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_mcp_server_request(
    request: &SaveMcpServerRequest,
) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("displayName", &request.display_name)?;
    ensure_mcp_server_id(&request.id)?;
    ensure_mcp_server_scope(&request.scope)?;
    ensure_save_mcp_server_transport(&request.transport)
}

pub(crate) fn ensure_mcp_server_record(
    record: &McpServerConfigRecord,
) -> Result<(), CommandErrorPayload> {
    harness_contracts::validate_persisted_mcp_server(record)
        .map_err(|error| invalid_payload(error.to_string()))
}

pub(crate) fn ensure_mcp_server_record_identity(
    record: &McpServerConfigRecord,
) -> Result<(), CommandErrorPayload> {
    harness_contracts::validate_persisted_mcp_server_identity(record)
        .map_err(|error| invalid_payload(error.to_string()))
}

pub(crate) fn ensure_save_mcp_server_transport(
    transport: &SaveMcpServerTransportConfig,
) -> Result<(), CommandErrorPayload> {
    match transport {
        SaveMcpServerTransportConfig::Stdio {
            command,
            args,
            env,
            inherit_env,
            working_dir,
        } => {
            ensure_non_empty("transport.command", command)?;
            if args.iter().any(|arg| arg.trim().is_empty()) {
                return Err(invalid_payload(
                    "transport.args must not contain empty values".to_owned(),
                ));
            }
            if args.len() > 64 {
                return Err(invalid_payload(
                    "transport.args must contain at most 64 values".to_owned(),
                ));
            }
            if env.len() > 64 {
                return Err(invalid_payload(
                    "transport.env must contain at most 64 values".to_owned(),
                ));
            }
            for item in env {
                ensure_env_var_name("transport.env.key", &item.key)?;
                ensure_save_mcp_name_value("transport.env", item, 4096)?;
                if mcp_env_key_looks_secret_bearing(&item.key) {
                    return Err(invalid_payload(
                        "transport.env must not contain secret-bearing values".to_owned(),
                    ));
                }
                if item
                    .value
                    .as_deref()
                    .is_some_and(|value| looks_like_raw_secret(value))
                {
                    return Err(invalid_payload(
                        "transport.env must not contain secret-bearing values".to_owned(),
                    ));
                }
            }
            if inherit_env.len() > 128 {
                return Err(invalid_payload(
                    "transport.inheritEnv must contain at most 128 values".to_owned(),
                ));
            }
            for item in inherit_env {
                ensure_env_var_name("transport.inheritEnv", item)?;
                if mcp_env_key_looks_secret_bearing(item) {
                    return Err(invalid_payload(
                        "transport.inheritEnv must not contain secret-bearing env names".to_owned(),
                    ));
                }
            }
            if let Some(working_dir) = working_dir {
                ensure_non_empty("transport.workingDir", working_dir)?;
                ensure_max_bytes("transport.workingDir", working_dir, 4096)?;
            }
        }
        SaveMcpServerTransportConfig::Http {
            url,
            bearer_token_env_var,
            headers,
            headers_from_env,
        } => {
            ensure_mcp_http_url(url)?;
            if let Some(env_var) = bearer_token_env_var {
                ensure_env_var_name("transport.bearerTokenEnvVar", env_var)?;
            }
            if headers.len() > 64 || headers_from_env.len() > 64 {
                return Err(invalid_payload(
                    "transport.headers must contain at most 64 values".to_owned(),
                ));
            }
            for header in headers {
                ensure_http_header_name("transport.headers.key", &header.key)?;
                ensure_save_mcp_name_value("transport.headers", header, 8192)?;
                if mcp_http_header_is_sensitive(&header.key)
                    || header.value.as_deref().is_some_and(|value| {
                        looks_like_raw_secret(value) || mcp_header_value_looks_secret_bearing(value)
                    })
                {
                    return Err(invalid_payload(
                        "transport.headers must not contain secret-bearing values".to_owned(),
                    ));
                }
            }
            for header in headers_from_env {
                ensure_http_header_name("transport.headersFromEnv.key", &header.key)?;
                ensure_env_var_name("transport.headersFromEnv.envVar", &header.env_var)?;
                if mcp_http_header_is_sensitive(&header.key) {
                    return Err(invalid_payload(
                        "transport.headersFromEnv must not contain sensitive header names"
                            .to_owned(),
                    ));
                }
            }
        }
        SaveMcpServerTransportConfig::InProcess => {
            return Err(invalid_payload(
                "transport.kind must be stdio or http for workspace MCP servers".to_owned(),
            ));
        }
    }

    Ok(())
}

fn ensure_save_mcp_name_value(
    field: &'static str,
    record: &McpNameValueSaveRecord,
    max_bytes: usize,
) -> Result<(), CommandErrorPayload> {
    match (record.value.as_deref(), record.preserve_existing) {
        (Some(value), false) => {
            ensure_max_bytes(field, value, max_bytes)?;
            if value.trim().is_empty() {
                return Err(invalid_payload(format!("{field}.value must not be empty")));
            }
            Ok(())
        }
        (None, true) => Ok(()),
        (Some(_), true) => Err(invalid_payload(format!(
            "{field}.preserveExisting must not include a replacement value"
        ))),
        (None, false) => Err(invalid_payload(format!("{field}.value must not be empty"))),
    }
}

pub(crate) fn ensure_env_var_name(
    field: &'static str,
    value: &str,
) -> Result<(), CommandErrorPayload> {
    ensure_non_empty(field, value)?;
    let mut chars = value.chars();
    let valid = chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !valid {
        return Err(invalid_payload(format!("{field} is invalid")));
    }
    Ok(())
}

pub(crate) fn ensure_http_header_name(
    field: &'static str,
    value: &str,
) -> Result<(), CommandErrorPayload> {
    ensure_non_empty(field, value)?;
    reqwest::header::HeaderName::from_bytes(value.trim().as_bytes())
        .map_err(|_| invalid_payload(format!("{field} is invalid")))?;
    Ok(())
}

pub(crate) fn ensure_mcp_http_url(value: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("transport.url", value)?;
    let url = reqwest::Url::parse(value)
        .map_err(|error| invalid_payload(format!("transport.url is invalid: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid_payload(
            "transport.url must be an http or https URL".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn mcp_env_key_looks_secret_bearing(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase().replace('-', "_");
    [
        "auth",
        "api_key",
        "apikey",
        "authorization",
        "bearer",
        "password",
        "secret",
        "token",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

pub(crate) fn mcp_http_header_is_sensitive(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "authorization" | "cookie" | "set-cookie" | "proxy-authorization"
    )
}

pub(crate) fn mcp_header_value_looks_secret_bearing(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.starts_with("bearer ")
        || normalized.starts_with("oauth ")
        || normalized.contains(" token")
        || normalized.contains("secret")
        || normalized.contains("password")
}

pub(crate) fn looks_like_raw_secret(value: &str) -> bool {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    let known_prefix = [
        "ghp_",
        "github_pat_",
        "glpat-",
        "sk-",
        "xoxb-",
        "xoxp-",
        "xoxa-",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix));
    known_prefix || (trimmed.len() >= 32 && trimmed.chars().all(is_secretish_character))
}

pub(crate) fn is_secretish_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '=' | '/' | '+')
}

pub(crate) fn ensure_mcp_server_id(id: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("id", id)?;
    let valid = id.len() <= 64
        && id
            .chars()
            .enumerate()
            .all(|(index, character)| match character {
                'A'..='Z' | 'a'..='z' | '0'..='9' => true,
                '.' | '-' | '_' if index > 0 => true,
                _ => false,
            });
    if !valid {
        return Err(invalid_payload(
            "id must use letters, numbers, dot, dash, or underscore".to_owned(),
        ));
    }

    Ok(())
}

pub(crate) fn ensure_mcp_server_scope(scope: &str) -> Result<(), CommandErrorPayload> {
    match scope {
        "agent" | "global" | "session" => Ok(()),
        _ => Err(invalid_payload("unsupported MCP server scope".to_owned())),
    }
}

pub(crate) fn mcp_server_origin_payload(source: &McpServerSource) -> &'static str {
    match source {
        McpServerSource::Workspace | McpServerSource::Project => "workspace",
        McpServerSource::User => "user",
        McpServerSource::Policy => "policy",
        McpServerSource::Plugin(_) => "plugin",
        McpServerSource::Dynamic { .. } | McpServerSource::Managed { .. } => "managed",
        _ => "managed",
    }
}

pub(crate) fn mcp_source_plugin_id(source: &McpServerSource) -> Option<String> {
    match source {
        McpServerSource::Plugin(plugin_id) => Some(plugin_id.0.clone()),
        _ => None,
    }
}

pub(crate) fn mcp_server_scope_payload(scope: &McpServerScope) -> String {
    match scope {
        McpServerScope::Global => "global".to_owned(),
        McpServerScope::Session(_) => "session".to_owned(),
        McpServerScope::Agent(_) => "agent".to_owned(),
        _ => "session".to_owned(),
    }
}

pub(crate) fn mcp_transport_payload(transport: &TransportChoice) -> &'static str {
    match transport {
        TransportChoice::Stdio { .. } => "stdio",
        TransportChoice::Http { .. } => "http",
        TransportChoice::WebSocket { .. } => "websocket",
        TransportChoice::Sse { .. } => "sse",
        TransportChoice::InProcess => "inProcess",
        _ => "inProcess",
    }
}

pub(crate) fn mcp_transport_config_payload(transport: &McpServerTransportConfig) -> &'static str {
    match transport {
        McpServerTransportConfig::Stdio { .. } => "stdio",
        McpServerTransportConfig::Http { .. } => "http",
        McpServerTransportConfig::InProcess => "inProcess",
    }
}

pub(crate) fn mcp_connection_state_payload(
    state: &McpConnectionState,
) -> (&'static str, Option<String>) {
    match state {
        McpConnectionState::Connecting => ("connecting", None),
        McpConnectionState::Ready => ("ready", None),
        McpConnectionState::Reconnecting { .. } => (
            "reconnecting",
            Some("MCP server is reconnecting.".to_owned()),
        ),
        McpConnectionState::Failed { last_error } => (
            "failed",
            Some(mcp_safe_connection_error_message(last_error)),
        ),
        McpConnectionState::Closed => ("closed", None),
    }
}

pub(crate) fn mcp_safe_connection_error_message(last_error: &str) -> String {
    let normalized = last_error.to_ascii_lowercase();
    if normalized.contains("no such file or directory") || normalized.contains("program not found")
    {
        return "MCP server command was not found.".to_owned();
    }
    if normalized.contains("permission denied:") {
        return "MCP server permission was denied.".to_owned();
    }
    if normalized.contains("permission denied") {
        return "MCP server command could not be executed.".to_owned();
    }
    if normalized.contains("timed out") || normalized.contains("timeout") {
        return "MCP server did not respond before the timeout.".to_owned();
    }
    "MCP server failed to start.".to_owned()
}
