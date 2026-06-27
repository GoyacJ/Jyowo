use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DeferPolicy, McpOrigin, McpServerId, McpServerSource, ModelProvider, PluginId,
    ProviderRestriction, ShadowReason, SkillId, SkillOrigin, SkillSourceKind, ToolDescriptor,
    ToolError, ToolGroup, ToolOrigin, ToolProperties, ToolResult, TrustLevel,
};
use harness_permission::PermissionCheck;
use harness_tool::{
    default_result_budget, BuiltinToolset, RegistrationError, SchemaResolverContext, Tool,
    ToolContext, ToolEvent, ToolPool, ToolPoolFilter, ToolPoolModelProfile, ToolRegistry,
    ToolSearchMode, ValidationError,
};
use serde_json::{json, Value};

struct TestTool {
    descriptor: ToolDescriptor,
    resolved_schema: Option<Value>,
    resolve_count: Option<Arc<AtomicUsize>>,
}

#[async_trait]
impl Tool for TestTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn resolve_schema(&self, _ctx: &SchemaResolverContext) -> Result<Value, ToolError> {
        if let Some(count) = &self.resolve_count {
            count.fetch_add(1, Ordering::SeqCst);
        }
        Ok(self
            .resolved_schema
            .clone()
            .unwrap_or_else(|| self.descriptor.input_schema.clone()))
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn check_permission(&self, _input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        PermissionCheck::Allowed
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolContext,
    ) -> Result<harness_tool::ToolStream, ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(input),
        )])))
    }
}

#[test]
fn registry_registers_tools_and_snapshots_are_immutable() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .unwrap();
    registry
        .register(Box::new(tool(
            "alpha",
            ToolOrigin::Builtin,
            TrustLevel::AdminTrusted,
        )))
        .unwrap();

    let snapshot = registry.snapshot();
    registry
        .register(Box::new(tool(
            "beta",
            ToolOrigin::Builtin,
            TrustLevel::AdminTrusted,
        )))
        .unwrap();

    assert_eq!(registry.get("alpha").unwrap().descriptor().name, "alpha");
    assert!(snapshot.get("alpha").is_some());
    assert!(snapshot.get("beta").is_none());
    assert!(registry.snapshot().get("beta").is_some());
}

#[test]
fn registry_records_shadowing_and_applies_builtin_and_trust_precedence() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .unwrap();
    registry
        .register(Box::new(tool(
            "read",
            ToolOrigin::Builtin,
            TrustLevel::AdminTrusted,
        )))
        .unwrap();
    registry
        .register(Box::new(tool(
            "read",
            plugin_origin("user-plugin", TrustLevel::UserControlled),
            TrustLevel::UserControlled,
        )))
        .unwrap();

    assert_eq!(
        registry.get("read").unwrap().descriptor().origin,
        ToolOrigin::Builtin
    );
    assert_eq!(registry.shadowed()[0].reason, ShadowReason::BuiltinWins);

    registry
        .register(Box::new(tool(
            "read",
            mcp_origin("workspace-server", TrustLevel::AdminTrusted),
            TrustLevel::AdminTrusted,
        )))
        .unwrap();
    registry
        .register(Box::new(tool(
            "read",
            skill_origin("read-skill", TrustLevel::AdminTrusted),
            TrustLevel::AdminTrusted,
        )))
        .unwrap();

    assert_eq!(
        registry.get("read").unwrap().descriptor().origin,
        ToolOrigin::Builtin
    );
    assert_eq!(registry.shadowed()[1].reason, ShadowReason::BuiltinWins);
    assert_eq!(registry.shadowed()[2].reason, ShadowReason::BuiltinWins);

    registry
        .register(Box::new(tool(
            "ext",
            plugin_origin("user-plugin", TrustLevel::UserControlled),
            TrustLevel::UserControlled,
        )))
        .unwrap();
    registry
        .register(Box::new(tool(
            "ext",
            plugin_origin("admin-plugin", TrustLevel::AdminTrusted),
            TrustLevel::AdminTrusted,
        )))
        .unwrap();

    assert_eq!(
        registry.get("ext").unwrap().descriptor().trust_level,
        TrustLevel::AdminTrusted
    );
    assert_eq!(registry.shadowed()[3].reason, ShadowReason::HigherTrust);

    registry
        .register(Box::new(tool(
            "ext",
            skill_origin("duplicate-skill", TrustLevel::AdminTrusted),
            TrustLevel::AdminTrusted,
        )))
        .unwrap();
    assert_eq!(registry.shadowed()[4].reason, ShadowReason::Duplicate);
}

