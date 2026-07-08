#![allow(dead_code, unused_imports)]

pub use std::collections::BTreeSet;
pub use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

pub use async_trait::async_trait;
pub use futures::{executor::block_on, stream, StreamExt};
pub use harness_contracts::{
    BlobId, BlobRef, BudgetExceedanceSource, ConfigHash, ContextPatchSource, ContextStageId,
    ConversationAttachmentReference, ConversationContextReference, ConversationTurnInput, Decision,
    DecisionScope, DeferPolicy, DeferredToolHint, EndReason, Event, FallbackPolicy, HookEventKind,
    InteractivityLevel, ManifestValidationFailure as ContractManifestValidationFailure,
    McpServerId, McpServerSource, MemoryError, MemoryId, MemoryKind, MemorySessionCtx,
    MemorySource, MemoryVisibility, MessageId, MessagePart, ModelError, NetworkAccess,
    PermissionMode, PermissionSubject, PluginId, ProviderRestriction, RedactRules, Redactor,
    RequestId, RuleSource, SessionCreatedEvent, SessionSummaryView, Severity, SnapshotId,
    SteeringBody, SteeringKind, SteeringSource, TeamId, TenantId, ToolActionPlan,
    ToolDeferredPoolChangedEvent, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup,
    ToolOrigin, ToolPoolChangeSource, ToolProfile, ToolProperties, ToolResult, ToolSearchMode,
    ToolUseId, TrustLevel, UsageSnapshot, WorkspaceAccess,
};
pub use harness_hook::HookRegistry;
pub use harness_journal::{EventStore, ReplayCursor};
pub use harness_mcp::{
    McpConnection, McpConnectionState, McpError, McpRegistry, McpServerScope, McpServerSpec,
    McpToolDescriptor, McpToolResult, SamplingRequest, TransportChoice,
};
#[cfg(feature = "memory-consolidation")]
pub use harness_memory::{ConsolidationHook, ConsolidationOutcome};
pub use harness_memory::{MemoryLifecycle, MemoryMetadata, MemoryRecord, MemoryStore};
pub use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelLifecycle, ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
pub use harness_observability::{
    AttributeValue, InMemorySpan, Observer, Span, SpanAttributes, TraceCarrier, TraceContext,
    Tracer,
};
pub use harness_plugin::{
    DiscoverySource, ManifestLoaderError, ManifestOrigin, ManifestRecord, Plugin,
    PluginActivationContext, PluginActivationResult, PluginAdmissionPolicy, PluginCapabilities,
    PluginConfig, PluginError, PluginEventSink, PluginManifest, PluginManifestLoader, PluginName,
    PluginRegistry, StaticLinkRuntimeLoader,
};
pub use harness_session::{session_options_hash, ConfigDelta, ReloadMode};
pub use harness_skill::{
    BundledSkillRecord, SkillLoader, SkillPlatform, SkillRegistration, SkillSource,
    SkillSourceConfig,
};
pub use harness_tool::{
    action_plan_from_permission_check, default_result_budget, AuthorizedToolInput, BuiltinToolset,
    PermissionCheck, SchemaResolverContext, Tool, ToolContext, ToolEvent, ToolRegistry, ToolStream,
    ValidationError,
};
pub use jyowo_harness_sdk::{prelude::*, testing::*, AgentCapabilityResolutionContext};
pub use serde_json::{json, Value};
pub use tokio::sync::Notify;

mod agents;
mod authorization;
mod observability;
mod runtime_context;
pub use agents::*;
pub use authorization::*;
pub use observability::*;
pub use runtime_context::*;

pub fn conversation_turn_request(
    options: SessionOptions,
    input: ConversationTurnInput,
    permission_mode: Option<PermissionMode>,
    permission_actor_source: Option<harness_contracts::PermissionActorSource>,
    agent_tool_policy: Option<harness_contracts::AgentToolPolicy>,
) -> ConversationTurnRequest {
    let mut run_options = ConversationRunOptions::from_session_options(&options);
    if let Some(permission_mode) = permission_mode {
        run_options = run_options.with_permission_mode(permission_mode);
    }
    #[cfg(feature = "agents-subagent")]
    {
        run_options.agent_tool_policy = agent_tool_policy;
    }
    #[cfg(not(feature = "agents-subagent"))]
    {
        let _ = agent_tool_policy;
    }
    ConversationTurnRequest {
        options,
        run_options,
        input,
        permission_actor_source,
    }
}

