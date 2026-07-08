#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::artifacts::*;
#[allow(unused_imports)]
use super::automations::*;
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
use super::contracts::*;
#[allow(unused_imports)]
use super::conversations::*;
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
use super::evals::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
use super::memory::*;
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

pub(crate) fn validate_context_reference_payloads(
    references: Option<&[ContextReferencePayload]>,
) -> Result<(), CommandErrorPayload> {
    if let Some(references) = references {
        for reference in references {
            match reference {
                ContextReferencePayload::WorkspaceFile { path, label } => {
                    ensure_non_empty("contextReferences.path", path)?;
                    ensure_non_empty("contextReferences.label", label)?;
                }
                ContextReferencePayload::Artifact { id, label }
                | ContextReferencePayload::Conversation { id, label }
                | ContextReferencePayload::Memory { id, label }
                | ContextReferencePayload::Skill { id, label }
                | ContextReferencePayload::Tool { id, label }
                | ContextReferencePayload::McpServer { id, label } => {
                    ensure_non_empty("contextReferences.id", id)?;
                    ensure_non_empty("contextReferences.label", label)?;
                }
            }
        }
    }

    Ok(())
}

pub(crate) fn validate_attachment_reference_payloads(
    attachments: Option<&[AttachmentReferencePayload]>,
) -> Result<(), CommandErrorPayload> {
    if let Some(attachments) = attachments {
        let mut total_size = 0_u64;
        for attachment in attachments {
            ensure_attachment_id(&attachment.id)?;
            ensure_non_empty("attachments.name", &attachment.name)?;
            ensure_non_empty("attachments.mimeType", &attachment.mime_type)?;
            if attachment.size_bytes > MAX_ATTACHMENT_BYTES {
                return Err(invalid_payload(format!(
                    "attachment must be at most {} MB",
                    MAX_ATTACHMENT_BYTES / 1024 / 1024
                )));
            }
            total_size = total_size.saturating_add(attachment.size_bytes);
        }
        if total_size > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err(invalid_payload(format!(
                "attachments must total at most {} MB",
                MAX_TOTAL_ATTACHMENT_BYTES / 1024 / 1024
            )));
        }
    }

    Ok(())
}

pub(crate) fn ensure_attachment_id(value: &str) -> Result<(), CommandErrorPayload> {
    const PREFIX: &str = "attachment-";

    ensure_non_empty("attachments.id", value)?;
    let Some(hex) = value.strip_prefix(PREFIX) else {
        return Err(invalid_payload(
            "attachments.id must be a generated attachment id".to_owned(),
        ));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid_payload(
            "attachments.id must be a generated attachment id".to_owned(),
        ));
    }

    Ok(())
}

pub(crate) async fn build_conversation_turn_input(
    request: &StartRunRequest,
    state: &DesktopRuntimeState,
) -> Result<ConversationTurnInput, CommandErrorPayload> {
    validate_context_reference_payloads(request.context_references.as_deref())?;
    validate_attachment_reference_payloads(request.attachments.as_deref())?;
    let session_id = parse_session_id(&request.conversation_id)?;

    Ok(ConversationTurnInput {
        prompt: request.prompt.clone(),
        client_message_id: request.client_message_id.clone(),
        context_references: validate_context_references(
            request.context_references.as_deref().unwrap_or_default(),
            session_id,
            state,
        )
        .await?,
        attachments: validate_attachment_references(
            request.attachments.as_deref().unwrap_or_default(),
            state.runtime_root(),
            state
                .project_workspace_root()
                .is_none()
                .then_some(session_id),
        )?,
    })
}

