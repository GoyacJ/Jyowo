use async_trait::async_trait;
use harness_contracts::{
    AgentProfile, AgentProfileContextMode, AgentProfileMemoryScope, AgentProfileSandboxInheritance,
    AgentProfileScope, AgentWorkspaceIsolationMode, CapabilityRouteKind, McpConnectionLostEvent,
    McpConnectionLostReason, ModelModality, ModelProtocol, NoopRedactor, PermissionOptionId,
    ProviderCapabilityRoute, ProviderCapabilityRouteSettings, ProviderServiceAdapterAvailability,
    ToolServiceBinding, WorkspaceAccess,
};
use harness_skill::{parse_skill_markdown, SkillPlatform, SkillSource};
use jyowo_desktop_shell::commands::*;
use jyowo_desktop_shell::project_registry::ProjectRegistry;
use jyowo_harness_sdk::ext::{
    now, BudgetMetric, Decision, DeferPolicy, Event, McpConnection, McpError, McpEventSink,
    McpRegistry, McpServerId, McpServerScope, McpServerSource, McpServerSpec, McpToolDescriptor,
    McpToolResult, OverflowAction, PermissionMode, PermissionSubject,
    ProviderCredentialResolveContext, ProviderRestriction, ResultBudget, RunId, SessionId,
    StreamBrokerConfig, TenantId, Tool, ToolContext, ToolDescriptor, ToolError, ToolEvent,
    ToolGroup, ToolProfile, ToolProperties, ToolRegistry, ToolResult, ToolStream, TransportChoice,
    TrustLevel, ValidationError,
};
use jyowo_harness_sdk::testing::{InMemoryEventStore, NoopSandbox, TestModelProvider};
use jyowo_harness_sdk::{
    DesktopSettingsRuntime, HarnessOptions, McpConfig, StreamPermissionRuntime,
};
use parking_lot::RwLock as ParkingRwLock;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static WORKSPACE_ROOT_ENV_LOCK: Mutex<()> = Mutex::new(());
static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());
const HOME_ENV: &str = "HOME";
const TEST_MODEL_CONFIG_ID: &str = "test-model-config";

#[derive(Debug, Default)]
struct NoopMcpEventSink;

impl McpEventSink for NoopMcpEventSink {
    fn emit(&self, _event: Event) {}
}

#[path = "commands/agents.rs"]
mod agents;
#[path = "commands/app_info.rs"]
mod app_info;
#[path = "commands/execution_settings.rs"]
mod execution_settings;
#[path = "commands/mcp.rs"]
mod mcp;
#[path = "commands/official_quota.rs"]
mod official_quota;
#[path = "commands/provider_credential_routes.rs"]
mod provider_credential_routes;
#[path = "commands/provider_probe.rs"]
mod provider_probe;
#[path = "commands/provider_quota_settings.rs"]
mod provider_quota_settings;
#[path = "commands/provider_route_support.rs"]
mod provider_route_support;
#[path = "commands/provider_routes.rs"]
mod provider_routes;
#[path = "commands/provider_settings.rs"]
mod provider_settings;
#[path = "commands/provider_settings_store.rs"]
mod provider_settings_store;
#[path = "commands/provider_support.rs"]
mod provider_support;
#[path = "commands/runtime_execution_status.rs"]
mod runtime_execution_status;
#[path = "commands/runtime_tools.rs"]
mod runtime_tools;
#[path = "commands/support.rs"]
mod support;

pub(crate) use provider_support::*;
pub(crate) use support::*;