pub fn workspace_bootstrap_fixture(
    workspace: &std::path::Path,
    agents_content: &str,
    jyowo_agents_content: Option<&str>,
    bootstrap_addendum: Option<&str>,
) -> WorkspaceBootstrap {
    std::fs::write(workspace.join("AGENTS.md"), agents_content).unwrap();
    if let Some(content) = jyowo_agents_content {
        let jyowo_dir = workspace.join(".jyowo");
        std::fs::create_dir_all(&jyowo_dir).unwrap();
        std::fs::write(jyowo_dir.join("AGENTS.md"), content).unwrap();
    }
    let mut bootstrap = WorkspaceBootstrap::new(workspace);
    if let Some(addendum) = bootstrap_addendum {
        bootstrap = bootstrap.with_system_prompt_addendum(addendum);
    }
    bootstrap
}

pub async fn conversation_system_prompt_with_bootstrap(
    workspace: std::path::PathBuf,
    bootstrap: WorkspaceBootstrap,
    session_addendum: Option<&str>,
) -> String {
    let session_id = SessionId::new();
    let model = Arc::new(CapabilityScriptedProvider::new(
        ConversationModelCapability::default(),
        vec![vec![ModelStreamEvent::MessageStop]],
    ));
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model_arc(model.clone())
        .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");

    let mut options = SessionOptions::new(&workspace).with_session_id(session_id);
    options.workspace_bootstrap = Some(bootstrap);
    if let Some(addendum) = session_addendum {
        options = options.with_system_prompt_addendum(addendum);
    }

    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .expect("session should open");
    harness
        .submit_conversation_turn(conversation_turn_request(
            options,
            ConversationTurnInput::ask("hello"),
            None,
            None,
            None,
        ))
        .await
        .expect("turn should run");

    model.requests().await[0].system.clone().unwrap_or_default()
}

