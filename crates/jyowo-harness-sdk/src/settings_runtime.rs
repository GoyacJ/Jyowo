use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{
    Harness, HarnessBuilder, HarnessError, HarnessOptions, McpConfig, RuntimeSkillSummary,
    RuntimeSkillView, SkillConfigSnapshot, Unset,
};

/// Desktop-only facade for configuration, catalog, and diagnostics APIs.
///
/// Task execution is owned by the daemon. The desktop shell stores this facade
/// only for the reusable non-task APIs that have not moved into the daemon
/// protocol.
#[derive(Clone)]
pub struct DesktopSettingsRuntime {
    inner: Harness,
}

impl DesktopSettingsRuntime {
    #[must_use]
    pub fn builder() -> HarnessBuilder<Unset, Unset, Unset> {
        #[cfg(feature = "memory-provider-registry")]
        {
            return Harness::builder().without_memory_runtime();
        }
        #[cfg(not(feature = "memory-provider-registry"))]
        Harness::builder()
    }

    #[must_use]
    pub fn options(&self) -> &HarnessOptions {
        self.inner.options()
    }

    #[must_use]
    pub fn model_provider(&self) -> Arc<dyn harness_model::ModelProvider> {
        self.inner.model_provider()
    }

    #[must_use]
    pub fn provider_capability_routes(
        &self,
    ) -> Arc<parking_lot::RwLock<harness_contracts::ProviderCapabilityRouteSettings>> {
        self.inner.provider_capability_routes()
    }

    #[must_use]
    pub fn mcp_config(&self) -> Option<&McpConfig> {
        self.inner.mcp_config()
    }

    #[must_use]
    pub fn authorization_service(&self) -> Arc<harness_execution::AuthorizationService> {
        self.inner.authorization_service()
    }

    #[must_use]
    pub fn tool_registry(&self) -> &harness_tool::ToolRegistry {
        self.inner.tool_registry()
    }

    #[must_use]
    pub fn skill_registry(&self) -> &harness_skill::SkillRegistry {
        self.inner.skill_registry()
    }

    #[cfg(feature = "stream-permission")]
    #[must_use]
    pub fn permission_resolver_handle(&self) -> Option<harness_permission::ResolverHandle> {
        self.inner.permission_resolver_handle()
    }

    #[must_use]
    pub fn plugin_registry(&self) -> Option<&harness_plugin::PluginRegistry> {
        self.inner.plugin_registry()
    }

    #[must_use]
    pub fn runtime_execution_status(&self) -> harness_contracts::RuntimeExecutionStatus {
        self.inner.runtime_execution_status()
    }

    pub fn list_runtime_skills(
        &self,
    ) -> Result<Vec<RuntimeSkillSummary>, crate::SkillConfigStoreError> {
        self.inner.list_runtime_skills()
    }

    pub fn view_runtime_skill(
        &self,
        name: &str,
        full: bool,
    ) -> Result<Option<RuntimeSkillView>, crate::SkillConfigStoreError> {
        self.inner.view_runtime_skill(name, full)
    }

    pub fn replace_skill_config_snapshot(&self, snapshot: SkillConfigSnapshot) {
        self.inner.replace_skill_config_snapshot(snapshot);
    }

    pub async fn validate_workspace_skill_markdown(
        &self,
        markdown: &str,
        source_path: Option<PathBuf>,
    ) -> Result<RuntimeSkillView, HarnessError> {
        self.inner
            .validate_workspace_skill_markdown(markdown, source_path)
            .await
    }

    pub async fn reload_user_managed_skills_with_allowed_package_ids(
        &self,
        enabled_dir: impl AsRef<Path>,
        allowed_package_ids: Option<BTreeSet<String>>,
    ) -> Result<(), HarnessError> {
        self.inner
            .reload_user_managed_skills_with_allowed_package_ids(enabled_dir, allowed_package_ids)
            .await
    }
}

impl TryFrom<Harness> for DesktopSettingsRuntime {
    type Error = HarnessError;

    fn try_from(inner: Harness) -> Result<Self, Self::Error> {
        #[cfg(feature = "memory-provider-registry")]
        if inner.owns_memory_runtime() {
            return Err(HarnessError::Internal(
                "desktop settings runtime must not own memory runtime state".to_owned(),
            ));
        }
        Ok(Self { inner })
    }
}

#[cfg(all(test, feature = "memory-provider-registry"))]
mod tests {
    use super::*;
    use harness_contracts::NoopRedactor;
    use harness_journal::InMemoryEventStore;
    use harness_model::testing::TestModelProvider;
    use harness_sandbox::NoopSandbox;

    #[tokio::test]
    async fn settings_builder_strips_all_memory_runtime_ownership() {
        let workspace = std::env::temp_dir().join(format!(
            "jyowo-settings-runtime-ownership-{}",
            harness_contracts::SessionId::new()
        ));
        std::fs::create_dir_all(&workspace).expect("workspace");
        let harness = DesktopSettingsRuntime::builder()
            .with_workspace_root(&workspace)
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("settings harness");

        assert!(
            !harness.owns_memory_runtime(),
            "settings harness must not retain memory database, providers, extraction, or plugin registrations"
        );
        DesktopSettingsRuntime::try_from(harness).expect("ownership-free settings runtime");
    }
}
