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
use super::runtime::*;
#[allow(unused_imports)]
use super::skills::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;
use fs2::FileExt;
use jyowo_harness_sdk::AgentCapabilityResolutionContext;

#[derive(Clone)]
pub(crate) struct DesktopProviderCredentialResolver {
    provider_settings_store: Arc<dyn ProviderSettingsStore>,
    provider_capability_routes: Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
}

impl DesktopProviderCredentialResolver {
    pub(crate) fn new(
        _conversation_metadata_store: Arc<dyn ConversationMetadataStore>,
        provider_settings_store: Arc<dyn ProviderSettingsStore>,
        provider_capability_routes: Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
    ) -> Self {
        Self {
            provider_settings_store,
            provider_capability_routes,
        }
    }
}

impl ProviderCredentialResolverCap for DesktopProviderCredentialResolver {
    fn resolve_provider_credential(
        &self,
        context: ProviderCredentialResolveContext,
    ) -> futures::future::BoxFuture<'_, Result<ProviderCredential, ToolError>> {
        Box::pin(async move {
            if let (Some(operation_id), Some(route_kind)) =
                (context.operation_id.clone(), context.route_kind)
            {
                let routes = context_provider_capability_route(
                    &self.provider_capability_routes,
                    &context.provider_id,
                    &operation_id,
                    route_kind,
                )?;
                let record = self
                    .provider_settings_store
                    .load_record()
                    .map_err(|error| ToolError::PermissionDenied(error.message))?
                    .ok_or_else(|| {
                        ToolError::PermissionDenied(
                            "provider service credential resolution is unavailable".to_owned(),
                        )
                    })?;
                let selected = record
                    .configs
                    .iter()
                    .find(|config| config.id == routes.config_id)
                    .ok_or_else(|| {
                        ToolError::PermissionDenied(
                            "provider service credential resolution is unavailable".to_owned(),
                        )
                    })?;
                if selected.provider_id != context.provider_id
                    || selected.provider_id != routes.provider_id
                {
                    return Err(ToolError::PermissionDenied(
                        "provider service credential resolution is unavailable".to_owned(),
                    ));
                }
                if selected.api_key.trim().is_empty() {
                    return Err(ToolError::PermissionDenied(
                        "provider service credential resolution is unavailable".to_owned(),
                    ));
                }
                return Ok(ProviderCredential {
                    provider_id: selected.provider_id.clone(),
                    config_id: selected.id.clone(),
                    api_key: selected.api_key.clone(),
                    base_url: selected.base_url.clone(),
                });
            }
            let record = self
                .provider_settings_store
                .load_record()
                .map_err(|error| ToolError::PermissionDenied(error.message))?
                .ok_or_else(|| {
                    ToolError::PermissionDenied(
                        "MiniMax provider config is not configured".to_owned(),
                    )
                })?;
            let selected = context
                .model_config_id
                .as_deref()
                .and_then(|config_id| {
                    record.configs.iter().find(|config| {
                        config.id == config_id && config.provider_id == context.provider_id
                    })
                })
                .ok_or_else(|| {
                    ToolError::PermissionDenied(
                        "MiniMax provider config is not configured".to_owned(),
                    )
                })?;
            if selected.api_key.trim().is_empty() {
                return Err(ToolError::PermissionDenied(
                    "MiniMax provider config has no api key".to_owned(),
                ));
            }
            Ok(ProviderCredential {
                provider_id: selected.provider_id.clone(),
                config_id: selected.id.clone(),
                api_key: selected.api_key.clone(),
                base_url: selected.base_url.clone(),
            })
        })
    }
}

pub fn desktop_provider_credential_resolver_with_stores(
    conversation_metadata_store: Arc<dyn ConversationMetadataStore>,
    provider_settings_store: Arc<dyn ProviderSettingsStore>,
    provider_capability_routes: Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
) -> Arc<dyn ProviderCredentialResolverCap> {
    Arc::new(DesktopProviderCredentialResolver::new(
        conversation_metadata_store,
        provider_settings_store,
        provider_capability_routes,
    ))
}

pub(crate) struct ResolvedCapabilityRoute {
    config_id: String,
    provider_id: String,
}

pub(crate) fn context_provider_capability_route(
    routes: &Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
    provider_id: &str,
    operation_id: &str,
    route_kind: CapabilityRouteKind,
) -> Result<ResolvedCapabilityRoute, ToolError> {
    let routes = routes.read();
    let route = routes.routes.iter().find(|route| {
        route.enabled
            && route.kind == route_kind
            && route.provider_id == provider_id
            && route
                .operation_ids
                .iter()
                .any(|configured| configured == operation_id)
    });
    let route = route.ok_or_else(|| {
        ToolError::PermissionDenied(
            "provider service credential resolution is unavailable".to_owned(),
        )
    })?;
    Ok(ResolvedCapabilityRoute {
        config_id: route.config_id.clone(),
        provider_id: route.provider_id.clone(),
    })
}

pub(crate) fn load_provider_capability_route_settings(
    store: &dyn ProviderCapabilityRouteStore,
) -> Result<ProviderCapabilityRouteSettings, CommandErrorPayload> {
    Ok(store
        .load_record()?
        .unwrap_or_else(empty_provider_capability_route_settings))
}

pub(crate) fn shared_provider_capability_routes_from_store(
    store: &dyn ProviderCapabilityRouteStore,
) -> Result<Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>, CommandErrorPayload> {
    Ok(Arc::new(ParkingRwLock::new(
        load_provider_capability_route_settings(store)?,
    )))
}

pub(crate) fn desktop_provider_service_adapter_availability(
    runtime_state: &DesktopRuntimeState,
) -> ProviderServiceAdapterAvailability {
    runtime_state
        .harness()
        .map(|harness| {
            provider_service_adapter_availability_from_snapshot(&harness.tool_registry().snapshot())
        })
        .unwrap_or_else(default_desktop_provider_service_adapter_availability)
}

pub(crate) fn default_desktop_provider_service_adapter_availability(
) -> ProviderServiceAdapterAvailability {
    ToolRegistryBuilder::new()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .map(|registry| provider_service_adapter_availability_from_snapshot(&registry.snapshot()))
        .unwrap_or_default()
}

pub(crate) fn sync_runtime_provider_capability_routes(
    runtime_state: &DesktopRuntimeState,
    routes: &ProviderCapabilityRouteSettings,
) {
    *runtime_state.provider_capability_routes.write() = routes.clone();
    if let Some(harness) = runtime_state.harness() {
        *harness.provider_capability_routes().write() = routes.clone();
    }
}

#[derive(Clone)]
pub struct DesktopProviderSettingsStore {
    layout: crate::storage_layout::StorageLayout,
    selection_scope: ProviderSettingsSelectionScope,
}

#[derive(Clone)]
enum ProviderSettingsSelectionScope {
    Project { workspace_root: PathBuf },
    GlobalOnly,
}

impl DesktopProviderSettingsStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        let home = execution_settings_home_dir();
        Self::new_with_layout(
            crate::storage_layout::StorageLayout::new(crate::storage_layout::JyowoHome::new(home)),
            workspace_root,
        )
    }

    pub fn new_with_layout(
        layout: crate::storage_layout::StorageLayout,
        workspace_root: PathBuf,
    ) -> Self {
        Self {
            layout,
            selection_scope: ProviderSettingsSelectionScope::Project { workspace_root },
        }
    }

    pub fn global_only() -> Self {
        let home = execution_settings_home_dir();
        Self::global_only_with_layout(crate::storage_layout::StorageLayout::new(
            crate::storage_layout::JyowoHome::new(home),
        ))
    }

    pub fn global_only_with_layout(layout: crate::storage_layout::StorageLayout) -> Self {
        Self {
            layout,
            selection_scope: ProviderSettingsSelectionScope::GlobalOnly,
        }
    }

    pub fn from_runtime_layout(layout: &crate::storage_layout::RuntimeLayout) -> Self {
        match layout.workspace_root.as_ref() {
            Some(workspace_root) => Self::new(workspace_root.clone()),
            None => Self::global_only(),
        }
    }

    fn global_config_store(&self) -> GlobalConfigStore {
        GlobalConfigStore::new(self.layout.clone())
    }

    fn project_config_store(&self) -> Option<ProjectConfigStore> {
        match &self.selection_scope {
            ProviderSettingsSelectionScope::Project { workspace_root } => Some(
                ProjectConfigStore::new(self.layout.clone(), workspace_root.clone()),
            ),
            ProviderSettingsSelectionScope::GlobalOnly => None,
        }
    }

    fn load_selection(&self) -> Result<ProviderSelectionRecord, CommandErrorPayload> {
        if let Some(project_config) = self.project_config_store() {
            project_config.load_project_provider_selection()
        } else {
            self.global_config_store().load_global_provider_selection()
        }
    }

    fn save_selection(&self, record: &ProviderSelectionRecord) -> Result<(), CommandErrorPayload> {
        if let Some(project_config) = self.project_config_store() {
            project_config.save_project_provider_selection(record)
        } else {
            self.global_config_store()
                .save_global_provider_selection(record)
        }
    }
}

impl ProviderSettingsStore for DesktopProviderSettingsStore {
    fn selection_scope(&self) -> SettingsScope {
        match self.selection_scope {
            ProviderSettingsSelectionScope::Project { .. } => SettingsScope::Project,
            ProviderSettingsSelectionScope::GlobalOnly => SettingsScope::Global,
        }
    }

    fn load_record(&self) -> Result<Option<ProviderSettingsRecord>, CommandErrorPayload> {
        let global_config = self.global_config_store();
        let profiles = global_config.load_provider_profiles()?;
        if profiles.is_empty() {
            return Ok(None);
        }

        let selection = self.load_selection()?;
        let mut configs = Vec::with_capacity(profiles.len());
        for profile in profiles {
            let secret = global_config.load_provider_secret(&profile.id)?;
            configs.push(provider_config_record_from_profile(
                profile,
                secret.as_ref(),
            )?);
        }

        Ok(Some(ProviderSettingsRecord {
            default_config_id: selection.default_config_id,
            configs,
        }))
    }

    fn save_record(&self, record: &ProviderSettingsRecord) -> Result<(), CommandErrorPayload> {
        ensure_provider_settings_record(record)?;
        let global_config = self.global_config_store();
        let profiles = record
            .configs
            .iter()
            .map(|config| provider_profile_definition_from_config(config, config.id.clone()))
            .collect::<Vec<_>>();
        global_config.save_provider_profiles(&profiles)?;

        for config in &record.configs {
            global_config.save_provider_secret(&ProviderSecretEntry {
                config_id: config.id.clone(),
                api_key: config.api_key.clone(),
                official_quota_api_key: config.official_quota_api_key.clone(),
            })?;
        }

        self.save_selection(&ProviderSelectionRecord {
            default_config_id: record.default_config_id.clone(),
        })
    }
}

fn provider_config_record_from_profile(
    profile: ProviderProfileDefinition,
    secret: Option<&ProviderSecretEntry>,
) -> Result<ProviderConfigRecord, CommandErrorPayload> {
    Ok(ProviderConfigRecord {
        api_key: secret
            .map(|entry| entry.api_key.clone())
            .unwrap_or_default(),
        protocol: profile.protocol,
        base_url: profile.base_url,
        display_name: profile.display_name,
        id: profile.id,
        model_id: profile.model_id,
        official_quota_api_key: secret.and_then(|entry| entry.official_quota_api_key.clone()),
        provider_id: profile.provider_id,
        model_descriptor: provider_model_descriptor_record_from_profile(profile.model_descriptor)?,
    })
}

fn provider_model_descriptor_record_from_profile(
    descriptor: ProviderProfileModelDescriptor,
) -> Result<ProviderModelDescriptorRecord, CommandErrorPayload> {
    Ok(ProviderModelDescriptorRecord {
        protocol: descriptor.protocol,
        conversation_capability: ConversationModelCapabilityRecord {
            input_modalities: descriptor
                .conversation_capability
                .input_modalities
                .iter()
                .map(|modality| provider_modality_record_from_profile(modality))
                .collect::<Result<Vec<_>, _>>()?,
            output_modalities: descriptor
                .conversation_capability
                .output_modalities
                .iter()
                .map(|modality| provider_modality_record_from_profile(modality))
                .collect::<Result<Vec<_>, _>>()?,
            context_window: descriptor.conversation_capability.context_window,
            max_output_tokens: descriptor.conversation_capability.max_output_tokens,
            streaming: descriptor.conversation_capability.streaming,
            tool_calling: descriptor.conversation_capability.tool_calling,
            reasoning: descriptor.conversation_capability.reasoning,
            prompt_cache: descriptor.conversation_capability.prompt_cache,
            structured_output: descriptor.conversation_capability.structured_output,
        },
        context_window: descriptor.context_window,
        display_name: descriptor.display_name,
        lifecycle: provider_lifecycle_record_from_profile(descriptor.lifecycle),
        max_output_tokens: descriptor.max_output_tokens,
        model_id: descriptor.model_id,
        provider_id: descriptor.provider_id,
    })
}

fn provider_modality_record_from_profile(
    modality: &str,
) -> Result<ProviderModelModalityRecord, CommandErrorPayload> {
    match modality {
        "text" => Ok(ProviderModelModalityRecord::Text),
        "image" => Ok(ProviderModelModalityRecord::Image),
        "audio" => Ok(ProviderModelModalityRecord::Audio),
        "video" => Ok(ProviderModelModalityRecord::Video),
        "file" => Ok(ProviderModelModalityRecord::File),
        "embedding" => Ok(ProviderModelModalityRecord::Embedding),
        _ => Err(runtime_operation_failed(format!(
            "provider profile contains unsupported modality: {modality}"
        ))),
    }
}

fn provider_lifecycle_record_from_profile(
    lifecycle: harness_contracts::ProviderProfileModelLifecycle,
) -> ProviderModelLifecycleRecord {
    match lifecycle {
        harness_contracts::ProviderProfileModelLifecycle::Stable => {
            ProviderModelLifecycleRecord::Stable
        }
        harness_contracts::ProviderProfileModelLifecycle::Preview => {
            ProviderModelLifecycleRecord::Preview
        }
        harness_contracts::ProviderProfileModelLifecycle::Deprecated { retirement_date } => {
            ProviderModelLifecycleRecord::Deprecated { retirement_date }
        }
    }
}

#[derive(Clone)]
pub struct DesktopProviderCapabilityRouteStore {
    workspace_root: PathBuf,
}

impl DesktopProviderCapabilityRouteStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    fn settings_path(&self) -> PathBuf {
        let home = execution_settings_home_dir();
        let layout =
            crate::storage_layout::StorageLayout::new(crate::storage_layout::JyowoHome::new(home));
        layout.project_provider_routes_file(&self.workspace_root)
    }
}

impl provider_capability_route_store_seal::Sealed for DesktopProviderCapabilityRouteStore {}

impl ProviderCapabilityRouteStore for DesktopProviderCapabilityRouteStore {
    fn load_record(&self) -> Result<Option<ProviderCapabilityRouteSettings>, CommandErrorPayload> {
        let settings_path = self.settings_path();
        let Some(record) =
            read_secret_json_file_or_remove_invalid(&settings_path, "provider capability route")?
        else {
            return Ok(Some(empty_provider_capability_route_settings()));
        };
        Ok(Some(record))
    }

    fn save_record(
        &self,
        record: &ProviderCapabilityRouteSettings,
        _validation: ProviderCapabilityRouteValidationToken,
    ) -> Result<(), CommandErrorPayload> {
        ensure_provider_capability_route_settings_record(record)?;
        let settings_path = self.settings_path();
        write_secret_json_file_atomic(&settings_path, "provider capability route", record)
    }
}

#[derive(Clone, Default)]
pub struct NoWorkspaceProviderCapabilityRouteStore;

impl provider_capability_route_store_seal::Sealed for NoWorkspaceProviderCapabilityRouteStore {}

impl ProviderCapabilityRouteStore for NoWorkspaceProviderCapabilityRouteStore {
    fn project_scope_available(&self) -> bool {
        false
    }

    fn load_record(&self) -> Result<Option<ProviderCapabilityRouteSettings>, CommandErrorPayload> {
        Ok(Some(empty_provider_capability_route_settings()))
    }

    fn save_record(
        &self,
        _record: &ProviderCapabilityRouteSettings,
        _validation: ProviderCapabilityRouteValidationToken,
    ) -> Result<(), CommandErrorPayload> {
        Err(invalid_payload(
            "provider capability routes require an active project workspace".to_owned(),
        ))
    }
}

#[derive(Clone)]
pub struct DesktopExecutionSettingsStore {
    layout: crate::storage_layout::StorageLayout,
    scope: ExecutionSettingsScope,
}

#[derive(Clone)]
enum ExecutionSettingsScope {
    Project { workspace_root: PathBuf },
    GlobalOnly { policy_root: PathBuf },
}

impl DesktopExecutionSettingsStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        let home = execution_settings_home_dir();
        Self::new_with_layout(
            crate::storage_layout::StorageLayout::new(crate::storage_layout::JyowoHome::new(home)),
            workspace_root,
        )
    }

    pub fn new_with_layout(
        layout: crate::storage_layout::StorageLayout,
        workspace_root: PathBuf,
    ) -> Self {
        Self {
            layout,
            scope: ExecutionSettingsScope::Project { workspace_root },
        }
    }

    pub fn global_only() -> Self {
        let home = execution_settings_home_dir();
        let layout =
            crate::storage_layout::StorageLayout::new(crate::storage_layout::JyowoHome::new(home));
        Self::global_only_with_layout(layout)
    }

    pub fn global_only_with_layout(layout: crate::storage_layout::StorageLayout) -> Self {
        let policy_root = layout.global_runtime_root().join("global-conversations");
        Self {
            layout,
            scope: ExecutionSettingsScope::GlobalOnly { policy_root },
        }
    }

    pub fn from_runtime_layout(layout: &crate::storage_layout::RuntimeLayout) -> Self {
        match layout.workspace_root.as_ref() {
            Some(workspace_root) => Self::new(workspace_root.clone()),
            None => Self::global_only(),
        }
    }

    pub fn from_runtime_layout_with_layout(
        storage_layout: crate::storage_layout::StorageLayout,
        runtime_layout: &crate::storage_layout::RuntimeLayout,
    ) -> Self {
        match runtime_layout.workspace_root.as_ref() {
            Some(workspace_root) => Self::new_with_layout(storage_layout, workspace_root.clone()),
            None => Self::global_only_with_layout(storage_layout),
        }
    }

    #[must_use]
    pub fn is_global_only(&self) -> bool {
        matches!(self.scope, ExecutionSettingsScope::GlobalOnly { .. })
    }

    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        match &self.scope {
            ExecutionSettingsScope::Project { workspace_root } => workspace_root,
            ExecutionSettingsScope::GlobalOnly { policy_root } => policy_root,
        }
    }

    fn settings_path(&self) -> PathBuf {
        match &self.scope {
            ExecutionSettingsScope::Project { workspace_root } => {
                self.layout.project_execution_overrides_file(workspace_root)
            }
            ExecutionSettingsScope::GlobalOnly { .. } => {
                self.layout.global_execution_defaults_file()
            }
        }
    }

    pub fn load_record(
        &self,
    ) -> Result<harness_contracts::ExecutionDefaultsRecord, CommandErrorPayload> {
        let settings_path = self.settings_path();
        if self.is_global_only() {
            let Some(record) = read_json_file::<harness_contracts::ExecutionDefaultsRecord>(
                &settings_path,
                "execution settings",
            )?
            else {
                return Ok(harness_contracts::ExecutionDefaultsRecord::default());
            };
            if ensure_execution_defaults_structure(&record).is_err() {
                remove_invalid_json_file(&settings_path, "execution settings")?;
                return Ok(harness_contracts::ExecutionDefaultsRecord::default());
            }
            return Ok(record);
        }
        let Some(overrides) = read_json_file::<harness_contracts::ExecutionOverridesRecord>(
            &settings_path,
            "execution settings",
        )?
        else {
            return Ok(harness_contracts::ExecutionDefaultsRecord::default());
        };
        if ensure_execution_overrides_structure(&overrides).is_err() {
            remove_invalid_json_file(&settings_path, "execution settings")?;
            return Ok(harness_contracts::ExecutionDefaultsRecord::default());
        }
        let mut record = harness_contracts::ExecutionDefaultsRecord::default();
        apply_execution_overrides(&mut record, &overrides);
        Ok(record)
    }

    pub fn save_record(
        &self,
        record: &harness_contracts::ExecutionDefaultsRecord,
        context: Option<&AgentCapabilityResolutionContext>,
    ) -> Result<(), CommandErrorPayload> {
        if self.is_global_only() {
            ensure_no_workspace_execution_defaults_record(record, self.workspace_root(), context)?;
            let settings_path = self.settings_path();
            return write_json_file_atomic(&settings_path, "execution settings", record);
        } else {
            ensure_execution_defaults_record(record, self.workspace_root(), context)?;
        }
        let settings_path = self.settings_path();
        let overrides = harness_contracts::ExecutionOverridesRecord::from(record.clone());
        write_json_file_atomic(&settings_path, "execution settings", &overrides)
    }
}

fn execution_settings_home_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .unwrap_or_else(|| std::ffi::OsString::from("."));
    PathBuf::from(home).join(".jyowo")
}

pub(crate) fn ensure_execution_defaults_structure(
    record: &harness_contracts::ExecutionDefaultsRecord,
) -> Result<(), CommandErrorPayload> {
    match record.permission_mode {
        PermissionMode::Default | PermissionMode::Auto | PermissionMode::BypassPermissions => {
            Ok(())
        }
        _ => Err(invalid_payload(
            "permissionMode must be default, auto, or bypass_permissions".to_owned(),
        )),
    }?;
    if !(0.5..=0.95).contains(&record.context_compression_trigger_ratio)
        || !record.context_compression_trigger_ratio.is_finite()
    {
        return Err(invalid_payload(
            "contextCompressionTriggerRatio must be between 0.5 and 0.95".to_owned(),
        ));
    }

    Ok(())
}

pub(crate) fn ensure_execution_overrides_structure(
    record: &harness_contracts::ExecutionOverridesRecord,
) -> Result<(), CommandErrorPayload> {
    if let Some(permission_mode) = record.permission_mode {
        match permission_mode {
            PermissionMode::Default | PermissionMode::Auto | PermissionMode::BypassPermissions => {
                Ok(())
            }
            _ => Err(invalid_payload(
                "permissionMode must be default, auto, or bypass_permissions".to_owned(),
            )),
        }?;
    }
    if let Some(ratio) = record.context_compression_trigger_ratio {
        if !(0.5..=0.95).contains(&ratio) || !ratio.is_finite() {
            return Err(invalid_payload(
                "contextCompressionTriggerRatio must be between 0.5 and 0.95".to_owned(),
            ));
        }
    }

    Ok(())
}

pub(crate) fn ensure_execution_defaults_record(
    record: &harness_contracts::ExecutionDefaultsRecord,
    workspace_root: &Path,
    context: Option<&AgentCapabilityResolutionContext>,
) -> Result<(), CommandErrorPayload> {
    ensure_execution_defaults_structure(record)?;

    let policy = resolve_agent_capability_policy(workspace_root, context);
    ensure_agent_capability_setting_available(
        record.subagents_enabled,
        policy.subagents_available,
        "subagents",
    )?;
    ensure_agent_capability_setting_available(
        record.agent_teams_enabled,
        policy.agent_teams_available,
        "agentTeams",
    )?;
    ensure_agent_capability_setting_available(
        record.background_agents_enabled,
        policy.background_agents_available,
        "backgroundAgents",
    )
}

pub(crate) fn ensure_no_workspace_execution_defaults_record(
    record: &harness_contracts::ExecutionDefaultsRecord,
    runtime_root: &Path,
    context: Option<&AgentCapabilityResolutionContext>,
) -> Result<(), CommandErrorPayload> {
    ensure_execution_defaults_structure(record)?;
    let payload = no_workspace_agent_capabilities_payload(record, runtime_root, context);
    ensure_agent_capability_setting_available(
        record.subagents_enabled,
        payload.subagents_available,
        "subagents",
    )?;
    ensure_agent_capability_setting_available(
        record.agent_teams_enabled,
        payload.agent_teams_available,
        "agentTeams",
    )?;
    ensure_agent_capability_setting_available(
        record.background_agents_enabled,
        payload.background_agents_available,
        "backgroundAgents",
    )
}

pub(crate) fn ensure_agent_capability_setting_available(
    enabled: bool,
    available: bool,
    capability: &str,
) -> Result<(), CommandErrorPayload> {
    if enabled && !available {
        return Err(invalid_payload(format!(
            "{capability} cannot be enabled in this desktop build"
        )));
    }
    Ok(())
}

pub(crate) fn auto_mode_available() -> bool {
    // The desktop shell does not currently assemble an AuxLlmBroker-backed
    // permission runtime. Keep Auto fail-closed until that runtime is present.
    false
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilitiesPayload {
    pub subagents_enabled: bool,
    pub agent_teams_enabled: bool,
    pub background_agents_enabled: bool,
    pub subagents_available: bool,
    pub agent_teams_available: bool,
    pub background_agents_available: bool,
    pub unavailable_reasons: Vec<AgentCapabilityUnavailableReason>,
}

pub(crate) fn resolve_agent_capability_policy(
    workspace_root: &Path,
    context: Option<&AgentCapabilityResolutionContext>,
) -> jyowo_harness_sdk::ResolvedAgentCapabilityPolicy {
    jyowo_harness_sdk::resolve_agent_capabilities_with_context(
        workspace_root,
        context.copied().unwrap_or_default(),
    )
}

pub(crate) fn agent_capabilities_payload(
    record: &harness_contracts::ExecutionDefaultsRecord,
    workspace_root: &Path,
    context: Option<&AgentCapabilityResolutionContext>,
) -> AgentCapabilitiesPayload {
    let policy = resolve_agent_capability_policy(workspace_root, context);
    AgentCapabilitiesPayload {
        subagents_enabled: record.subagents_enabled,
        agent_teams_enabled: record.agent_teams_enabled,
        background_agents_enabled: record.background_agents_enabled,
        subagents_available: policy.subagents_available,
        agent_teams_available: policy.agent_teams_available,
        background_agents_available: policy.background_agents_available,
        unavailable_reasons: policy.unavailable_reasons,
    }
}

pub(crate) fn no_workspace_agent_capabilities_payload(
    record: &harness_contracts::ExecutionDefaultsRecord,
    runtime_root: &Path,
    context: Option<&AgentCapabilityResolutionContext>,
) -> AgentCapabilitiesPayload {
    no_workspace_agent_capabilities_payload_for_conversation(record, runtime_root, None, context)
}

pub(crate) fn no_workspace_agent_capabilities_payload_for_conversation(
    record: &harness_contracts::ExecutionDefaultsRecord,
    runtime_root: &Path,
    conversation_id: Option<SessionId>,
    context: Option<&AgentCapabilityResolutionContext>,
) -> AgentCapabilitiesPayload {
    let stream_permission_runtime_available = context
        .copied()
        .unwrap_or_default()
        .stream_permission_runtime_available;
    let runtime_store = jyowo_harness_sdk::AgentRuntimeStore::open_runtime_dir(runtime_root);
    let runtime_store_available = runtime_store.is_ok();
    let restart_recovery_ok = runtime_store
        .as_ref()
        .ok()
        .and_then(|store| store.table_exists("restart_recovery_markers").ok())
        .unwrap_or(false);
    let supervisor_scope = conversation_id.map_or_else(
        || crate::agent_supervisor::AgentSupervisorScope::runtime(runtime_root.to_path_buf()),
        |conversation_id| {
            crate::agent_supervisor::AgentSupervisorScope::runtime_conversation(
                runtime_root.to_path_buf(),
                conversation_id,
            )
        },
    );
    let background_supervisor_available =
        crate::agent_supervisor::agent_supervisor_available_for_scope(&supervisor_scope);
    let environment = jyowo_harness_sdk::default_agent_capability_environment();

    let subagents_available = environment.subagents_compiled
        && runtime_store_available
        && stream_permission_runtime_available;
    let agent_teams_available =
        subagents_available && environment.agent_teams_compiled && runtime_store_available;
    let background_agents_available = runtime_store_available
        && restart_recovery_ok
        && background_supervisor_available
        && stream_permission_runtime_available;

    let mut unavailable_reasons = Vec::new();
    if !runtime_store_available {
        for capability in [
            AgentCapabilityKind::Subagents,
            AgentCapabilityKind::AgentTeams,
            AgentCapabilityKind::BackgroundAgents,
        ] {
            unavailable_reasons.push(AgentCapabilityUnavailableReason::RuntimeStoreUnavailable {
                capability,
                message: "runtime-scope agent store is unavailable".to_owned(),
            });
        }
    }
    if !environment.subagents_compiled {
        unavailable_reasons.push(AgentCapabilityUnavailableReason::NotCompiled {
            capability: AgentCapabilityKind::Subagents,
        });
    }
    if subagents_available && !environment.agent_teams_compiled {
        unavailable_reasons.push(AgentCapabilityUnavailableReason::NotCompiled {
            capability: AgentCapabilityKind::AgentTeams,
        });
    }
    if !stream_permission_runtime_available {
        unavailable_reasons.push(
            AgentCapabilityUnavailableReason::PermissionRuntimeUnavailable {
                capability: AgentCapabilityKind::Subagents,
            },
        );
        unavailable_reasons.push(
            AgentCapabilityUnavailableReason::PermissionRuntimeUnavailable {
                capability: AgentCapabilityKind::BackgroundAgents,
            },
        );
    }
    if !background_supervisor_available {
        unavailable_reasons.push(
            AgentCapabilityUnavailableReason::BackgroundSupervisorUnavailable {
                message: "runtime-scope background agent supervisor is unavailable".to_owned(),
            },
        );
    }

    AgentCapabilitiesPayload {
        subagents_enabled: record.subagents_enabled,
        agent_teams_enabled: record.agent_teams_enabled,
        background_agents_enabled: record.background_agents_enabled,
        subagents_available,
        agent_teams_available,
        background_agents_available,
        unavailable_reasons,
    }
}

pub(crate) fn ensure_provider_settings_record(
    record: &ProviderSettingsRecord,
) -> Result<(), CommandErrorPayload> {
    if record.configs.is_empty() {
        if record.default_config_id.is_some() {
            return Err(runtime_operation_failed(
                "defaultConfigId requires at least one provider config".to_owned(),
            ));
        }
        return Ok(());
    }

    let Some(default_config_id) = record
        .default_config_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(runtime_operation_failed(
            "defaultConfigId is required when provider configs exist".to_owned(),
        ));
    };
    if !record
        .configs
        .iter()
        .any(|config| config.id == default_config_id)
    {
        return Err(runtime_operation_failed(
            "defaultConfigId must reference an existing provider config".to_owned(),
        ));
    }
    if record
        .configs
        .iter()
        .any(|config| config.api_key.trim().is_empty())
    {
        return Err(runtime_operation_failed(
            "apiKey is required for every provider config".to_owned(),
        ));
    }

    Ok(())
}

pub(crate) fn empty_provider_capability_route_settings() -> ProviderCapabilityRouteSettings {
    ProviderCapabilityRouteSettings {
        version: 1,
        routes: Vec::new(),
    }
}

pub(crate) fn ensure_provider_capability_route_settings_record(
    record: &ProviderCapabilityRouteSettings,
) -> Result<(), CommandErrorPayload> {
    if record.version != 1 {
        return Err(runtime_operation_failed(
            "provider capability route version must be 1".to_owned(),
        ));
    }
    for route in &record.routes {
        validate_provider_capability_route(route).map_err(runtime_operation_failed)?;
    }

    Ok(())
}

#[derive(Clone)]
pub struct DesktopConversationMetadataStore {
    runtime_root: PathBuf,
}

impl DesktopConversationMetadataStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            runtime_root: workspace_root.join(".jyowo").join("runtime"),
        }
    }

    pub fn new_runtime_root(runtime_root: PathBuf) -> Self {
        Self { runtime_root }
    }

    fn metadata_path(&self) -> PathBuf {
        self.runtime_root.join("conversation-metadata.json")
    }
}

impl ConversationMetadataStore for DesktopConversationMetadataStore {
    fn load_record(&self) -> Result<ConversationMetadataFile, CommandErrorPayload> {
        let metadata_path = self.metadata_path();
        Ok(read_json_file(&metadata_path, "conversation metadata")?.unwrap_or_default())
    }

    fn save_record(&self, record: &ConversationMetadataFile) -> Result<(), CommandErrorPayload> {
        if record.version != 1 {
            return Err(runtime_operation_failed(
                "conversation metadata version must be 1".to_owned(),
            ));
        }
        let metadata_path = self.metadata_path();
        write_json_file_atomic(&metadata_path, "conversation metadata", record)
    }
}

#[must_use]
pub fn list_model_provider_catalog_payload() -> ModelProviderCatalogResponse {
    model_provider_catalog_response(provider_catalog_entries())
}

pub async fn list_model_provider_catalog_payload_with_remote() -> ModelProviderCatalogResponse {
    let mut entries = provider_catalog_entries();
    if let Some(openrouter_inventory) = fetch_openrouter_inventory().await {
        if let Some(openrouter) = entries
            .iter_mut()
            .find(|entry| entry.provider_id == "openrouter")
        {
            openrouter.models = runnable_inventory_models(&openrouter_inventory);
        }
    }
    model_provider_catalog_response(entries)
}