pub fn assert_workspace_bootstrap_prompt_order(system: &str) {
    let jyowo = system.find("<jyowo-system>").expect("jyowo-system");
    let runtime = system.find("<runtime-context>").expect("runtime-context");
    let agents = system
        .find(r#"<workspace-instructions source="AGENTS.md">"#)
        .expect("AGENTS.md workspace instructions");
    let jyowo_agents = system
        .find(r#"<workspace-instructions source=".jyowo/AGENTS.md">"#)
        .expect(".jyowo/AGENTS.md workspace instructions");
    let workspace_addendum = system
        .find(r#"<workspace-addendum source="workspace-bootstrap">"#)
        .expect("workspace bootstrap addendum");
    let session_addendum = system.find("<session-addendum>").expect("session addendum");

    assert!(jyowo < runtime);
    assert!(runtime < agents);
    assert!(agents < jyowo_agents);
    assert!(jyowo_agents < workspace_addendum);
    assert!(workspace_addendum < session_addendum);
}

pub fn test_blob_ref(size: u64, content_type: &str) -> BlobRef {
    BlobRef {
        id: BlobId::new(),
        size,
        content_hash: [9; 32],
        content_type: Some(content_type.to_owned()),
    }
}

#[cfg(feature = "memory-builtin")]
pub async fn conversation_system_prompt_with_builtin_memory(
    workspace: std::path::PathBuf,
    memdir_root: std::path::PathBuf,
    bootstrap: Option<WorkspaceBootstrap>,
    session_addendum: Option<&str>,
    seed_memory: Option<(&str, &str)>,
) -> String {
    let session_id = SessionId::new();
    let builtin = harness_memory::BuiltinMemory::at(&memdir_root, TenantId::SINGLE);
    if let Some((section, content)) = seed_memory {
        builtin
            .append_section(harness_memory::MemdirFile::Memory, section, content)
            .await
            .expect("seed memory");
    }
    let model = Arc::new(CapabilityScriptedProvider::new(
        ConversationModelCapability::default(),
        vec![vec![ModelStreamEvent::MessageStop]],
    ));
    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model_arc(model.clone())
        .with_store_arc(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_sandbox(NoopSandbox::new())
        .with_builtin_memory(builtin)
        .build()
        .await
        .expect("harness should build");

    let mut options = SessionOptions::new(&workspace).with_session_id(session_id);
    if let Some(bootstrap) = bootstrap {
        options.workspace_bootstrap = Some(bootstrap);
    }
    if let Some(addendum) = session_addendum {
        options = options.with_system_prompt_addendum(addendum);
    }

    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .expect("session should open");
    harness
        .submit_conversation_turn(conversation_turn_request(
            options,
            ConversationTurnInput::ask("hello"),
            None,
            None,
            None,
        ))
        .await
        .expect("turn should run");

    model.requests().await[0].system.clone().unwrap_or_default()
}

pub fn tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("tokio runtime")
}

pub struct ReadySubagentRunner;

#[cfg(feature = "agents-subagent")]
#[async_trait]
impl harness_subagent::SubagentRunner for ReadySubagentRunner {
    async fn spawn(
        &self,
        spec: harness_subagent::SubagentSpec,
        _input: harness_contracts::TurnInput,
        parent_ctx: harness_subagent::ParentContext,
    ) -> Result<harness_subagent::SubagentHandle, harness_subagent::SubagentError> {
        Ok(harness_subagent::SubagentHandle::ready(
            harness_subagent::SubagentAnnouncement {
                subagent_id: harness_contracts::SubagentId::new(),
                parent_session_id: parent_ctx.parent_session_id,
                status: harness_contracts::SubagentStatus::Completed,
                summary: spec.task,
                result: None,
                usage: harness_contracts::UsageSnapshot::default(),
                transcript_ref: None,
                context_report: None,
            },
        ))
    }
}

pub fn unique_workspace(name: &str) -> std::path::PathBuf {
    let session_id = harness_contracts::SessionId::new();
    std::env::temp_dir().join(format!("jyowo-{name}-{}-{session_id}", std::process::id()))
}

pub fn skill_registration_from(markdown: &str, source: SkillSource) -> SkillRegistration {
    SkillRegistration {
        skill: harness_skill::parse_skill_markdown(markdown, source, None, SkillPlatform::Macos)
            .expect("skill should parse"),
        force_allowlist: None,
    }
}

pub struct BlockingSkillListProvider {
    pub tool_use_id: ToolUseId,
    pub started: Notify,
    pub release: Notify,
    pub calls: AtomicUsize,
}

impl BlockingSkillListProvider {
    pub fn new(tool_use_id: ToolUseId) -> Self {
        Self {
            tool_use_id,
            started: Notify::new(),
            release: Notify::new(),
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ModelProvider for BlockingSkillListProvider {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        TestModelProvider::default().supported_models()
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            self.started.notify_one();
            self.release.notified().await;
            return Ok(Box::pin(stream::iter(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: self.tool_use_id,
                        name: "skills_list".to_owned(),
                        input: json!({}),
                    },
                },
                ModelStreamEvent::MessageStop,
            ])));
        }

        Ok(Box::pin(stream::iter(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("done".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ])))
    }
}

pub fn sdk_default_features(manifest: &str) -> Vec<String> {
    let mut in_default = false;
    let mut features = Vec::new();
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("default = [") {
            in_default = true;
            continue;
        }
        if in_default && trimmed.starts_with(']') {
            break;
        }
        if in_default {
            if let Some(feature) = trimmed
                .trim_end_matches(',')
                .trim()
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
            {
                features.push(feature.to_owned());
            }
        }
    }
    features
}

pub fn mcp_tool(name: &str, always_load: bool) -> McpToolDescriptor {
    let mut meta = std::collections::BTreeMap::new();
    if always_load {
        meta.insert("anthropic/alwaysLoad".to_owned(), json!(true));
    }
    McpToolDescriptor {
        name: name.to_owned(),
        description: Some(format!("{name} mcp tool")),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        annotations: None,
        meta,
    }
}

