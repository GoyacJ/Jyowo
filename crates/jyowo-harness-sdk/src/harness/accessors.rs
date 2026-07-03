use super::*;

impl Harness {
    pub async fn resolve_elicitation(
        &self,
        request_id: harness_contracts::RequestId,
        response: serde_json::Value,
    ) -> Result<(), HarnessError> {
        let Some(handler) = &self.inner.stream_elicitation_handler else {
            return Err(HarnessError::Other(
                "elicitation resolver is not configured".to_owned(),
            ));
        };

        handler
            .resolve_elicitation(request_id, response)
            .await
            .map_err(|error| HarnessError::Other(error.to_string()))
    }

    pub fn options(&self) -> &HarnessOptions {
        &self.inner.options
    }

    #[must_use]
    pub fn model_provider(&self) -> Arc<dyn ModelProvider> {
        Arc::clone(&self.inner.model)
    }

    #[must_use]
    pub fn mcp_sampling_provider(
        &self,
        tenant_id: TenantId,
        session_id: Option<harness_contracts::SessionId>,
        run_id: Option<RunId>,
    ) -> HarnessSamplingProvider {
        HarnessSamplingProvider::new(
            Arc::clone(&self.inner.model),
            self.inner.options.model_id.clone(),
            tenant_id,
            session_id,
            run_id,
        )
    }

    pub fn sandbox(&self) -> Arc<dyn SandboxBackend> {
        Arc::clone(&self.inner.sandbox)
    }

    #[must_use]
    pub fn permission_broker(&self) -> Option<Arc<dyn PermissionBroker>> {
        Some(Arc::clone(&self.inner.permission_broker))
    }

    #[must_use]
    pub fn permission_authority(&self) -> Option<Arc<harness_permission::PermissionAuthority>> {
        Some(Arc::clone(&self.inner.permission_authority))
    }

    #[must_use]
    pub fn authorization_service(&self) -> Arc<harness_execution::AuthorizationService> {
        Arc::clone(&self.inner.authorization_service)
    }

    #[cfg(feature = "stream-permission")]
    #[must_use]
    pub fn permission_resolver_handle(&self) -> Option<ResolverHandle> {
        self.inner.permission_resolver.clone()
    }

    #[must_use]
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.inner.tool_registry
    }

    #[must_use]
    pub fn provider_capability_routes(
        &self,
    ) -> Arc<parking_lot::RwLock<ProviderCapabilityRouteSettings>> {
        Arc::clone(&self.inner.provider_capability_routes)
    }

    #[must_use]
    pub fn hook_dispatcher(&self) -> HookDispatcher {
        HookDispatcher::new(self.inner.hook_registry.snapshot())
    }

    #[must_use]
    pub fn memory_provider(&self) -> Option<Arc<dyn MemoryProvider>> {
        self.effective_memory_provider()
    }

    pub fn blob_store(&self) -> Option<Arc<dyn BlobStore>> {
        self.inner.blob_store.as_ref().map(Arc::clone)
    }

    pub fn mcp_config(&self) -> Option<&McpConfig> {
        self.inner.mcp_config.as_ref()
    }

    #[must_use]
    pub fn elicitation_handler(&self) -> Option<Arc<dyn ElicitationHandler>> {
        self.inner.elicitation_handler.as_ref().map(Arc::clone)
    }

    #[must_use]
    pub fn plugin_registry(&self) -> Option<&harness_plugin::PluginRegistry> {
        self.inner.plugin_registry.as_ref()
    }

    #[must_use]
    pub fn tracer(&self) -> Option<Arc<dyn Tracer>> {
        self.inner.tracer.as_ref().map(Arc::clone)
    }

    #[must_use]
    pub fn observer(&self) -> Option<Arc<Observer>> {
        self.inner.observer.as_ref().map(Arc::clone)
    }

    #[must_use]
    pub fn aux_model(&self) -> Option<Arc<dyn AuxModelProvider>> {
        self.inner.aux_model.as_ref().map(Arc::clone)
    }

    #[must_use]
    pub fn rule_providers(&self) -> &[Arc<dyn RuleProvider>] {
        &self.inner.rule_providers
    }

    #[must_use]
    pub fn enabled_features(&self) -> &HashSet<String> {
        &self.inner.enabled_features
    }

    #[must_use]
    pub fn enabled_feature_set() -> HashSet<String> {
        let mut features = HashSet::new();
        for feature in compiled_features() {
            features.insert(feature.to_owned());
        }
        features
    }

    #[must_use]
    pub fn resolve_agent_capabilities(
        &self,
        context: crate::AgentCapabilityResolutionContext,
    ) -> harness_agent_runtime::ResolvedAgentCapabilityPolicy {
        crate::agent_runtime::resolve_agent_capabilities_with_context(
            &self.inner.options.workspace_root,
            context,
        )
    }
}

