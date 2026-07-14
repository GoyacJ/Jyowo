use std::{path::PathBuf, sync::Arc};

use chrono::Utc;
use harness_contracts::{
    ActionPlanHash, ActionPlanId, ActionResource, DecisionScope, FallbackPolicy,
    InteractivityLevel, ManifestOriginRef, McpPromptOperation, McpResourceOperation,
    McpServerScope, McpTransportTarget, NetworkAccess, PermissionActorSource, PermissionMode,
    PermissionReview, PermissionSubject, ResourceLimits, SandboxMode, SandboxPolicy, SandboxScope,
    Severity, TenantId, ToolActionPlan, ToolExecutionChannel, ToolUseId, WorkspaceAccess,
};
use harness_execution::{AuthorizationContext, AuthorizationService, ExecutionError};
use harness_permission::{canonical_permission_fingerprint, PermissionRequest};

use crate::{McpConnectContext, McpError, McpServerSpec, TransportChoice};

#[derive(Clone)]
pub struct McpAuthorizationContext {
    pub authorization_service: Arc<AuthorizationService>,
    pub tenant_id: TenantId,
    pub scope: McpServerScope,
    pub session_id: harness_contracts::SessionId,
    pub run_id: harness_contracts::RunId,
    pub permission_mode: PermissionMode,
    pub interactivity: InteractivityLevel,
    pub fallback_policy: FallbackPolicy,
    pub workspace_root: PathBuf,
}

impl McpAuthorizationContext {
    #[must_use]
    pub fn authorization_context(&self) -> AuthorizationContext {
        AuthorizationContext {
            tenant_id: self.tenant_id,
            session_id: self.session_id,
            run_id: self.run_id,
            permission_mode: self.permission_mode,
            interactivity: self.interactivity,
            fallback_policy: self.fallback_policy,
            workspace_root: self.workspace_root.clone(),
        }
    }
}

pub async fn authorize_mcp_transport(
    context: &McpAuthorizationContext,
    spec: &McpServerSpec,
) -> Result<(), McpError> {
    let target = transport_target(&spec.transport);
    let mut resources = vec![ActionResource::McpTransport {
        server_id: spec.server_id.0.clone(),
        origin: spec.manifest_origin.clone(),
        target,
    }];
    resources.extend(transport_resources(&spec.transport));
    let network_access = transport_network_access(&spec.transport);
    let target_payload = serde_json::to_value(transport_target(&spec.transport))
        .unwrap_or_else(|_| serde_json::json!({}));
    let subject = PermissionSubject::Custom {
        kind: "mcp_transport".to_owned(),
        payload: serde_json::json!({
            "server_id": spec.server_id.0,
            "transport": transport_id(&spec.transport),
            "target": target_payload,
        }),
    };
    let plan = mcp_action_plan(
        context,
        spec,
        "mcp_transport",
        ToolUseId::new(),
        subject,
        DecisionScope::ToolName("mcp_transport".to_owned()),
        Severity::Medium,
        resources,
        network_access,
        PermissionReview {
            summary: format!("MCP server {} requests transport access", spec.server_id.0),
            details: Vec::new(),
            confirmation: harness_contracts::PermissionConfirmation::None,
            redacted: true,
        },
    );
    context
        .authorization_service
        .authorize_operation(context.authorization_context(), plan)
        .await
        .map(|_| ())
        .map_err(to_mcp_permission_error)
}

pub async fn authorize_mcp_transport_connect(
    context: &McpConnectContext,
    spec: &McpServerSpec,
) -> Result<(), McpError> {
    if matches!(spec.transport, TransportChoice::InProcess) || context.transport_authorized {
        return Ok(());
    }
    let Some(authorization) = context.authorization.as_ref() else {
        return Err(McpError::PermissionDenied(
            "mcp transport authorization context is required".to_owned(),
        ));
    };
    authorize_mcp_transport(authorization, spec).await
}

pub async fn authorize_mcp_sampling(
    context: &McpAuthorizationContext,
    spec: &McpServerSpec,
    request_id: harness_contracts::RequestId,
    model_id: Option<&str>,
    prompt_cache_namespace: &str,
) -> Result<(), McpError> {
    let tool_use_id = ToolUseId::new();
    let subject = PermissionSubject::Custom {
        kind: "mcp_sampling".to_owned(),
        payload: serde_json::json!({
            "server_id": spec.server_id.0,
            "model_id": model_id,
            "request_id": request_id,
            "prompt_cache_namespace": prompt_cache_namespace,
        }),
    };
    let plan = mcp_action_plan(
        context,
        spec,
        "mcp_sampling",
        tool_use_id,
        subject,
        DecisionScope::ToolName("mcp_sampling".to_owned()),
        Severity::Medium,
        vec![ActionResource::McpSampling {
            server_id: spec.server_id.0.clone(),
            origin: spec.manifest_origin.clone(),
        }],
        NetworkAccess::None,
        PermissionReview {
            summary: format!("MCP server {} requests sampling access", spec.server_id.0),
            details: Vec::new(),
            confirmation: harness_contracts::PermissionConfirmation::None,
            redacted: true,
        },
    );
    context
        .authorization_service
        .authorize_operation(context.authorization_context(), plan)
        .await
        .map(|_| ())
        .map_err(to_mcp_permission_error)
}