pub struct TestMcpConnection {
    pub tools: Vec<McpToolDescriptor>,
}

#[async_trait]
impl McpConnection for TestMcpConnection {
    fn connection_id(&self) -> &'static str {
        "test-mcp"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Ok(self.tools.clone())
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Ok(McpToolResult::text("ok"))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

pub fn memory_record(session_id: SessionId, content: &str) -> MemoryRecord {
    MemoryRecord {
        id: MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::UserPreference,
        visibility: MemoryVisibility::Private { session_id },
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: MemorySource::UserInput,
            confidence: 1.0,
            evidence: None,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 1.0,
            recall_score_breakdown: None,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: harness_contracts::now(),
        updated_at: harness_contracts::now(),
    }
}

pub fn memory_record_with_visibility(visibility: MemoryVisibility, content: &str) -> MemoryRecord {
    MemoryRecord {
        id: MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::UserPreference,
        visibility,
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: MemorySource::UserInput,
            confidence: 1.0,
            evidence: None,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 1.0,
            recall_score_breakdown: None,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: harness_contracts::now(),
        updated_at: harness_contracts::now(),
    }
}

pub fn request_text(request: &ModelRequest) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            harness_contracts::MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Default)]
pub struct InitializingMemoryProvider {
    pub initializes: AtomicUsize,
    pub initialized_identity: Mutex<Option<(Option<String>, Option<TeamId>)>>,
}