pub(crate) fn model_provider_catalog_response(
    entries: Vec<jyowo_harness_sdk::ext::ProviderCatalogEntry>,
) -> ModelProviderCatalogResponse {
    ModelProviderCatalogResponse {
        providers: entries
            .into_iter()
            .map(|entry| ModelProviderCatalogEntry {
                default_base_url: entry.default_base_url,
                display_name: entry.display_name,
                models: entry
                    .models
                    .into_iter()
                    .map(model_descriptor_catalog_entry)
                    .collect(),
                provider_id: entry.provider_id,
                runtime_capability: runtime_capability_payload(entry.runtime_capability),
                service_capabilities: entry
                    .service_capabilities
                    .into_iter()
                    .map(service_capability_payload)
                    .collect(),
                source_url: entry.source_url,
                verified_date: entry.verified_date.to_string(),
            })
            .collect(),
    }
}

pub(crate) fn runtime_capability_payload(
    capability: ProviderRuntimeCapability,
) -> ProviderRuntimeCapabilityPayload {
    ProviderRuntimeCapabilityPayload {
        auth_scheme: provider_auth_scheme_payload(capability.auth_scheme),
        base_url_regions: capability
            .base_url_regions
            .into_iter()
            .map(base_url_region_payload)
            .collect(),
        supports_live_validation: capability.supports_live_validation,
        supports_streaming_validation: capability.supports_streaming_validation,
        secret_reveal_supported: capability.secret_reveal_supported,
    }
}

pub(crate) fn base_url_region_payload(
    region: ProviderBaseUrlRegion,
) -> ProviderBaseUrlRegionPayload {
    ProviderBaseUrlRegionPayload {
        id: region.id,
        label: region.label,
        base_url: region.base_url,
    }
}

pub(crate) fn service_capability_payload(
    capability: ProviderServiceCapability,
) -> ProviderServiceCapabilityPayload {
    ProviderServiceCapabilityPayload {
        operation_id: capability.operation_id,
        category: provider_service_category_payload(capability.category),
        input_modalities: capability
            .input_modalities
            .iter()
            .map(model_modality_record)
            .collect(),
        output_artifact: model_modality_record(&capability.output_artifact),
        execution: provider_service_execution_payload(capability.execution),
        requires_polling: capability.requires_polling,
        permission_subject: capability.permission_subject,
        cost_risk: provider_service_cost_risk_payload(capability.cost_risk),
    }
}

pub(crate) fn provider_auth_scheme_payload(scheme: ProviderAuthScheme) -> &'static str {
    match scheme {
        ProviderAuthScheme::Bearer => "bearer",
        ProviderAuthScheme::ApiKey => "api_key",
        ProviderAuthScheme::XApiKey => "x_api_key",
        ProviderAuthScheme::None => "none",
    }
}

pub(crate) fn provider_service_category_payload(category: ProviderServiceCategory) -> &'static str {
    match category {
        ProviderServiceCategory::Conversation => "conversation",
        ProviderServiceCategory::Image => "image",
        ProviderServiceCategory::Video => "video",
        ProviderServiceCategory::Audio => "audio",
        ProviderServiceCategory::Music => "music",
        ProviderServiceCategory::File => "file",
        ProviderServiceCategory::Model => "model",
    }
}

pub(crate) fn provider_service_execution_payload(
    execution: ProviderServiceExecution,
) -> &'static str {
    match execution {
        ProviderServiceExecution::Sync => "sync",
        ProviderServiceExecution::AsyncJob => "async_job",
        ProviderServiceExecution::Websocket => "websocket",
    }
}

pub(crate) fn provider_service_cost_risk_payload(
    cost_risk: ProviderServiceCostRisk,
) -> &'static str {
    match cost_risk {
        ProviderServiceCostRisk::Low => "low",
        ProviderServiceCostRisk::Medium => "medium",
        ProviderServiceCostRisk::High => "high",
    }
}

pub(crate) async fn fetch_openrouter_inventory() -> Option<Vec<ModelInventoryEntry>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;
    let mut response = client
        .get("https://openrouter.ai/api/v1/models")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_OPENROUTER_MODELS_API_BYTES as u64)
    {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len().saturating_add(chunk.len()) > MAX_OPENROUTER_MODELS_API_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    inventory_from_models_api_json(&bytes).ok()
}

pub async fn validate_provider_settings_payload(
    request: ValidateProviderSettingsRequest,
) -> Result<ValidateProviderSettingsResponse, CommandErrorPayload> {
    ensure_provider_model_supported(&request).await?;

    Ok(ValidateProviderSettingsResponse {
        model_id: request.model_id,
        provider_id: request.provider_id,
        status: "accepted",
    })
}

pub fn get_execution_settings_with_store(
    store: &DesktopExecutionSettingsStore,
    context: Option<&AgentCapabilityResolutionContext>,
) -> Result<GetExecutionSettingsResponse, CommandErrorPayload> {
    let record = store.load_record()?;
    execution_settings_response_from_record(
        &record,
        store.is_global_only(),
        store.workspace_root(),
        context,
    )
}

fn execution_settings_response_from_record(
    record: &harness_contracts::ExecutionDefaultsRecord,
    global_only: bool,
    policy_root: &Path,
    context: Option<&AgentCapabilityResolutionContext>,
) -> Result<GetExecutionSettingsResponse, CommandErrorPayload> {
    let permission_mode = effective_execution_settings_permission_mode(record.permission_mode);
    let agent_capabilities = if global_only {
        no_workspace_agent_capabilities_payload(record, policy_root, context)
    } else {
        agent_capabilities_payload(record, policy_root, context)
    };
    Ok(GetExecutionSettingsResponse {
        permission_mode,
        tool_profile: record.tool_profile.clone(),
        context_compression_trigger_ratio: record.context_compression_trigger_ratio,
        scope: execution_settings_scope(global_only),
        auto_mode_available: auto_mode_available(),
        agent_capabilities,
    })
}

fn execution_settings_scope(global_only: bool) -> SettingsScope {
    if global_only {
        SettingsScope::Global
    } else {
        SettingsScope::Project
    }
}

pub fn get_execution_settings_for_state_request(
    request: GetExecutionSettingsRequest,
    state: &DesktopRuntimeState,
    project_registry: &ProjectRegistry,
    context: Option<&AgentCapabilityResolutionContext>,
) -> Result<GetExecutionSettingsResponse, CommandErrorPayload> {
    let Some(workspace_path) = request.workspace_path else {
        let record = state.effective_execution_settings(None)?;
        let global_only = state.project_workspace_root().is_none();
        let policy_root = state
            .project_workspace_root()
            .unwrap_or_else(|| state.runtime_root());
        return execution_settings_response_from_record(&record, global_only, policy_root, context);
    };
    let workspace_root =
        canonical_workspace_root(PathBuf::from(workspace_path), "workspace path".to_owned())?;
    let workspace_root_text = workspace_root.to_string_lossy();
    if !project_registry
        .list_projects()
        .iter()
        .any(|project| project.path == workspace_root_text.as_ref())
    {
        return Err(invalid_payload("project is not registered".to_owned()));
    }
    let global = global_config_store_for_home();
    let project = project_config_store_for_workspace(&workspace_root);
    let record = resolve_effective_execution_settings(Some(&global), Some(&project), None, None)?;
    execution_settings_response_from_record(&record, false, &workspace_root, context)
}

pub fn get_execution_settings_for_request(
    request: GetExecutionSettingsRequest,
    active_store: &DesktopExecutionSettingsStore,
    project_registry: &ProjectRegistry,
    context: Option<&AgentCapabilityResolutionContext>,
) -> Result<GetExecutionSettingsResponse, CommandErrorPayload> {
    let Some(workspace_path) = request.workspace_path else {
        return get_execution_settings_with_store(active_store, context);
    };
    let workspace_root =
        canonical_workspace_root(PathBuf::from(workspace_path), "workspace path".to_owned())?;
    let workspace_root_text = workspace_root.to_string_lossy();
    if !project_registry
        .list_projects()
        .iter()
        .any(|project| project.path == workspace_root_text.as_ref())
    {
        return Err(invalid_payload("project is not registered".to_owned()));
    }
    let store = DesktopExecutionSettingsStore::new(workspace_root);
    get_execution_settings_with_store(&store, context)
}

pub fn set_execution_settings_with_store(
    request: SetExecutionSettingsRequest,
    store: &DesktopExecutionSettingsStore,
    context: Option<&AgentCapabilityResolutionContext>,
) -> Result<SetExecutionSettingsResponse, CommandErrorPayload> {
    let requested_record = harness_contracts::ExecutionDefaultsRecord {
        permission_mode: request.permission_mode,
        tool_profile: request.tool_profile.clone(),
        context_compression_trigger_ratio: request.context_compression_trigger_ratio,
        subagents_enabled: request.subagents_enabled,
        agent_teams_enabled: request.agent_teams_enabled,
        background_agents_enabled: request.background_agents_enabled,
    };
    if store.is_global_only() {
        ensure_no_workspace_execution_defaults_record(
            &requested_record,
            store.workspace_root(),
            context,
        )?;
    } else {
        ensure_execution_defaults_record(&requested_record, store.workspace_root(), context)?;
    }
    if request.permission_mode == PermissionMode::Auto && !auto_mode_available() {
        return Err(invalid_payload(
            "auto permission mode is unavailable in this desktop build".to_owned(),
        ));
    }
    let record = requested_record;
    store.save_record(&record, context)?;
    let agent_capabilities = if store.is_global_only() {
        no_workspace_agent_capabilities_payload(&record, store.workspace_root(), context)
    } else {
        agent_capabilities_payload(&record, store.workspace_root(), context)
    };
    Ok(SetExecutionSettingsResponse {
        permission_mode: record.permission_mode,
        tool_profile: record.tool_profile.clone(),
        context_compression_trigger_ratio: record.context_compression_trigger_ratio,
        scope: execution_settings_scope(store.is_global_only()),
        auto_mode_available: auto_mode_available(),
        agent_capabilities,
    })
}

/// Migrate old workspace `execution-settings.json` (runtime path) to the new
/// project config `execution-overrides.json`.
///
/// Old files use snake_case field names; `ExecutionDefaultsRecord` accepts
/// them via `#[serde(alias)]` annotations and always writes camelCase.
pub fn migrate_execution_settings(
    workspace_root: &Path,
) -> Result<crate::commands::stores::migration::MigrationResult, CommandErrorPayload> {
    let old_path = workspace_root
        .join(".jyowo")
        .join("runtime")
        .join("execution-settings.json");

    let home = execution_settings_home_dir();
    let layout =
        crate::storage_layout::StorageLayout::new(crate::storage_layout::JyowoHome::new(home));
    let new_path = layout.project_execution_overrides_file(workspace_root);

    crate::commands::stores::migration::migrate_json_file::<
        harness_contracts::ExecutionOverridesRecord,
    >(&old_path, &new_path, "execution settings", true)
}

/// Migrate old workspace `provider-capability-routes.json` (runtime path) to
/// the new project config `provider-capability-routes.json`.
///
/// Provider routes are project-scoped config, not runtime diagnostics.
/// Diagnostics and quota cache remain under `<workspace>/.jyowo/runtime/`.
pub fn migrate_provider_capability_routes(
    workspace_root: &Path,
) -> Result<crate::commands::stores::migration::MigrationResult, CommandErrorPayload> {
    let old_path = workspace_root
        .join(".jyowo")
        .join("runtime")
        .join("provider-capability-routes.json");

    let home = execution_settings_home_dir();
    let layout =
        crate::storage_layout::StorageLayout::new(crate::storage_layout::JyowoHome::new(home));
    let new_path = layout.project_provider_routes_file(workspace_root);

    crate::commands::stores::migration::migrate_secret_json_file::<ProviderCapabilityRouteSettings>(
        &old_path,
        &new_path,
        "provider capability routes",
        true,
    )
}

/// Resolve effective execution settings by merging global defaults, project
/// overrides, and optional run-level overrides.
///
/// Precedence (highest wins):
/// 1. Run explicit params (`run_permission_mode`, `run_tool_profile`)
/// 2. Project execution overrides
/// 3. Global execution defaults
/// 4. Contract defaults in [`harness_contracts::ExecutionDefaultsRecord::default`]
///
/// This is the single source of truth for effective execution settings.
/// Frontend code must not reimplement this overlay.
pub fn resolve_effective_execution_settings(
    global_config: Option<&crate::commands::stores::GlobalConfigStore>,
    project_config: Option<&crate::commands::stores::ProjectConfigStore>,
    run_permission_mode: Option<PermissionMode>,
    run_tool_profile: Option<ToolProfile>,
) -> Result<harness_contracts::ExecutionDefaultsRecord, CommandErrorPayload> {
    // 1. Start with contract defaults.
    let mut effective = harness_contracts::ExecutionDefaultsRecord::default();

    // 2. Apply global defaults (overwrite contract defaults where set).
    if let Some(global) = global_config {
        let global_defaults = global.load_execution_defaults()?;
        effective.permission_mode = global_defaults.permission_mode;
        effective.tool_profile = global_defaults.tool_profile;
        effective.context_compression_trigger_ratio =
            global_defaults.context_compression_trigger_ratio;
        effective.subagents_enabled = global_defaults.subagents_enabled;
        effective.agent_teams_enabled = global_defaults.agent_teams_enabled;
        effective.background_agents_enabled = global_defaults.background_agents_enabled;
    }

    // 3. Apply project overrides (overwrite global defaults where project has
    //    explicit non-default values).
    if let Some(project) = project_config {
        let overrides = project.load_execution_overrides()?;
        ensure_execution_overrides_structure(&overrides)?;
        apply_execution_overrides(&mut effective, &overrides);
    }

    // 4. Apply run explicit params.
    if let Some(permission_mode) = run_permission_mode {
        effective.permission_mode = permission_mode;
    }
    if let Some(tool_profile) = run_tool_profile {
        effective.tool_profile = tool_profile;
    }

    Ok(effective)
}

fn apply_execution_overrides(
    effective: &mut harness_contracts::ExecutionDefaultsRecord,
    overrides: &harness_contracts::ExecutionOverridesRecord,
) {
    if let Some(permission_mode) = overrides.permission_mode {
        effective.permission_mode = permission_mode;
    }
    if let Some(tool_profile) = &overrides.tool_profile {
        effective.tool_profile = tool_profile.clone();
    }
    if let Some(context_compression_trigger_ratio) = overrides.context_compression_trigger_ratio {
        effective.context_compression_trigger_ratio = context_compression_trigger_ratio;
    }
    if let Some(subagents_enabled) = overrides.subagents_enabled {
        effective.subagents_enabled = subagents_enabled;
    }
    if let Some(agent_teams_enabled) = overrides.agent_teams_enabled {
        effective.agent_teams_enabled = agent_teams_enabled;
    }
    if let Some(background_agents_enabled) = overrides.background_agents_enabled {
        effective.background_agents_enabled = background_agents_enabled;
    }
}

pub async fn list_provider_settings_with_store(
    store: &dyn ProviderSettingsStore,
) -> Result<ListProviderSettingsResponse, CommandErrorPayload> {
    let record = store.load_record()?.unwrap_or_default();

    Ok(ListProviderSettingsResponse {
        default_config_id: record.default_config_id.clone(),
        selection_scope: store.selection_scope(),
        configs: provider_config_payloads(&record)?,
    })
}

pub async fn list_provider_capability_routes_with_store(
    store: &dyn ProviderCapabilityRouteStore,
    provider_settings: &ProviderSettingsRecord,
    provider_catalog: &ModelProviderCatalogResponse,
    adapter_availability: &ProviderServiceAdapterAvailability,
) -> Result<ListProviderCapabilityRoutesResponse, CommandErrorPayload> {
    let record = store
        .load_record()?
        .unwrap_or_else(empty_provider_capability_route_settings);
    let _validation = ensure_provider_capability_route_settings(
        &record,
        provider_settings,
        provider_catalog,
        adapter_availability,
    )?;

    Ok(ListProviderCapabilityRoutesResponse {
        version: record.version,
        routes: record.routes,
    })
}

pub async fn save_provider_capability_route_settings_with_store(
    settings: ProviderCapabilityRouteSettings,
    store: &dyn ProviderCapabilityRouteStore,
    provider_settings: &ProviderSettingsRecord,
    provider_catalog: &ModelProviderCatalogResponse,
    adapter_availability: &ProviderServiceAdapterAvailability,
) -> Result<SaveProviderCapabilityRouteResponse, CommandErrorPayload> {
    let validation = ensure_provider_capability_route_settings(
        &settings,
        provider_settings,
        provider_catalog,
        adapter_availability,
    )?;
    store.save_record(&settings, validation)?;

    Ok(SaveProviderCapabilityRouteResponse {
        version: settings.version,
        routes: settings.routes,
        status: "saved",
    })
}