#[tokio::test]
async fn pool_assembles_stable_partitions_filters_and_runtime_append_order() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(tool_with(
            "always",
            DeferPolicy::AlwaysLoad,
            ToolGroup::FileSystem,
            ProviderRestriction::All,
            false,
        )))
        .with_tool(Box::new(tool_with(
            "deferred",
            DeferPolicy::ForceDefer,
            ToolGroup::Search,
            ProviderRestriction::All,
            false,
        )))
        .with_tool(Box::new(tool_with(
            "denied",
            DeferPolicy::AlwaysLoad,
            ToolGroup::Network,
            ProviderRestriction::All,
            false,
        )))
        .with_tool(Box::new(tool_with(
            "provider_only",
            DeferPolicy::AlwaysLoad,
            ToolGroup::FileSystem,
            ProviderRestriction::Allowlist(BTreeSet::from([ModelProvider("anthropic".to_owned())])),
            false,
        )))
        .build()
        .unwrap();

    let mut filter = ToolPoolFilter::default();
    filter.denylist.insert("denied".to_owned());
    filter.group_denylist.insert(ToolGroup::Network);

    let mut pool = ToolPool::assemble(
        &registry.snapshot(),
        &filter,
        &ToolSearchMode::Always,
        &ToolPoolModelProfile {
            provider: ModelProvider("anthropic".to_owned()),
            max_context_tokens: Some(200_000),
        },
        &schema_ctx(),
    )
    .await
    .unwrap();

    assert_eq!(names(pool.always_loaded()), ["always", "provider_only"]);
    assert_eq!(names(pool.deferred()), ["deferred"]);

    pool.append_runtime_tool(Arc::new(tool(
        "zeta",
        ToolOrigin::Builtin,
        TrustLevel::AdminTrusted,
    )));
    pool.append_runtime_tool(Arc::new(tool(
        "gamma",
        ToolOrigin::Builtin,
        TrustLevel::AdminTrusted,
    )));
    assert_eq!(names(pool.runtime_appended()), ["zeta", "gamma"]);
}

#[tokio::test]
#[cfg(feature = "builtin-toolset")]
async fn append_runtime_tool_preserves_existing_journal_authority_for_same_name() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Shell)
        .build()
        .unwrap();
    let mut pool = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile {
            provider: ModelProvider("anthropic".to_owned()),
            max_context_tokens: Some(200_000),
        },
        &schema_ctx(),
    )
    .await
    .unwrap();

    assert_eq!(
        pool.journal_authority("Bash"),
        harness_tool::ToolJournalAuthority::Sandbox
    );

    pool.append_runtime_tool(Arc::new(tool(
        "Bash",
        plugin_origin("shadow", TrustLevel::UserControlled),
        TrustLevel::UserControlled,
    )));

    assert_eq!(
        pool.journal_authority("Bash"),
        harness_tool::ToolJournalAuthority::Sandbox
    );
    assert_eq!(
        pool.descriptor("Bash").expect("Bash descriptor").origin,
        ToolOrigin::Builtin
    );

    let prompt_visible = pool.prompt_visible_descriptors();
    let bash_descriptors = prompt_visible
        .iter()
        .filter(|descriptor| descriptor.name == "Bash")
        .collect::<Vec<_>>();
    assert_eq!(bash_descriptors.len(), 1);
    assert_eq!(bash_descriptors[0].origin, ToolOrigin::Builtin);

    let filtered = pool.filtered(&ToolPoolFilter::default());
    assert_eq!(
        filtered.descriptor("Bash").expect("Bash descriptor").origin,
        ToolOrigin::Builtin
    );
    assert_eq!(
        filtered.journal_authority("Bash"),
        harness_tool::ToolJournalAuthority::Sandbox
    );
}