pub async fn authorize_mcp_resource(
    context: &McpAuthorizationContext,
    spec: &McpServerSpec,
    operation: McpResourceOperation,
) -> Result<(), McpError> {
    let operation_payload =
        serde_json::to_value(&operation).unwrap_or_else(|_| serde_json::json!({}));
    let subject = PermissionSubject::Custom {
        kind: "mcp_resource".to_owned(),
        payload: serde_json::json!({
            "server_id": spec.server_id.0,
            "operation": operation_payload,
        }),
    };
    let plan = mcp_action_plan(
        context,
        spec,
        "mcp_resource",
        ToolUseId::new(),
        subject,
        DecisionScope::ToolName("mcp_resource".to_owned()),
        Severity::Medium,
        vec![mcp_resource_action(
            &spec.server_id,
            &spec.manifest_origin,
            operation,
        )],
        NetworkAccess::None,
        PermissionReview {
            summary: format!("MCP server {} requests resource access", spec.server_id.0),
            details: Vec::new(),
            confirmation: harness_contracts::PermissionConfirmation::None,
            redacted: true,
        },
    );
    context
        .authorization_service
        .authorize_operation(context.authorization_context(), plan)
        .await
        .map(|_| ())
        .map_err(to_mcp_permission_error)
}

pub async fn authorize_mcp_prompt(
    context: &McpAuthorizationContext,
    spec: &McpServerSpec,
    operation: McpPromptOperation,
) -> Result<(), McpError> {
    let operation_payload =
        serde_json::to_value(&operation).unwrap_or_else(|_| serde_json::json!({}));
    let subject = PermissionSubject::Custom {
        kind: "mcp_prompt".to_owned(),
        payload: serde_json::json!({
            "server_id": spec.server_id.0,
            "operation": operation_payload,
        }),
    };
    let plan = mcp_action_plan(
        context,
        spec,
        "mcp_prompt",
        ToolUseId::new(),
        subject,
        DecisionScope::ToolName("mcp_prompt".to_owned()),
        Severity::Medium,
        vec![mcp_prompt_action(
            &spec.server_id,
            &spec.manifest_origin,
            operation,
        )],
        NetworkAccess::None,
        PermissionReview {
            summary: format!("MCP server {} requests prompt access", spec.server_id.0),
            details: Vec::new(),
            confirmation: harness_contracts::PermissionConfirmation::None,
            redacted: true,
        },
    );
    context
        .authorization_service
        .authorize_operation(context.authorization_context(), plan)
        .await
        .map(|_| ())
        .map_err(to_mcp_permission_error)
}

#[must_use]
pub fn mcp_tool_resource(
    server_id: &harness_contracts::McpServerId,
    origin: &ManifestOriginRef,
    tool_name: &str,
) -> ActionResource {
    ActionResource::McpTool {
        server_id: server_id.0.clone(),
        origin: origin.clone(),
        tool_name: tool_name.to_owned(),
    }
}

#[must_use]
pub fn mcp_resource_action(
    server_id: &harness_contracts::McpServerId,
    origin: &ManifestOriginRef,
    operation: McpResourceOperation,
) -> ActionResource {
    ActionResource::McpResource {
        server_id: server_id.0.clone(),
        origin: origin.clone(),
        operation,
    }
}

#[must_use]
pub fn mcp_prompt_action(
    server_id: &harness_contracts::McpServerId,
    origin: &ManifestOriginRef,
    operation: McpPromptOperation,
) -> ActionResource {
    ActionResource::McpPrompt {
        server_id: server_id.0.clone(),
        origin: origin.clone(),
        operation,
    }
}