pub async fn save_provider_capability_route_with_store(
    request: SaveProviderCapabilityRouteRequest,
    store: &dyn ProviderCapabilityRouteStore,
    provider_settings: &ProviderSettingsRecord,
    provider_catalog: &ModelProviderCatalogResponse,
    adapter_availability: &ProviderServiceAdapterAvailability,
) -> Result<SaveProviderCapabilityRouteResponse, CommandErrorPayload> {
    let mut record = store
        .load_record()?
        .unwrap_or_else(empty_provider_capability_route_settings);
    if request.route.enabled {
        record
            .routes
            .retain(|route| route.kind != request.route.kind);
    } else {
        record.routes.retain(|route| {
            route.kind != request.route.kind
                || route.config_id != request.route.config_id
                || route.provider_id != request.route.provider_id
        });
    }
    record.routes.push(request.route);
    let validation = ensure_provider_capability_route_settings(
        &record,
        provider_settings,
        provider_catalog,
        adapter_availability,
    )?;
    store.save_record(&record, validation)?;

    Ok(SaveProviderCapabilityRouteResponse {
        version: record.version,
        routes: record.routes,
        status: "saved",
    })
}

pub async fn delete_provider_capability_route_with_store(
    request: DeleteProviderCapabilityRouteRequest,
    store: &dyn ProviderCapabilityRouteStore,
    provider_settings: &ProviderSettingsRecord,
    provider_catalog: &ModelProviderCatalogResponse,
    adapter_availability: &ProviderServiceAdapterAvailability,
) -> Result<DeleteProviderCapabilityRouteResponse, CommandErrorPayload> {
    ensure_non_empty("configId", &request.config_id)?;
    ensure_non_empty("providerId", &request.provider_id)?;
    let mut record = store
        .load_record()?
        .unwrap_or_else(empty_provider_capability_route_settings);
    ensure_provider_capability_route_settings_record(&record)?;
    record.routes.retain(|route| {
        route.kind != request.kind
            || route.config_id != request.config_id
            || route.provider_id != request.provider_id
    });
    let validation = ensure_provider_capability_route_settings(
        &record,
        provider_settings,
        provider_catalog,
        adapter_availability,
    )?;
    store.save_record(&record, validation)?;

    Ok(DeleteProviderCapabilityRouteResponse {
        version: record.version,
        routes: record.routes,
        status: "deleted",
    })
}

pub fn list_provider_capability_route_options_from_inputs(
    store: &dyn ProviderCapabilityRouteStore,
    provider_settings: &ProviderSettingsRecord,
    provider_catalog: &ModelProviderCatalogResponse,
    adapter_availability: &ProviderServiceAdapterAvailability,
) -> Result<ListProviderCapabilityRouteOptionsResponse, CommandErrorPayload> {
    if !store.project_scope_available() {
        return Ok(ListProviderCapabilityRouteOptionsResponse {
            options: Vec::new(),
        });
    }

    let mut options = Vec::new();

    for config in &provider_settings.configs {
        let Some(provider) = provider_catalog
            .providers
            .iter()
            .find(|provider| provider.provider_id == config.provider_id)
        else {
            continue;
        };

        for capability in &provider.service_capabilities {
            let Some(kind) = route_kind_for_service_capability(capability) else {
                continue;
            };
            let runtime_supported = has_service_adapter(
                adapter_availability,
                &config.provider_id,
                &capability.operation_id,
                kind,
            );
            options.push(ProviderCapabilityRouteOption {
                kind,
                config_id: config.id.clone(),
                provider_id: config.provider_id.clone(),
                operation_id: capability.operation_id.clone(),
                output_artifact: model_modality_from_record(&capability.output_artifact),
                execution: provider_service_execution_from_payload(capability.execution)?,
                cost_risk: provider_service_cost_risk_from_payload(capability.cost_risk)?,
                runtime_supported,
                unavailable_reason: (!runtime_supported)
                    .then(|| "runtime adapter unavailable".to_owned()),
            });
        }
    }

    Ok(ListProviderCapabilityRouteOptionsResponse { options })
}

pub(crate) async fn provider_capability_route_runtime_context(
    runtime_state: &DesktopRuntimeState,
) -> Result<
    (
        Arc<dyn ProviderCapabilityRouteStore>,
        ProviderSettingsRecord,
        ModelProviderCatalogResponse,
        ProviderServiceAdapterAvailability,
    ),
    CommandErrorPayload,
> {
    Ok((
        Arc::clone(&runtime_state.provider_capability_route_store),
        runtime_state
            .provider_settings_store
            .load_record()?
            .unwrap_or_default(),
        list_model_provider_catalog_payload_with_remote().await,
        desktop_provider_service_adapter_availability(runtime_state),
    ))
}

pub(crate) fn ensure_provider_capability_route_settings(
    routes: &ProviderCapabilityRouteSettings,
    provider_settings: &ProviderSettingsRecord,
    provider_catalog: &ModelProviderCatalogResponse,
    adapter_availability: &ProviderServiceAdapterAvailability,
) -> Result<ProviderCapabilityRouteValidationToken, CommandErrorPayload> {
    ensure_provider_capability_route_settings_record(routes).map_err(|error| {
        if error.code == "RUNTIME_OPERATION_FAILED" {
            invalid_payload(error.message)
        } else {
            error
        }
    })?;

    let mut enabled_kind_targets: HashMap<CapabilityRouteKind, (&str, &str)> = HashMap::new();
    for route in &routes.routes {
        if !route.enabled {
            continue;
        }

        let Some(config) = provider_settings
            .configs
            .iter()
            .find(|config| config.id == route.config_id)
        else {
            return Err(invalid_payload(
                "provider capability route configId must reference an existing provider config"
                    .to_owned(),
            ));
        };
        if config.api_key.trim().is_empty() {
            return Err(invalid_payload(
                "provider capability route config must have an apiKey".to_owned(),
            ));
        }
        if route.provider_id != config.provider_id {
            return Err(invalid_payload(
                "provider capability route providerId must match the provider config".to_owned(),
            ));
        }

        for operation_id in &route.operation_ids {
            let Some(capability) =
                provider_service_capability(provider_catalog, &route.provider_id, operation_id)
            else {
                return Err(invalid_payload(
                    "provider capability route operationId is not declared by the provider catalog"
                        .to_owned(),
                ));
            };
            if route_kind_for_service_capability(capability) != Some(route.kind) {
                return Err(invalid_payload(
                    "provider capability route kind does not match operationId".to_owned(),
                ));
            }
            if route.enabled
                && !has_service_adapter(
                    adapter_availability,
                    &route.provider_id,
                    operation_id,
                    route.kind,
                )
            {
                return Err(invalid_payload(
                    "provider capability route operationId has no runtime adapter".to_owned(),
                ));
            }
        }

        if route.enabled {
            match enabled_kind_targets.insert(route.kind, (&route.config_id, &route.provider_id)) {
                Some((config_id, provider_id))
                    if config_id != route.config_id || provider_id != route.provider_id =>
                {
                    return Err(invalid_payload(
                        "provider capability route kind cannot target multiple provider configs"
                            .to_owned(),
                    ));
                }
                _ => {}
            }
        }
    }

    Ok(ProviderCapabilityRouteValidationToken { _private: () })
}

pub(crate) fn provider_service_capability<'a>(
    provider_catalog: &'a ModelProviderCatalogResponse,
    provider_id: &str,
    operation_id: &str,
) -> Option<&'a ProviderServiceCapabilityPayload> {
    provider_catalog
        .providers
        .iter()
        .find(|provider| provider.provider_id == provider_id)?
        .service_capabilities
        .iter()
        .find(|capability| capability.operation_id == operation_id)
}

pub(crate) fn has_service_adapter(
    availability: &ProviderServiceAdapterAvailability,
    provider_id: &str,
    operation_id: &str,
    route_kind: CapabilityRouteKind,
) -> bool {
    availability.bindings.iter().any(|binding| {
        binding.provider_id == provider_id
            && binding.operation_id == operation_id
            && binding.route_kind == route_kind
    })
}

pub(crate) fn route_kind_for_service_capability(
    capability: &ProviderServiceCapabilityPayload,
) -> Option<CapabilityRouteKind> {
    match capability.category {
        "image" => Some(CapabilityRouteKind::ImageGeneration),
        "video" => Some(CapabilityRouteKind::VideoGeneration),
        "music" => Some(CapabilityRouteKind::MusicGeneration),
        "audio" if operation_id_is_speech_to_text(&capability.operation_id) => {
            Some(CapabilityRouteKind::SpeechToText)
        }
        "audio" => Some(CapabilityRouteKind::TextToSpeech),
        _ => None,
    }
}

pub(crate) fn operation_id_is_speech_to_text(operation_id: &str) -> bool {
    operation_id.contains("speech_to_text")
        || operation_id.contains("speech-to-text")
        || operation_id.contains("transcription")
}

pub(crate) fn provider_service_execution_from_payload(
    execution: &str,
) -> Result<ProviderServiceExecution, CommandErrorPayload> {
    match execution {
        "sync" => Ok(ProviderServiceExecution::Sync),
        "async_job" => Ok(ProviderServiceExecution::AsyncJob),
        "websocket" => Ok(ProviderServiceExecution::Websocket),
        _ => Err(invalid_payload(
            "provider service execution is not supported".to_owned(),
        )),
    }
}

pub(crate) fn provider_service_cost_risk_from_payload(
    cost_risk: &str,
) -> Result<ProviderServiceCostRisk, CommandErrorPayload> {
    match cost_risk {
        "low" => Ok(ProviderServiceCostRisk::Low),
        "medium" => Ok(ProviderServiceCostRisk::Medium),
        "high" => Ok(ProviderServiceCostRisk::High),
        _ => Err(invalid_payload(
            "provider service cost risk is not supported".to_owned(),
        )),
    }
}

pub(crate) fn provider_config_with_api_key<'a>(
    record: &'a ProviderSettingsRecord,
    config_id: &str,
) -> Result<&'a ProviderConfigRecord, CommandErrorPayload> {
    let Some(config) = record.configs.iter().find(|config| config.id == config_id) else {
        return Err(not_found(format!("provider config not found: {config_id}")));
    };
    ensure_provider_config_has_api_key(config)?;
    Ok(config)
}

pub(crate) fn provider_api_key_fingerprint(api_key: &str) -> [u8; 32] {
    *blake3::hash(api_key.as_bytes()).as_bytes()
}

pub(crate) fn provider_config_runtime_fingerprint(
    config: &ProviderConfigRecord,
) -> Result<[u8; 32], CommandErrorPayload> {
    let bytes = serde_json::to_vec(config).map_err(|error| {
        runtime_init_failed(format!("provider config fingerprint failed: {error}"))
    })?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

pub(crate) fn prune_expired_provider_api_key_reveal_tokens(
    tokens: &mut HashMap<String, ProviderConfigRevealTokenRecord>,
    now: Instant,
) {
    tokens.retain(|_, token| token.expires_at > now);
}

pub(crate) async fn clear_provider_api_key_reveal_tokens_for_config(
    runtime_state: &DesktopRuntimeState,
    config_id: &str,
) {
    let mut tokens = runtime_state.provider_api_key_reveal_tokens.lock().await;
    tokens.retain(|_, token| token.config_id != config_id);
}

pub async fn request_provider_config_api_key_reveal_with_store(
    request: RequestProviderConfigApiKeyRevealRequest,
    store: &dyn ProviderSettingsStore,
) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("configId", &request.config_id)?;
    let record = store.load_record()?.unwrap_or_default();
    provider_config_with_api_key(&record, &request.config_id)?;
    Err(invalid_payload(
        "provider API key reveal requires runtime state".to_owned(),
    ))
}