#[tokio::test]
async fn prompt_visible_descriptors_preserve_deferred_descriptor_for_runtime_same_name() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(tool_with_origin(
            "deferred",
            DeferPolicy::ForceDefer,
            ToolGroup::Search,
            ProviderRestriction::All,
            false,
            ToolOrigin::Builtin,
        )))
        .build()
        .unwrap();
    let mut pool = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &ToolSearchMode::Always,
        &ToolPoolModelProfile {
            provider: ModelProvider("anthropic".to_owned()),
            max_context_tokens: Some(200_000),
        },
        &schema_ctx(),
    )
    .await
    .unwrap();

    pool.append_runtime_tool(Arc::new(tool(
        "deferred",
        plugin_origin("shadow", TrustLevel::UserControlled),
        TrustLevel::UserControlled,
    )));

    assert_eq!(
        pool.get("deferred")
            .expect("deferred tool")
            .descriptor()
            .origin,
        ToolOrigin::Builtin
    );
    assert_eq!(
        pool.descriptor("deferred")
            .expect("deferred descriptor")
            .origin,
        ToolOrigin::Builtin
    );
    let prompt_visible = pool.prompt_visible_descriptors();
    assert!(
        prompt_visible
            .iter()
            .all(|descriptor| descriptor.name != "deferred"),
        "unmaterialized deferred tools must not be prompt-visible, and same-name runtime tools must not impersonate them"
    );

    let filtered_without_deferred = pool.filtered(&ToolPoolFilter {
        group_denylist: HashSet::from([ToolGroup::Search]),
        ..Default::default()
    });
    assert!(
        filtered_without_deferred
            .prompt_visible_descriptors()
            .iter()
            .all(|descriptor| descriptor.name != "deferred"),
        "filtering out the deferred canonical tool must not promote a same-name runtime tool"
    );
    assert!(filtered_without_deferred.descriptor("deferred").is_none());
    assert!(filtered_without_deferred.get("deferred").is_none());

    assert_eq!(
        pool.materialize_deferred_tools(&["deferred".to_owned()]),
        ["deferred"]
    );
    let materialized = pool.prompt_visible_descriptors();
    let deferred = materialized
        .iter()
        .filter(|descriptor| descriptor.name == "deferred")
        .collect::<Vec<_>>();
    assert_eq!(deferred.len(), 1);
    assert_eq!(deferred[0].origin, ToolOrigin::Builtin);

    let filtered = pool.filtered(&ToolPoolFilter::default());
    let filtered_visible = filtered.prompt_visible_descriptors();
    let filtered_deferred = filtered_visible
        .iter()
        .filter(|descriptor| descriptor.name == "deferred")
        .collect::<Vec<_>>();
    assert_eq!(filtered_deferred.len(), 1);
    assert_eq!(filtered_deferred[0].origin, ToolOrigin::Builtin);
}

#[tokio::test]
async fn pool_applies_allowlists_origin_filters_and_provider_denylists() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(tool_with_origin(
            "alpha",
            DeferPolicy::AlwaysLoad,
            ToolGroup::FileSystem,
            ProviderRestriction::All,
            false,
            ToolOrigin::Builtin,
        )))
        .with_tool(Box::new(tool_with_origin(
            "blocked_provider",
            DeferPolicy::AlwaysLoad,
            ToolGroup::FileSystem,
            ProviderRestriction::Denylist(BTreeSet::from([ModelProvider("anthropic".to_owned())])),
            false,
            ToolOrigin::Builtin,
        )))
        .with_tool(Box::new(tool_with_origin(
            "custom_group",
            DeferPolicy::AlwaysLoad,
            ToolGroup::Custom("ops".to_owned()),
            ProviderRestriction::All,
            false,
            ToolOrigin::Builtin,
        )))
        .with_tool(Box::new(tool_with_origin(
            "mcp_tool",
            DeferPolicy::AlwaysLoad,
            ToolGroup::FileSystem,
            ProviderRestriction::All,
            false,
            mcp_origin("workspace-server", TrustLevel::AdminTrusted),
        )))
        .with_tool(Box::new(tool_with_origin(
            "plugin_tool",
            DeferPolicy::AlwaysLoad,
            ToolGroup::FileSystem,
            ProviderRestriction::All,
            false,
            plugin_origin("admin-plugin", TrustLevel::AdminTrusted),
        )))
        .build()
        .unwrap();

    let mut filter = ToolPoolFilter {
        allowlist: Some(HashSet::from([
            "alpha".to_owned(),
            "blocked_provider".to_owned(),
            "custom_group".to_owned(),
            "mcp_tool".to_owned(),
            "plugin_tool".to_owned(),
        ])),
        mcp_included: false,
        plugin_included: false,
        group_allowlist: Some(HashSet::from([ToolGroup::FileSystem])),
        ..Default::default()
    };

    let pool = ToolPool::assemble(
        &registry.snapshot(),
        &filter,
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile {
            provider: ModelProvider("anthropic".to_owned()),
            max_context_tokens: Some(200_000),
        },
        &schema_ctx(),
    )
    .await
    .unwrap();

    assert_eq!(names(pool.always_loaded()), ["alpha"]);

    filter.allowlist = Some(HashSet::from(["mcp_tool".to_owned()]));
    filter.mcp_included = true;
    filter.group_allowlist = None;
    let pool = ToolPool::assemble(
        &registry.snapshot(),
        &filter,
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile::default(),
        &schema_ctx(),
    )
    .await
    .unwrap();

    assert_eq!(names(pool.always_loaded()), ["mcp_tool"]);
}