pub(crate) fn resolve_start_run_permission_mode(
    requested: Option<PermissionMode>,
    state: &DesktopRuntimeState,
) -> Result<PermissionMode, CommandErrorPayload> {
    if let Some(permission_mode) = requested {
        ensure_start_run_permission_mode(permission_mode)?;
    }
    let permission_mode = effective_execution_settings_permission_mode(
        state
            .effective_execution_settings(requested)?
            .permission_mode,
    );
    ensure_start_run_permission_mode(permission_mode)?;
    Ok(permission_mode)
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

pub(crate) fn ensure_start_run_permission_mode(
    permission_mode: PermissionMode,
) -> Result<(), CommandErrorPayload> {
    match permission_mode {
        PermissionMode::Default | PermissionMode::Auto | PermissionMode::BypassPermissions => {}
        _ => {
            return Err(invalid_payload(
                "permissionMode must be default, auto, or bypass_permissions".to_owned(),
            ));
        }
    }
    if permission_mode == PermissionMode::Auto && !auto_mode_available() {
        return Err(invalid_payload(
            "permissionMode auto is not available in this desktop build".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_automation_spec(
    automation: &AutomationSpec,
) -> Result<(), CommandErrorPayload> {
    ensure_automation_id(&automation.id)?;
    ensure_non_empty("prompt", &automation.prompt)?;
    ensure_max_bytes("prompt", &automation.prompt, 64 * 1024)?;
    if contains_obvious_secret(&automation.prompt) || looks_like_raw_secret(&automation.prompt) {
        return Err(invalid_payload(
            "automation prompt must not contain raw secret-like values".to_owned(),
        ));
    }
    if automation.schedule.interval_minutes == 0 {
        return Err(invalid_payload(
            "automation schedule intervalMinutes must be greater than zero".to_owned(),
        ));
    }
    ensure_start_run_permission_mode(automation.permission_mode)?;
    ensure_automation_tool_profile(&automation.tool_profile)?;
    match automation.workspace_scope {
        AutomationWorkspaceScope::CurrentWorkspace => {}
    }
    if automation.sandbox_mode != SandboxMode::None {
        return Err(invalid_payload(
            "automation sandboxMode must be none for the MVP scheduler".to_owned(),
        ));
    }
    match &automation.workspace_access {
        WorkspaceAccess::ReadOnly => {}
        _ => {
            return Err(invalid_payload(
                "automation workspaceAccess must be read_only for the MVP scheduler".to_owned(),
            ));
        }
    }
    match automation.missed_run_policy {
        MissedRunPolicy::Skip | MissedRunPolicy::RunOnce => {}
    }
    Ok(())
}

pub(crate) fn ensure_automation_run_record(
    record: &AutomationRunRecord,
) -> Result<(), CommandErrorPayload> {
    ensure_automation_id(&record.automation_id)?;
    ensure_non_empty("id", &record.id)?;
    ensure_max_bytes("id", &record.id, 128)?;
    if let Some(message) = record.message.as_deref() {
        ensure_max_bytes("message", message, 4096)?;
        if looks_like_raw_secret(message) {
            return Err(invalid_payload(
                "automation run message must not contain raw secret-like values".to_owned(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn ensure_automation_tool_profile(
    tool_profile: &ToolProfile,
) -> Result<(), CommandErrorPayload> {
    match tool_profile {
        ToolProfile::Minimal | ToolProfile::Coding | ToolProfile::Full => Ok(()),
        ToolProfile::Custom {
            allowlist,
            denylist,
            group_allowlist,
            group_denylist,
            ..
        } => {
            if allowlist.len() > 256
                || denylist.len() > 256
                || group_allowlist.len() > 64
                || group_denylist.len() > 64
            {
                return Err(invalid_payload(
                    "automation custom toolProfile is too large".to_owned(),
                ));
            }
            for name in allowlist.iter().chain(denylist.iter()) {
                ensure_tool_name_fragment("toolProfile", name)?;
            }
            Ok(())
        }
        _ => Err(invalid_payload(
            "automation toolProfile is unsupported".to_owned(),
        )),
    }
}

pub(crate) fn ensure_tool_name_fragment(
    field: &'static str,
    value: &str,
) -> Result<(), CommandErrorPayload> {
    ensure_non_empty(field, value)?;
    if value.len() > 128
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
    {
        return Err(invalid_payload(format!(
            "{field} contains an invalid tool name"
        )));
    }
    Ok(())
}

pub(crate) fn ensure_automation_id(id: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("id", id)?;
    let valid = id.len() <= 96
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
            "automation id must use letters, numbers, dot, dash, or underscore".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) async fn validate_context_references(
    references: &[ContextReferencePayload],
    session_id: SessionId,
    state: &DesktopRuntimeState,
) -> Result<Vec<ConversationContextReference>, CommandErrorPayload> {
    let mut validated = Vec::with_capacity(references.len());

    for reference in references {
        validated.push(match reference {
            ContextReferencePayload::WorkspaceFile { path, label } => {
                let Some(workspace_root) = state.project_workspace_root() else {
                    return Err(invalid_payload(
                        "workspace file references require an active project workspace".to_owned(),
                    ));
                };
                let absolute_path = workspace_root.join(path);
                let canonical_path = absolute_path.canonicalize().map_err(|error| {
                    invalid_payload(format!("workspace file reference is invalid: {error}"))
                })?;
                let relative_path = workspace_relative_path(&canonical_path, workspace_root)
                    .ok_or_else(|| {
                        invalid_payload(
                            "workspace file reference must stay inside the workspace".to_owned(),
                        )
                    })?;
                ConversationContextReference::WorkspaceFile {
                    path: relative_path,
                    label: label.clone(),
                }
            }
            ContextReferencePayload::Artifact { id, label } => {
                ensure_artifact_exists(id, session_id, state).await?;
                ConversationContextReference::Artifact {
                    id: id.clone(),
                    label: label.clone(),
                }
            }
            ContextReferencePayload::Conversation { id, label } => {
                ensure_conversation_exists(id, state).await?;
                ConversationContextReference::Conversation {
                    id: id.clone(),
                    label: label.clone(),
                }
            }
            ContextReferencePayload::Memory { id, label } => {
                ensure_memory_exists(id, state).await?;
                ConversationContextReference::Memory {
                    id: id.clone(),
                    label: label.clone(),
                    resolved_content: None,
                }
            }
            ContextReferencePayload::Skill { id, label } => {
                ensure_skill_exists(id, state).await?;
                ConversationContextReference::Skill {
                    id: id.clone(),
                    label: label.clone(),
                }
            }
            ContextReferencePayload::Tool { id, label } => {
                ensure_tool_exists(id, state)?;
                ConversationContextReference::Tool {
                    id: id.clone(),
                    label: label.clone(),
                }
            }
            ContextReferencePayload::McpServer { id, label } => {
                ensure_mcp_server_exists(id, state).await?;
                ConversationContextReference::McpServer {
                    id: id.clone(),
                    label: label.clone(),
                }
            }
        });
    }

    Ok(validated)
}

pub(crate) fn validate_attachment_references(
    attachments: &[AttachmentReferencePayload],
    runtime_root: &Path,
    no_workspace_session_id: Option<SessionId>,
) -> Result<Vec<ConversationAttachmentReference>, CommandErrorPayload> {
    let mut validated = Vec::with_capacity(attachments.len());

    for attachment in attachments {
        if let Some(session_id) = no_workspace_session_id {
            if !no_workspace_attachment_belongs_to_conversation(
                runtime_root,
                session_id,
                &attachment.id,
            )? {
                return Err(invalid_payload(
                    "attachment reference does not belong to conversation".to_owned(),
                ));
            }
        }
        let record = read_attachment_record(runtime_root, &attachment.id)?;
        if record.attachment != *attachment {
            return Err(invalid_payload(
                "attachment reference does not match stored metadata".to_owned(),
            ));
        }
        validated.push(ConversationAttachmentReference {
            id: attachment.id.clone(),
            name: attachment.name.clone(),
            mime_type: attachment.mime_type.clone(),
            size_bytes: attachment.size_bytes,
            blob_ref: record.blob_ref.clone(),
        });
    }

    Ok(validated)
}

pub(crate) fn attachment_blob_ref_payload(blob_ref: &BlobRef) -> AttachmentBlobRefPayload {
    AttachmentBlobRefPayload {
        id: blob_ref.id.to_string(),
        size: blob_ref.size,
        content_hash: blob_ref.content_hash,
        content_type: blob_ref.content_type.clone(),
    }
}

pub(crate) async fn ensure_artifact_exists(
    id: &str,
    session_id: SessionId,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let artifacts = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: session_id.to_string(),
        },
        state,
    )
    .await?;
    if artifacts.artifacts.iter().any(|artifact| artifact.id == id) {
        Ok(())
    } else {
        Err(invalid_payload(
            "artifact reference does not exist".to_owned(),
        ))
    }
}

pub(crate) async fn ensure_conversation_exists(
    id: &str,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let conversations = list_conversations_with_runtime_state(state).await;
    if conversations
        .conversations
        .iter()
        .any(|conversation| conversation.id == id)
    {
        Ok(())
    } else {
        Err(invalid_payload(
            "conversation reference does not exist".to_owned(),
        ))
    }
}

pub(crate) async fn ensure_memory_exists(
    id: &str,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let memories = list_memory_items_with_runtime_state(state).await?;
    if memories.items.iter().any(|memory| memory.id == id) {
        Ok(())
    } else {
        Err(invalid_payload(
            "memory reference does not exist".to_owned(),
        ))
    }
}

pub(crate) async fn ensure_skill_exists(
    id: &str,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let skills = list_skills_with_runtime_state(state).await?;
    if skills.skills.iter().any(|skill| skill.id == id) {
        Ok(())
    } else {
        Err(invalid_payload("skill reference does not exist".to_owned()))
    }
}

pub(crate) fn ensure_tool_exists(
    id: &str,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Validating tool references requires the runtime tool registry.",
        ));
    };

    if harness.tool_registry().snapshot().descriptor(id).is_some() {
        Ok(())
    } else {
        Err(invalid_payload("tool reference does not exist".to_owned()))
    }
}

pub(crate) async fn ensure_mcp_server_exists(
    id: &str,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let servers = list_mcp_servers_with_runtime_state(state).await?;
    if servers.servers.iter().any(|server| server.id == id) {
        Ok(())
    } else {
        Err(invalid_payload(
            "mcp server reference does not exist".to_owned(),
        ))
    }
}

pub(crate) fn ensure_eval_case_id(value: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("caseId", value)?;
    if value.len() > 64 {
        return Err(invalid_payload(
            "caseId must be at most 64 bytes".to_owned(),
        ));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid_payload(
            "caseId may only contain ASCII letters, digits, dots, underscores, and hyphens"
                .to_owned(),
        ));
    }

    Ok(())
}

pub(crate) fn require_conversation_id_for_replay(
    value: Option<&str>,
) -> Result<(), CommandErrorPayload> {
    if value.is_none() {
        return Err(invalid_payload(
            "conversationId is required for replay and support bundle export".to_owned(),
        ));
    }

    Ok(())
}

pub(crate) fn require_conversation_id_for_activity(
    value: Option<&str>,
) -> Result<(), CommandErrorPayload> {
    if value.is_none() {
        return Err(invalid_payload(
            "conversationId is required for activity listing".to_owned(),
        ));
    }

    Ok(())
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
    let _ = normalized_base_url(request.base_url.as_deref())?;

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
    ensure_non_empty("displayName", &record.display_name)?;
    ensure_mcp_server_id(&record.id)?;
    ensure_mcp_server_scope(&record.scope)?;
    ensure_mcp_server_transport(&record.transport)
}

pub(crate) fn ensure_mcp_server_transport(
    transport: &McpServerTransportConfig,
) -> Result<(), CommandErrorPayload> {
    match transport {
        McpServerTransportConfig::Stdio {
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
                ensure_max_bytes("transport.env.value", &item.value, 4096)?;
                if mcp_env_key_looks_secret_bearing(&item.key) || looks_like_raw_secret(&item.value)
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
        McpServerTransportConfig::Http {
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
                ensure_max_bytes("transport.headers.value", &header.value, 8192)?;
                if mcp_http_header_is_sensitive(&header.key)
                    || looks_like_raw_secret(&header.value)
                    || mcp_header_value_looks_secret_bearing(&header.value)
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
        McpServerTransportConfig::InProcess => {
            return Err(invalid_payload(
                "transport.kind must be stdio or http for workspace MCP servers".to_owned(),
            ));
        }
    }

    Ok(())
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
    if normalized.contains("permission denied") {
        return "MCP server command could not be executed.".to_owned();
    }
    if normalized.contains("timed out") || normalized.contains("timeout") {
        return "MCP server did not respond before the timeout.".to_owned();
    }
    "MCP server failed to start.".to_owned()
}