pub(crate) async fn request_provider_config_api_key_reveal_with_runtime_state_unlocked(
    request: RequestProviderConfigApiKeyRevealRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<RequestProviderConfigApiKeyRevealResponse, CommandErrorPayload> {
    ensure_non_empty("configId", &request.config_id)?;
    let record = runtime_state
        .provider_settings_store
        .load_record()?
        .unwrap_or_default();
    let config = provider_config_with_api_key(&record, &request.config_id)?;
    let api_key_fingerprint = provider_api_key_fingerprint(&config.api_key);

    let now = Instant::now();
    let reveal_token = RunId::new().to_string();
    let mut tokens = runtime_state.provider_api_key_reveal_tokens.lock().await;
    prune_expired_provider_api_key_reveal_tokens(&mut tokens, now);
    tokens.insert(
        reveal_token.clone(),
        ProviderConfigRevealTokenRecord {
            api_key_fingerprint,
            config_id: request.config_id.clone(),
            expires_at: now + PROVIDER_API_KEY_REVEAL_TTL,
        },
    );

    Ok(RequestProviderConfigApiKeyRevealResponse {
        config_id: request.config_id,
        expires_in_seconds: PROVIDER_API_KEY_REVEAL_TTL.as_secs(),
        reveal_token,
        status: "ready",
    })
}

pub async fn request_provider_config_api_key_reveal_with_runtime_state(
    request: RequestProviderConfigApiKeyRevealRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<RequestProviderConfigApiKeyRevealResponse, CommandErrorPayload> {
    let _provider_settings_guard = runtime_state.provider_settings_lock.lock().await;
    request_provider_config_api_key_reveal_with_runtime_state_unlocked(request, runtime_state).await
}

pub async fn get_provider_config_api_key_with_store(
    request: GetProviderConfigApiKeyRequest,
    store: &dyn ProviderSettingsStore,
) -> Result<GetProviderConfigApiKeyResponse, CommandErrorPayload> {
    ensure_non_empty("configId", &request.config_id)?;
    ensure_non_empty("revealToken", &request.reveal_token)?;
    let record = store.load_record()?.unwrap_or_default();
    provider_config_with_api_key(&record, &request.config_id)?;

    Err(invalid_payload(
        "provider API key reveal requires runtime state".to_owned(),
    ))
}

pub(crate) async fn get_provider_config_api_key_with_runtime_state_unlocked(
    request: GetProviderConfigApiKeyRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<GetProviderConfigApiKeyResponse, CommandErrorPayload> {
    ensure_non_empty("configId", &request.config_id)?;
    ensure_non_empty("revealToken", &request.reveal_token)?;

    let now = Instant::now();
    let token_record = {
        let mut tokens = runtime_state.provider_api_key_reveal_tokens.lock().await;
        let token_record = tokens.remove(&request.reveal_token);
        prune_expired_provider_api_key_reveal_tokens(&mut tokens, now);
        token_record
    }
    .ok_or_else(|| {
        invalid_payload("provider API key reveal token is invalid or expired".to_owned())
    })?;

    if token_record.expires_at <= now {
        return Err(invalid_payload(
            "provider API key reveal token expired".to_owned(),
        ));
    }
    if token_record.config_id != request.config_id {
        return Err(invalid_payload(
            "provider API key reveal token does not match configId".to_owned(),
        ));
    }

    let record = runtime_state
        .provider_settings_store
        .load_record()?
        .unwrap_or_default();
    let config = provider_config_with_api_key(&record, &request.config_id)?;
    if token_record.api_key_fingerprint != provider_api_key_fingerprint(&config.api_key) {
        return Err(invalid_payload(
            "provider API key reveal token no longer matches config".to_owned(),
        ));
    }
    Ok(GetProviderConfigApiKeyResponse {
        api_key: config.api_key.clone(),
        config_id: request.config_id,
    })
}

pub async fn get_provider_config_api_key_with_runtime_state(
    request: GetProviderConfigApiKeyRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<GetProviderConfigApiKeyResponse, CommandErrorPayload> {
    let _provider_settings_guard = runtime_state.provider_settings_lock.lock().await;
    get_provider_config_api_key_with_runtime_state_unlocked(request, runtime_state).await
}

pub async fn save_provider_settings_with_store(
    request: ProviderSettingsRequest,
    store: &dyn ProviderSettingsStore,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    ensure_provider_settings(&request)?;
    let base_url = normalized_base_url(request.base_url.as_deref())?;
    let mut record = store.load_record()?.unwrap_or_default();
    let config_id = provider_config_id(&record, &request);
    let previous_config = record
        .configs
        .iter()
        .find(|config| config.id == config_id)
        .cloned();
    let descriptor = provider_settings_descriptor(&request, previous_config.as_ref()).await?;
    let api_key = request
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let config_api_key = if let Some(api_key) = api_key {
        api_key.to_owned()
    } else if let Some(config) = previous_config.as_ref() {
        if config.provider_id != request.provider_id || config.base_url != base_url {
            return Err(invalid_payload(
                "apiKey is required when changing provider or baseUrl".to_owned(),
            ));
        }
        let api_key = match ensure_provider_config_has_api_key(config) {
            Ok(api_key) => api_key,
            Err(_) => {
                return Err(invalid_payload(
                    "apiKey is required for provider configs without a stored key".to_owned(),
                ));
            }
        };
        api_key.to_owned()
    } else {
        return Err(invalid_payload(
            "apiKey is required for new provider configs".to_owned(),
        ));
    };
    let official_quota_api_key = request
        .official_quota_api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            previous_config
                .as_ref()
                .filter(|config| {
                    config.provider_id == request.provider_id && config.base_url == base_url
                })
                .and_then(|config| config.official_quota_api_key.clone())
        });
    let config = ProviderConfigRecord {
        api_key: config_api_key,
        protocol: descriptor.protocol,
        base_url,
        display_name: request
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| provider_display_name(&request.provider_id)),
        id: config_id.clone(),
        model_id: request.model_id.clone(),
        official_quota_api_key,
        provider_id: request.provider_id.clone(),
        model_descriptor: model_descriptor_record(&descriptor),
    };
    record.configs.retain(|existing| existing.id != config_id);
    record.configs.push(config);
    record.configs.sort_by(|left, right| left.id.cmp(&right.id));
    if request.set_default || record.default_config_id.is_none() {
        record.default_config_id = Some(config_id.clone());
    }
    store.save_record(&record)?;

    Ok(SaveProviderSettingsResponse {
        config: provider_config_payload(
            record
                .configs
                .iter()
                .find(|config| config.id == config_id)
                .expect("saved config should exist"),
            record.default_config_id.as_deref(),
        )?,
        status: "saved",
    })
}

pub(crate) async fn save_provider_settings_with_runtime_state_unlocked(
    request: ProviderSettingsRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    let response =
        save_provider_settings_with_store(request, runtime_state.provider_settings_store.as_ref())
            .await?;
    clear_provider_api_key_reveal_tokens_for_config(runtime_state, &response.config.id).await;
    Ok(response)
}

pub async fn save_provider_settings_with_runtime_state(
    request: ProviderSettingsRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    let _provider_settings_guard = runtime_state.provider_settings_lock.lock().await;
    save_provider_settings_with_runtime_state_unlocked(request, runtime_state).await
}

pub(crate) trait ProviderSettingsMetadata {
    fn provider_id(&self) -> &str;
    fn model_id(&self) -> &str;
}

impl ProviderSettingsMetadata for ProviderSettingsRequest {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

impl ProviderSettingsMetadata for ValidateProviderSettingsRequest {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

pub(crate) async fn ensure_provider_model_supported<T: ProviderSettingsMetadata>(
    request: &T,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    ensure_provider_metadata_shape(request.provider_id(), request.model_id())?;
    resolve_provider_model_descriptor(request.provider_id(), request.model_id()).await
}

pub(crate) async fn provider_settings_descriptor(
    request: &ProviderSettingsRequest,
    previous_config: Option<&ProviderConfigRecord>,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    ensure_provider_metadata_shape(&request.provider_id, &request.model_id)?;
    match resolve_model_descriptor(&request.provider_id, &request.model_id) {
        Ok(descriptor) => Ok(descriptor),
        Err(error) if request.provider_id == "openrouter" => {
            if let Some(previous_config) = previous_config {
                if previous_config.provider_id == request.provider_id
                    && previous_config.model_id == request.model_id
                {
                    return provider_config_descriptor(previous_config);
                }
            }
            resolve_provider_model_descriptor(&request.provider_id, &request.model_id)
                .await
                .map_err(|_| provider_registry_error(error))
        }
        Err(error) => Err(provider_registry_error(error)),
    }
}

pub(crate) async fn resolve_provider_model_descriptor(
    provider_id: &str,
    model_id: &str,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    match resolve_model_descriptor(provider_id, model_id) {
        Ok(descriptor) => Ok(descriptor),
        Err(error) if provider_id == "openrouter" => {
            let Some(inventory) = fetch_openrouter_inventory().await else {
                return Err(provider_registry_error(error));
            };
            let Some(model) = inventory
                .into_iter()
                .find(|model| model.model_id == model_id)
            else {
                return Err(provider_registry_error(error));
            };
            descriptor_from_inventory_model(model)
        }
        Err(error) => Err(provider_registry_error(error)),
    }
}

pub(crate) fn provider_registry_error(error: ProviderRegistryError) -> CommandErrorPayload {
    invalid_payload(error.to_string())
}

pub(crate) fn provider_registry_init_error(error: ProviderRegistryError) -> CommandErrorPayload {
    runtime_init_failed(error.to_string())
}

pub(crate) fn provider_display_name(provider_id: &str) -> String {
    provider_catalog_entries()
        .into_iter()
        .find(|entry| entry.provider_id == provider_id)
        .map_or_else(|| provider_id.to_owned(), |entry| entry.display_name)
}

pub(crate) fn model_descriptor_catalog_entry(descriptor: ModelDescriptor) -> ModelCatalogEntry {
    let conversation_capability = descriptor.conversation_capability;
    ModelCatalogEntry {
        protocol: descriptor.protocol,
        conversation_capability: conversation_capability_record(&conversation_capability),
        context_window: descriptor.context_window,
        display_name: descriptor.display_name,
        lifecycle: model_lifecycle_payload(descriptor.lifecycle),
        max_output_tokens: descriptor.max_output_tokens,
        model_id: descriptor.model_id,
        runtime_status: ModelRuntimeStatusPayload {
            kind: "runnable",
            reason: None,
        },
    }
}

pub(crate) fn model_lifecycle_payload(lifecycle: ModelLifecycle) -> ModelLifecyclePayload {
    match lifecycle {
        ModelLifecycle::Stable => ModelLifecyclePayload {
            kind: "stable",
            retirement_date: None,
        },
        ModelLifecycle::Preview => ModelLifecyclePayload {
            kind: "preview",
            retirement_date: None,
        },
        ModelLifecycle::Deprecated { retirement_date } => ModelLifecyclePayload {
            kind: "deprecated",
            retirement_date: Some(retirement_date.to_string()),
        },
    }
}

pub(crate) fn model_descriptor_record(
    descriptor: &ModelDescriptor,
) -> ProviderModelDescriptorRecord {
    ProviderModelDescriptorRecord {
        protocol: descriptor.protocol,
        conversation_capability: conversation_capability_record(
            &descriptor.conversation_capability,
        ),
        context_window: descriptor.context_window,
        display_name: descriptor.display_name.clone(),
        lifecycle: model_lifecycle_record(&descriptor.lifecycle),
        max_output_tokens: descriptor.max_output_tokens,
        model_id: descriptor.model_id.clone(),
        provider_id: descriptor.provider_id.clone(),
    }
}

pub(crate) fn conversation_capability_record(
    capabilities: &ConversationModelCapability,
) -> ConversationModelCapabilityRecord {
    ConversationModelCapabilityRecord {
        input_modalities: capabilities
            .input_modalities
            .iter()
            .map(model_modality_record)
            .collect(),
        output_modalities: capabilities
            .output_modalities
            .iter()
            .map(model_modality_record)
            .collect(),
        context_window: capabilities.context_window,
        max_output_tokens: capabilities.max_output_tokens,
        streaming: capabilities.streaming,
        tool_calling: capabilities.tool_calling,
        reasoning: capabilities.reasoning,
        prompt_cache: capabilities.prompt_cache,
        structured_output: capabilities.structured_output,
    }
}

pub(crate) fn model_lifecycle_record(lifecycle: &ModelLifecycle) -> ProviderModelLifecycleRecord {
    match lifecycle {
        ModelLifecycle::Stable => ProviderModelLifecycleRecord::Stable,
        ModelLifecycle::Preview => ProviderModelLifecycleRecord::Preview,
        ModelLifecycle::Deprecated { retirement_date } => {
            ProviderModelLifecycleRecord::Deprecated {
                retirement_date: retirement_date.to_string(),
            }
        }
    }
}

pub(crate) fn model_modality_record(modality: &ModelModality) -> ProviderModelModalityRecord {
    match modality {
        ModelModality::Text => ProviderModelModalityRecord::Text,
        ModelModality::Image => ProviderModelModalityRecord::Image,
        ModelModality::Audio => ProviderModelModalityRecord::Audio,
        ModelModality::Video => ProviderModelModalityRecord::Video,
        ModelModality::File => ProviderModelModalityRecord::File,
        ModelModality::Embedding => ProviderModelModalityRecord::Embedding,
    }
}

pub(crate) fn model_descriptor_from_record(
    record: &ProviderModelDescriptorRecord,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    Ok(ModelDescriptor {
        provider_id: record.provider_id.clone(),
        model_id: record.model_id.clone(),
        display_name: record.display_name.clone(),
        protocol: record.protocol,
        context_window: record.context_window,
        max_output_tokens: record.max_output_tokens,
        conversation_capability: conversation_capability_from_record(
            &record.conversation_capability,
        ),
        runtime_semantics: ModelRuntimeSemantics::messages_default(record.protocol),
        lifecycle: model_lifecycle_from_record(&record.lifecycle)?,
        pricing: None,
    })
}

pub(crate) fn conversation_capability_from_record(
    record: &ConversationModelCapabilityRecord,
) -> ConversationModelCapability {
    ConversationModelCapability {
        input_modalities: record
            .input_modalities
            .iter()
            .map(model_modality_from_record)
            .collect(),
        output_modalities: record
            .output_modalities
            .iter()
            .map(model_modality_from_record)
            .collect(),
        context_window: record.context_window,
        max_output_tokens: record.max_output_tokens,
        streaming: record.streaming,
        tool_calling: record.tool_calling,
        reasoning: record.reasoning,
        prompt_cache: record.prompt_cache,
        structured_output: record.structured_output,
    }
}

pub(crate) fn model_lifecycle_from_record(
    record: &ProviderModelLifecycleRecord,
) -> Result<ModelLifecycle, CommandErrorPayload> {
    match record {
        ProviderModelLifecycleRecord::Stable => Ok(ModelLifecycle::Stable),
        ProviderModelLifecycleRecord::Preview => Ok(ModelLifecycle::Preview),
        ProviderModelLifecycleRecord::Deprecated { retirement_date } => {
            let retirement_date =
                NaiveDate::parse_from_str(retirement_date, "%Y-%m-%d").map_err(|_| {
                    runtime_init_failed("provider model descriptor is invalid".to_owned())
                })?;
            Ok(ModelLifecycle::Deprecated { retirement_date })
        }
    }
}

pub(crate) fn model_modality_from_record(record: &ProviderModelModalityRecord) -> ModelModality {
    match record {
        ProviderModelModalityRecord::Text => ModelModality::Text,
        ProviderModelModalityRecord::Image => ModelModality::Image,
        ProviderModelModalityRecord::Audio => ModelModality::Audio,
        ProviderModelModalityRecord::Video => ModelModality::Video,
        ProviderModelModalityRecord::File => ModelModality::File,
        ProviderModelModalityRecord::Embedding => ModelModality::Embedding,
    }
}

pub(crate) fn descriptor_from_inventory_model(
    model: ModelInventoryEntry,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    match model.runtime_status {
        ModelRuntimeStatus::Runnable => Ok(ModelDescriptor {
            provider_id: model.provider_id,
            model_id: model.model_id,
            display_name: model.display_name,
            protocol: model.protocol,
            context_window: model.context_window,
            max_output_tokens: model.max_output_tokens,
            conversation_capability: model.conversation_capability,
            runtime_semantics: model.runtime_semantics,
            lifecycle: model.lifecycle,
            pricing: model.pricing,
        }),
        ModelRuntimeStatus::Unsupported { reason } => Err(invalid_payload(format!(
            "model is not supported by the current runtime: {reason}"
        ))),
    }
}

pub(crate) fn provider_config_id(
    record: &ProviderSettingsRecord,
    request: &ProviderSettingsRequest,
) -> String {
    if let Some(config_id) = request
        .config_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return config_id.to_owned();
    }
    let provider_id = request.provider_id.clone();
    if record.configs.iter().any(|config| config.id == provider_id) {
        return format!("{provider_id}-{}", RunId::new());
    }
    provider_id
}

pub(crate) fn provider_config_payloads(
    record: &ProviderSettingsRecord,
) -> Result<Vec<ProviderConfigPayload>, CommandErrorPayload> {
    record
        .configs
        .iter()
        .map(|config| provider_config_payload(config, record.default_config_id.as_deref()))
        .collect()
}

pub(crate) fn provider_config_payload(
    config: &ProviderConfigRecord,
    default_config_id: Option<&str>,
) -> Result<ProviderConfigPayload, CommandErrorPayload> {
    let descriptor = provider_config_descriptor(config)?;
    Ok(ProviderConfigPayload {
        protocol: descriptor.protocol,
        base_url: config.base_url.clone(),
        display_name: config.display_name.clone(),
        has_api_key: provider_config_has_api_key(config),
        has_official_quota_api_key: provider_config_has_official_quota_api_key(config),
        id: config.id.clone(),
        is_default: default_config_id.is_some_and(|id| id == config.id),
        model_id: config.model_id.clone(),
        provider_id: config.provider_id.clone(),
        model_descriptor: model_descriptor_catalog_entry(descriptor),
    })
}

pub(crate) fn provider_config_has_api_key(config: &ProviderConfigRecord) -> bool {
    !config.api_key.trim().is_empty()
}

pub(crate) fn provider_config_has_official_quota_api_key(config: &ProviderConfigRecord) -> bool {
    config
        .official_quota_api_key
        .as_deref()
        .is_some_and(|api_key| !api_key.trim().is_empty())
}

pub(crate) fn ensure_provider_config_has_api_key(
    config: &ProviderConfigRecord,
) -> Result<&str, CommandErrorPayload> {
    config
        .api_key
        .trim()
        .is_empty()
        .then(|| invalid_payload("apiKey is not configured for this provider config".to_owned()))
        .map_or_else(|| Ok(config.api_key.trim()), Err)
}

pub(crate) fn provider_config_descriptor(
    config: &ProviderConfigRecord,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    match resolve_model_descriptor(&config.provider_id, &config.model_id) {
        Ok(descriptor) => Ok(descriptor),
        Err(_) if config.provider_id == "openrouter" => {
            let descriptor = model_descriptor_from_record(&config.model_descriptor)?;
            if descriptor.provider_id != config.provider_id
                || descriptor.model_id != config.model_id
            {
                return Err(runtime_init_failed(
                    "provider model descriptor does not match provider config".to_owned(),
                ));
            }
            if descriptor.protocol != ModelProtocol::ChatCompletions {
                return Err(runtime_init_failed(
                    "provider model descriptor protocol is not supported".to_owned(),
                ));
            }
            if !descriptor.conversation_capability.streaming {
                return Err(runtime_init_failed(
                    "provider model descriptor is not runnable".to_owned(),
                ));
            }
            if !descriptor_has_runtime_supported_modalities(&descriptor) {
                return Err(runtime_init_failed(
                    "provider model descriptor is not supported by the current runtime".to_owned(),
                ));
            }
            Ok(descriptor)
        }
        Err(error) => Err(provider_registry_error(error)),
    }
}

pub(crate) fn descriptor_has_runtime_supported_modalities(descriptor: &ModelDescriptor) -> bool {
    descriptor
        .conversation_capability
        .input_modalities
        .iter()
        .all(|modality| matches!(modality, ModelModality::Text))
        && descriptor
            .conversation_capability
            .output_modalities
            .iter()
            .all(|modality| matches!(modality, ModelModality::Text))
        && !descriptor
            .conversation_capability
            .input_modalities
            .is_empty()
        && !descriptor
            .conversation_capability
            .output_modalities
            .is_empty()
}

pub(crate) fn normalized_base_url(
    value: Option<&str>,
) -> Result<Option<String>, CommandErrorPayload> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if !(value.starts_with("https://") || value.starts_with("http://")) {
        return Err(invalid_payload(
            "baseUrl must start with http:// or https://".to_owned(),
        ));
    }
    if value.contains('?') || value.contains('#') {
        return Err(invalid_payload(
            "baseUrl must not include query parameters or fragments".to_owned(),
        ));
    }
    let parsed = reqwest::Url::parse(value)
        .map_err(|_| invalid_payload("baseUrl must be a valid http(s) URL".to_owned()))?;
    if parsed.host_str().is_none() || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(invalid_payload(
            "baseUrl must not include credentials".to_owned(),
        ));
    }
    if parsed.scheme() == "http" && !url_targets_loopback(&parsed) {
        return Err(invalid_payload(
            "baseUrl must use https:// unless it targets localhost".to_owned(),
        ));
    }
    Ok(Some(value.trim_end_matches('/').to_owned()))
}

pub(crate) fn url_targets_loopback(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

pub(crate) fn build_provider_for_config(
    config: &ProviderConfigRecord,
) -> Result<(Arc<dyn ModelProvider>, ModelProtocol), CommandErrorPayload> {
    let descriptor = provider_config_descriptor(config)?;
    let api_key = config.api_key.trim();
    if api_key.is_empty() {
        return Err(runtime_init_failed(
            "provider config has no api key".to_owned(),
        ));
    }
    let base_url = normalized_base_url(config.base_url.as_deref())?;
    let provider = build_provider(ProviderBuildConfig {
        provider_id: config.provider_id.clone(),
        api_key: api_key.to_owned(),
        base_url,
        model_descriptor: Some(descriptor.clone()),
    })
    .map_err(provider_registry_init_error)?;
    Ok((Arc::from(provider), descriptor.protocol))
}

pub(crate) fn provider_config_by_id<'a>(
    record: &'a ProviderSettingsRecord,
    config_id: &str,
) -> Result<&'a ProviderConfigRecord, CommandErrorPayload> {
    record
        .configs
        .iter()
        .find(|config| config.id == config_id)
        .ok_or_else(|| invalid_payload("provider config was not found".to_owned()))
}