fn mcp_action_plan(
    context: &McpAuthorizationContext,
    spec: &McpServerSpec,
    tool_name: &str,
    tool_use_id: ToolUseId,
    subject: PermissionSubject,
    scope: DecisionScope,
    severity: Severity,
    resources: Vec<ActionResource>,
    network_access: NetworkAccess,
    review: PermissionReview,
) -> ToolActionPlan {
    let request = PermissionRequest {
        request_id: harness_contracts::RequestId::new(),
        tenant_id: context.tenant_id,
        session_id: context.session_id,
        tool_use_id,
        tool_name: tool_name.to_owned(),
        subject: subject.clone(),
        severity,
        scope_hint: scope.clone(),
        action_plan_hash: harness_contracts::ActionPlanHash::default(),
        decision_options: Vec::new(),
        confirmation_expected: None,
        created_at: Utc::now(),
    };
    ToolActionPlan {
        plan_id: ActionPlanId::new(),
        tool_use_id,
        tool_name: tool_name.to_owned(),
        actor_source: PermissionActorSource::McpServer {
            server_id: spec.server_id.clone(),
            origin: spec.manifest_origin.clone(),
            scope: context.scope.clone(),
        },
        subject,
        scope,
        severity,
        resources,
        sandbox_policy: SandboxPolicy {
            mode: SandboxMode::None,
            scope: SandboxScope::WorkspaceOnly,
            network: network_access.clone(),
            resource_limits: ResourceLimits {
                max_memory_bytes: None,
                max_cpu_cores: None,
                max_pids: None,
                max_wall_clock_ms: None,
                max_open_files: None,
            },
            denied_host_paths: Vec::new(),
        },
        workspace_access: WorkspaceAccess::None,
        network_access,
        review,
        execution_channel: ToolExecutionChannel::DirectAuthorizedRust,
        plan_hash: ActionPlanHash::from_bytes(canonical_permission_fingerprint(&request).0),
        created_at: Utc::now(),
    }
}

fn transport_resources(transport: &TransportChoice) -> Vec<ActionResource> {
    match transport {
        TransportChoice::Stdio {
            command,
            args,
            policy,
            ..
        } => {
            let mut hasher = blake3::Hasher::new();
            hash_field(&mut hasher, command.as_bytes());
            for arg in args {
                hash_field(&mut hasher, arg.as_bytes());
            }
            if let Some(cwd) = &policy.working_dir {
                hash_field(&mut hasher, cwd.to_string_lossy().as_bytes());
            }
            vec![ActionResource::Command {
                command: command.clone(),
                argv: args.clone(),
                cwd: policy.working_dir.clone(),
                fingerprint: harness_contracts::ExecFingerprint(*hasher.finalize().as_bytes()),
            }]
        }
        TransportChoice::Http { url, .. }
        | TransportChoice::WebSocket { url, .. }
        | TransportChoice::Sse { url, .. } => network_resource(url).into_iter().collect(),
        TransportChoice::InProcess => Vec::new(),
    }
}

fn transport_network_access(transport: &TransportChoice) -> NetworkAccess {
    match transport {
        TransportChoice::Http { url, .. }
        | TransportChoice::WebSocket { url, .. }
        | TransportChoice::Sse { url, .. } => network_rule(url)
            .map(|rule| NetworkAccess::AllowList(vec![rule]))
            .unwrap_or(NetworkAccess::None),
        TransportChoice::Stdio { .. } | TransportChoice::InProcess => NetworkAccess::None,
    }
}

fn network_resource(url: &str) -> Option<ActionResource> {
    let rule = network_rule(url)?;
    Some(ActionResource::Network {
        host: rule.pattern,
        port: rule.ports.and_then(|ports| ports.first().copied()),
    })
}

fn network_rule(url: &str) -> Option<harness_contracts::HostRule> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https" | "ws" | "wss")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let port = parsed.port_or_known_default()?;
    Some(harness_contracts::HostRule {
        pattern: host,
        ports: Some(vec![port]),
    })
}

fn transport_target(transport: &TransportChoice) -> McpTransportTarget {
    let (endpoint_label, fingerprint_material) = match transport {
        TransportChoice::Stdio {
            command,
            args,
            policy,
            ..
        } => (
            command.clone(),
            serde_json::json!({
                "transport": "stdio",
                "command": command,
                "args": args,
                "cwd": policy.working_dir,
            })
            .to_string(),
        ),
        TransportChoice::Http { url, .. }
        | TransportChoice::WebSocket { url, .. }
        | TransportChoice::Sse { url, .. } => (
            url.clone(),
            serde_json::json!({
                "transport": transport_id(transport),
                "url": url,
            })
            .to_string(),
        ),
        TransportChoice::InProcess => ("in-process".to_owned(), "in-process".to_owned()),
    };
    McpTransportTarget {
        transport: transport_id(transport).to_owned(),
        endpoint_fingerprint: blake3::hash(fingerprint_material.as_bytes())
            .to_hex()
            .to_string(),
        endpoint_label,
    }
}

fn transport_id(transport: &TransportChoice) -> &'static str {
    match transport {
        TransportChoice::Stdio { .. } => "stdio",
        TransportChoice::Http { .. } => "http",
        TransportChoice::WebSocket { .. } => "websocket",
        TransportChoice::Sse { .. } => "sse",
        TransportChoice::InProcess => "in-process",
    }
}

fn hash_field(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn to_mcp_permission_error(error: ExecutionError) -> McpError {
    McpError::PermissionDenied(error.to_string())
}