#[tokio::test]
async fn pool_resolves_dynamic_schema_once_during_assembly() {
    let count = Arc::new(AtomicUsize::new(0));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(dynamic_tool(Arc::clone(&count))))
        .build()
        .unwrap();

    let pool = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile::default(),
        &schema_ctx(),
    )
    .await
    .unwrap();

    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(
        pool.descriptor("dynamic").unwrap().input_schema,
        json!({ "type": "object", "title": "resolved" })
    );
}

#[tokio::test]
async fn force_defer_tools_fail_with_deferral_required_when_tool_search_is_disabled() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(tool_with(
            "must_defer",
            DeferPolicy::ForceDefer,
            ToolGroup::Search,
            ProviderRestriction::All,
            false,
        )))
        .build()
        .unwrap();

    let result = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile::default(),
        &schema_ctx(),
    )
    .await;
    let Err(error) = result else {
        panic!("force defer should fail when tool search is disabled");
    };

    assert!(matches!(
        error,
        ToolError::DeferralRequired { ref tool } if tool == "must_defer"
    ));
}

#[tokio::test]
async fn auto_mode_defers_auto_defer_tools_when_threshold_is_met_without_tool_reference() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(tool_with_input_schema(
            "large_auto",
            DeferPolicy::AutoDefer,
            json!({
                "type": "object",
                "description": "x".repeat(30_000)
            }),
        )))
        .build()
        .unwrap();

    let pool = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &ToolSearchMode::Auto {
            ratio: 0.10,
            min_absolute_tokens: 100,
        },
        &ToolPoolModelProfile {
            provider: ModelProvider("local".to_owned()),
            max_context_tokens: Some(1_000),
        },
        &schema_ctx(),
    )
    .await
    .unwrap();

    assert_eq!(names(pool.always_loaded()), Vec::<&str>::new());
    assert_eq!(names(pool.deferred()), ["large_auto"]);
}

#[tokio::test]
async fn auto_mode_keeps_auto_defer_tools_loaded_when_threshold_is_not_met() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(tool_with(
            "small_auto",
            DeferPolicy::AutoDefer,
            ToolGroup::FileSystem,
            ProviderRestriction::All,
            false,
        )))
        .build()
        .unwrap();

    let pool = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &ToolSearchMode::Auto {
            ratio: 0.10,
            min_absolute_tokens: 4_000,
        },
        &ToolPoolModelProfile {
            provider: ModelProvider("anthropic".to_owned()),
            max_context_tokens: Some(200_000),
        },
        &schema_ctx(),
    )
    .await
    .unwrap();

    assert_eq!(names(pool.always_loaded()), ["small_auto"]);
    assert_eq!(names(pool.deferred()), Vec::<&str>::new());
}

#[test]
fn registry_rejects_user_controlled_tools_requesting_admin_only_capability() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .unwrap();
    let error = registry
        .register(Box::new(tool_requiring_subagent_runner()))
        .unwrap_err();

    assert!(matches!(
        error,
        RegistrationError::CapabilityNotPermitted { .. }
    ));
}

#[test]
fn tool_crate_does_not_depend_on_harness_model() {
    #[cfg(not(feature = "minimax-tools"))]
    {
        let manifest =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
        assert!(manifest.contains(
            "jyowo-harness-model = { path = \"../jyowo-harness-model\", optional = true }"
        ));
        assert!(manifest.contains(
            "minimax-tools = [\"builtin-toolset\", \"dep:base64\", \"dep:jyowo-harness-model\", \"jyowo-harness-model/minimax\"]"
        ));
    }
}

fn names(tools: &[Arc<dyn Tool>]) -> Vec<&str> {
    tools
        .iter()
        .map(|tool| tool.descriptor().name.as_str())
        .collect()
}

fn schema_ctx() -> SchemaResolverContext {
    SchemaResolverContext {
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: harness_contracts::TenantId::SINGLE,
    }
}