pub(crate) fn model_from_provider_settings(
    store: &dyn ProviderSettingsStore,
    selected_config_id: Option<&str>,
) -> Result<Option<(Arc<dyn ModelProvider>, String, ModelProtocol)>, CommandErrorPayload> {
    let Some(record) = store.load_record()? else {
        if selected_config_id.is_some() {
            return Err(runtime_init_failed("provider config is missing".to_owned()));
        }
        return Ok(None);
    };
    let Some(config_id) = selected_config_id.or(record.default_config_id.as_deref()) else {
        return Ok(None);
    };
    let Some(config) = record.configs.iter().find(|config| config.id == config_id) else {
        return Err(runtime_init_failed("provider config is missing".to_owned()));
    };
    let descriptor = provider_config_descriptor(config)?;
    let api_key = config.api_key.trim();
    if api_key.is_empty() {
        return Err(runtime_init_failed(
            "provider config has no api key".to_owned(),
        ));
    }
    let base_url = normalized_base_url(config.base_url.as_deref())?;
    let provider = build_provider(ProviderBuildConfig {
        provider_id: config.provider_id.clone(),
        api_key: api_key.to_owned(),
        base_url,
        model_descriptor: Some(descriptor.clone()),
    })
    .map_err(provider_registry_init_error)?;
    let protocol = descriptor.protocol;

    Ok(Some((
        Arc::from(provider),
        config.model_id.clone(),
        protocol,
    )))
}

// ── Provider settings migration ──────────────────────────────────────────

use harness_contracts::{
    ModelProtocol, ProviderProfileDefinition, ProviderProfileModelDescriptor, ProviderSecretEntry,
    ProviderSelectionRecord,
};

/// Migrate old workspace `<workspace>/.jyowo/runtime/provider-settings.json`
/// into the split layout:
/// - `~/.jyowo/config/provider-profiles.json`
/// - `~/.jyowo/config/provider-secrets.json`
/// - `<workspace>/.jyowo/config/provider-selection.json`
///
/// Rules:
/// - Profile ids reuse old config ids when no collision.
/// - Same id + identical non-secret fields + same secret fingerprint → reuse existing.
/// - Same id + different fields or secret → mint `<oldId>-<workspaceHash8>`.
/// - Project `provider-selection.json` remaps old `defaultConfigId`.
/// - Secrets are written only to global secret storage.
pub fn migrate_provider_settings_to_split_layout(
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let Some(workspace_root) = state.project_workspace_root() else {
        return Ok(());
    };
    let old_path = workspace_root
        .join(".jyowo")
        .join("runtime")
        .join("provider-settings.json");

    let global_config = state.global_config_store.as_ref().ok_or_else(|| {
        runtime_init_failed("global config store is unavailable for migration".to_owned())
    })?;
    let project_config = state.project_config_store.as_ref().ok_or_else(|| {
        runtime_init_failed("project config store is unavailable for migration".to_owned())
    })?;
    let workspace_hash8 = workspace_hash_short(workspace_root);
    let provider_profiles_path = global_config.layout().global_provider_profiles_file();
    let provider_secrets_path = global_config.layout().global_provider_secrets_file();
    let project_selection_path = project_config
        .layout()
        .project_provider_selection_file(project_config.workspace_root());
    let staging = ProviderMigrationStagingPaths::new(
        &provider_profiles_path,
        &provider_secrets_path,
        &project_selection_path,
        &workspace_hash8,
    )?;

    with_provider_migration_lock(&staging, || {
        let quarantine_path = provider_migration_quarantine_path(&old_path, &workspace_hash8)?;
        recover_provider_migration_artifacts(&staging, &old_path, Some(&quarantine_path))?;

        let old_record: Option<ProviderSettingsRecord> =
            read_secret_json_file_invalid_payload(&old_path, "provider settings migration")?;
        let Some(old_record) = old_record else {
            return Ok(());
        };
        ensure_provider_settings_record(&old_record)?;

        let mut profiles = global_config.load_provider_profiles()?;
        let mut secrets = read_secret_json_file::<Vec<ProviderSecretEntry>>(
            &provider_secrets_path,
            "provider secrets",
        )?
        .unwrap_or_default();
        let mut migrated_ids: Vec<String> = Vec::new();

        for config in &old_record.configs {
            let (mut profile_id, _) =
                resolve_migrated_profile_id(config, &profiles, &workspace_hash8);
            if profile_id == config.id {
                if let Some(existing_secret) = global_config.load_provider_secret(&profile_id)? {
                    if !provider_secret_matches_config(&existing_secret, config) {
                        profile_id = format!("{}-{}", config.id, workspace_hash8);
                    }
                }
            }
            profile_id = resolve_available_migrated_profile_id(
                config,
                &profiles,
                global_config,
                &workspace_hash8,
                profile_id,
            )?;

            let profile = ProviderProfileDefinition {
                id: profile_id.clone(),
                display_name: config.display_name.clone(),
                provider_id: config.provider_id.clone(),
                model_id: config.model_id.clone(),
                protocol: config.protocol,
                base_url: config.base_url.clone(),
                model_descriptor: provider_profile_descriptor_from_config(config),
            };

            profiles.retain(|p| p.id != profile_id);
            profiles.push(profile);

            let secret_entry = ProviderSecretEntry {
                config_id: profile_id.clone(),
                api_key: config.api_key.clone(),
                official_quota_api_key: config.official_quota_api_key.clone(),
            };
            if let Some(existing) = secrets
                .iter_mut()
                .find(|entry| entry.config_id == secret_entry.config_id)
            {
                *existing = secret_entry;
            } else {
                secrets.push(secret_entry);
            }

            migrated_ids.push(profile_id);
        }

        let project_selection = ProviderSelectionRecord {
            default_config_id: old_record.default_config_id.and_then(|old_default| {
                old_record
                    .configs
                    .iter()
                    .position(|c| c.id == old_default)
                    .and_then(|idx| migrated_ids.get(idx))
                    .cloned()
            }),
        };

        write_json_file_atomic(
            &staging.provider_profiles,
            "provider profiles migration staging",
            &profiles,
        )?;
        write_secret_json_file_atomic(
            &staging.provider_secrets,
            "provider secrets migration staging",
            &secrets,
        )?;
        write_json_file_atomic(
            &staging.project_selection,
            "project provider selection migration staging",
            &project_selection,
        )?;

        let commit = commit_provider_migration_staging_retaining_backups(&staging)?;
        match quarantine_migrated_provider_settings(&old_path, &workspace_hash8) {
            Ok(_) => finish_provider_migration_commit(commit)?,
            Err(error) => {
                let rollback_result =
                    rollback_provider_migration_commit(&commit.committed_targets, &commit.backups);
                let restore_result = restore_quarantined_provider_settings(
                    &old_path,
                    Some(&provider_migration_quarantine_path(
                        &old_path,
                        &workspace_hash8,
                    )?),
                );
                if let Err(rollback_error) = rollback_result {
                    return Err(runtime_operation_failed(format!(
                        "provider settings quarantine failed: {}; migration rollback failed: {}",
                        error.message, rollback_error.message
                    )));
                }
                if let Err(restore_error) = restore_result {
                    return Err(runtime_operation_failed(format!(
                        "provider settings quarantine failed: {}; old settings restore failed: {}",
                        error.message, restore_error.message
                    )));
                }
                return Err(error);
            }
        }
        Ok(())
    })
}

struct ProviderMigrationStagingPaths {
    provider_profiles: PathBuf,
    provider_profiles_target: PathBuf,
    provider_secrets: PathBuf,
    provider_secrets_target: PathBuf,
    project_selection: PathBuf,
    project_selection_target: PathBuf,
    workspace_hash8: String,
}

impl ProviderMigrationStagingPaths {
    fn new(
        provider_profiles_target: &Path,
        provider_secrets_target: &Path,
        project_selection_target: &Path,
        workspace_hash8: &str,
    ) -> Result<Self, CommandErrorPayload> {
        Ok(Self {
            provider_profiles: provider_migration_staging_path(
                provider_profiles_target,
                workspace_hash8,
            )?,
            provider_profiles_target: provider_profiles_target.to_path_buf(),
            provider_secrets: provider_migration_staging_path(
                provider_secrets_target,
                workspace_hash8,
            )?,
            provider_secrets_target: provider_secrets_target.to_path_buf(),
            project_selection: provider_migration_staging_path(
                project_selection_target,
                workspace_hash8,
            )?,
            project_selection_target: project_selection_target.to_path_buf(),
            workspace_hash8: workspace_hash8.to_owned(),
        })
    }
}

fn with_provider_migration_lock<T>(
    staging: &ProviderMigrationStagingPaths,
    action: impl FnOnce() -> Result<T, CommandErrorPayload>,
) -> Result<T, CommandErrorPayload> {
    let lock_path = staging
        .provider_profiles_target
        .with_file_name(".provider-settings-migration.lock");
    let parent = lock_path.parent().ok_or_else(|| {
        runtime_operation_failed("provider migration lock path has no parent".to_owned())
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        runtime_operation_failed(format!(
            "provider migration lock directory unavailable: {error}"
        ))
    })?;
    ensure_app_dir_no_symlink(parent, "provider migration lock directory")?;
    ensure_no_symlink_components(&lock_path, "provider migration lock")?;
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| {
            runtime_operation_failed(format!("provider migration lock open failed: {error}"))
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        lock_file
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|error| {
                runtime_operation_failed(format!(
                    "provider migration lock permissions failed: {error}"
                ))
            })?;
    }
    lock_file.lock_exclusive().map_err(|error| {
        runtime_operation_failed(format!("provider migration lock failed: {error}"))
    })?;
    let result = action();
    let unlock_result = lock_file.unlock().map_err(|error| {
        runtime_operation_failed(format!("provider migration unlock failed: {error}"))
    });
    match (result, unlock_result) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

fn provider_migration_staging_path(
    target: &Path,
    workspace_hash8: &str,
) -> Result<PathBuf, CommandErrorPayload> {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            runtime_operation_failed("provider migration target path has no file name".to_owned())
        })?;
    Ok(target.with_file_name(format!("{file_name}.migration-staging-{workspace_hash8}")))
}

fn provider_migration_backup_path(
    target: &Path,
    workspace_hash8: &str,
) -> Result<PathBuf, CommandErrorPayload> {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            runtime_operation_failed("provider migration target path has no file name".to_owned())
        })?;
    Ok(target.with_file_name(format!("{file_name}.migration-backup-{workspace_hash8}")))
}

fn provider_migration_quarantine_path(
    old_path: &Path,
    workspace_hash8: &str,
) -> Result<PathBuf, CommandErrorPayload> {
    let file_name = old_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            runtime_operation_failed("old provider settings path has no file name".to_owned())
        })?;
    Ok(old_path.with_file_name(format!("{file_name}.migrated-{workspace_hash8}")))
}

struct ProviderMigrationEntry<'a> {
    staging: &'a Path,
    target: &'a Path,
    label: &'static str,
}

fn provider_migration_entries(
    staging: &ProviderMigrationStagingPaths,
) -> [ProviderMigrationEntry<'_>; 3] {
    [
        ProviderMigrationEntry {
            staging: &staging.provider_profiles,
            target: &staging.provider_profiles_target,
            label: "provider profiles migration commit",
        },
        ProviderMigrationEntry {
            staging: &staging.provider_secrets,
            target: &staging.provider_secrets_target,
            label: "provider secrets migration commit",
        },
        ProviderMigrationEntry {
            staging: &staging.project_selection,
            target: &staging.project_selection_target,
            label: "project provider selection migration commit",
        },
    ]
}

fn recover_provider_migration_artifacts(
    staging: &ProviderMigrationStagingPaths,
    old_path: &Path,
    quarantine_path: Option<&Path>,
) -> Result<(), CommandErrorPayload> {
    let entries = provider_migration_entries(staging);
    let mut staging_present = Vec::new();
    let mut backup_present = Vec::new();
    for entry in &entries {
        staging_present.push(regular_file_artifact_present(entry.staging, entry.label)?);
        let backup = provider_migration_backup_path(entry.target, &staging.workspace_hash8)?;
        backup_present.push(regular_file_artifact_present(
            &backup,
            "provider migration backup",
        )?);
    }
    let quarantine_present = match quarantine_path {
        Some(path) => regular_file_artifact_present(path, "provider settings quarantine")?,
        None => false,
    };
    let any_staging = staging_present.iter().any(|present| *present);
    let any_backup = backup_present.iter().any(|present| *present);
    let partial_commit = any_staging && staging_present.iter().any(|present| !*present);

    if any_backup || quarantine_present || partial_commit {
        for entry in entries.iter().rev() {
            retire_existing_regular_file_no_follow(
                entry.target,
                "provider migration recovery target",
            )?;
        }
        for entry in entries.iter().rev() {
            let backup = provider_migration_backup_path(entry.target, &staging.workspace_hash8)?;
            rename_existing_regular_file_no_follow_if_present(
                &backup,
                entry.target,
                "provider migration recovery backup restore",
            )?;
        }
    }

    for entry in &entries {
        retire_existing_regular_file_no_follow(
            entry.staging,
            "provider migration recovery staging",
        )?;
    }
    if let Some(quarantine_path) = quarantine_path {
        rename_existing_regular_file_no_follow_if_present(
            quarantine_path,
            old_path,
            "provider migration recovery quarantine restore",
        )?;
    }
    Ok(())
}

