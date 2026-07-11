use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{
    Harness, HarnessBuilder, HarnessError, HarnessOptions, McpConfig, RuntimeSkillSummary,
    RuntimeSkillView, SessionOptions, Unset,
};

/// Desktop-only facade for configuration, catalog, memory, and diagnostics APIs.
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

    pub fn list_runtime_skills(&self) -> Vec<RuntimeSkillSummary> {
        self.inner.list_runtime_skills()
    }

    pub fn view_runtime_skill(&self, name: &str, full: bool) -> Option<RuntimeSkillView> {
        self.inner.view_runtime_skill(name, full)
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

    #[cfg(feature = "memory-provider-registry")]
    pub async fn list_memory_items(
        &self,
        options: SessionOptions,
    ) -> Result<Vec<harness_memory::MemorySummary>, HarnessError> {
        self.inner.list_memory_items(options).await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn get_memory_item(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
    ) -> Result<harness_memory::MemoryRecord, HarnessError> {
        self.inner.get_memory_item(options, id).await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn update_memory_item_content(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
        content: impl Into<String>,
        action_plan_id: Option<harness_contracts::ActionPlanId>,
    ) -> Result<harness_memory::MemoryRecord, HarnessError> {
        self.inner
            .update_memory_item_content(options, id, content, action_plan_id)
            .await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn delete_memory_item(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
        action_plan_id: Option<harness_contracts::ActionPlanId>,
    ) -> Result<(), HarnessError> {
        self.inner
            .delete_memory_item(options, id, action_plan_id)
            .await
    }

    #[cfg(feature = "memory-provider-registry")]
    #[allow(clippy::too_many_arguments)]
    pub async fn export_memory_items(
        &self,
        options: SessionOptions,
        scope: &str,
        format: &str,
        include_raw_content: bool,
        include_metadata: bool,
        include_hashes: bool,
        explicit_user_action: bool,
    ) -> Result<crate::harness::MemoryExportFile, HarnessError> {
        self.inner
            .export_memory_items(
                options,
                scope,
                format,
                include_raw_content,
                include_metadata,
                include_hashes,
                explicit_user_action,
            )
            .await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn list_memory_candidates(
        &self,
        options: SessionOptions,
        request: harness_contracts::ListMemoryCandidatesRequest,
    ) -> Result<harness_contracts::ListMemoryCandidatesResponse, HarnessError> {
        self.inner.list_memory_candidates(options, request).await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn approve_memory_candidate(
        &self,
        options: SessionOptions,
        request: harness_contracts::ApproveMemoryCandidateRequest,
    ) -> Result<harness_contracts::ApproveMemoryCandidateResponse, HarnessError> {
        self.inner.approve_memory_candidate(options, request).await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn reject_memory_candidate(
        &self,
        options: SessionOptions,
        request: harness_contracts::RejectMemoryCandidateRequest,
    ) -> Result<harness_contracts::RejectMemoryCandidateResponse, HarnessError> {
        self.inner.reject_memory_candidate(options, request).await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn merge_memory_candidate(
        &self,
        options: SessionOptions,
        request: harness_contracts::MergeMemoryCandidateRequest,
    ) -> Result<harness_contracts::MergeMemoryCandidateResponse, HarnessError> {
        self.inner.merge_memory_candidate(options, request).await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn get_memory_settings(
        &self,
        options: SessionOptions,
        request: harness_contracts::GetMemorySettingsRequest,
    ) -> Result<harness_contracts::GetMemorySettingsResponse, HarnessError> {
        self.inner.get_memory_settings(options, request).await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn update_memory_settings(
        &self,
        options: SessionOptions,
        request: harness_contracts::UpdateMemorySettingsRequest,
    ) -> Result<harness_contracts::UpdateMemorySettingsResponse, HarnessError> {
        self.inner.update_memory_settings(options, request).await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn get_thread_memory_settings(
        &self,
        options: SessionOptions,
        request: harness_contracts::GetThreadMemorySettingsRequest,
    ) -> Result<harness_contracts::GetThreadMemorySettingsResponse, HarnessError> {
        self.inner
            .get_thread_memory_settings(options, request)
            .await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn update_thread_memory_settings(
        &self,
        options: SessionOptions,
        request: harness_contracts::UpdateThreadMemorySettingsRequest,
    ) -> Result<harness_contracts::UpdateThreadMemorySettingsResponse, HarnessError> {
        self.inner
            .update_thread_memory_settings(options, request)
            .await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn list_memory_recall_traces(
        &self,
        options: SessionOptions,
        request: harness_contracts::ListMemoryRecallTracesRequest,
    ) -> Result<harness_contracts::ListMemoryRecallTracesResponse, HarnessError> {
        self.inner.list_memory_recall_traces(options, request).await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn get_memory_recall_trace(
        &self,
        options: SessionOptions,
        request: harness_contracts::GetMemoryRecallTraceRequest,
    ) -> Result<harness_contracts::GetMemoryRecallTraceResponse, HarnessError> {
        self.inner.get_memory_recall_trace(options, request).await
    }

    #[cfg(feature = "memory-provider-registry")]
    pub async fn get_model_request_preview(
        &self,
        options: SessionOptions,
        request: harness_contracts::GetModelRequestPreviewRequest,
    ) -> Result<harness_contracts::GetModelRequestPreviewResponse, HarnessError> {
        self.inner.get_model_request_preview(options, request).await
    }
}

impl From<Harness> for DesktopSettingsRuntime {
    fn from(inner: Harness) -> Self {
        Self { inner }
    }
}
