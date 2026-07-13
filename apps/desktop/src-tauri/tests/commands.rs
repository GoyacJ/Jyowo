#![allow(dead_code)]
#![allow(unused_imports)]

use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use futures::stream;
use futures::StreamExt;
use harness_contracts::{
    ActionPlanHash, AgentProfile, AgentProfileContextMode, AgentProfileMemoryScope,
    AgentProfileSandboxInheritance, AgentProfileScope, AgentUsePolicy, AgentWorkspaceIsolationMode,
    ArtifactRevisionId, AssistantClarificationRequestedEvent, AssistantDeltaProducedEvent,
    AssistantMessageCompletedEvent, AssistantNoticeEvent, AssistantReviewRequestedEvent,
    CapabilityRouteKind, ConfigHash, ConversationAttachmentReference, ConversationCursor,
    ConversationModelCapability, CorrelationId, DecidedBy, DecisionLifetime, DecisionMatcherKind,
    DecisionMatcherSummary, EngineError, EngineFailedEvent, EventId, McpConnectionLostEvent,
    McpConnectionLostReason, MessageContent, MessageId, MessageMetadata, ModelModality,
    ModelProtocol, NoopRedactor, PermissionActorSource, PermissionDecisionOption,
    PermissionOptionId, PermissionRequestedEvent, PermissionResolvedEvent, ProviderCapabilityRoute,
    ProviderCapabilityRouteSettings, ProviderServiceAdapterAvailability, ReasoningSummaryChunk,
    RedactPatternSet, RedactRules, RedactScope, Redactor, RunModelSnapshot, RunStartedEvent,
    SandboxMode, SnapshotId, StopReason, ToolErrorPayload, ToolServiceBinding,
    ToolUseCompletedEvent, ToolUseFailedEvent, ToolUseRequestedEvent, ToolUseSummary, TurnInput,
    UiSafeText, UserMessageAppendedEvent, WorkspaceAccess,
};
use harness_journal::ReplayCursor;
use harness_skill::{parse_skill_markdown, SkillPlatform, SkillSource};
use harness_tool::BuiltinToolset;
use image::codecs::{gif::GifEncoder, jpeg::JpegEncoder, webp::WebPEncoder};
use image::{ExtendedColorType, ImageEncoder};
use jyowo_desktop_shell::commands::*;
use jyowo_desktop_shell::project_registry::ProjectRegistry;
use jyowo_harness_sdk::ext::{
    now, ArtifactCreatedEvent, ArtifactSource, ArtifactStatus, ArtifactUpdatedEvent, BlobMeta,
    BlobRetention, BlobStore, BudgetMetric, Decision, DecisionScope, DeferPolicy, DeltaChunk,
    Event, EventStore, FallbackPolicy, InteractivityLevel, McpConnection, McpError, McpRegistry,
    McpServerId, McpServerScope, McpServerSource, McpServerSpec, McpToolDescriptor, McpToolResult,
    Message, MessagePart, MessageRole, ModelError, OverflowAction, PermissionCheck,
    PermissionContext, PermissionMode, PermissionRequest, PermissionSubject,
    ProviderCredentialResolveContext, ProviderRestriction, RequestId, ResultBudget, RuleSnapshot,
    RunId, SessionId, Severity, StreamBrokerConfig, TenantId, ThinkingDelta, Tool, ToolCapability,
    ToolContext, ToolDescriptor, ToolError, ToolEvent, ToolGroup, ToolProfile, ToolProperties,
    ToolRegistry, ToolResult, ToolStream, ToolUseId, TransportChoice, TrustLevel, UsageSnapshot,
    ValidationError,
};
use jyowo_harness_sdk::ext::{ContentDelta, ModelStreamEvent};
use jyowo_harness_sdk::testing::{InMemoryEventStore, NoopSandbox, TestModelProvider};
use jyowo_harness_sdk::{
    ConversationEventsPageRequest, DesktopSettingsRuntime, HarnessOptions, McpConfig,
    StreamPermissionRuntime,
};
use parking_lot::RwLock as ParkingRwLock;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static WORKSPACE_ROOT_ENV_LOCK: Mutex<()> = Mutex::new(());
static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());
const WORKSPACE_ROOT_ENV: &str = "JYOWO_WORKSPACE_ROOT";
const HOME_ENV: &str = "HOME";
const TEST_MODEL_CONFIG_ID: &str = "test-model-config";

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
#[path = "commands/skills.rs"]
mod skills;
#[path = "commands/support.rs"]
mod support;

pub(crate) use provider_route_support::*;
pub(crate) use provider_support::*;
pub(crate) use support::*;