fn regular_file_artifact_present(path: &Path, label: &str) -> Result<bool, CommandErrorPayload> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(runtime_operation_failed(
            format!("{label} must not use symlinks"),
        )),
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => Err(runtime_operation_failed(format!(
            "{label} is not a regular file"
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(runtime_operation_failed(format!(
            "{label} metadata failed: {error}"
        ))),
    }
}

#[cfg(test)]
fn commit_provider_migration_staging(
    staging: &ProviderMigrationStagingPaths,
) -> Result<(), CommandErrorPayload> {
    let commit = commit_provider_migration_staging_retaining_backups(staging)?;
    finish_provider_migration_commit(commit)
}

struct ProviderMigrationCommit {
    committed_targets: Vec<PathBuf>,
    backups: Vec<(PathBuf, PathBuf)>,
}

fn commit_provider_migration_staging_retaining_backups(
    staging: &ProviderMigrationStagingPaths,
) -> Result<ProviderMigrationCommit, CommandErrorPayload> {
    let entries = provider_migration_entries(staging);
    let mut backups: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut committed_targets: Vec<PathBuf> = Vec::new();

    for entry in &entries {
        let backup = provider_migration_backup_path(entry.target, &staging.workspace_hash8)?;
        match rename_existing_regular_file_no_follow_if_present(entry.target, &backup, entry.label)
        {
            Ok(true) => backups.push((entry.target.to_path_buf(), backup)),
            Ok(false) => {}
            Err(error) => {
                if let Err(rollback_error) =
                    rollback_provider_migration_commit(&committed_targets, &backups)
                {
                    return Err(runtime_operation_failed(format!(
                        "{} backup failed: {}; rollback failed: {}",
                        entry.label, error.message, rollback_error.message
                    )));
                }
                return Err(runtime_operation_failed(format!(
                    "{} backup failed: {}",
                    entry.label, error.message
                )));
            }
        }
    }

    for entry in &entries {
        if let Err(error) =
            rename_existing_regular_file_no_follow(entry.staging, entry.target, entry.label)
        {
            if let Err(rollback_error) =
                rollback_provider_migration_commit(&committed_targets, &backups)
            {
                return Err(runtime_operation_failed(format!(
                    "{} failed: {}; rollback failed: {}",
                    entry.label, error.message, rollback_error.message
                )));
            }
            return Err(runtime_operation_failed(format!(
                "{} failed: {}",
                entry.label, error.message
            )));
        }
        committed_targets.push(entry.target.to_path_buf());
    }

    Ok(ProviderMigrationCommit {
        committed_targets,
        backups,
    })
}

fn finish_provider_migration_commit(
    commit: ProviderMigrationCommit,
) -> Result<(), CommandErrorPayload> {
    for (_, backup) in commit.backups {
        retire_existing_regular_file_no_follow(&backup, "provider migration backup")?;
    }
    Ok(())
}

fn rollback_provider_migration_commit(
    committed_targets: &[PathBuf],
    backups: &[(PathBuf, PathBuf)],
) -> Result<(), CommandErrorPayload> {
    for target in committed_targets.iter().rev() {
        retire_existing_regular_file_no_follow(target, "provider migration rollback target")?;
    }
    for (target, backup) in backups.iter().rev() {
        if let Err(error) = rename_existing_regular_file_no_follow_if_present(
            backup,
            target,
            "provider migration rollback backup restore",
        ) {
            return Err(runtime_operation_failed(format!(
                "provider migration rollback backup restore failed: {}",
                error.message
            )));
        }
    }
    Ok(())
}

fn quarantine_migrated_provider_settings(
    old_path: &Path,
    workspace_hash8: &str,
) -> Result<Option<PathBuf>, CommandErrorPayload> {
    let quarantine_path = provider_migration_quarantine_path(old_path, workspace_hash8)?;
    match rename_existing_regular_file_no_follow_if_present(
        old_path,
        &quarantine_path,
        "provider settings migration quarantine",
    )? {
        true => Ok(Some(quarantine_path)),
        false => Ok(None),
    }
}

fn restore_quarantined_provider_settings(
    old_path: &Path,
    quarantine_path: Option<&Path>,
) -> Result<(), CommandErrorPayload> {
    let Some(quarantine_path) = quarantine_path else {
        return Ok(());
    };
    match rename_existing_regular_file_no_follow_if_present(
        quarantine_path,
        old_path,
        "provider settings quarantine restore",
    ) {
        Ok(true) | Ok(false) => Ok(()),
        Err(error) => Err(runtime_operation_failed(format!(
            "provider settings quarantine restore failed: {}",
            error.message
        ))),
    }
}

/// Resolve the target global profile id for a migrated config.
///
/// Returns `(profile_id, is_new)`.
fn resolve_migrated_profile_id(
    config: &ProviderConfigRecord,
    existing_profiles: &[ProviderProfileDefinition],
    workspace_hash8: &str,
) -> (String, bool) {
    let candidate_id = &config.id;

    // Check if an existing global profile with this id already exists.
    if let Some(existing) = existing_profiles.iter().find(|p| &p.id == candidate_id) {
        // Check if the non-secret fields match.
        let profile = provider_profile_definition_from_config(config, candidate_id.clone());
        if profiles_equivalent(existing, &profile) {
            // Same id + identical profile → reuse.
            return (candidate_id.clone(), false);
        }
        // Same id + different profile → mint new id.
        let new_id = format!("{candidate_id}-{workspace_hash8}");
        return (new_id, true);
    }

    // No existing profile with this id → use as-is.
    (candidate_id.clone(), true)
}

fn resolve_available_migrated_profile_id(
    config: &ProviderConfigRecord,
    existing_profiles: &[ProviderProfileDefinition],
    global_config: &GlobalConfigStore,
    workspace_hash8: &str,
    preferred_id: String,
) -> Result<String, CommandErrorPayload> {
    let mut candidate_id = preferred_id;
    let mut suffix = 2usize;
    loop {
        if migrated_profile_id_can_be_used(&candidate_id, config, existing_profiles, global_config)?
        {
            return Ok(candidate_id);
        }
        candidate_id = format!("{}-{}-{suffix}", config.id, workspace_hash8);
        suffix += 1;
    }
}

fn migrated_profile_id_can_be_used(
    candidate_id: &str,
    config: &ProviderConfigRecord,
    existing_profiles: &[ProviderProfileDefinition],
    global_config: &GlobalConfigStore,
) -> Result<bool, CommandErrorPayload> {
    let Some(existing_profile) = existing_profiles
        .iter()
        .find(|profile| profile.id == candidate_id)
    else {
        return Ok(true);
    };
    let candidate_profile =
        provider_profile_definition_from_config(config, candidate_id.to_owned());
    if !profiles_equivalent(existing_profile, &candidate_profile) {
        return Ok(false);
    }
    let Some(existing_secret) = global_config.load_provider_secret(candidate_id)? else {
        return Ok(true);
    };
    Ok(provider_secret_matches_config(&existing_secret, config))
}

fn provider_profile_definition_from_config(
    config: &ProviderConfigRecord,
    id: String,
) -> ProviderProfileDefinition {
    ProviderProfileDefinition {
        id,
        display_name: config.display_name.clone(),
        provider_id: config.provider_id.clone(),
        model_id: config.model_id.clone(),
        protocol: config.protocol,
        base_url: config.base_url.clone(),
        model_descriptor: provider_profile_descriptor_from_config(config),
    }
}

fn provider_profile_descriptor_from_config(
    config: &ProviderConfigRecord,
) -> ProviderProfileModelDescriptor {
    ProviderProfileModelDescriptor {
        protocol: config.protocol,
        context_window: config.model_descriptor.context_window,
        display_name: config.model_descriptor.display_name.clone(),
        lifecycle: provider_profile_lifecycle_from_record(&config.model_descriptor.lifecycle),
        max_output_tokens: config.model_descriptor.max_output_tokens,
        model_id: config.model_descriptor.model_id.clone(),
        provider_id: config.model_descriptor.provider_id.clone(),
        conversation_capability: harness_contracts::ProviderProfileConversationCapability {
            input_modalities: config
                .model_descriptor
                .conversation_capability
                .input_modalities
                .iter()
                .map(modality_to_string)
                .collect(),
            output_modalities: config
                .model_descriptor
                .conversation_capability
                .output_modalities
                .iter()
                .map(modality_to_string)
                .collect(),
            context_window: config
                .model_descriptor
                .conversation_capability
                .context_window,
            max_output_tokens: config
                .model_descriptor
                .conversation_capability
                .max_output_tokens,
            streaming: config.model_descriptor.conversation_capability.streaming,
            tool_calling: config.model_descriptor.conversation_capability.tool_calling,
            reasoning: config.model_descriptor.conversation_capability.reasoning,
            prompt_cache: config.model_descriptor.conversation_capability.prompt_cache,
            structured_output: config
                .model_descriptor
                .conversation_capability
                .structured_output,
        },
    }
}

fn provider_profile_lifecycle_from_record(
    lifecycle: &ProviderModelLifecycleRecord,
) -> harness_contracts::ProviderProfileModelLifecycle {
    match lifecycle {
        ProviderModelLifecycleRecord::Stable => {
            harness_contracts::ProviderProfileModelLifecycle::Stable
        }
        ProviderModelLifecycleRecord::Preview => {
            harness_contracts::ProviderProfileModelLifecycle::Preview
        }
        ProviderModelLifecycleRecord::Deprecated { retirement_date } => {
            harness_contracts::ProviderProfileModelLifecycle::Deprecated {
                retirement_date: retirement_date.clone(),
            }
        }
    }
}

fn profiles_equivalent(
    existing: &ProviderProfileDefinition,
    new: &ProviderProfileDefinition,
) -> bool {
    existing.provider_id == new.provider_id
        && existing.model_id == new.model_id
        && existing.protocol == new.protocol
        && existing.base_url == new.base_url
        && existing.display_name == new.display_name
}

fn provider_secret_matches_config(
    existing: &harness_contracts::global_config::ProviderSecretEntry,
    config: &ProviderConfigRecord,
) -> bool {
    existing.api_key == config.api_key
        && existing.official_quota_api_key == config.official_quota_api_key
}

pub(crate) fn workspace_hash_short(workspace_root: &Path) -> String {
    let hash = blake3::hash(workspace_root.to_string_lossy().as_bytes());
    // Use the first 4 bytes as an 8-character hex string.
    let bytes = hash.as_bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

fn modality_to_string(modality: &ProviderModelModalityRecord) -> String {
    match modality {
        ProviderModelModalityRecord::Text => "text".to_owned(),
        ProviderModelModalityRecord::Image => "image".to_owned(),
        ProviderModelModalityRecord::Audio => "audio".to_owned(),
        ProviderModelModalityRecord::Video => "video".to_owned(),
        ProviderModelModalityRecord::File => "file".to_owned(),
        ProviderModelModalityRecord::Embedding => "embedding".to_owned(),
    }
}

#[cfg(test)]
mod execution_settings_tests {
    use harness_contracts::{PermissionMode, ToolProfile};

    use crate::storage_layout::{JyowoHome, StorageLayout};

    use super::*;

    fn temp_execution_settings_home() -> tempfile::TempDir {
        let base = std::env::current_dir()
            .expect("current dir")
            .join("target")
            .join("execution-settings-tests");
        std::fs::create_dir_all(&base).expect("test home base");
        tempfile::Builder::new()
            .prefix("home-")
            .tempdir_in(base)
            .expect("home tempdir")
    }

    #[test]
    fn global_only_execution_settings_can_save_available_runtime_scope_subagents() {
        let home = temp_execution_settings_home();
        let layout = StorageLayout::new(JyowoHome::new(home.path().join(".jyowo")));
        let store = DesktopExecutionSettingsStore::global_only_with_layout(layout.clone());
        let runtime_root = layout.global_runtime_root().join("global-conversations");
        jyowo_harness_sdk::AgentRuntimeStore::open_runtime_dir(&runtime_root)
            .expect("runtime-scope agent store");
        let context = AgentCapabilityResolutionContext {
            stream_permission_runtime_available: true,
        };

        let response = set_execution_settings_with_store(
            SetExecutionSettingsRequest {
                permission_mode: PermissionMode::Default,
                tool_profile: ToolProfile::Minimal,
                context_compression_trigger_ratio: 0.8,
                subagents_enabled: true,
                agent_teams_enabled: false,
                background_agents_enabled: false,
            },
            &store,
            Some(&context),
        )
        .expect("runtime-scope subagents should be saveable when available");

        assert!(response.agent_capabilities.subagents_enabled);
        assert!(response.agent_capabilities.subagents_available);
        assert!(store.load_record().expect("load record").subagents_enabled);
    }
}

#[cfg(test)]
mod migration_tests {
    use harness_contracts::{
        ModelProtocol, ProviderProfileConversationCapability, ProviderProfileDefinition,
        ProviderProfileModelDescriptor, ProviderProfileModelLifecycle,
    };

    use crate::commands::contracts::{
        ConversationModelCapabilityRecord, ProviderConfigRecord, ProviderModelDescriptorRecord,
        ProviderModelLifecycleRecord, ProviderModelModalityRecord, ProviderSettingsRecord,
    };
    use crate::commands::providers::{resolve_migrated_profile_id, workspace_hash_short};
    use crate::commands::stores::GlobalConfigStore;
    use crate::storage_layout::{JyowoHome, StorageLayout};

    use super::{
        commit_provider_migration_staging, migrate_provider_settings_to_split_layout,
        quarantine_migrated_provider_settings, recover_provider_migration_artifacts,
        restore_quarantined_provider_settings, ProviderMigrationStagingPaths,
    };

    fn make_descriptor() -> ProviderModelDescriptorRecord {
        ProviderModelDescriptorRecord {
            protocol: ModelProtocol::ChatCompletions,
            context_window: 128000,
            display_name: "GPT-5".to_owned(),
            lifecycle: ProviderModelLifecycleRecord::Stable,
            max_output_tokens: 16384,
            model_id: "gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            conversation_capability: ConversationModelCapabilityRecord {
                input_modalities: vec![ProviderModelModalityRecord::Text],
                output_modalities: vec![ProviderModelModalityRecord::Text],
                context_window: 128000,
                max_output_tokens: 16384,
                streaming: true,
                tool_calling: true,
                reasoning: true,
                prompt_cache: false,
                structured_output: true,
            },
        }
    }

    #[test]
    fn resolve_migrated_profile_id_uses_existing_id_when_no_collision() {
        let (id, is_new) = resolve_migrated_profile_id(
            &ProviderConfigRecord {
                api_key: "sk-test".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                display_name: "GPT-5".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                model_descriptor: make_descriptor(),
            },
            &[],
            "abc12345",
        );
        assert_eq!(id, "openai");
        assert!(is_new);
    }

    #[test]
    fn resolve_migrated_profile_id_mints_new_id_on_collision_with_different_provider() {
        let existing = ProviderProfileDefinition {
            id: "openai".to_owned(),
            display_name: "GPT-4".to_owned(),
            provider_id: "openai".to_owned(),
            model_id: "gpt-4".to_owned(),
            protocol: ModelProtocol::ChatCompletions,
            base_url: None,
            model_descriptor: ProviderProfileModelDescriptor {
                protocol: ModelProtocol::ChatCompletions,
                context_window: 8192,
                display_name: "GPT-4".to_owned(),
                lifecycle: ProviderProfileModelLifecycle::Stable,
                max_output_tokens: 4096,
                model_id: "gpt-4".to_owned(),
                provider_id: "openai".to_owned(),
                conversation_capability: ProviderProfileConversationCapability {
                    input_modalities: vec!["text".to_owned()],
                    output_modalities: vec!["text".to_owned()],
                    context_window: 8192,
                    max_output_tokens: 4096,
                    streaming: true,
                    tool_calling: true,
                    reasoning: false,
                    prompt_cache: false,
                    structured_output: false,
                },
            },
        };

        let (id, is_new) = resolve_migrated_profile_id(
            &ProviderConfigRecord {
                api_key: "sk-test".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                display_name: "GPT-5".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                model_descriptor: make_descriptor(),
            },
            &[existing],
            "abc12345",
        );
        assert_eq!(id, "openai-abc12345");
        assert!(is_new);
    }

    #[test]
    fn migration_skips_when_old_file_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = temp.path().canonicalize().expect("canonical");
        let home_root = temp_root.join(".jyowo");
        let layout = StorageLayout::new(JyowoHome::new(&home_root));
        let workspace = temp_root.join("workspace");
        std::fs::create_dir_all(&workspace).expect("create workspace");

        let mut state = crate::commands::stores::DesktopRuntimeState::with_workspace_for_test(
            workspace.clone(),
        )
        .expect("create state");
        state.global_config_store = Some(GlobalConfigStore::new(layout.clone()));
        state.project_config_store = Some(crate::commands::stores::ProjectConfigStore::new(
            layout, workspace,
        ));

        let result = migrate_provider_settings_to_split_layout(&state);
        assert!(result.is_ok());
    }

    #[test]
    fn migration_splits_old_provider_settings_record() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = temp.path().canonicalize().expect("canonical");
        let home_root = temp_root.join(".jyowo");
        let layout = StorageLayout::new(JyowoHome::new(&home_root));
        let workspace = temp_root.join("workspace");
        std::fs::create_dir_all(&workspace).expect("create workspace");

        // Write old-style provider-settings.json
        let runtime_dir = workspace.join(".jyowo").join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        let old_record = ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "sk-migrate-me".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                display_name: "GPT-5".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                model_descriptor: make_descriptor(),
            }],
        };
        let old_path = runtime_dir.join("provider-settings.json");
        std::fs::write(
            &old_path,
            serde_json::to_vec_pretty(&old_record).expect("serialize"),
        )
        .expect("write old file");

        let mut state = crate::commands::stores::DesktopRuntimeState::with_workspace_for_test(
            workspace.clone(),
        )
        .expect("create state");
        state.global_config_store = Some(GlobalConfigStore::new(layout.clone()));
        state.project_config_store = Some(crate::commands::stores::ProjectConfigStore::new(
            layout.clone(),
            workspace.clone(),
        ));

        migrate_provider_settings_to_split_layout(&state).expect("migrate");

        // Old file should be removed.
        assert!(!old_path.exists());

        // Global profiles should have the profile.
        let profiles = state
            .global_config_store
            .as_ref()
            .unwrap()
            .load_provider_profiles()
            .expect("load profiles");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "openai");

        // Global secrets should have the key.
        let secret = state
            .global_config_store
            .as_ref()
            .unwrap()
            .load_provider_secret("openai")
            .expect("load secret")
            .expect("secret present");
        assert_eq!(secret.api_key, "sk-migrate-me");

        // Project selection should map to migrated profile id.
        let selection = state
            .project_config_store
            .as_ref()
            .unwrap()
            .load_project_provider_selection()
            .expect("load selection");
        assert_eq!(selection.default_config_id.as_deref(), Some("openai"));
    }

    #[test]
    fn migration_fails_closed_on_invalid_old_provider_settings_json() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = temp.path().canonicalize().expect("canonical");
        let home_root = temp_root.join(".jyowo");
        let layout = StorageLayout::new(JyowoHome::new(&home_root));
        let workspace = temp_root.join("workspace");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        let runtime_dir = workspace.join(".jyowo").join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        let old_path = runtime_dir.join("provider-settings.json");
        std::fs::write(&old_path, b"{not-json").expect("write invalid old file");

        let mut state = crate::commands::stores::DesktopRuntimeState::with_workspace_for_test(
            workspace.clone(),
        )
        .expect("create state");
        state.global_config_store = Some(GlobalConfigStore::new(layout.clone()));
        state.project_config_store = Some(crate::commands::stores::ProjectConfigStore::new(
            layout, workspace,
        ));

        let error = migrate_provider_settings_to_split_layout(&state)
            .expect_err("invalid old provider settings must fail closed");

        assert_eq!(error.code, "INVALID_PAYLOAD");
        assert!(old_path.exists());
        assert!(!home_root
            .join("config")
            .join("provider-profiles.json")
            .exists());
    }

    #[test]
    fn migration_commit_rolls_back_when_later_target_rename_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("config");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        let provider_profiles_target = config_dir.join("provider-profiles.json");
        let provider_secrets_target = config_dir.join("provider-secrets.json");
        let project_selection_target = temp.path().join("project-provider-selection.json");
        std::fs::write(
            config_dir.join("provider-profiles.json.migration-staging-testhash"),
            b"[]",
        )
        .expect("profiles staging");
        std::fs::write(
            temp.path()
                .join("project-provider-selection.json.migration-staging-testhash"),
            br#"{"defaultConfigId":null}"#,
        )
        .expect("selection staging");

        let staging = ProviderMigrationStagingPaths {
            provider_profiles: config_dir.join("provider-profiles.json.migration-staging-testhash"),
            provider_profiles_target: provider_profiles_target.clone(),
            provider_secrets: config_dir.join("provider-secrets.json.migration-staging-testhash"),
            provider_secrets_target: provider_secrets_target.clone(),
            project_selection: temp
                .path()
                .join("project-provider-selection.json.migration-staging-testhash"),
            project_selection_target: project_selection_target.clone(),
            workspace_hash8: "testhash".to_owned(),
        };

        let error = commit_provider_migration_staging(&staging)
            .expect_err("second rename failure must fail migration");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(!provider_profiles_target.exists());
        assert!(!provider_secrets_target.exists());
        assert!(!project_selection_target.exists());
    }

    #[test]
    fn migration_commit_restores_prior_backups_when_later_backup_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("config");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        let provider_profiles_target = config_dir.join("provider-profiles.json");
        let provider_secrets_target = config_dir.join("provider-secrets.json");
        let project_selection_target = temp.path().join("project-provider-selection.json");
        std::fs::write(&provider_profiles_target, br#"[{"id":"existing"}]"#)
            .expect("profiles target");
        std::fs::write(&provider_secrets_target, br#"[{"configId":"existing"}]"#)
            .expect("secrets target");
        std::fs::create_dir(config_dir.join("provider-secrets.json.migration-backup-testhash"))
            .expect("block secrets backup");

        let staging = ProviderMigrationStagingPaths {
            provider_profiles: config_dir.join("provider-profiles.json.migration-staging-testhash"),
            provider_profiles_target: provider_profiles_target.clone(),
            provider_secrets: config_dir.join("provider-secrets.json.migration-staging-testhash"),
            provider_secrets_target: provider_secrets_target.clone(),
            project_selection: temp
                .path()
                .join("project-provider-selection.json.migration-staging-testhash"),
            project_selection_target,
            workspace_hash8: "testhash".to_owned(),
        };

        let error =
            commit_provider_migration_staging(&staging).expect_err("backup failure must fail");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert_eq!(
            std::fs::read(&provider_profiles_target).expect("profiles restored"),
            br#"[{"id":"existing"}]"#
        );
        assert_eq!(
            std::fs::read(&provider_secrets_target).expect("secrets unchanged"),
            br#"[{"configId":"existing"}]"#
        );
        assert!(!config_dir
            .join("provider-profiles.json.migration-backup-testhash")
            .exists());
    }

    #[test]
    fn migration_restore_quarantined_old_file_when_commit_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().canonicalize().expect("canonical temp");
        let old_path = root.join("provider-settings.json");
        std::fs::write(&old_path, b"old").expect("write old");

        let quarantine_path = quarantine_migrated_provider_settings(&old_path, "testhash")
            .expect("quarantine")
            .expect("quarantine path");
        assert!(!old_path.exists());
        assert!(quarantine_path.exists());

        restore_quarantined_provider_settings(&old_path, Some(&quarantine_path))
            .expect("restore quarantine");

        assert!(old_path.exists());
        assert!(!quarantine_path.exists());
        assert_eq!(std::fs::read(&old_path).expect("read old"), b"old");
    }

    #[test]
    fn migration_recovery_rolls_back_partial_commit_and_restores_old_file() {
        let local_temp_root = std::env::current_dir()
            .expect("current dir")
            .join("target")
            .join("provider-migration-tests");
        std::fs::create_dir_all(&local_temp_root).expect("local temp root");
        let temp = tempfile::Builder::new()
            .prefix("recovery-")
            .tempdir_in(local_temp_root)
            .expect("tempdir");
        let config_dir = temp.path().join("config");
        let project_dir = temp.path().join("project");
        let runtime_dir = temp.path().join("runtime");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        std::fs::create_dir_all(&project_dir).expect("project dir");
        std::fs::create_dir_all(&runtime_dir).expect("runtime dir");

        let provider_profiles_target = config_dir.join("provider-profiles.json");
        let provider_secrets_target = config_dir.join("provider-secrets.json");
        let project_selection_target = project_dir.join("project-provider-selection.json");
        let old_path = runtime_dir.join("provider-settings.json");
        let quarantine_path = runtime_dir.join("provider-settings.json.migrated-testhash");
        std::fs::write(&provider_profiles_target, b"new profiles").expect("partial target");
        std::fs::write(
            config_dir.join("provider-secrets.json.migration-staging-testhash"),
            b"new secrets",
        )
        .expect("secrets staging");
        std::fs::write(
            project_dir.join("project-provider-selection.json.migration-staging-testhash"),
            b"new selection",
        )
        .expect("selection staging");
        std::fs::write(&quarantine_path, b"old settings").expect("quarantined old");

        let staging = ProviderMigrationStagingPaths {
            provider_profiles: config_dir.join("provider-profiles.json.migration-staging-testhash"),
            provider_profiles_target: provider_profiles_target.clone(),
            provider_secrets: config_dir.join("provider-secrets.json.migration-staging-testhash"),
            provider_secrets_target: provider_secrets_target.clone(),
            project_selection: project_dir
                .join("project-provider-selection.json.migration-staging-testhash"),
            project_selection_target: project_selection_target.clone(),
            workspace_hash8: "testhash".to_owned(),
        };

        recover_provider_migration_artifacts(&staging, &old_path, Some(&quarantine_path))
            .expect("recover partial migration");

        assert_eq!(
            std::fs::read(&old_path).expect("old restored"),
            b"old settings"
        );
        assert!(!quarantine_path.exists());
        assert!(!provider_profiles_target.exists());
        assert!(!provider_secrets_target.exists());
        assert!(!project_selection_target.exists());
        assert!(!staging.provider_profiles.exists());
        assert!(!staging.provider_secrets.exists());
        assert!(!staging.project_selection.exists());
    }

    #[test]
    #[cfg(unix)]
    fn migration_rejects_symlink_target_during_commit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("config");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        let provider_profiles_target = config_dir.join("provider-profiles.json");
        let provider_secrets_target = config_dir.join("provider-secrets.json");
        let project_selection_target = temp.path().join("project-provider-selection.json");
        let symlink_target = config_dir.join("external-provider-profiles.json");
        std::fs::write(&symlink_target, br#"[{"id":"external"}]"#).expect("external target");
        std::os::unix::fs::symlink(&symlink_target, &provider_profiles_target)
            .expect("profiles symlink");
        std::fs::write(
            config_dir.join("provider-profiles.json.migration-staging-testhash"),
            b"[]",
        )
        .expect("profiles staging");
        std::fs::write(
            config_dir.join("provider-secrets.json.migration-staging-testhash"),
            b"[]",
        )
        .expect("secrets staging");
        std::fs::write(
            temp.path()
                .join("project-provider-selection.json.migration-staging-testhash"),
            br#"{"defaultConfigId":null}"#,
        )
        .expect("selection staging");

        let staging = ProviderMigrationStagingPaths {
            provider_profiles: config_dir.join("provider-profiles.json.migration-staging-testhash"),
            provider_profiles_target: provider_profiles_target.clone(),
            provider_secrets: config_dir.join("provider-secrets.json.migration-staging-testhash"),
            provider_secrets_target,
            project_selection: temp
                .path()
                .join("project-provider-selection.json.migration-staging-testhash"),
            project_selection_target,
            workspace_hash8: "testhash".to_owned(),
        };

        let error =
            commit_provider_migration_staging(&staging).expect_err("symlink target must fail");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(
            std::fs::symlink_metadata(&provider_profiles_target)
                .expect("profiles symlink metadata")
                .file_type()
                .is_symlink(),
            "symlink target must not be replaced"
        );
        assert_eq!(
            std::fs::read(&symlink_target).expect("external target unchanged"),
            br#"[{"id":"external"}]"#
        );
    }

    #[test]
    #[cfg(unix)]
    fn migration_rejects_symlink_old_file_during_quarantine() {
        let temp = tempfile::tempdir().expect("tempdir");
        let old_path = temp.path().join("provider-settings.json");
        let symlink_target = temp.path().join("external-provider-settings.json");
        std::fs::write(&symlink_target, b"old").expect("external old target");
        std::os::unix::fs::symlink(&symlink_target, &old_path).expect("old symlink");

        let error = quarantine_migrated_provider_settings(&old_path, "testhash")
            .expect_err("symlink old file must fail closed");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(
            std::fs::symlink_metadata(&old_path)
                .expect("old symlink metadata")
                .file_type()
                .is_symlink(),
            "old symlink must not be renamed"
        );
    }

    #[test]
    fn migration_mints_profile_id_when_matching_profile_has_different_secret() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = temp.path().canonicalize().expect("canonical");
        let home_root = temp_root.join(".jyowo");
        let layout = StorageLayout::new(JyowoHome::new(&home_root));
        let workspace = temp_root.join("workspace");
        std::fs::create_dir_all(&workspace).expect("create workspace");

        let global_config = GlobalConfigStore::new(layout.clone());
        global_config
            .save_provider_profiles(&[ProviderProfileDefinition {
                id: "openai".to_owned(),
                display_name: "GPT-5".to_owned(),
                provider_id: "openai".to_owned(),
                model_id: "gpt-5".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                model_descriptor: ProviderProfileModelDescriptor {
                    protocol: ModelProtocol::ChatCompletions,
                    context_window: 128000,
                    display_name: "GPT-5".to_owned(),
                    lifecycle: ProviderProfileModelLifecycle::Stable,
                    max_output_tokens: 16384,
                    model_id: "gpt-5".to_owned(),
                    provider_id: "openai".to_owned(),
                    conversation_capability: ProviderProfileConversationCapability {
                        input_modalities: vec!["text".to_owned()],
                        output_modalities: vec!["text".to_owned()],
                        context_window: 128000,
                        max_output_tokens: 16384,
                        streaming: true,
                        tool_calling: true,
                        reasoning: true,
                        prompt_cache: false,
                        structured_output: true,
                    },
                },
            }])
            .expect("seed profile");
        global_config
            .save_provider_secret(&harness_contracts::global_config::ProviderSecretEntry {
                config_id: "openai".to_owned(),
                api_key: "sk-existing-project".to_owned(),
                official_quota_api_key: None,
            })
            .expect("seed secret");

        let runtime_dir = workspace.join(".jyowo").join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        let old_record = ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "sk-new-project".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                display_name: "GPT-5".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                model_descriptor: make_descriptor(),
            }],
        };
        let old_path = runtime_dir.join("provider-settings.json");
        std::fs::write(
            &old_path,
            serde_json::to_vec_pretty(&old_record).expect("serialize"),
        )
        .expect("write old file");

        let mut state = crate::commands::stores::DesktopRuntimeState::with_workspace_for_test(
            workspace.clone(),
        )
        .expect("create state");
        state.global_config_store = Some(global_config);
        state.project_config_store = Some(crate::commands::stores::ProjectConfigStore::new(
            layout,
            workspace.clone(),
        ));

        migrate_provider_settings_to_split_layout(&state).expect("migrate");

        let original_secret = state
            .global_config_store
            .as_ref()
            .unwrap()
            .load_provider_secret("openai")
            .expect("load original secret")
            .expect("original secret present");
        assert_eq!(original_secret.api_key, "sk-existing-project");

        let migrated_selection = state
            .project_config_store
            .as_ref()
            .unwrap()
            .load_project_provider_selection()
            .expect("load selection")
            .default_config_id
            .expect("selection present");
        assert_ne!(migrated_selection, "openai");
        let migrated_secret = state
            .global_config_store
            .as_ref()
            .unwrap()
            .load_provider_secret(&migrated_selection)
            .expect("load migrated secret")
            .expect("migrated secret present");
        assert_eq!(migrated_secret.api_key, "sk-new-project");
    }

    #[test]
    fn migration_does_not_overwrite_existing_minted_profile_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = temp.path().canonicalize().expect("canonical");
        let home_root = temp_root.join(".jyowo");
        let layout = StorageLayout::new(JyowoHome::new(&home_root));
        let workspace = temp_root.join("workspace");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        let workspace_hash8 = workspace_hash_short(&workspace);
        let minted_collision_id = format!("openai-{workspace_hash8}");

        let global_config = GlobalConfigStore::new(layout.clone());
        global_config
            .save_provider_profiles(&[
                ProviderProfileDefinition {
                    id: "openai".to_owned(),
                    display_name: "GPT-5".to_owned(),
                    provider_id: "openai".to_owned(),
                    model_id: "gpt-5".to_owned(),
                    protocol: ModelProtocol::ChatCompletions,
                    base_url: None,
                    model_descriptor: ProviderProfileModelDescriptor {
                        protocol: ModelProtocol::ChatCompletions,
                        context_window: 128000,
                        display_name: "GPT-5".to_owned(),
                        lifecycle: ProviderProfileModelLifecycle::Stable,
                        max_output_tokens: 16384,
                        model_id: "gpt-5".to_owned(),
                        provider_id: "openai".to_owned(),
                        conversation_capability: ProviderProfileConversationCapability {
                            input_modalities: vec!["text".to_owned()],
                            output_modalities: vec!["text".to_owned()],
                            context_window: 128000,
                            max_output_tokens: 16384,
                            streaming: true,
                            tool_calling: true,
                            reasoning: true,
                            prompt_cache: false,
                            structured_output: true,
                        },
                    },
                },
                ProviderProfileDefinition {
                    id: minted_collision_id.clone(),
                    display_name: "Existing collision".to_owned(),
                    provider_id: "openai".to_owned(),
                    model_id: "gpt-4.1".to_owned(),
                    protocol: ModelProtocol::ChatCompletions,
                    base_url: None,
                    model_descriptor: ProviderProfileModelDescriptor {
                        protocol: ModelProtocol::ChatCompletions,
                        context_window: 128000,
                        display_name: "Existing collision".to_owned(),
                        lifecycle: ProviderProfileModelLifecycle::Stable,
                        max_output_tokens: 16384,
                        model_id: "gpt-4.1".to_owned(),
                        provider_id: "openai".to_owned(),
                        conversation_capability: ProviderProfileConversationCapability {
                            input_modalities: vec!["text".to_owned()],
                            output_modalities: vec!["text".to_owned()],
                            context_window: 128000,
                            max_output_tokens: 16384,
                            streaming: true,
                            tool_calling: true,
                            reasoning: false,
                            prompt_cache: false,
                            structured_output: true,
                        },
                    },
                },
            ])
            .expect("seed profiles");
        global_config
            .save_provider_secret(&harness_contracts::global_config::ProviderSecretEntry {
                config_id: "openai".to_owned(),
                api_key: "sk-existing-project".to_owned(),
                official_quota_api_key: None,
            })
            .expect("seed original secret");
        global_config
            .save_provider_secret(&harness_contracts::global_config::ProviderSecretEntry {
                config_id: minted_collision_id.clone(),
                api_key: "sk-existing-minted".to_owned(),
                official_quota_api_key: None,
            })
            .expect("seed minted secret");

        let runtime_dir = workspace.join(".jyowo").join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        let old_record = ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "sk-new-project".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                display_name: "GPT-5".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                model_descriptor: make_descriptor(),
            }],
        };
        std::fs::write(
            runtime_dir.join("provider-settings.json"),
            serde_json::to_vec_pretty(&old_record).expect("serialize"),
        )
        .expect("write old file");

        let mut state = crate::commands::stores::DesktopRuntimeState::with_workspace_for_test(
            workspace.clone(),
        )
        .expect("create state");
        state.global_config_store = Some(global_config);
        state.project_config_store = Some(crate::commands::stores::ProjectConfigStore::new(
            layout, workspace,
        ));

        migrate_provider_settings_to_split_layout(&state).expect("migrate");

        let existing_minted_secret = state
            .global_config_store
            .as_ref()
            .unwrap()
            .load_provider_secret(&minted_collision_id)
            .expect("load existing minted secret")
            .expect("existing minted secret present");
        assert_eq!(existing_minted_secret.api_key, "sk-existing-minted");

        let migrated_selection = state
            .project_config_store
            .as_ref()
            .unwrap()
            .load_project_provider_selection()
            .expect("load selection")
            .default_config_id
            .expect("selection present");
        assert_ne!(migrated_selection, "openai");
        assert_ne!(migrated_selection, minted_collision_id);
        let migrated_secret = state
            .global_config_store
            .as_ref()
            .unwrap()
            .load_provider_secret(&migrated_selection)
            .expect("load migrated secret")
            .expect("migrated secret present");
        assert_eq!(migrated_secret.api_key, "sk-new-project");
    }
}