#[async_trait]
impl MemoryStore for InitializingMemoryProvider {
    fn provider_id(&self) -> &'static str {
        "initializing"
    }

    async fn recall(
        &self,
        _query: harness_memory::MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(Vec::new())
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(
        &self,
        _scope: harness_memory::MemoryListScope,
    ) -> Result<Vec<harness_memory::MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryLifecycle for InitializingMemoryProvider {
    async fn initialize(&self, ctx: &MemorySessionCtx<'_>) -> Result<(), MemoryError> {
        assert_eq!(ctx.tenant_id, TenantId::SINGLE);
        assert!(ctx.session_id != SessionId::from_u128(0));
        *self.initialized_identity.lock().unwrap() =
            Some((ctx.user_id.map(str::to_owned), ctx.team_id));
        self.initializes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl harness_memory::MemoryProvider for InitializingMemoryProvider {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndedMemorySnapshot {
    pub user_id: Option<String>,
    pub team_id: Option<TeamId>,
    pub turn_count: u32,
    pub tool_use_count: u32,
    pub final_assistant_text: Option<String>,
}

#[derive(Default)]
pub struct EndingMemoryProvider {
    pub ended: Mutex<Option<EndedMemorySnapshot>>,
    pub shutdowns: AtomicUsize,
}

#[async_trait]
impl MemoryStore for EndingMemoryProvider {
    fn provider_id(&self) -> &'static str {
        "ending"
    }

    async fn recall(
        &self,
        _query: harness_memory::MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(Vec::new())
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(
        &self,
        _scope: harness_memory::MemoryListScope,
    ) -> Result<Vec<harness_memory::MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryLifecycle for EndingMemoryProvider {
    async fn on_session_end(
        &self,
        ctx: &MemorySessionCtx<'_>,
        summary: &SessionSummaryView<'_>,
    ) -> Result<(), MemoryError> {
        *self.ended.lock().unwrap() = Some(EndedMemorySnapshot {
            user_id: ctx.user_id.map(str::to_owned),
            team_id: ctx.team_id,
            turn_count: summary.turn_count,
            tool_use_count: summary.tool_use_count,
            final_assistant_text: summary.final_assistant_text.map(str::to_owned),
        });
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), MemoryError> {
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl harness_memory::MemoryProvider for EndingMemoryProvider {}

#[cfg(feature = "memory-consolidation")]
pub struct RecordingConsolidationHook {
    pub calls: AtomicUsize,
    pub promoted: MemoryId,
}

#[cfg(feature = "memory-consolidation")]
impl Default for RecordingConsolidationHook {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            promoted: MemoryId::new(),
        }
    }
}

#[cfg(feature = "memory-consolidation")]
#[async_trait]
impl ConsolidationHook for RecordingConsolidationHook {
    fn hook_id(&self) -> &str {
        "sdk-consolidation"
    }

    async fn on_session_end(
        &self,
        _ctx: &MemorySessionCtx<'_>,
        _summary: &SessionSummaryView<'_>,
    ) -> Result<ConsolidationOutcome, MemoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ConsolidationOutcome {
            promoted: vec![self.promoted],
            demoted: Vec::new(),
            inbox_candidates_created: 1,
        })
    }
}

pub fn plugin_manifest(name: &str) -> ManifestRecord {
    ManifestRecord::new(
        PluginManifest {
            name: PluginName::new(name).unwrap(),
            version: semver::Version::parse("0.1.0").unwrap(),
            trust_level: TrustLevel::UserControlled,
            description: None,
            authors: Vec::new(),
            repository: None,
            signature: None,
            capabilities: PluginCapabilities {
                tools: vec![harness_plugin::ToolManifestEntry {
                    name: "plugin-tool".to_owned(),
                    destructive: false,
                    input_schema: serde_json::json!({ "type": "object" }),
                }],
                memory_provider: Some(harness_plugin::MemoryProviderManifestEntry {
                    name: "plugin-memory".to_owned(),
                }),
                ..PluginCapabilities::default()
            },
            dependencies: Vec::new(),
            min_harness_version: semver::VersionReq::parse(">=0.0.0").unwrap(),
        },
        ManifestOrigin::File {
            path: format!("/plugins/{name}/plugin.json").into(),
        },
        [7; 32],
    )
    .unwrap()
}

pub fn plugin_mcp_manifest(name: &str) -> ManifestRecord {
    ManifestRecord::new(
        PluginManifest {
            name: PluginName::new(name).unwrap(),
            version: semver::Version::parse("0.1.0").unwrap(),
            trust_level: TrustLevel::UserControlled,
            description: None,
            authors: Vec::new(),
            repository: None,
            signature: None,
            capabilities: PluginCapabilities {
                mcp_servers: vec![harness_plugin::McpManifestEntry {
                    name: "plugin-mcp".to_owned(),
                }],
                ..PluginCapabilities::default()
            },
            dependencies: Vec::new(),
            min_harness_version: semver::VersionReq::parse(">=0.0.0").unwrap(),
        },
        ManifestOrigin::File {
            path: format!("/plugins/{name}/plugin.json").into(),
        },
        [9; 32],
    )
    .unwrap()
}

pub fn plugin_id(name: &str) -> PluginId {
    PluginId(format!("{name}@0.1.0"))
}

pub struct SdkStaticManifestLoader {
    pub records: Vec<ManifestRecord>,
}

#[async_trait]
impl PluginManifestLoader for SdkStaticManifestLoader {
    async fn enumerate(
        &self,
        _source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        Ok(self.records.clone())
    }
}

pub struct SdkFailingManifestLoader;

#[async_trait]
impl PluginManifestLoader for SdkFailingManifestLoader {
    async fn enumerate(
        &self,
        _source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        Err(ManifestLoaderError::Validation(
            harness_plugin::ManifestValidationFailure {
                origin: Some(ManifestOrigin::File {
                    path: "/plugins/typed-bad/plugin.json".into(),
                }),
                partial_name: Some("typed-bad".to_owned()),
                partial_version: Some("0.1.0".to_owned()),
                raw_bytes_hash: [8; 32],
                failure: ContractManifestValidationFailure::SchemaViolation {
                    json_pointer: "/capabilities".to_owned(),
                    details: "expected object".to_owned(),
                },
                details: "expected object".to_owned(),
            },
        ))
    }
}

#[derive(Default)]
pub struct RecordingPluginEventSink {
    events: Mutex<Vec<Event>>,
}

impl RecordingPluginEventSink {
    pub fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap().clone()
    }
}

impl PluginEventSink for RecordingPluginEventSink {
    fn emit(&self, event: Event) {
        self.events.lock().unwrap().push(event);
    }
}

pub struct RuntimePlugin {
    pub manifest: PluginManifest,
    pub session_id: SessionId,
}

pub struct McpRuntimePlugin {
    pub manifest: PluginManifest,
}

#[async_trait]
impl Plugin for McpRuntimePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: harness_plugin::PluginActivationContext,
    ) -> Result<harness_plugin::PluginActivationResult, PluginError> {
        ctx.mcp
            .as_ref()
            .expect("plugin MCP handle")
            .register_ready(
                McpServerSpec::new(
                    McpServerId("plugin-mcp".to_owned()),
                    "Plugin MCP",
                    TransportChoice::InProcess,
                    McpServerSource::Plugin(self.manifest.plugin_id()),
                ),
                Arc::new(TestMcpConnection {
                    tools: vec![mcp_tool("echo", false)],
                }),
            )
            .await?;
        Ok(harness_plugin::PluginActivationResult {
            registered_mcp: vec![McpServerId("plugin-mcp".to_owned())],
            ..harness_plugin::PluginActivationResult::default()
        })
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

#[async_trait]
impl Plugin for RuntimePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        ctx.tools
            .as_ref()
            .expect("plugin tool handle")
            .register(Box::new(SdkPluginTool::new("plugin-tool")))
            .await?;
        ctx.memory
            .as_ref()
            .expect("plugin memory handle")
            .register(Arc::new(SdkPluginMemoryProvider {
                record: memory_record(self.session_id, "plugin memory is active"),
            }))
            .await?;
        Ok(PluginActivationResult {
            registered_tools: vec!["plugin-tool".to_owned()],
            occupied_slots: vec![harness_plugin::CapabilitySlot::MemoryProvider],
            ..PluginActivationResult::default()
        })
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

pub struct FailingRuntimePlugin {
    pub manifest: PluginManifest,
    pub failure: String,
}

#[async_trait]
impl Plugin for FailingRuntimePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        _ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        Err(PluginError::ActivateFailed(self.failure.clone()))
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

pub struct SdkPluginTool {
    pub descriptor: ToolDescriptor,
}

impl SdkPluginTool {
    pub fn new(name: &str) -> Self {
        Self::with_defer_policy(name, DeferPolicy::AlwaysLoad)
    }

    pub fn new_deferred(name: &str) -> Self {
        Self::with_defer_policy(name, DeferPolicy::ForceDefer)
    }

    pub fn with_defer_policy(name: &str, defer_policy: DeferPolicy) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: name.to_owned(),
                description: "plugin tool".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.1.0".to_owned(),
                input_schema: json!({ "type": "object" }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy,
                },
                trust_level: TrustLevel::UserControlled,
                required_capabilities: Vec::new(),
                budget: default_result_budget(),
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
            },
        }
    }
}