fn compiled_features() -> Vec<&'static str> {
    let mut features = Vec::new();
    push_feature(
        &mut features,
        "sqlite-store",
        cfg!(feature = "sqlite-store"),
    );
    push_feature(&mut features, "jsonl-store", cfg!(feature = "jsonl-store"));
    push_feature(
        &mut features,
        "in-memory-store",
        cfg!(feature = "in-memory-store"),
    );
    push_feature(&mut features, "blob-file", cfg!(feature = "blob-file"));
    push_feature(&mut features, "blob-sqlite", cfg!(feature = "blob-sqlite"));
    push_feature(
        &mut features,
        "provider-openai",
        cfg!(feature = "provider-openai"),
    );
    push_feature(
        &mut features,
        "provider-anthropic",
        cfg!(feature = "provider-anthropic"),
    );
    push_feature(
        &mut features,
        "provider-gemini",
        cfg!(feature = "provider-gemini"),
    );
    push_feature(
        &mut features,
        "provider-openrouter",
        cfg!(feature = "provider-openrouter"),
    );
    push_feature(
        &mut features,
        "provider-bedrock",
        cfg!(feature = "provider-bedrock"),
    );
    push_feature(
        &mut features,
        "provider-codex",
        cfg!(feature = "provider-codex"),
    );
    push_feature(
        &mut features,
        "provider-local-llama",
        cfg!(feature = "provider-local-llama"),
    );
    push_feature(
        &mut features,
        "provider-deepseek",
        cfg!(feature = "provider-deepseek"),
    );
    push_feature(
        &mut features,
        "provider-minimax",
        cfg!(feature = "provider-minimax"),
    );
    push_feature(
        &mut features,
        "provider-qwen",
        cfg!(feature = "provider-qwen"),
    );
    push_feature(
        &mut features,
        "provider-doubao",
        cfg!(feature = "provider-doubao"),
    );
    push_feature(
        &mut features,
        "provider-zhipu",
        cfg!(feature = "provider-zhipu"),
    );
    push_feature(&mut features, "provider-km", cfg!(feature = "provider-km"));
    push_feature(
        &mut features,
        "local-sandbox",
        cfg!(feature = "local-sandbox"),
    );
    push_feature(
        &mut features,
        "docker-sandbox",
        cfg!(feature = "docker-sandbox"),
    );
    push_feature(&mut features, "ssh-sandbox", cfg!(feature = "ssh-sandbox"));
    push_feature(
        &mut features,
        "noop-sandbox",
        cfg!(feature = "noop-sandbox"),
    );
    push_feature(&mut features, "mcp-stdio", cfg!(feature = "mcp-stdio"));
    push_feature(&mut features, "mcp-http", cfg!(feature = "mcp-http"));
    push_feature(
        &mut features,
        "mcp-websocket",
        cfg!(feature = "mcp-websocket"),
    );
    push_feature(&mut features, "mcp-sse", cfg!(feature = "mcp-sse"));
    push_feature(
        &mut features,
        "mcp-in-process",
        cfg!(feature = "mcp-in-process"),
    );
    push_feature(
        &mut features,
        "mcp-server-adapter",
        cfg!(feature = "mcp-server-adapter"),
    );
    push_feature(
        &mut features,
        "interactive-permission",
        cfg!(feature = "interactive-permission"),
    );
    push_feature(
        &mut features,
        "stream-permission",
        cfg!(feature = "stream-permission"),
    );
    push_feature(
        &mut features,
        "rule-engine-permission",
        cfg!(feature = "rule-engine-permission"),
    );
    push_feature(
        &mut features,
        "memory-builtin",
        cfg!(feature = "memory-builtin"),
    );
    push_feature(
        &mut features,
        "memory-external-slot",
        cfg!(feature = "memory-external-slot"),
    );
    push_feature(
        &mut features,
        "agents-subagent",
        cfg!(feature = "agents-subagent"),
    );
    push_feature(&mut features, "agents-team", cfg!(feature = "agents-team"));
    push_feature(
        &mut features,
        "observability-replay",
        cfg!(feature = "observability-replay"),
    );
    push_feature(
        &mut features,
        "observability-otel",
        cfg!(feature = "observability-otel"),
    );
    push_feature(
        &mut features,
        "observability-prometheus",
        cfg!(feature = "observability-prometheus"),
    );
    push_feature(
        &mut features,
        "observability-redactor",
        cfg!(feature = "observability-redactor"),
    );
    push_feature(
        &mut features,
        "plugin-dynamic-load",
        cfg!(feature = "plugin-dynamic-load"),
    );
    push_feature(
        &mut features,
        "plugin-manifest-sign",
        cfg!(feature = "plugin-manifest-sign"),
    );
    push_feature(
        &mut features,
        "builtin-toolset",
        cfg!(feature = "builtin-toolset"),
    );
    push_feature(&mut features, "tool-search", cfg!(feature = "tool-search"));
    push_feature(
        &mut features,
        "tool-loading-anthropic",
        cfg!(feature = "tool-loading-anthropic"),
    );
    push_feature(
        &mut features,
        "tool-loading-inline",
        cfg!(feature = "tool-loading-inline"),
    );
    push_feature(
        &mut features,
        "tool-search-default-scorer",
        cfg!(feature = "tool-search-default-scorer"),
    );
    push_feature(
        &mut features,
        "programmatic-tool-calling",
        cfg!(feature = "programmatic-tool-calling"),
    );
    push_feature(
        &mut features,
        "steering-queue",
        cfg!(feature = "steering-queue"),
    );
    push_feature(&mut features, "testing", cfg!(feature = "testing"));
    features
}

fn push_feature(features: &mut Vec<&'static str>, name: &'static str, enabled: bool) {
    if enabled {
        features.push(name);
    }
}