fn tool(name: &str, origin: ToolOrigin, trust_level: TrustLevel) -> TestTool {
    TestTool {
        descriptor: descriptor(
            name,
            DeferPolicy::AlwaysLoad,
            ToolGroup::FileSystem,
            ProviderRestriction::All,
            origin,
            trust_level,
            false,
        ),
        resolved_schema: None,
        resolve_count: None,
    }
}

fn tool_with(
    name: &str,
    defer_policy: DeferPolicy,
    group: ToolGroup,
    provider_restriction: ProviderRestriction,
    dynamic_schema: bool,
) -> TestTool {
    TestTool {
        descriptor: descriptor(
            name,
            defer_policy,
            group,
            provider_restriction,
            ToolOrigin::Builtin,
            TrustLevel::AdminTrusted,
            dynamic_schema,
        ),
        resolved_schema: None,
        resolve_count: None,
    }
}

fn tool_with_origin(
    name: &str,
    defer_policy: DeferPolicy,
    group: ToolGroup,
    provider_restriction: ProviderRestriction,
    dynamic_schema: bool,
    origin: ToolOrigin,
) -> TestTool {
    TestTool {
        descriptor: descriptor(
            name,
            defer_policy,
            group,
            provider_restriction,
            origin,
            TrustLevel::AdminTrusted,
            dynamic_schema,
        ),
        resolved_schema: None,
        resolve_count: None,
    }
}

fn dynamic_tool(resolve_count: Arc<AtomicUsize>) -> TestTool {
    TestTool {
        descriptor: descriptor(
            "dynamic",
            DeferPolicy::AlwaysLoad,
            ToolGroup::FileSystem,
            ProviderRestriction::All,
            ToolOrigin::Builtin,
            TrustLevel::AdminTrusted,
            true,
        ),
        resolved_schema: Some(json!({ "type": "object", "title": "resolved" })),
        resolve_count: Some(resolve_count),
    }
}

fn tool_with_input_schema(name: &str, defer_policy: DeferPolicy, input_schema: Value) -> TestTool {
    let mut descriptor = descriptor(
        name,
        defer_policy,
        ToolGroup::FileSystem,
        ProviderRestriction::All,
        ToolOrigin::Builtin,
        TrustLevel::AdminTrusted,
        false,
    );
    descriptor.input_schema = input_schema;
    TestTool {
        descriptor,
        resolved_schema: None,
        resolve_count: None,
    }
}

fn tool_requiring_subagent_runner() -> TestTool {
    let mut descriptor = descriptor(
        "agent",
        DeferPolicy::AlwaysLoad,
        ToolGroup::Agent,
        ProviderRestriction::All,
        plugin_origin("user-plugin", TrustLevel::UserControlled),
        TrustLevel::UserControlled,
        false,
    );
    descriptor.required_capabilities = vec![harness_contracts::ToolCapability::SubagentRunner];
    TestTool {
        descriptor,
        resolved_schema: None,
        resolve_count: None,
    }
}

fn descriptor(
    name: &str,
    defer_policy: DeferPolicy,
    group: ToolGroup,
    provider_restriction: ProviderRestriction,
    origin: ToolOrigin,
    trust_level: TrustLevel,
    dynamic_schema: bool,
) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        display_name: name.to_owned(),
        description: format!("{name} tool"),
        category: "test".to_owned(),
        group,
        version: "0.0.1".to_owned(),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        dynamic_schema,
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_read_only: true,
            is_destructive: false,
            long_running: None,
            defer_policy,
        },
        trust_level,
        required_capabilities: vec![],
        budget: default_result_budget(),
        provider_restriction,
        origin,
        search_hint: None,
    }
}

fn plugin_origin(plugin_id: &str, trust: TrustLevel) -> ToolOrigin {
    ToolOrigin::Plugin {
        plugin_id: PluginId(plugin_id.to_owned()),
        trust,
    }
}

fn mcp_origin(server_id: &str, trust: TrustLevel) -> ToolOrigin {
    ToolOrigin::Mcp(McpOrigin {
        server_id: McpServerId(server_id.to_owned()),
        upstream_name: "test/tool".to_owned(),
        server_meta: BTreeMap::new(),
        server_source: McpServerSource::Workspace,
        server_trust: trust,
    })
}

fn skill_origin(skill_id: &str, trust: TrustLevel) -> ToolOrigin {
    ToolOrigin::Skill(SkillOrigin {
        skill_id: SkillId(skill_id.to_owned()),
        skill_name: skill_id.to_owned(),
        source_kind: SkillSourceKind::User,
        trust,
    })
}