#[async_trait]
impl Tool for SdkPluginTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn resolve_schema(
        &self,
        _ctx: &SchemaResolverContext,
    ) -> Result<Value, harness_contracts::ToolError> {
        Ok(self.descriptor.input_schema.clone())
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            PermissionCheck::Allowed,
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        Ok(Box::pin(futures::stream::empty()))
    }
}

pub struct DeferredDeltaEmitterTool {
    pub descriptor: ToolDescriptor,
    pub deferred_name: String,
}

impl DeferredDeltaEmitterTool {
    pub fn new(deferred_name: &str) -> Self {
        Self {
            descriptor: SdkPluginTool::new("emit_deferred_delta").descriptor,
            deferred_name: deferred_name.to_owned(),
        }
    }
}

#[async_trait]
impl Tool for DeferredDeltaEmitterTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            PermissionCheck::Allowed,
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let event = Event::ToolDeferredPoolChanged(ToolDeferredPoolChangedEvent {
            session_id: ctx.session_id,
            added: vec![DeferredToolHint {
                name: self.deferred_name.clone(),
                hint: None,
            }],
            removed: Vec::new(),
            source: ToolPoolChangeSource::InitialClassification,
            deferred_total: 1,
            at: harness_contracts::now(),
        });
        Ok(Box::pin(futures::stream::iter([
            ToolEvent::Journal(event),
            ToolEvent::Final(ToolResult::Text("delta emitted".to_owned())),
        ])))
    }
}

