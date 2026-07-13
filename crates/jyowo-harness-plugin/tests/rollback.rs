use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    BudgetMetric, DeferPolicy, HookError, HookEventKind, McpServerId, McpServerSource,
    NetworkAccess, OverflowAction, PluginId, ProviderRestriction, ResultBudget, SemverString,
    SkillId, ToolActionPlan, ToolDescriptor, ToolDescriptorMetadata, ToolError,
    ToolExecutionChannel, ToolGroup, ToolIntegrationSource, ToolOrigin, ToolProperties, ToolResult,
    TrustLevel, WorkspaceAccess,
};
use harness_hook::{HookContext, HookEvent, HookHandler, HookOutcome, HookRegistry};
use harness_mcp::{McpRegistry, McpServerSpec, TransportChoice};
use harness_plugin::{
    DiscoverySource, HookManifestEntry, ManifestLoaderError, ManifestOrigin, ManifestRecord,
    McpManifestEntry, Plugin, PluginActivationContext, PluginActivationResult, PluginCapabilities,
    PluginCapabilityRegistries, PluginError, PluginManifest, PluginManifestLoader, PluginName,
    PluginRegistry, PluginRuntimeLoader, RuntimeLoaderError, SkillManifestEntry, ToolManifestEntry,
};
use harness_skill::{
    BuiltinHookKind, Skill, SkillConfigDecl, SkillFrontmatter, SkillHookDecl, SkillHookTransport,
    SkillPrerequisites, SkillRegistry, SkillSource,
};
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, BuiltinToolset, PermissionCheck, Tool,
    ToolContext, ToolEvent, ToolRegistry, ToolStream, ValidationError,
};
use serde_json::{json, Value};

#[tokio::test]
async fn failed_activation_rolls_back_registered_capabilities() {
    let registries = Registries::new();
    let manifest = manifest();
    let registry = registry_with(
        manifest.clone(),
        Arc::new(RegisteringPlugin {
            manifest: manifest.manifest.clone(),
            invalid_result: true,
        }),
        registries.capabilities(),
    );
    registry.discover().await.expect("discover");

    let error = registry
        .activate(&plugin_id())
        .await
        .expect_err("activation fails validation");

    assert!(matches!(error, PluginError::Registration(_)));
    registries.assert_unregistered().await;
}

#[tokio::test]
async fn deactivate_unregisters_registered_capabilities() {
    let registries = Registries::new();
    let manifest = manifest();
    let registry = registry_with(
        manifest.clone(),
        Arc::new(RegisteringPlugin {
            manifest: manifest.manifest.clone(),
            invalid_result: false,
        }),
        registries.capabilities(),
    );
    registry.discover().await.expect("discover");

    registry.activate(&plugin_id()).await.expect("activate");
    let skill_handler_id = registries.skills.hook_bindings()[0].handler_id.clone();
    registries
        .hooks
        .register(Box::new(FakeSkillBoundHook {
            handler_id: skill_handler_id,
        }))
        .expect("skill-bound hook should register");
    registries.assert_registered().await;

    registry.deactivate(&plugin_id()).await.expect("deactivate");
    registries.assert_unregistered().await;
}

struct Registries {
    tools: ToolRegistry,
    hooks: HookRegistry,
    mcp: McpRegistry,
    skills: SkillRegistry,
}

impl Registries {
    fn new() -> Self {
        Self {
            tools: ToolRegistry::builder()
                .with_builtin_toolset(BuiltinToolset::Empty)
                .build()
                .expect("tool registry"),
            hooks: HookRegistry::builder().build().expect("hook registry"),
            mcp: McpRegistry::new(),
            skills: SkillRegistry::builder().build(),
        }
    }

    fn capabilities(&self) -> PluginCapabilityRegistries {
        PluginCapabilityRegistries::default()
            .with_tool_registry(self.tools.clone())
            .with_hook_registry(self.hooks.clone())
            .with_mcp_registry(self.mcp.clone())
            .with_skill_registry(self.skills.clone())
    }

    async fn assert_registered(&self) {
        assert!(self.tools.get("registered-tool").is_some());
        assert!(self.hooks.origin_for("registered-hook").is_some());
        assert!(self
            .mcp
            .server_spec(&McpServerId("registered-mcp".into()))
            .await
            .is_some());
        assert!(self.skills.get("registered-skill").is_some());
    }

    async fn assert_unregistered(&self) {
        assert!(self.tools.get("registered-tool").is_none());
        assert!(self.hooks.origin_for("registered-hook").is_none());
        assert!(self
            .mcp
            .server_spec(&McpServerId("registered-mcp".into()))
            .await
            .is_none());
        assert!(self.skills.get("registered-skill").is_none());
        assert!(self
            .hooks
            .snapshot()
            .handlers_for(HookEventKind::SessionStart)
            .is_empty());
    }
}

fn registry_with(
    record: ManifestRecord,
    plugin: Arc<dyn Plugin>,
    registries: PluginCapabilityRegistries,
) -> PluginRegistry {
    PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader {
            record: record.clone(),
        }))
        .with_runtime_loader(Arc::new(StaticRuntimeLoader { plugin }))
        .with_capability_registries(registries)
        .build()
        .expect("registry")
}