pub struct CapabilityScriptedProvider {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub protocol: ModelProtocol,
    pub capabilities: ConversationModelCapability,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub responses: tokio::sync::Mutex<Vec<Vec<ModelStreamEvent>>>,
    pub requests: tokio::sync::Mutex<Vec<ModelRequest>>,
}

impl CapabilityScriptedProvider {
    pub fn new(
        capabilities: ConversationModelCapability,
        responses: Vec<Vec<ModelStreamEvent>>,
    ) -> Self {
        Self {
            provider_id: "test".to_owned(),
            model_id: "test-model".to_owned(),
            display_name: "Test model".to_owned(),
            protocol: ModelProtocol::Messages,
            capabilities,
            context_window: 128_000,
            max_output_tokens: 8_192,
            responses: tokio::sync::Mutex::new(responses),
            requests: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn with_identity(
        mut self,
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Self {
        self.provider_id = provider_id.into();
        self.model_id = model_id.into();
        self.display_name = display_name.into();
        self
    }

    pub fn with_protocol(mut self, protocol: ModelProtocol) -> Self {
        self.protocol = protocol;
        self
    }

    pub fn with_context_limits(mut self, context_window: u32, max_output_tokens: u32) -> Self {
        self.context_window = context_window;
        self.max_output_tokens = max_output_tokens;
        self
    }

    pub async fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl ModelProvider for CapabilityScriptedProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            provider_id: self.provider_id.clone(),
            model_id: self.model_id.clone(),
            display_name: self.display_name.clone(),
            protocol: self.protocol,
            context_window: self.context_window,
            max_output_tokens: self.max_output_tokens,
            conversation_capability: self.capabilities.clone(),
            runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
                self.protocol,
            ),
            lifecycle: ModelLifecycle::Stable,
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);
        let events = {
            let mut responses = self.responses.lock().await;
            if responses.is_empty() {
                vec![ModelStreamEvent::MessageStop]
            } else {
                responses.remove(0)
            }
        };
        Ok(Box::pin(futures::stream::iter(events)))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

pub struct TwoModelProvider;

#[async_trait]
impl ModelProvider for TwoModelProvider {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![
            ModelDescriptor {
                provider_id: "test".to_owned(),
                model_id: "model-a".to_owned(),
                display_name: "Model A".to_owned(),
                protocol: ModelProtocol::Messages,
                context_window: 128_000,
                max_output_tokens: 8_192,
                conversation_capability: ConversationModelCapability::default(),
                runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
                    ModelProtocol::Messages,
                ),
                lifecycle: ModelLifecycle::Stable,
                pricing: None,
            },
            ModelDescriptor {
                provider_id: "test".to_owned(),
                model_id: "model-b".to_owned(),
                display_name: "Model B".to_owned(),
                protocol: ModelProtocol::Responses,
                context_window: 128_000,
                max_output_tokens: 8_192,
                conversation_capability: ConversationModelCapability::default(),
                runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
                    ModelProtocol::Responses,
                ),
                lifecycle: ModelLifecycle::Stable,
                pricing: None,
            },
        ]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(futures::stream::empty()))
    }
}

pub struct SdkPluginMemoryProvider {
    pub record: MemoryRecord,
}

#[async_trait]
impl harness_memory::MemoryStore for SdkPluginMemoryProvider {
    fn provider_id(&self) -> &str {
        "sdk-plugin-memory"
    }

    async fn recall(
        &self,
        _query: harness_memory::MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(vec![self.record.clone()])
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(
        &self,
        _scope: harness_memory::MemoryListScope,
    ) -> Result<Vec<harness_memory::MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

impl harness_memory::MemoryLifecycle for SdkPluginMemoryProvider {}

impl harness_memory::MemoryProvider for SdkPluginMemoryProvider {}