fn manifest() -> ManifestRecord {
    ManifestRecord::new(
        PluginManifest {
            name: PluginName::new("rollback").unwrap(),
            version: semver::Version::parse("0.1.0").unwrap(),
            trust_level: TrustLevel::UserControlled,
            description: None,
            authors: Vec::new(),
            repository: None,
            signature: None,
            capabilities: PluginCapabilities {
                tools: vec![ToolManifestEntry {
                    name: "registered-tool".to_owned(),
                    destructive: false,
                    input_schema: serde_json::json!({ "type": "object" }),
                }],
                hooks: vec![HookManifestEntry {
                    name: "registered-hook".to_owned(),
                    events: Vec::new(),
                }],
                mcp_servers: vec![McpManifestEntry {
                    name: "registered-mcp".to_owned(),
                }],
                skills: vec![SkillManifestEntry {
                    name: "registered-skill".to_owned(),
                }],
                ..PluginCapabilities::default()
            },
            dependencies: Vec::new(),
            min_harness_version: semver::VersionReq::parse(">=0.0.0").unwrap(),
        },
        ManifestOrigin::File {
            path: "/plugins/rollback/plugin.json".into(),
        },
        [9; 32],
    )
    .unwrap()
}

fn plugin_id() -> PluginId {
    PluginId("rollback@0.1.0".to_owned())
}

struct StaticManifestLoader {
    record: ManifestRecord,
}

#[async_trait]
impl PluginManifestLoader for StaticManifestLoader {
    async fn enumerate(
        &self,
        _source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        Ok(vec![self.record.clone()])
    }
}

struct StaticRuntimeLoader {
    plugin: Arc<dyn Plugin>,
}

#[async_trait]
impl PluginRuntimeLoader for StaticRuntimeLoader {
    fn can_load(&self, manifest: &PluginManifest, _origin: &ManifestOrigin) -> bool {
        self.plugin.manifest().plugin_id() == manifest.plugin_id()
    }

    async fn load(
        &self,
        _manifest: &PluginManifest,
        _origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        Ok(self.plugin.clone())
    }
}

struct RegisteringPlugin {
    manifest: PluginManifest,
    invalid_result: bool,
}

#[async_trait]
impl Plugin for RegisteringPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        ctx.tools
            .as_ref()
            .expect("tools")
            .register(Box::new(FakeTool::new()))
            .await?;
        ctx.hooks
            .as_ref()
            .expect("hooks")
            .register(Box::new(FakeHook))
            .await?;
        let mcp_id = ctx.mcp.as_ref().expect("mcp").register(mcp_spec()).await?;
        ctx.skills
            .as_ref()
            .expect("skills")
            .register(fake_skill())
            .await?;

        let mut result = PluginActivationResult {
            registered_tools: vec!["registered-tool".to_owned()],
            registered_hooks: vec!["registered-hook".to_owned()],
            registered_mcp: vec![mcp_id],
            registered_skills: vec!["registered-skill".to_owned()],
            occupied_slots: Vec::new(),
        };
        if self.invalid_result {
            result.registered_tools.push("undeclared-tool".to_owned());
        }
        Ok(result)
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

fn mcp_spec() -> McpServerSpec {
    McpServerSpec::new(
        McpServerId("registered-mcp".into()),
        "registered MCP",
        TransportChoice::InProcess,
        McpServerSource::Workspace,
    )
}

fn fake_skill() -> Skill {
    Skill {
        id: SkillId("registered-skill".to_owned()),
        name: "registered-skill".to_owned(),
        description: "registered skill".to_owned(),
        source: SkillSource::Bundled,
        frontmatter: SkillFrontmatter {
            name: "registered-skill".to_owned(),
            description: "registered skill".to_owned(),
            allowlist_agents: None,
            parameters: Vec::new(),
            config: Vec::<SkillConfigDecl>::new(),
            platforms: Vec::new(),
            prerequisites: SkillPrerequisites::default(),
            hooks: vec![SkillHookDecl {
                id: "audit".to_owned(),
                events: vec![HookEventKind::SessionStart],
                transport: SkillHookTransport::Builtin(BuiltinHookKind::AuditLog),
            }],
            tags: Vec::new(),
            category: None,
            metadata: Default::default(),
        },
        body: String::new(),
        raw_path: None,
    }
}

struct FakeHook;

#[async_trait]
impl HookHandler for FakeHook {
    fn handler_id(&self) -> &str {
        "registered-hook"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        Ok(HookOutcome::Continue)
    }
}

struct FakeSkillBoundHook {
    handler_id: String,
}

#[async_trait]
impl HookHandler for FakeSkillBoundHook {
    fn handler_id(&self) -> &str {
        &self.handler_id
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::SessionStart]
    }

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        Ok(HookOutcome::Continue)
    }
}

struct FakeTool {
    descriptor: ToolDescriptor,
}

impl FakeTool {
    fn new() -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "registered-tool".to_owned(),
                display_name: "registered-tool".to_owned(),
                description: "registered tool".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: SemverString::from("0.1.0"),
                input_schema: json!({ "type": "object" }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::UserControlled,
                required_capabilities: Vec::new(),
                budget: ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 1024,
                    on_overflow: OverflowAction::Truncate,
                    preview_head_chars: 128,
                    preview_tail_chars: 128,
                },
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Plugin {
                    plugin_id: plugin_id(),
                    trust: TrustLevel::UserControlled,
                },
                search_hint: None,
                service_binding: None,
                metadata: ToolDescriptorMetadata {
                    integration_source: ToolIntegrationSource::Plugin,
                    ..Default::default()
                },
            },
        }
    }
}

#[async_trait]
impl Tool for FakeTool {
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
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text("ok".to_owned()),
        )])))
    }
}
