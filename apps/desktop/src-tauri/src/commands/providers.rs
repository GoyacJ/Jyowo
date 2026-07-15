#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
use super::contracts::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
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
use harness_contracts::{ProviderSecretEntry, ProviderSelectionRecord};
use harness_model::{
    provider_requires_api_key, CacheProtocolSemantics, MediaProtocolSemantics,
    OutputProtocolSemantics, ProviderAuthScheme, ReasoningProtocolSemantics,
    StreamingProtocolSemantics, ToolProtocolSemantics,
};
use harness_provider_state::ProviderContinuationKind;

static PROVIDER_SETTINGS_PROCESS_LOCK: RwLock<()> = RwLock::new(());

#[doc(hidden)]
pub fn provider_settings_process_lock_for_test() -> &'static RwLock<()> {
    &PROVIDER_SETTINGS_PROCESS_LOCK
}

#[derive(Clone)]
pub(crate) struct DesktopProviderCredentialResolver {
    provider_settings_store: Arc<dyn ProviderSettingsStore>,
    provider_capability_routes: Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
}

impl DesktopProviderCredentialResolver {
    pub(crate) fn new(
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
    provider_settings_store: Arc<dyn ProviderSettingsStore>,
    provider_capability_routes: Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
) -> Arc<dyn ProviderCredentialResolverCap> {
    Arc::new(DesktopProviderCredentialResolver::new(
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
        .settings_runtime()
        .map(|settings_runtime| {
            provider_service_adapter_availability_from_snapshot(
                &settings_runtime.tool_registry().snapshot(),
            )
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
    if let Some(settings_runtime) = runtime_state.settings_runtime() {
        *settings_runtime.provider_capability_routes().write() = routes.clone();
    }
}

#[derive(Clone)]
pub struct DesktopProviderSettingsStore {
    layout: crate::storage_layout::StorageLayout,
}

impl DesktopProviderSettingsStore {
    pub fn new(_workspace_root: PathBuf) -> Self {
        let home = execution_settings_home_dir();
        Self::global_only_with_layout(crate::storage_layout::StorageLayout::new(
            crate::storage_layout::JyowoHome::new(home),
        ))
    }

    pub fn new_with_layout(
        layout: crate::storage_layout::StorageLayout,
        _workspace_root: PathBuf,
    ) -> Self {
        Self::global_only_with_layout(layout)
    }

    pub fn global_only() -> Self {
        let home = execution_settings_home_dir();
        Self::global_only_with_layout(crate::storage_layout::StorageLayout::new(
            crate::storage_layout::JyowoHome::new(home),
        ))
    }

    pub fn global_only_with_layout(layout: crate::storage_layout::StorageLayout) -> Self {
        Self { layout }
    }

    pub fn from_runtime_layout(_layout: &crate::storage_layout::RuntimeLayout) -> Self {
        Self::global_only()
    }

    pub fn from_runtime_layout_with_layout(
        storage_layout: crate::storage_layout::StorageLayout,
        _runtime_layout: &crate::storage_layout::RuntimeLayout,
    ) -> Self {
        Self::global_only_with_layout(storage_layout)
    }

    fn global_config_store(&self) -> GlobalConfigStore {
        GlobalConfigStore::new(self.layout.clone())
    }
}

impl ProviderSettingsStore for DesktopProviderSettingsStore {
    fn selection_scope(&self) -> SettingsScope {
        SettingsScope::Global
    }

    fn load_record(&self) -> Result<Option<ProviderSettingsRecord>, CommandErrorPayload> {
        let _process_guard = PROVIDER_SETTINGS_PROCESS_LOCK.read().map_err(|_| {
            runtime_operation_failed("provider settings process lock is poisoned".to_owned())
        })?;
        let global_config = self.global_config_store();
        let _generation_guard = global_config.lock_provider_generation_shared()?;
        load_provider_settings_record(&global_config)
    }

    fn save_record(&self, record: &ProviderSettingsRecord) -> Result<(), CommandErrorPayload> {
        let _process_guard = PROVIDER_SETTINGS_PROCESS_LOCK.write().map_err(|_| {
            runtime_operation_failed("provider settings process lock is poisoned".to_owned())
        })?;
        ensure_provider_settings_record(record)?;
        let global_config = self.global_config_store();
        save_provider_settings_record(&global_config, record)
    }

    fn compare_and_swap_record(
        &self,
        expected: Option<&ProviderSettingsRecord>,
        record: &ProviderSettingsRecord,
    ) -> Result<ProviderSettingsSaveOutcome, CommandErrorPayload> {
        let _process_guard = PROVIDER_SETTINGS_PROCESS_LOCK.write().map_err(|_| {
            runtime_operation_failed("provider settings process lock is poisoned".to_owned())
        })?;
        ensure_provider_settings_record(record)?;
        let global_config = self.global_config_store();
        let _generation_guard = global_config.lock_provider_generation_exclusive()?;
        if load_provider_settings_record(&global_config)?.as_ref() != expected {
            return Ok(ProviderSettingsSaveOutcome::Conflict);
        }
        save_provider_settings_record_locked(&global_config, record)?;
        Ok(ProviderSettingsSaveOutcome::Saved)
    }
}

fn load_provider_settings_record(
    global_config: &GlobalConfigStore,
) -> Result<Option<ProviderSettingsRecord>, CommandErrorPayload> {
    let profiles = global_config.load_provider_profiles()?;
    if profiles.is_empty() {
        return Ok(None);
    }
    let selection = global_config.load_global_provider_selection()?;
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

fn save_provider_settings_record(
    global_config: &GlobalConfigStore,
    record: &ProviderSettingsRecord,
) -> Result<(), CommandErrorPayload> {
    let (profiles, secrets, selection) = provider_generation_from_record(record);
    global_config.save_provider_generation(&profiles, &secrets, &selection)
}

fn save_provider_settings_record_locked(
    global_config: &GlobalConfigStore,
    record: &ProviderSettingsRecord,
) -> Result<(), CommandErrorPayload> {
    let (profiles, secrets, selection) = provider_generation_from_record(record);
    global_config.save_provider_generation_locked(&profiles, &secrets, &selection)
}

fn provider_generation_from_record(
    record: &ProviderSettingsRecord,
) -> (
    Vec<ProviderProfileDefinition>,
    Vec<ProviderSecretEntry>,
    ProviderSelectionRecord,
) {
    let profiles = record
        .configs
        .iter()
        .map(|config| provider_profile_definition_from_config(config, config.id.clone()))
        .collect();
    let secrets = record
        .configs
        .iter()
        .map(|config| ProviderSecretEntry {
            config_id: config.id.clone(),
            api_key: config.api_key.clone(),
            official_quota_api_key: config.official_quota_api_key.clone(),
        })
        .collect();
    let selection = ProviderSelectionRecord {
        default_config_id: record.default_config_id.clone(),
    };
    (profiles, secrets, selection)
}

fn provider_config_record_from_profile(
    profile: ProviderProfileDefinition,
    secret: Option<&ProviderSecretEntry>,
) -> Result<ProviderConfigRecord, CommandErrorPayload> {
    let profile = migrate_provider_profile(profile);
    let provider_defaults = profile
        .provider_defaults
        .map(provider_defaults_record_from_profile);
    validate_provider_defaults(profile.provider_id.as_str(), provider_defaults.as_ref())?;
    validate_provider_defaults_for_protocol(
        profile.provider_id.as_str(),
        profile.protocol,
        provider_defaults.as_ref(),
    )?;
    let base_url = normalized_provider_base_url(&profile.provider_id, profile.base_url.as_deref())?;
    validate_provider_protocol_base_url(
        &profile.provider_id,
        profile.protocol,
        base_url.as_deref(),
    )?;
    validate_provider_profile_defaults_for_model(
        profile.provider_id.as_str(),
        profile.model_id.as_str(),
        provider_defaults.as_ref(),
    )?;
    Ok(ProviderConfigRecord {
        api_key: secret
            .map(|entry| entry.api_key.clone())
            .unwrap_or_default(),
        protocol: profile.protocol,
        base_url,
        display_name: profile.display_name,
        id: profile.id,
        model_id: profile.model_id,
        model_options: profile.model_options,
        official_quota_api_key: secret.and_then(|entry| entry.official_quota_api_key.clone()),
        provider_id: profile.provider_id,
        provider_defaults,
        model_descriptor: provider_model_descriptor_record_from_profile(profile.model_descriptor)?,
    })
}

fn validate_provider_profile_defaults_for_model(
    provider_id: &str,
    model_id: &str,
    defaults: Option<&ProviderDefaultsRecord>,
) -> Result<(), CommandErrorPayload> {
    if provider_id != "doubao" {
        return Ok(());
    }
    let descriptor =
        resolve_model_descriptor(provider_id, model_id).map_err(provider_registry_error)?;
    validate_provider_defaults_for_descriptor(defaults, &descriptor)
}

fn migrate_provider_profile(mut profile: ProviderProfileDefinition) -> ProviderProfileDefinition {
    if profile.provider_id == "qwen" && profile.model_id == "qwen3.7-max-thinking" {
        profile.model_id = "qwen3.7-max".to_owned();
        profile.model_descriptor.model_id = "qwen3.7-max".to_owned();
        profile.model_descriptor.display_name = "Qwen3.7 Max".to_owned();
        let mut defaults = profile.provider_defaults.unwrap_or_default();
        let mut body = defaults
            .body
            .and_then(|body| body.as_object().cloned())
            .unwrap_or_default();
        body.insert("enable_thinking".to_owned(), Value::Bool(true));
        defaults.body = Some(Value::Object(body));
        profile.provider_defaults = Some(defaults);
    }
    profile
}

fn provider_defaults_record_from_profile(
    defaults: harness_contracts::ProviderProfileDefaults,
) -> ProviderDefaultsRecord {
    ProviderDefaultsRecord {
        body: defaults.body,
        headers: defaults.headers,
    }
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
        runtime_semantics: descriptor.runtime_semantics,
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
        harness_contracts::ProviderProfileModelLifecycle::Retiring { retirement_date } => {
            ProviderModelLifecycleRecord::Retiring { retirement_date }
        }
    }
}

#[derive(Clone)]
pub struct DesktopProviderCapabilityRouteStore {
    layout: crate::storage_layout::StorageLayout,
    workspace_root: Option<PathBuf>,
}

impl DesktopProviderCapabilityRouteStore {
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
            workspace_root: Some(workspace_root),
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
            workspace_root: None,
        }
    }

    #[must_use]
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    fn settings_path(&self) -> PathBuf {
        match &self.workspace_root {
            Some(workspace_root) => self.layout.project_provider_routes_file(workspace_root),
            None => self.layout.global_provider_routes_file(),
        }
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

    pub fn from_runtime_layout(_layout: &crate::storage_layout::RuntimeLayout) -> Self {
        Self::global_only()
    }

    pub fn from_runtime_layout_with_layout(
        storage_layout: crate::storage_layout::StorageLayout,
        _runtime_layout: &crate::storage_layout::RuntimeLayout,
    ) -> Self {
        Self::global_only_with_layout(storage_layout)
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
        capabilities: Option<&harness_contracts::AgentCapabilities>,
    ) -> Result<(), CommandErrorPayload> {
        if self.is_global_only() {
            ensure_no_workspace_execution_defaults_record(
                record,
                self.workspace_root(),
                capabilities,
            )?;
            let settings_path = self.settings_path();
            return write_json_file_atomic(&settings_path, "execution settings", record);
        } else {
            ensure_execution_defaults_record(record, self.workspace_root(), capabilities)?;
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
    harness_contracts::validate_execution_defaults_dependencies(record)
        .map_err(|error| invalid_payload(error.to_string()))?;
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
    _workspace_root: &Path,
    capabilities: Option<&harness_contracts::AgentCapabilities>,
) -> Result<(), CommandErrorPayload> {
    ensure_execution_defaults_structure(record)?;

    let policy = agent_capabilities_payload(record, capabilities);
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
    _runtime_root: &Path,
    capabilities: Option<&harness_contracts::AgentCapabilities>,
) -> Result<(), CommandErrorPayload> {
    ensure_execution_defaults_structure(record)?;
    let payload = agent_capabilities_payload(record, capabilities);
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

pub(crate) fn agent_capabilities_payload(
    record: &harness_contracts::ExecutionDefaultsRecord,
    capabilities: Option<&harness_contracts::AgentCapabilities>,
) -> AgentCapabilitiesPayload {
    let supported = |capability| {
        capabilities.is_some_and(|value| match capability {
            AgentCapabilityKind::Subagents => value.subagents,
            AgentCapabilityKind::AgentTeams => value.agent_teams,
            AgentCapabilityKind::BackgroundAgents => value.background_agents,
        })
    };
    let unavailable_reasons = [
        AgentCapabilityKind::Subagents,
        AgentCapabilityKind::AgentTeams,
        AgentCapabilityKind::BackgroundAgents,
    ]
    .into_iter()
    .filter(|capability| !supported(*capability))
    .map(
        |capability| AgentCapabilityUnavailableReason::DaemonUnavailable {
            capability,
            message: if capabilities.is_some() {
                "connected task daemon does not support this capability".to_owned()
            } else {
                "task daemon is unavailable or incompatible".to_owned()
            },
        },
    )
    .collect();
    AgentCapabilitiesPayload {
        subagents_enabled: record.subagents_enabled,
        agent_teams_enabled: record.agent_teams_enabled,
        background_agents_enabled: record.background_agents_enabled,
        subagents_available: capabilities.is_some_and(|value| value.subagents),
        agent_teams_available: capabilities.is_some_and(|value| value.agent_teams),
        background_agents_available: capabilities.is_some_and(|value| value.background_agents),
        unavailable_reasons,
    }
}

pub(crate) fn no_workspace_agent_capabilities_payload(
    record: &harness_contracts::ExecutionDefaultsRecord,
    _runtime_root: &Path,
    capabilities: Option<&harness_contracts::AgentCapabilities>,
) -> AgentCapabilitiesPayload {
    agent_capabilities_payload(record, capabilities)
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
    if record.configs.iter().any(|config| {
        provider_requires_api_key(&config.provider_id) && config.api_key.trim().is_empty()
    }) {
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
            .map(|entry| {
                let default_base_url = desktop_default_base_url(&entry);
                ModelProviderCatalogEntry {
                    default_base_url,
                    display_name: entry.display_name,
                    models: entry
                        .models
                        .into_iter()
                        .map(model_descriptor_catalog_entry)
                        .collect(),
                    provider_id: entry.provider_id,
                    provider_defaults: None,
                    runtime_capability: runtime_capability_payload(entry.runtime_capability),
                    service_capabilities: entry
                        .service_capabilities
                        .into_iter()
                        .map(service_capability_payload)
                        .collect(),
                    source_url: entry.source_url,
                    verified_date: entry.verified_date.to_string(),
                }
            })
            .collect(),
    }
}

fn desktop_default_base_url(entry: &jyowo_harness_sdk::ext::ProviderCatalogEntry) -> String {
    if entry.provider_id != "qwen" {
        return entry.default_base_url.clone();
    }
    let timezone_id =
        iana_time_zone::get_timezone().unwrap_or_else(|_| "America/New_York".to_owned());
    let region_id = qwen_region_id_for_timezone(&timezone_id);
    entry
        .runtime_capability
        .base_url_regions
        .iter()
        .find(|region| region.id == region_id)
        .or_else(|| {
            entry
                .runtime_capability
                .base_url_regions
                .iter()
                .find(|region| region.id == "us")
        })
        .map(|region| region.base_url.clone())
        .unwrap_or_else(|| entry.default_base_url.clone())
}

fn qwen_region_id_for_timezone(timezone_id: &str) -> &'static str {
    match timezone_id {
        "Asia/Shanghai" => "beijing",
        "Asia/Hong_Kong" | "Asia/Macau" => "hong-kong",
        "Asia/Tokyo" => "japan",
        "Asia/Singapore" | "Asia/Kuala_Lumpur" | "Asia/Jakarta" | "Asia/Bangkok"
        | "Asia/Manila" | "Asia/Ho_Chi_Minh" => "singapore",
        value if value.starts_with("Europe/") => "germany",
        value if value.starts_with("America/") => "us",
        _ => "us",
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
        ProviderServiceCategory::ThreeD => "three_d",
        ProviderServiceCategory::Embedding => "embedding",
        ProviderServiceCategory::Audio => "audio",
        ProviderServiceCategory::Music => "music",
        ProviderServiceCategory::File => "file",
        ProviderServiceCategory::Model => "model",
        ProviderServiceCategory::Moderation => "moderation",
        ProviderServiceCategory::VectorStore => "vector_store",
        ProviderServiceCategory::Batch => "batch",
        ProviderServiceCategory::FineTuning => "fine_tuning",
        ProviderServiceCategory::Eval => "eval",
        ProviderServiceCategory::Grader => "grader",
        ProviderServiceCategory::Container => "container",
        ProviderServiceCategory::Upload => "upload",
        ProviderServiceCategory::Realtime => "realtime",
        ProviderServiceCategory::Admin => "admin",
        ProviderServiceCategory::Webhook => "webhook",
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
    capabilities: Option<&harness_contracts::AgentCapabilities>,
) -> Result<GetExecutionSettingsResponse, CommandErrorPayload> {
    let record = store.load_record()?;
    execution_settings_response_from_record(
        &record,
        store.is_global_only(),
        store.workspace_root(),
        capabilities,
    )
}

fn execution_settings_response_from_record(
    record: &harness_contracts::ExecutionDefaultsRecord,
    global_only: bool,
    policy_root: &Path,
    capabilities: Option<&harness_contracts::AgentCapabilities>,
) -> Result<GetExecutionSettingsResponse, CommandErrorPayload> {
    let permission_mode = effective_execution_settings_permission_mode(record.permission_mode);
    let agent_capabilities = if global_only {
        no_workspace_agent_capabilities_payload(record, policy_root, capabilities)
    } else {
        agent_capabilities_payload(record, capabilities)
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
    capabilities: Option<&harness_contracts::AgentCapabilities>,
) -> Result<GetExecutionSettingsResponse, CommandErrorPayload> {
    let Some(workspace_path) = request.workspace_path else {
        return get_execution_settings_with_store(
            state.execution_settings_store.as_ref(),
            capabilities,
        );
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
    get_execution_settings_with_store(state.execution_settings_store.as_ref(), capabilities)
}

pub fn get_execution_settings_for_request(
    request: GetExecutionSettingsRequest,
    active_store: &DesktopExecutionSettingsStore,
    project_registry: &ProjectRegistry,
    capabilities: Option<&harness_contracts::AgentCapabilities>,
) -> Result<GetExecutionSettingsResponse, CommandErrorPayload> {
    let Some(workspace_path) = request.workspace_path else {
        return get_execution_settings_with_store(active_store, capabilities);
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
    get_execution_settings_with_store(active_store, capabilities)
}

pub fn set_execution_settings_with_store(
    request: SetExecutionSettingsRequest,
    store: &DesktopExecutionSettingsStore,
    capabilities: Option<&harness_contracts::AgentCapabilities>,
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
            capabilities,
        )?;
    } else {
        ensure_execution_defaults_record(&requested_record, store.workspace_root(), capabilities)?;
    }
    if request.permission_mode == PermissionMode::Auto && !auto_mode_available() {
        return Err(invalid_payload(
            "auto permission mode is unavailable in this desktop build".to_owned(),
        ));
    }
    let record = requested_record;
    store.save_record(&record, capabilities)?;
    let agent_capabilities = if store.is_global_only() {
        no_workspace_agent_capabilities_payload(&record, store.workspace_root(), capabilities)
    } else {
        agent_capabilities_payload(&record, capabilities)
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

/// Resolve effective execution settings by merging global defaults and optional
/// run-level overrides.
///
/// Precedence (highest wins):
/// 1. Run explicit params (`run_permission_mode`, `run_tool_profile`)
/// 2. Global execution defaults
/// 3. Contract defaults in [`harness_contracts::ExecutionDefaultsRecord::default`]
///
/// This is the single source of truth for effective execution settings.
/// Frontend code must not reimplement this overlay.
pub fn resolve_effective_execution_settings(
    global_config: Option<&crate::commands::stores::GlobalConfigStore>,
    _project_config: Option<&crate::commands::stores::ProjectConfigStore>,
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

    // 3. Apply run explicit params.
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
    list_provider_settings_for_workspace_with_store(store, None).await
}

pub async fn list_provider_settings_for_workspace_with_store(
    store: &dyn ProviderSettingsStore,
    project_store: Option<&ProjectConfigStore>,
) -> Result<ListProviderSettingsResponse, CommandErrorPayload> {
    let mut record = store.load_record()?.unwrap_or_default();
    let mut selection_scope = store.selection_scope();
    if let Some(selection) = project_store
        .map(ProjectConfigStore::load_project_provider_selection_optional)
        .transpose()?
        .flatten()
    {
        let default_config_id = selection
            .default_config_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                invalid_payload("project provider selection defaultConfigId is empty".to_owned())
            })?;
        if !record
            .configs
            .iter()
            .any(|config| config.id == default_config_id)
        {
            return Err(invalid_payload(
                "project provider selection references an unknown provider config".to_owned(),
            ));
        }
        record.default_config_id = Some(default_config_id.to_owned());
        selection_scope = SettingsScope::Project;
    }

    Ok(ListProviderSettingsResponse {
        default_config_id: record.default_config_id.clone(),
        selection_scope,
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
        super::model_settings::local_model_provider_catalog(runtime_state)?.0,
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
        "three_d" => Some(CapabilityRouteKind::ThreeDGeneration),
        "embedding" => Some(CapabilityRouteKind::EmbeddingGeneration),
        "file" => Some(CapabilityRouteKind::FileOperation),
        "music" => Some(CapabilityRouteKind::MusicGeneration),
        "moderation" => Some(CapabilityRouteKind::Moderation),
        "upload" => Some(CapabilityRouteKind::FileManagement),
        "vector_store" => Some(CapabilityRouteKind::VectorStoreManagement),
        "batch" => Some(CapabilityRouteKind::BatchJob),
        "fine_tuning" => Some(CapabilityRouteKind::FineTuningJob),
        "eval" | "grader" => Some(CapabilityRouteKind::EvalRun),
        "container" => Some(CapabilityRouteKind::ContainerSession),
        "realtime" => Some(CapabilityRouteKind::RealtimeSession),
        "admin" => Some(CapabilityRouteKind::AdminOperation),
        "webhook" => Some(CapabilityRouteKind::WebhookVerification),
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

pub(crate) struct PreparedProviderSettingsSave {
    pub(crate) expected_record: Option<ProviderSettingsRecord>,
    pub(crate) record: ProviderSettingsRecord,
    pub(crate) response: SaveProviderSettingsResponse,
}

pub(crate) async fn prepare_provider_settings_with_store(
    request: ProviderSettingsRequest,
    store: &dyn ProviderSettingsStore,
) -> Result<PreparedProviderSettingsSave, CommandErrorPayload> {
    ensure_provider_settings(&request)?;
    let base_url = normalized_provider_base_url(&request.provider_id, request.base_url.as_deref())?;
    let expected_record = store.load_record()?;
    let mut record = expected_record.clone().unwrap_or_default();
    let config_id = provider_config_id(&record, &request);
    let previous_config = record
        .configs
        .iter()
        .find(|config| config.id == config_id)
        .cloned();
    let descriptor = provider_settings_descriptor(&request, previous_config.as_ref()).await?;
    validate_provider_protocol_base_url(
        &request.provider_id,
        descriptor.protocol,
        base_url.as_deref(),
    )?;
    let api_key = request
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let provider_requires_api_key = provider_requires_api_key(&request.provider_id);
    let config_api_key = if let Some(api_key) = api_key {
        api_key.to_owned()
    } else if !provider_requires_api_key {
        previous_config
            .as_ref()
            .filter(|config| {
                config.provider_id == request.provider_id && config.base_url == base_url
            })
            .map(|config| config.api_key.clone())
            .unwrap_or_default()
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
    let provider_defaults = request.provider_defaults.clone().or_else(|| {
        previous_config
            .as_ref()
            .filter(|config| previous_config_matches_request_model(config, &request, &base_url))
            .and_then(|config| config.provider_defaults.clone())
    });
    validate_provider_defaults_for_protocol(
        &request.provider_id,
        descriptor.protocol,
        provider_defaults.as_ref(),
    )?;
    validate_provider_defaults_for_descriptor(provider_defaults.as_ref(), &descriptor)?;
    let model_options = match request.model_options.as_ref() {
        Some(model_options) => model_options.clone(),
        None if previous_config.as_ref().is_some_and(|config| {
            config.provider_id == request.provider_id && config.base_url == base_url
        }) =>
        {
            previous_config
                .as_ref()
                .map(|config| config.model_options.clone())
                .unwrap_or_default()
        }
        None => harness_contracts::ModelRequestOptions::default(),
    };
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
        model_options,
        official_quota_api_key,
        provider_id: request.provider_id.clone(),
        provider_defaults,
        model_descriptor: model_descriptor_record(&descriptor),
    };
    record.configs.retain(|existing| existing.id != config_id);
    record.configs.push(config);
    record.configs.sort_by(|left, right| left.id.cmp(&right.id));
    if request.set_default || record.default_config_id.is_none() {
        record.default_config_id = Some(config_id.clone());
    }
    let response = SaveProviderSettingsResponse {
        config: provider_config_payload(
            record
                .configs
                .iter()
                .find(|config| config.id == config_id)
                .expect("saved config should exist"),
            record.default_config_id.as_deref(),
        )?,
        status: "saved",
    };
    Ok(PreparedProviderSettingsSave {
        expected_record,
        record,
        response,
    })
}

pub async fn save_provider_settings_with_store(
    request: ProviderSettingsRequest,
    store: &dyn ProviderSettingsStore,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    for _ in 0..PROVIDER_SETTINGS_COMMIT_ATTEMPTS {
        let prepared = prepare_provider_settings_with_store(request.clone(), store).await?;
        match store.compare_and_swap_record(prepared.expected_record.as_ref(), &prepared.record)? {
            ProviderSettingsSaveOutcome::Saved => return Ok(prepared.response),
            ProviderSettingsSaveOutcome::Conflict => continue,
        }
    }
    Err(runtime_operation_failed(
        "provider settings changed concurrently; retry the save".to_owned(),
    ))
}

const PROVIDER_SETTINGS_COMMIT_ATTEMPTS: usize = 8;

fn previous_config_matches_request_model(
    config: &ProviderConfigRecord,
    request: &ProviderSettingsRequest,
    base_url: &Option<String>,
) -> bool {
    config.provider_id == request.provider_id
        && config.base_url == *base_url
        && config.model_id == request.model_id
}

struct CandidateProviderSettingsStore {
    record: ProviderSettingsRecord,
    committed_store: ParkingRwLock<Option<Arc<dyn ProviderSettingsStore>>>,
}

impl CandidateProviderSettingsStore {
    fn new(record: ProviderSettingsRecord) -> Self {
        Self {
            record,
            committed_store: ParkingRwLock::new(None),
        }
    }

    fn activate(&self, committed_store: Arc<dyn ProviderSettingsStore>) {
        *self.committed_store.write() = Some(committed_store);
    }
}

impl ProviderSettingsStore for CandidateProviderSettingsStore {
    fn load_record(&self) -> Result<Option<ProviderSettingsRecord>, CommandErrorPayload> {
        if let Some(store) = self.committed_store.read().as_ref().map(Arc::clone) {
            return store.load_record();
        }
        Ok(Some(self.record.clone()))
    }

    fn save_record(&self, _record: &ProviderSettingsRecord) -> Result<(), CommandErrorPayload> {
        Err(runtime_operation_failed(
            "candidate provider settings store is read-only".to_owned(),
        ))
    }
}

fn compare_and_swap_provider_settings_with_candidate(
    store: &Arc<dyn ProviderSettingsStore>,
    expected: Option<&ProviderSettingsRecord>,
    record: &ProviderSettingsRecord,
    candidate_store: Option<&CandidateProviderSettingsStore>,
) -> Result<ProviderSettingsSaveOutcome, CommandErrorPayload> {
    let outcome = store.compare_and_swap_record(expected, record)?;
    if outcome == ProviderSettingsSaveOutcome::Saved {
        if let Some(candidate_store) = candidate_store {
            candidate_store.activate(Arc::clone(store));
        }
    }
    Ok(outcome)
}

pub(crate) async fn save_provider_settings_with_runtime_state_unlocked(
    request: ProviderSettingsRequest,
    runtime_state: &DesktopRuntimeState,
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload> {
    for _ in 0..PROVIDER_SETTINGS_COMMIT_ATTEMPTS {
        let prepared = prepare_provider_settings_with_store(
            request.clone(),
            runtime_state.provider_settings_store.as_ref(),
        )
        .await?;
        let _settings_reload_guard = runtime_state.settings_reload_lock.lock().await;
        let candidate_runtime = if prepared.response.config.is_default {
            let candidate_store =
                Arc::new(CandidateProviderSettingsStore::new(prepared.record.clone()));
            let candidate_runtime = build_desktop_settings_runtime(
                runtime_state.runtime_layout(),
                Some(&prepared.response.config.id),
                Arc::clone(&runtime_state.provider_capability_routes),
                Some(Arc::clone(&candidate_store) as Arc<dyn ProviderSettingsStore>),
                Some(Arc::clone(&runtime_state.skill_config_store)),
            )
            .await?;
            Some((candidate_runtime, candidate_store))
        } else {
            None
        };

        if compare_and_swap_provider_settings_with_candidate(
            &runtime_state.provider_settings_store,
            prepared.expected_record.as_ref(),
            &prepared.record,
            candidate_runtime
                .as_ref()
                .map(|(_, candidate_store)| candidate_store.as_ref()),
        )? == ProviderSettingsSaveOutcome::Conflict
        {
            continue;
        }
        if let Some((candidate_runtime, _candidate_store)) = candidate_runtime {
            let (settings_runtime, model_id, protocol, model_options) = candidate_runtime;
            runtime_state.replace_settings_runtime(
                Arc::new(settings_runtime),
                model_id,
                protocol,
                model_options,
            );
        }
        clear_provider_api_key_reveal_tokens_for_config(
            runtime_state,
            &prepared.response.config.id,
        )
        .await;
        return Ok(prepared.response);
    }
    Err(runtime_operation_failed(
        "provider settings changed concurrently; retry the save".to_owned(),
    ))
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
    let requested_protocol = request.protocol.or_else(|| {
        previous_config
            .filter(|config| {
                config.provider_id == request.provider_id && config.model_id == request.model_id
            })
            .map(|config| config.protocol)
    });
    match resolve_model_descriptor(&request.provider_id, &request.model_id) {
        Ok(descriptor) => apply_protocol_override(descriptor, requested_protocol),
        Err(error) if request.provider_id == "openrouter" => {
            if let Some(previous_config) = previous_config {
                if previous_config.provider_id == request.provider_id
                    && previous_config.model_id == request.model_id
                {
                    return provider_config_descriptor(previous_config);
                }
            }
            let descriptor =
                resolve_provider_model_descriptor(&request.provider_id, &request.model_id)
                    .await
                    .map_err(|_| provider_registry_error(error))?;
            apply_protocol_override(descriptor, requested_protocol)
        }
        Err(error) => Err(provider_registry_error(error)),
    }
}

fn apply_protocol_override(
    mut descriptor: ModelDescriptor,
    protocol: Option<ModelProtocol>,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    let Some(protocol) = protocol else {
        return Ok(descriptor);
    };
    if protocol == descriptor.protocol {
        return Ok(descriptor);
    }
    match descriptor.provider_id.as_str() {
        "deepseek" => apply_deepseek_protocol_override(descriptor, protocol),
        "qwen" => apply_qwen_protocol_override(descriptor, protocol),
        "minimax" => match protocol {
            ModelProtocol::Responses => {
                descriptor.protocol = ModelProtocol::Responses;
                descriptor.runtime_semantics = ModelRuntimeSemantics::openai_responses_default();
                Ok(descriptor)
            }
            ModelProtocol::ChatCompletions => {
                descriptor.protocol = ModelProtocol::ChatCompletions;
                descriptor.runtime_semantics = ModelRuntimeSemantics::openai_chat_minimax();
                Ok(descriptor)
            }
            ModelProtocol::Messages => {
                descriptor.protocol = ModelProtocol::Messages;
                descriptor.runtime_semantics = ModelRuntimeSemantics::anthropic_messages_default();
                Ok(descriptor)
            }
            _ => Err(invalid_payload(
                "MiniMax protocol must be responses, chat_completions, or messages".to_owned(),
            )),
        },
        _ => Err(invalid_payload(
            "protocol override is only supported for DeepSeek, Qwen, and MiniMax provider configs"
                .to_owned(),
        )),
    }
}

fn apply_deepseek_protocol_override(
    mut descriptor: ModelDescriptor,
    protocol: ModelProtocol,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    match protocol {
        ModelProtocol::ChatCompletions => {
            descriptor.protocol = ModelProtocol::ChatCompletions;
            descriptor.runtime_semantics = ModelRuntimeSemantics::openai_chat_deepseek();
            Ok(descriptor)
        }
        ModelProtocol::Messages => {
            descriptor.protocol = ModelProtocol::Messages;
            descriptor.runtime_semantics = ModelRuntimeSemantics::deepseek_anthropic_messages();
            Ok(descriptor)
        }
        _ => Err(invalid_payload(
            "DeepSeek provider configs support chat_completions or messages".to_owned(),
        )),
    }
}

fn apply_qwen_protocol_override(
    mut descriptor: ModelDescriptor,
    protocol: ModelProtocol,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    match protocol {
        ModelProtocol::ChatCompletions => {
            descriptor.protocol = ModelProtocol::ChatCompletions;
            descriptor.runtime_semantics = ModelRuntimeSemantics::openai_chat_qwen();
            Ok(descriptor)
        }
        ModelProtocol::Responses => {
            descriptor.protocol = ModelProtocol::Responses;
            descriptor.runtime_semantics = ModelRuntimeSemantics::openai_responses_default();
            Ok(descriptor)
        }
        ModelProtocol::Messages => {
            descriptor.protocol = ModelProtocol::Messages;
            descriptor.runtime_semantics = ModelRuntimeSemantics::anthropic_messages_default();
            Ok(descriptor)
        }
        ModelProtocol::Dashscope => {
            descriptor.protocol = ModelProtocol::Dashscope;
            descriptor.runtime_semantics = ModelRuntimeSemantics::qwen_dashscope_default();
            Ok(descriptor)
        }
        _ => Err(invalid_payload(
            "Qwen protocol must be chat_completions, responses, messages or dashscope".to_owned(),
        )),
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
    let provider_capability_metadata = provider_capability_metadata_for_model(&descriptor);
    let supported_protocols = supported_protocols_for_model(&descriptor);
    let conversation_capability = descriptor.conversation_capability;
    ModelCatalogEntry {
        protocol: descriptor.protocol,
        supported_protocols,
        supported_parameters: descriptor.supported_parameters,
        provider_capability_metadata,
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

fn provider_capability_metadata_for_model(descriptor: &ModelDescriptor) -> Option<Value> {
    if descriptor.provider_id == "minimax" {
        return Some(serde_json::json!({
            "provider": "minimax",
            "serviceTiers": ["standard", "priority"],
            "protocolSupportedParameters": {
                "responses": ["input", "instructions", "max_output_tokens", "metadata", "prompt_cache_key", "reasoning", "service_tier", "stream", "temperature", "text", "tool_choice", "tools", "top_p"],
                "chat_completions": ["max_completion_tokens", "max_tokens", "messages", "reasoning_split", "service_tier", "stream", "stream_options", "temperature", "thinking", "tools", "top_p"],
                "messages": ["max_tokens", "messages", "metadata", "service_tier", "stop_sequences", "stream", "system", "thinking", "tool_choice", "tools", "top_p"]
            },
            "pricing": {
                "currency": "usd",
                "sourceUrl": "https://platform.minimax.io/docs/pricing",
                "tokenPricingInModelPricing": true,
                "nonTokenBillingUnits": [
                    {"category": "text_to_speech", "unit": "characters"},
                    {"category": "voice_clone", "unit": "requests"},
                    {"category": "image_generation", "unit": "images"},
                    {"category": "video_generation", "unit": "seconds"},
                    {"category": "music_generation", "unit": "songs"},
                    {"category": "mcp", "unit": "requests"},
                    {"category": "server_tools", "unit": "requests"}
                ]
            }
        }));
    }
    if descriptor.provider_id != "anthropic" {
        return None;
    }
    let effort_levels = if descriptor.model_id.contains("sonnet-5")
        || descriptor.model_id.contains("fable-5")
        || descriptor.model_id.contains("opus-4-8")
        || descriptor.model_id.contains("opus-4-7")
        || descriptor.model_id.contains("opus-4-6")
        || descriptor.model_id.contains("sonnet-4-6")
    {
        vec!["low", "medium", "high", "xhigh", "max"]
    } else {
        vec!["low", "medium", "high"]
    };
    let thinking_modes = if descriptor.model_id.contains("fable-5") {
        vec!["adaptive"]
    } else if descriptor.model_id.contains("sonnet-5")
        || descriptor.model_id.contains("opus-4-8")
        || descriptor.model_id.contains("opus-4-7")
    {
        vec!["adaptive", "disabled"]
    } else {
        vec!["adaptive", "enabled", "disabled"]
    };
    let sampling_locked = descriptor.model_id.contains("fable-5")
        || descriptor.model_id.contains("sonnet-5")
        || descriptor.model_id.contains("opus-4-8")
        || descriptor.model_id.contains("opus-4-7");
    Some(serde_json::json!({
        "provider": "anthropic",
        "thinkingModes": thinking_modes,
        "thinkingDisplayModes": ["", "summarized", "omitted"],
        "effortLevels": effort_levels,
        "serviceTiers": ["auto", "standard_only"],
        "cacheTtls": ["5m", "1h"],
        "toolChoiceModes": ["auto", "none", "any", "tool"],
        "supportsFilesApi": true,
        "supportsBatches": true,
        "supportsCountTokens": true,
        "supportsProviderFiles": true,
        "samplingLocked": sampling_locked,
        "disabledParameters": if sampling_locked {
            serde_json::json!(["temperature", "top_p", "top_k"])
        } else {
            serde_json::json!([])
        },
    }))
}

fn supported_protocols_for_model(descriptor: &ModelDescriptor) -> Vec<ModelProtocol> {
    match descriptor.provider_id.as_str() {
        "qwen" => vec![ModelProtocol::Responses, ModelProtocol::ChatCompletions],
        "minimax" => vec![
            ModelProtocol::Responses,
            ModelProtocol::ChatCompletions,
            ModelProtocol::Messages,
        ],
        _ => vec![descriptor.protocol],
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
        ModelLifecycle::Retiring { retirement_date } => ModelLifecyclePayload {
            kind: "retiring",
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
        runtime_semantics: Some(runtime_semantics_record(&descriptor.runtime_semantics)),
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
        ModelLifecycle::Retiring { retirement_date } => ProviderModelLifecycleRecord::Retiring {
            retirement_date: retirement_date.to_string(),
        },
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
    let conversation_capability =
        conversation_capability_from_record(&record.conversation_capability);
    Ok(ModelDescriptor {
        provider_id: record.provider_id.clone(),
        model_id: record.model_id.clone(),
        display_name: record.display_name.clone(),
        protocol: record.protocol,
        supported_parameters: supported_parameters_for_provider_model(
            &record.provider_id,
            record.protocol,
        ),
        context_window: record.context_window,
        max_output_tokens: record.max_output_tokens,
        provider_declared_capability: conversation_capability.clone(),
        conversation_capability,
        runtime_semantics: runtime_semantics_from_record(record)?,
        lifecycle: model_lifecycle_from_record(&record.lifecycle)?,
        pricing: None,
    })
}

fn supported_parameters_for_provider_model(
    provider_id: &str,
    protocol: ModelProtocol,
) -> Vec<String> {
    let values = match provider_id {
        "anthropic" => provider_default_body_fields("anthropic"),
        "bedrock" => provider_default_body_fields("bedrock"),
        "gemini" => provider_default_body_fields("gemini"),
        "qwen" => provider_default_body_fields("qwen"),
        "km" => provider_default_body_fields("km"),
        "codex" | "openai" => provider_default_body_fields("openai"),
        "deepseek" => provider_default_body_fields("deepseek"),
        "zhipu" => provider_default_body_fields("zhipu"),
        _ if protocol == ModelProtocol::ChatCompletions => {
            provider_default_body_fields("__openai_compatible")
        }
        _ => &[],
    };
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn runtime_semantics_from_record(
    record: &ProviderModelDescriptorRecord,
) -> Result<ModelRuntimeSemantics, CommandErrorPayload> {
    let Some(semantics) = &record.runtime_semantics else {
        return Ok(runtime_semantics_fallback(
            &record.provider_id,
            record.protocol,
        ));
    };

    Ok(ModelRuntimeSemantics {
        protocol: semantics.protocol,
        tool_protocol: tool_protocol_from_record(&semantics.tool_protocol)?,
        reasoning_protocol: reasoning_protocol_from_record(&semantics.reasoning_protocol)?,
        streaming_protocol: streaming_protocol_from_record(&semantics.streaming_protocol)?,
        cache_protocol: cache_protocol_from_record(&semantics.cache_protocol)?,
        media_protocol: media_protocol_from_record(&semantics.media_protocol)?,
        output_protocol: output_protocol_from_record(&semantics.output_protocol)?,
        provider_continuation_dialect: semantics.provider_continuation_dialect.clone(),
    })
}

fn runtime_semantics_fallback(provider_id: &str, protocol: ModelProtocol) -> ModelRuntimeSemantics {
    if provider_id == "openai" && protocol == ModelProtocol::Responses {
        return ModelRuntimeSemantics::openai_responses_default();
    }
    ModelRuntimeSemantics::messages_default(protocol)
}

fn runtime_semantics_record(
    semantics: &ModelRuntimeSemantics,
) -> harness_contracts::ProviderRuntimeSemanticsDescriptor {
    harness_contracts::ProviderRuntimeSemanticsDescriptor {
        protocol: semantics.protocol,
        tool_protocol: tool_protocol_record(&semantics.tool_protocol).to_owned(),
        reasoning_protocol: reasoning_protocol_record(&semantics.reasoning_protocol),
        streaming_protocol: streaming_protocol_record(&semantics.streaming_protocol).to_owned(),
        cache_protocol: cache_protocol_record(&semantics.cache_protocol).to_owned(),
        media_protocol: media_protocol_record(&semantics.media_protocol).to_owned(),
        output_protocol: output_protocol_record(&semantics.output_protocol).to_owned(),
        provider_continuation_dialect: semantics.provider_continuation_dialect.clone(),
    }
}

fn tool_protocol_record(protocol: &ToolProtocolSemantics) -> &'static str {
    match protocol {
        ToolProtocolSemantics::None => "none",
        ToolProtocolSemantics::OpenAiChatTools => "openai_chat_tools",
        ToolProtocolSemantics::OpenAiResponsesTools => "openai_responses_tools",
        ToolProtocolSemantics::AnthropicTools => "anthropic_tools",
        ToolProtocolSemantics::GeminiTools => "gemini_tools",
        ToolProtocolSemantics::BedrockConverseTools => "bedrock_converse_tools",
    }
}

fn tool_protocol_from_record(value: &str) -> Result<ToolProtocolSemantics, CommandErrorPayload> {
    match value {
        "none" => Ok(ToolProtocolSemantics::None),
        "openai_chat_tools" => Ok(ToolProtocolSemantics::OpenAiChatTools),
        "openai_responses_tools" => Ok(ToolProtocolSemantics::OpenAiResponsesTools),
        "anthropic_tools" => Ok(ToolProtocolSemantics::AnthropicTools),
        "gemini_tools" => Ok(ToolProtocolSemantics::GeminiTools),
        "bedrock_converse_tools" => Ok(ToolProtocolSemantics::BedrockConverseTools),
        _ => Err(invalid_payload(format!(
            "unknown runtime semantics toolProtocol: {value}"
        ))),
    }
}

fn reasoning_protocol_record(
    protocol: &ReasoningProtocolSemantics,
) -> harness_contracts::ProviderRuntimeReasoningProtocolDescriptor {
    match protocol {
        ReasoningProtocolSemantics::None => {
            harness_contracts::ProviderRuntimeReasoningProtocolDescriptor::None
        }
        ReasoningProtocolSemantics::PublicThinking => {
            harness_contracts::ProviderRuntimeReasoningProtocolDescriptor::PublicThinking
        }
        ReasoningProtocolSemantics::PublicSummary => {
            harness_contracts::ProviderRuntimeReasoningProtocolDescriptor::PublicSummary
        }
        ReasoningProtocolSemantics::ProviderPrivateReplay {
            continuation_kind,
            required_for_assistant_tool_replay,
        } => harness_contracts::ProviderRuntimeReasoningProtocolDescriptor::ProviderPrivateReplay {
            continuation_kind: continuation_kind_record(continuation_kind),
            required_for_assistant_tool_replay: *required_for_assistant_tool_replay,
        },
    }
}

fn reasoning_protocol_from_record(
    protocol: &harness_contracts::ProviderRuntimeReasoningProtocolDescriptor,
) -> Result<ReasoningProtocolSemantics, CommandErrorPayload> {
    match protocol {
        harness_contracts::ProviderRuntimeReasoningProtocolDescriptor::None => {
            Ok(ReasoningProtocolSemantics::None)
        }
        harness_contracts::ProviderRuntimeReasoningProtocolDescriptor::PublicThinking => {
            Ok(ReasoningProtocolSemantics::PublicThinking)
        }
        harness_contracts::ProviderRuntimeReasoningProtocolDescriptor::PublicSummary => {
            Ok(ReasoningProtocolSemantics::PublicSummary)
        }
        harness_contracts::ProviderRuntimeReasoningProtocolDescriptor::ProviderPrivateReplay {
            continuation_kind,
            required_for_assistant_tool_replay,
        } => Ok(ReasoningProtocolSemantics::ProviderPrivateReplay {
            continuation_kind: continuation_kind_from_record(continuation_kind)?,
            required_for_assistant_tool_replay: *required_for_assistant_tool_replay,
        }),
    }
}

fn continuation_kind_record(kind: &ProviderContinuationKind) -> String {
    match kind {
        ProviderContinuationKind::ReasoningReplay => "reasoning_replay".to_owned(),
        ProviderContinuationKind::ToolReplay => "tool_replay".to_owned(),
        ProviderContinuationKind::CacheReplay => "cache_replay".to_owned(),
        ProviderContinuationKind::ProviderNative(value) => format!("provider_native:{value}"),
    }
}

fn continuation_kind_from_record(
    value: &str,
) -> Result<ProviderContinuationKind, CommandErrorPayload> {
    match value {
        "reasoning_replay" => Ok(ProviderContinuationKind::ReasoningReplay),
        "tool_replay" => Ok(ProviderContinuationKind::ToolReplay),
        "cache_replay" => Ok(ProviderContinuationKind::CacheReplay),
        _ => value
            .strip_prefix("provider_native:")
            .map(|native| ProviderContinuationKind::ProviderNative(native.to_owned()))
            .ok_or_else(|| {
                invalid_payload(format!(
                    "unknown runtime semantics continuation kind: {value}"
                ))
            }),
    }
}

fn streaming_protocol_record(protocol: &StreamingProtocolSemantics) -> &'static str {
    match protocol {
        StreamingProtocolSemantics::None => "none",
        StreamingProtocolSemantics::Sse => "sse",
        StreamingProtocolSemantics::JsonLines => "json_lines",
        StreamingProtocolSemantics::ProviderNative => "provider_native",
    }
}

fn streaming_protocol_from_record(
    value: &str,
) -> Result<StreamingProtocolSemantics, CommandErrorPayload> {
    match value {
        "none" => Ok(StreamingProtocolSemantics::None),
        "sse" => Ok(StreamingProtocolSemantics::Sse),
        "json_lines" => Ok(StreamingProtocolSemantics::JsonLines),
        "provider_native" => Ok(StreamingProtocolSemantics::ProviderNative),
        _ => Err(invalid_payload(format!(
            "unknown runtime semantics streamingProtocol: {value}"
        ))),
    }
}

fn cache_protocol_record(protocol: &CacheProtocolSemantics) -> &'static str {
    match protocol {
        CacheProtocolSemantics::None => "none",
        CacheProtocolSemantics::OpenAiAuto => "openai_auto",
        CacheProtocolSemantics::AnthropicEphemeral => "anthropic_ephemeral",
        CacheProtocolSemantics::GeminiContextCache => "gemini_context_cache",
    }
}

fn cache_protocol_from_record(value: &str) -> Result<CacheProtocolSemantics, CommandErrorPayload> {
    match value {
        "none" => Ok(CacheProtocolSemantics::None),
        "openai_auto" => Ok(CacheProtocolSemantics::OpenAiAuto),
        "anthropic_ephemeral" => Ok(CacheProtocolSemantics::AnthropicEphemeral),
        "gemini_context_cache" => Ok(CacheProtocolSemantics::GeminiContextCache),
        _ => Err(invalid_payload(format!(
            "unknown runtime semantics cacheProtocol: {value}"
        ))),
    }
}

fn media_protocol_record(protocol: &MediaProtocolSemantics) -> &'static str {
    match protocol {
        MediaProtocolSemantics::TextOnly => "text_only",
        MediaProtocolSemantics::OpenAiContentParts => "openai_content_parts",
        MediaProtocolSemantics::ProviderNative => "provider_native",
    }
}

fn media_protocol_from_record(value: &str) -> Result<MediaProtocolSemantics, CommandErrorPayload> {
    match value {
        "text_only" => Ok(MediaProtocolSemantics::TextOnly),
        "openai_content_parts" => Ok(MediaProtocolSemantics::OpenAiContentParts),
        "provider_native" => Ok(MediaProtocolSemantics::ProviderNative),
        _ => Err(invalid_payload(format!(
            "unknown runtime semantics mediaProtocol: {value}"
        ))),
    }
}

fn output_protocol_record(protocol: &OutputProtocolSemantics) -> &'static str {
    match protocol {
        OutputProtocolSemantics::Text => "text",
        OutputProtocolSemantics::TextAndToolUse => "text_and_tool_use",
        OutputProtocolSemantics::StructuredJson => "structured_json",
    }
}

fn output_protocol_from_record(
    value: &str,
) -> Result<OutputProtocolSemantics, CommandErrorPayload> {
    match value {
        "text" => Ok(OutputProtocolSemantics::Text),
        "text_and_tool_use" => Ok(OutputProtocolSemantics::TextAndToolUse),
        "structured_json" => Ok(OutputProtocolSemantics::StructuredJson),
        _ => Err(invalid_payload(format!(
            "unknown runtime semantics outputProtocol: {value}"
        ))),
    }
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
        ProviderModelLifecycleRecord::Retiring { retirement_date } => {
            let retirement_date =
                NaiveDate::parse_from_str(retirement_date, "%Y-%m-%d").map_err(|_| {
                    runtime_init_failed("provider model descriptor is invalid".to_owned())
                })?;
            Ok(ModelLifecycle::Retiring { retirement_date })
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
            supported_parameters: model.supported_parameters,
            context_window: model.context_window,
            max_output_tokens: model.max_output_tokens,
            provider_declared_capability: model.provider_declared_capability,
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
        model_options: config.model_options.clone(),
        provider_id: config.provider_id.clone(),
        provider_defaults: config.provider_defaults.clone(),
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

pub(crate) fn provider_request_defaults(
    config: &ProviderConfigRecord,
) -> Option<ProviderRequestDefaults> {
    config
        .provider_defaults
        .as_ref()
        .map(|defaults| ProviderRequestDefaults {
            body: defaults
                .body
                .clone()
                .unwrap_or_else(|| Value::Object(serde_json::Map::new())),
            headers: defaults.headers.clone(),
        })
}

pub(crate) fn provider_defaults_body(config: &ProviderConfigRecord) -> Value {
    config
        .provider_defaults
        .as_ref()
        .and_then(|defaults| defaults.body.clone())
        .unwrap_or(Value::Null)
}

pub(crate) fn validate_provider_defaults(
    provider_id: &str,
    defaults: Option<&ProviderDefaultsRecord>,
) -> Result<(), CommandErrorPayload> {
    let Some(defaults) = defaults else {
        return Ok(());
    };
    if let Some(body) = &defaults.body {
        let Some(object) = body.as_object() else {
            return Err(invalid_payload(
                "providerDefaults.body must be an object".to_owned(),
            ));
        };
        for forbidden in [
            "model",
            "messages",
            "input",
            "contents",
            "stream",
            "tools",
            "max_output_tokens",
            "previous_response_id",
        ] {
            if object.contains_key(forbidden) {
                return Err(invalid_payload(format!(
                    "providerDefaults.body must not include core field {forbidden}"
                )));
            }
        }
        for key in object.keys() {
            if !provider_default_body_fields(provider_id).contains(&key.as_str()) {
                return Err(invalid_payload(format!(
                    "providerDefaults.body includes unsupported field {key} for provider {provider_id}"
                )));
            }
        }
        ensure_provider_defaults_body_has_no_sensitive_keys(body)?;
    }
    for (name, value) in &defaults.headers {
        match provider_id {
            "qwen"
                if name.eq_ignore_ascii_case("x-dashscope-session-cache") && value == "enable" => {}
            "anthropic" if is_valid_anthropic_default_header(name, value) => {}
            _ => {
                return Err(invalid_payload(format!(
                    "providerDefaults.headers includes unsupported header {name} for provider {provider_id}"
                )));
            }
        }
    }
    Ok(())
}

fn is_valid_anthropic_default_header(name: &str, value: &str) -> bool {
    if name.eq_ignore_ascii_case("anthropic-user-profile-id") {
        return !value.trim().is_empty();
    }
    if !name.eq_ignore_ascii_case("anthropic-beta") {
        return false;
    }
    value
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .all(is_valid_anthropic_beta_token)
}

fn is_valid_anthropic_beta_token(token: &str) -> bool {
    matches!(
        token,
        "files-api-2025-04-14"
            | "output-300k-2026-03-24"
            | "fast-mode-2026-02-01"
            | "server-side-fallback-2026-06-01"
            | "fallback-credit-2026-06-01"
            | "user-profiles-2026-03-24"
            | "context-management-2025-06-27"
    ) || is_dated_anthropic_beta_token(token)
}

fn is_dated_anthropic_beta_token(token: &str) -> bool {
    let Some((name, date)) = token.rsplit_once('-') else {
        return false;
    };
    let Some((name, month)) = name.rsplit_once('-') else {
        return false;
    };
    let Some((name, year)) = name.rsplit_once('-') else {
        return false;
    };
    !name.is_empty()
        && year.len() == 4
        && month.len() == 2
        && date.len() == 2
        && year.chars().all(|ch| ch.is_ascii_digit())
        && month.chars().all(|ch| ch.is_ascii_digit())
        && date.chars().all(|ch| ch.is_ascii_digit())
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

pub(crate) fn validate_provider_defaults_for_protocol(
    provider_id: &str,
    protocol: ModelProtocol,
    defaults: Option<&ProviderDefaultsRecord>,
) -> Result<(), CommandErrorPayload> {
    let Some(defaults) = defaults else {
        return Ok(());
    };
    let Some(body) = defaults
        .body
        .as_ref()
        .and_then(serde_json::Value::as_object)
    else {
        return Ok(());
    };
    let Some(allowed) = provider_default_body_fields_for_protocol(provider_id, protocol) else {
        return Ok(());
    };
    for key in body.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(invalid_payload(format!(
                "providerDefaults.body includes unsupported field {key} for provider {provider_id} protocol {protocol:?}"
            )));
        }
    }
    Ok(())
}

fn validate_provider_defaults_for_descriptor(
    defaults: Option<&ProviderDefaultsRecord>,
    descriptor: &ModelDescriptor,
) -> Result<(), CommandErrorPayload> {
    if descriptor.provider_id != "doubao" {
        return Ok(());
    }
    let Some(body) = defaults.and_then(|defaults| defaults.body.as_ref()) else {
        return Ok(());
    };
    let Some(object) = body.as_object() else {
        return Ok(());
    };
    for key in object.keys() {
        if !descriptor.supported_parameters.iter().any(|parameter| {
            parameter == key
                || (key == "thinking" && parameter == "thinking")
                || (key == "reasoning_effort" && parameter == "reasoning_effort")
        }) {
            return Err(invalid_payload(format!(
                "providerDefaults.body includes unsupported field {key} for model {}",
                descriptor.model_id
            )));
        }
        validate_doubao_provider_default_value(key, &object[key])?;
    }
    Ok(())
}

fn provider_default_body_fields_for_protocol(
    provider_id: &str,
    protocol: ModelProtocol,
) -> Option<&'static [&'static str]> {
    match (provider_id, protocol) {
        ("deepseek", ModelProtocol::ChatCompletions) => Some(&[
            "thinking",
            "reasoning_effort",
            "temperature",
            "top_p",
            "max_tokens",
            "stop",
            "response_format",
            "tool_choice",
            "stream_options",
            "frequency_penalty",
            "presence_penalty",
        ]),
        ("deepseek", ModelProtocol::Messages) => Some(&[
            "thinking",
            "output_config",
            "temperature",
            "top_p",
            "max_tokens",
            "stop_sequences",
            "tool_choice",
            "metadata",
        ]),
        _ => None,
    }
}

fn validate_doubao_provider_default_value(
    key: &str,
    value: &Value,
) -> Result<(), CommandErrorPayload> {
    match key {
        "thinking" => {
            let Some(object) = value.as_object() else {
                return Err(invalid_payload(
                    "providerDefaults.body thinking must be an object".to_owned(),
                ));
            };
            match object.get("type").and_then(Value::as_str) {
                Some("enabled" | "disabled" | "auto") => {}
                _ => {
                    return Err(invalid_payload(
                        "providerDefaults.body thinking.type must be enabled, disabled, or auto"
                            .to_owned(),
                    ));
                }
            }
        }
        "reasoning_effort" => match value.as_str() {
            Some("none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max") => {}
            _ => {
                return Err(invalid_payload(
                    "providerDefaults.body reasoning_effort must be none, minimal, low, medium, high, xhigh, or max"
                        .to_owned(),
                ));
            }
        },
        "service_tier" => match value.as_str() {
            Some("fast" | "auto" | "default") => {}
            _ => {
                return Err(invalid_payload(
                    "providerDefaults.body service_tier must be fast, auto, or default".to_owned(),
                ));
            }
        },
        "max_completion_tokens" => match value.as_u64() {
            Some(1..=65_536) => {}
            _ => {
                return Err(invalid_payload(
                    "providerDefaults.body max_completion_tokens must be between 1 and 65536"
                        .to_owned(),
                ));
            }
        },
        _ => {}
    }
    Ok(())
}

fn ensure_provider_defaults_body_has_no_sensitive_keys(
    value: &Value,
) -> Result<(), CommandErrorPayload> {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                if is_sensitive_provider_default_key(key) {
                    return Err(invalid_payload(format!(
                        "providerDefaults.body includes sensitive field {key}"
                    )));
                }
                ensure_provider_defaults_body_has_no_sensitive_keys(nested)?;
            }
        }
        Value::Array(values) => {
            for nested in values {
                ensure_provider_defaults_body_has_no_sensitive_keys(nested)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_sensitive_provider_default_key(key: &str) -> bool {
    let normalized = key.trim().to_lowercase();
    let compact = normalized
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>();
    if [
        "apikey",
        "accesskey",
        "privatekey",
        "clientsecret",
        "secretkey",
    ]
    .iter()
    .any(|pattern| compact.contains(pattern))
    {
        return true;
    }
    let components = normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    if components.iter().any(|component| {
        matches!(
            *component,
            "authorization" | "bearer" | "cookie" | "credential" | "password" | "secret" | "token"
        )
    }) {
        return true;
    }
    components.windows(2).any(|window| {
        matches!(
            window,
            ["api", "key"] | ["access", "key"] | ["private", "key"] | ["client", "secret"]
        )
    })
}

fn provider_default_body_fields(provider_id: &str) -> &'static [&'static str] {
    match provider_id {
        "anthropic" => &[
            "thinking",
            "output_config",
            "service_tier",
            "stop_sequences",
            "top_k",
            "top_p",
            "tool_choice",
            "metadata",
            "container",
            "context_management",
            "mcp_servers",
            "inference_geo",
            "speed",
            "fallbacks",
            "cache_control",
        ],
        "bedrock" => &[
            "inferenceConfig",
            "additionalModelRequestFields",
            "additionalModelResponseFieldPaths",
            "performanceConfig",
            "requestMetadata",
        ],
        "gemini" => &[
            "thinkingConfig",
            "stopSequences",
            "topP",
            "topK",
            "seed",
            "responseMimeType",
            "responseSchema",
            "responseJsonSchema",
            "toolConfig",
            "safetySettings",
            "cachedContent",
            "cached_content",
            "serviceTier",
            "store",
        ],
        "qwen" => &[
            "enable_thinking",
            "thinking_budget",
            "preserve_thinking",
            "reasoning",
            "thinking",
            "tools",
            "enable_search",
            "enable_code_interpreter",
            "search_options",
        ],
        "km" => &[
            "max_completion_tokens",
            "temperature",
            "top_p",
            "n",
            "stop",
            "stream_options",
            "tools",
            "tool_choice",
            "response_format",
            "thinking",
            "prompt_cache_key",
            "safety_identifier",
            "partial",
            "presence_penalty",
            "frequency_penalty",
        ],
        "codex" | "openai" => &[
            "background",
            "conversation",
            "include",
            "instructions",
            "max_tool_calls",
            "prompt",
            "prompt_cache_key",
            "prompt_cache_retention",
            "reasoning",
            "safety_identifier",
            "service_tier",
            "text",
            "top_logprobs",
            "top_p",
            "metadata",
            "store",
            "truncation",
            "parallel_tool_calls",
            "tool_choice",
            "user",
        ],
        "deepseek" => &[
            "thinking",
            "reasoning_effort",
            "temperature",
            "top_p",
            "output_config",
            "max_tokens",
            "stop",
            "stop_sequences",
            "response_format",
            "tool_choice",
            "stream_options",
            "frequency_penalty",
            "presence_penalty",
            "metadata",
        ],
        "zhipu" => &[
            "thinking",
            "reasoning_effort",
            "do_sample",
            "temperature",
            "top_p",
            "max_tokens",
            "tool_stream",
            "stop",
            "response_format",
            "user_id",
        ],
        _ => &[
            "temperature",
            "top_p",
            "top_k",
            "max_tokens",
            "max_completion_tokens",
            "stop",
            "response_format",
            "tool_choice",
            "reasoning",
            "reasoning_effort",
            "thinking",
            "service_tier",
            "stream_options",
            "frequency_penalty",
            "presence_penalty",
            "logprobs",
            "top_logprobs",
            "seed",
            "n",
        ],
    }
}

pub(crate) fn provider_config_descriptor(
    config: &ProviderConfigRecord,
) -> Result<ModelDescriptor, CommandErrorPayload> {
    match resolve_model_descriptor(&config.provider_id, &config.model_id) {
        Ok(descriptor) => apply_protocol_override(descriptor, Some(config.protocol)),
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

pub(crate) fn normalized_provider_base_url(
    provider_id: &str,
    value: Option<&str>,
) -> Result<Option<String>, CommandErrorPayload> {
    let normalized = normalized_base_url(value)?;
    if provider_id == "qwen" {
        if let Some(base_url) = normalized.as_deref() {
            if base_url.contains("{WorkspaceId}") {
                return Err(invalid_payload(
                    "Qwen baseUrl template requires replacing {WorkspaceId} before saving"
                        .to_owned(),
                ));
            }
            return Ok(Some(jyowo_harness_sdk::builtin::normalize_qwen_base_url(
                base_url,
            )));
        }
    }
    if provider_id == "gemini" {
        if let Some(base_url) = normalized.as_deref() {
            validate_gemini_base_url(base_url)?;
        }
    }
    Ok(normalized)
}

fn validate_provider_protocol_base_url(
    provider_id: &str,
    protocol: ModelProtocol,
    base_url: Option<&str>,
) -> Result<(), CommandErrorPayload> {
    if provider_id != "deepseek" {
        return Ok(());
    }
    let Some(base_url) = base_url else {
        return Ok(());
    };
    let base_url = base_url.trim_end_matches('/');
    if protocol == ModelProtocol::Messages && !base_url.ends_with("/anthropic") {
        return Err(invalid_payload(
            "DeepSeek Anthropic Messages requires an /anthropic baseUrl".to_owned(),
        ));
    }
    if protocol == ModelProtocol::ChatCompletions && base_url.ends_with("/anthropic") {
        return Err(invalid_payload(
            "DeepSeek Chat Completions must not use the /anthropic baseUrl".to_owned(),
        ));
    }
    Ok(())
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

fn validate_gemini_base_url(base_url: &str) -> Result<(), CommandErrorPayload> {
    let parsed = reqwest::Url::parse(base_url)
        .map_err(|_| invalid_payload("Gemini baseUrl must be a valid URL".to_owned()))?;
    let Some(host) = parsed.host_str() else {
        return Err(invalid_payload(
            "Gemini baseUrl must include a host".to_owned(),
        ));
    };
    if host.eq_ignore_ascii_case("generativelanguage.googleapis.com") {
        if parsed.scheme() != "https" {
            return Err(invalid_payload(
                "Gemini baseUrl must use https://".to_owned(),
            ));
        }
        return Ok(());
    }
    #[cfg(debug_assertions)]
    if url_targets_loopback(&parsed) {
        return Ok(());
    }
    Err(invalid_payload(
        "Gemini baseUrl must target generativelanguage.googleapis.com".to_owned(),
    ))
}

pub(crate) fn build_provider_for_config(
    config: &ProviderConfigRecord,
) -> Result<(Arc<dyn ModelProvider>, ModelProtocol), CommandErrorPayload> {
    let descriptor = provider_config_descriptor(config)?;
    let api_key = config.api_key.trim();
    if provider_requires_api_key(&config.provider_id) && api_key.is_empty() {
        return Err(runtime_init_failed(
            "provider config has no api key".to_owned(),
        ));
    }
    let base_url = normalized_provider_base_url(&config.provider_id, config.base_url.as_deref())?;
    let provider = build_provider(ProviderBuildConfig {
        provider_id: config.provider_id.clone(),
        api_key: api_key.to_owned(),
        base_url,
        model_descriptor: Some(descriptor.clone()),
        provider_defaults: provider_request_defaults(config),
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
) -> Result<
    Option<(
        Arc<dyn ModelProvider>,
        String,
        ModelProtocol,
        harness_contracts::ModelRequestOptions,
    )>,
    CommandErrorPayload,
> {
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
    if provider_requires_api_key(&config.provider_id) && api_key.is_empty() {
        return Err(runtime_init_failed(
            "provider config has no api key".to_owned(),
        ));
    }
    let base_url = normalized_provider_base_url(&config.provider_id, config.base_url.as_deref())?;
    let provider = build_provider(ProviderBuildConfig {
        provider_id: config.provider_id.clone(),
        api_key: api_key.to_owned(),
        base_url,
        model_descriptor: Some(descriptor.clone()),
        provider_defaults: provider_request_defaults(config),
    })
    .map_err(provider_registry_init_error)?;
    let protocol = descriptor.protocol;

    Ok(Some((
        Arc::from(provider),
        config.model_id.clone(),
        protocol,
        config.model_options.clone(),
    )))
}

use harness_contracts::{
    ModelProtocol, ProviderProfileDefaults, ProviderProfileDefinition,
    ProviderProfileModelDescriptor,
};

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
        model_options: config.model_options.clone(),
        base_url: config.base_url.clone(),
        provider_defaults: config
            .provider_defaults
            .clone()
            .map(provider_profile_defaults_from_record),
        model_descriptor: provider_profile_descriptor_from_config(config),
    }
}

fn provider_profile_defaults_from_record(
    defaults: ProviderDefaultsRecord,
) -> ProviderProfileDefaults {
    ProviderProfileDefaults {
        body: defaults.body,
        headers: defaults.headers,
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
        runtime_semantics: config
            .model_descriptor
            .runtime_semantics
            .clone()
            .or_else(|| {
                Some(runtime_semantics_record(&runtime_semantics_fallback(
                    &config.model_descriptor.provider_id,
                    config.model_descriptor.protocol,
                )))
            }),
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
        ProviderModelLifecycleRecord::Retiring { retirement_date } => {
            harness_contracts::ProviderProfileModelLifecycle::Retiring {
                retirement_date: retirement_date.clone(),
            }
        }
    }
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
    use std::collections::BTreeMap;

    use harness_contracts::{
        ModelProtocol, ModelRequestOptions, PermissionMode, ProviderSelectionRecord, ToolProfile,
    };
    use serde_json::json;

    use crate::commands::stores::ProjectConfigStore;
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

    fn provider_config(id: &str) -> ProviderConfigRecord {
        ProviderConfigRecord {
            api_key: format!("{id}-api-key"),
            base_url: None,
            display_name: id.to_owned(),
            id: id.to_owned(),
            model_descriptor: ProviderModelDescriptorRecord {
                context_window: 128_000,
                conversation_capability: ConversationModelCapabilityRecord {
                    context_window: 128_000,
                    input_modalities: vec![ProviderModelModalityRecord::Text],
                    max_output_tokens: 8_192,
                    output_modalities: vec![ProviderModelModalityRecord::Text],
                    prompt_cache: false,
                    reasoning: false,
                    streaming: true,
                    structured_output: true,
                    tool_calling: true,
                },
                display_name: "GPT".to_owned(),
                lifecycle: ProviderModelLifecycleRecord::Stable,
                max_output_tokens: 8_192,
                model_id: "gpt-5.4-mini".to_owned(),
                protocol: ModelProtocol::Responses,
                provider_id: "openai".to_owned(),
                runtime_semantics: None,
            },
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: ModelRequestOptions::default(),
            official_quota_api_key: None,
            protocol: ModelProtocol::Responses,
            provider_defaults: None,
            provider_id: "openai".to_owned(),
        }
    }

    #[test]
    fn provider_defaults_reject_sensitive_body_keys_recursively() {
        let valid_defaults = ProviderDefaultsRecord {
            body: Some(json!({
                "max_completion_tokens": 1024,
                "response_format": { "type": "json_object" }
            })),
            headers: BTreeMap::new(),
        };
        validate_provider_defaults("doubao", Some(&valid_defaults))
            .expect("ordinary provider defaults should be accepted");

        let sensitive_defaults = ProviderDefaultsRecord {
            body: Some(json!({
                "response_format": {
                    "api_key": "secret-value"
                }
            })),
            headers: BTreeMap::new(),
        };

        let error = validate_provider_defaults("doubao", Some(&sensitive_defaults))
            .expect_err("sensitive provider defaults should be rejected");

        assert!(error.message.contains("sensitive field api_key"));
        assert!(!error.message.contains("secret-value"));

        for key in [
            "access_key",
            "private_key",
            "bearer",
            "clientSecret",
            "secretKey",
        ] {
            let defaults = ProviderDefaultsRecord {
                body: Some(json!({ "response_format": { key: "secret-value" } })),
                headers: BTreeMap::new(),
            };
            let error = validate_provider_defaults("doubao", Some(&defaults))
                .expect_err("sensitive provider defaults should be rejected");

            assert!(error.message.contains(&format!("sensitive field {key}")));
            assert!(!error.message.contains("secret-value"));
        }
    }

    #[test]
    fn doubao_provider_defaults_reject_invalid_official_values() {
        let descriptor = resolve_model_descriptor("doubao", "doubao-seed-2-1-pro-260628")
            .expect("doubao descriptor should resolve");

        for (body, message) in [
            (
                json!({ "thinking": { "type": "manual" } }),
                "thinking.type must be enabled, disabled, or auto",
            ),
            (
                json!({ "reasoning_effort": "ultra" }),
                "reasoning_effort must be none, minimal, low, medium, high, xhigh, or max",
            ),
            (
                json!({ "service_tier": "standard_only" }),
                "service_tier must be fast, auto, or default",
            ),
            (
                json!({ "max_completion_tokens": 0 }),
                "max_completion_tokens must be between 1 and 65536",
            ),
        ] {
            let defaults = ProviderDefaultsRecord {
                body: Some(body),
                headers: BTreeMap::new(),
            };
            let error = validate_provider_defaults_for_descriptor(Some(&defaults), &descriptor)
                .expect_err("invalid doubao provider defaults should be rejected");

            assert!(error.message.contains(message));
        }
    }

    #[tokio::test]
    async fn doubao_model_change_does_not_inherit_incompatible_provider_defaults() {
        let home = temp_execution_settings_home();
        let layout = StorageLayout::new(JyowoHome::new(home.path().join(".jyowo")));
        let store = DesktopProviderSettingsStore::global_only_with_layout(layout);

        save_provider_settings_with_store(
            ProviderSettingsRequest {
                api_key: Some("ark-test-key".to_owned()),
                base_url: None,
                config_id: Some("doubao-main".to_owned()),
                display_name: Some("Doubao Main".to_owned()),
                model_id: "doubao-seed-2-1-pro-260628".to_owned(),
                model_options: None,
                official_quota_api_key: None,
                provider_id: "doubao".to_owned(),
                protocol: None,
                provider_defaults: Some(ProviderDefaultsRecord {
                    body: Some(json!({
                        "reasoning_effort": "xhigh",
                        "thinking": { "type": "auto" }
                    })),
                    headers: BTreeMap::new(),
                }),
                set_default: true,
            },
            &store,
        )
        .await
        .expect("save reasoning doubao config");

        let response = save_provider_settings_with_store(
            ProviderSettingsRequest {
                api_key: None,
                base_url: None,
                config_id: Some("doubao-main".to_owned()),
                display_name: Some("Doubao Main".to_owned()),
                model_id: "doubao-seed-character-260628".to_owned(),
                model_options: None,
                official_quota_api_key: None,
                provider_id: "doubao".to_owned(),
                protocol: None,
                provider_defaults: None,
                set_default: true,
            },
            &store,
        )
        .await
        .expect("save non-reasoning doubao config");

        assert!(response.config.provider_defaults.is_none());

        let rejected = save_provider_settings_with_store(
            ProviderSettingsRequest {
                api_key: None,
                base_url: None,
                config_id: Some("doubao-main".to_owned()),
                display_name: Some("Doubao Main".to_owned()),
                model_id: "doubao-seed-character-260628".to_owned(),
                model_options: None,
                official_quota_api_key: None,
                provider_id: "doubao".to_owned(),
                protocol: None,
                provider_defaults: Some(ProviderDefaultsRecord {
                    body: Some(json!({ "thinking": { "type": "auto" } })),
                    headers: BTreeMap::new(),
                }),
                set_default: true,
            },
            &store,
        )
        .await
        .expect_err("unsupported doubao defaults should be rejected");

        assert!(rejected
            .message
            .contains("unsupported field thinking for model doubao-seed-character-260628"));
    }

    #[test]
    fn doubao_saved_profile_load_rejects_incompatible_provider_defaults() {
        let home = temp_execution_settings_home();
        let layout = StorageLayout::new(JyowoHome::new(home.path().join(".jyowo")));
        let store = DesktopProviderSettingsStore::global_only_with_layout(layout);
        let mut config = provider_config("doubao-main");
        config.provider_id = "doubao".to_owned();
        config.model_id = "doubao-seed-character-260628".to_owned();
        config.protocol = ModelProtocol::Responses;
        config.model_descriptor.provider_id = "doubao".to_owned();
        config.model_descriptor.model_id = "doubao-seed-character-260628".to_owned();
        config.model_descriptor.protocol = ModelProtocol::Responses;
        config.provider_defaults = Some(ProviderDefaultsRecord {
            body: Some(json!({ "thinking": { "type": "auto" } })),
            headers: BTreeMap::new(),
        });
        let profile = provider_profile_definition_from_config(&config, config.id.clone());
        store
            .global_config_store()
            .save_provider_profiles(&[profile])
            .expect("legacy provider profile should save");

        let error = store
            .load_record()
            .expect_err("legacy incompatible doubao defaults should fail to load");

        assert!(error
            .message
            .contains("unsupported field thinking for model doubao-seed-character-260628"));
    }

    fn execution_record(
        permission_mode: PermissionMode,
        tool_profile: ToolProfile,
    ) -> harness_contracts::ExecutionDefaultsRecord {
        harness_contracts::ExecutionDefaultsRecord {
            permission_mode,
            tool_profile,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        }
    }

    #[test]
    fn provider_settings_store_from_project_runtime_layout_uses_global_selection() {
        let home = temp_execution_settings_home();
        let layout = StorageLayout::new(JyowoHome::new(home.path().join(".jyowo")));
        let workspace = home.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let runtime_layout = layout.runtime_layout_for_project(&workspace);
        let configs = vec![
            provider_config("global-default"),
            provider_config("project-default"),
        ];

        DesktopProviderSettingsStore::global_only_with_layout(layout.clone())
            .save_record(&ProviderSettingsRecord {
                default_config_id: Some("global-default".to_owned()),
                configs,
            })
            .expect("save global provider selection");
        ProjectConfigStore::new(layout.clone(), workspace.clone())
            .save_project_provider_selection(&ProviderSelectionRecord {
                default_config_id: Some("project-default".to_owned()),
            })
            .expect("save stale project provider selection");

        let store =
            DesktopProviderSettingsStore::from_runtime_layout_with_layout(layout, &runtime_layout);
        let record = store
            .load_record()
            .expect("load provider settings")
            .expect("provider settings");

        assert_eq!(store.selection_scope(), SettingsScope::Global);
        assert_eq!(record.default_config_id.as_deref(), Some("global-default"));
    }

    #[test]
    fn provider_settings_store_new_with_layout_uses_global_selection() {
        let home = temp_execution_settings_home();
        let layout = StorageLayout::new(JyowoHome::new(home.path().join(".jyowo")));
        let workspace = home.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let configs = vec![
            provider_config("global-default"),
            provider_config("project-default"),
        ];

        DesktopProviderSettingsStore::global_only_with_layout(layout.clone())
            .save_record(&ProviderSettingsRecord {
                default_config_id: Some("global-default".to_owned()),
                configs,
            })
            .expect("save global provider selection");
        ProjectConfigStore::new(layout.clone(), workspace.clone())
            .save_project_provider_selection(&ProviderSelectionRecord {
                default_config_id: Some("project-default".to_owned()),
            })
            .expect("save stale project provider selection");

        let store = DesktopProviderSettingsStore::new_with_layout(layout, workspace);
        let record = store
            .load_record()
            .expect("load provider settings")
            .expect("provider settings");

        assert_eq!(store.selection_scope(), SettingsScope::Global);
        assert_eq!(record.default_config_id.as_deref(), Some("global-default"));
    }

    #[test]
    fn execution_settings_store_from_project_runtime_layout_uses_global_defaults() {
        let home = temp_execution_settings_home();
        let layout = StorageLayout::new(JyowoHome::new(home.path().join(".jyowo")));
        let workspace = home.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let runtime_layout = layout.runtime_layout_for_project(&workspace);

        DesktopExecutionSettingsStore::global_only_with_layout(layout.clone())
            .save_record(
                &execution_record(PermissionMode::BypassPermissions, ToolProfile::Coding),
                None,
            )
            .expect("save global execution defaults");
        DesktopExecutionSettingsStore::new_with_layout(layout.clone(), workspace.clone())
            .save_record(
                &execution_record(PermissionMode::Default, ToolProfile::Minimal),
                None,
            )
            .expect("save stale project execution overrides");

        let store =
            DesktopExecutionSettingsStore::from_runtime_layout_with_layout(layout, &runtime_layout);
        let response = get_execution_settings_with_store(&store, None).expect("load settings");

        assert_eq!(response.scope, SettingsScope::Global);
        assert_eq!(response.permission_mode, PermissionMode::BypassPermissions);
        assert_eq!(response.tool_profile, ToolProfile::Coding);
    }

    #[test]
    fn get_execution_settings_for_state_request_uses_global_defaults_with_active_project() {
        let temp = temp_execution_settings_home();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let state = DesktopRuntimeState::with_workspace_for_test(workspace)
            .expect("runtime state should initialize");
        state
            .global_config_store
            .as_ref()
            .expect("global config store")
            .save_execution_defaults(&execution_record(
                PermissionMode::BypassPermissions,
                ToolProfile::Coding,
            ))
            .expect("save global execution defaults");
        state
            .project_config_store
            .as_ref()
            .expect("project config store")
            .save_execution_overrides(
                &execution_record(PermissionMode::Default, ToolProfile::Minimal).into(),
            )
            .expect("save stale project execution overrides");
        let registry = crate::project_registry::ProjectRegistry::load().expect("project registry");

        let response = get_execution_settings_for_state_request(
            GetExecutionSettingsRequest {
                workspace_path: None,
            },
            &state,
            &registry,
            None,
        )
        .expect("load settings");

        assert_eq!(response.scope, SettingsScope::Global);
        assert_eq!(response.permission_mode, PermissionMode::BypassPermissions);
        assert_eq!(response.tool_profile, ToolProfile::Coding);
    }

    #[test]
    fn global_only_execution_settings_can_save_daemon_supported_subagents() {
        let home = temp_execution_settings_home();
        let layout = StorageLayout::new(JyowoHome::new(home.path().join(".jyowo")));
        let store = DesktopExecutionSettingsStore::global_only_with_layout(layout);
        let capabilities = harness_contracts::AgentCapabilities::daemon_native();

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
            Some(&capabilities),
        )
        .expect("daemon-supported subagents should be saveable");

        assert!(response.agent_capabilities.subagents_enabled);
        assert!(response.agent_capabilities.subagents_available);
        assert!(store.load_record().expect("load record").subagents_enabled);
    }

    #[tokio::test]
    async fn activated_candidate_store_reads_committed_route_config_updates() {
        let home = temp_execution_settings_home();
        let layout = StorageLayout::new(JyowoHome::new(home.path().join(".jyowo")));
        let active_store = Arc::new(DesktopProviderSettingsStore::global_only_with_layout(
            layout,
        ));
        let mut route_config = provider_config("route-config");
        route_config.api_key = "old-route-key".to_owned();
        let original = ProviderSettingsRecord {
            default_config_id: Some("default-config".to_owned()),
            configs: vec![provider_config("default-config"), route_config],
        };
        active_store
            .save_record(&original)
            .expect("save original provider settings");
        let original = active_store
            .load_record()
            .expect("load original provider settings")
            .expect("original provider settings");
        let candidate_store = Arc::new(CandidateProviderSettingsStore::new(original.clone()));
        let routes = Arc::new(ParkingRwLock::new(ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![ProviderCapabilityRoute {
                kind: CapabilityRouteKind::ImageGeneration,
                config_id: "route-config".to_owned(),
                provider_id: "openai".to_owned(),
                operation_ids: vec!["test.image_generation".to_owned()],
                enabled: true,
            }],
        }));
        let resolver = DesktopProviderCredentialResolver::new(candidate_store.clone(), routes);

        let mut committed = original.clone();
        committed
            .configs
            .iter_mut()
            .find(|config| config.id == "route-config")
            .expect("route config")
            .api_key = "new-route-key".to_owned();
        let resolve_context = || ProviderCredentialResolveContext {
            tenant_id: TenantId::SINGLE,
            session_id: SessionId::new(),
            run_id: RunId::new(),
            provider_id: "openai".to_owned(),
            model_config_id: None,
            operation_id: Some("test.image_generation".to_owned()),
            route_kind: Some(CapabilityRouteKind::ImageGeneration),
        };
        assert_eq!(
            resolver
                .resolve_provider_credential(resolve_context())
                .await
                .expect("candidate credential before activation")
                .api_key,
            "old-route-key"
        );
        let active_store: Arc<dyn ProviderSettingsStore> = active_store;
        let conflict = compare_and_swap_provider_settings_with_candidate(
            &active_store,
            Some(&committed),
            &committed,
            Some(candidate_store.as_ref()),
        )
        .expect("conflicting commit result");
        assert_eq!(conflict, ProviderSettingsSaveOutcome::Conflict);
        assert_eq!(
            resolver
                .resolve_provider_credential(resolve_context())
                .await
                .expect("candidate credential after conflict")
                .api_key,
            "old-route-key"
        );

        let saved = compare_and_swap_provider_settings_with_candidate(
            &active_store,
            Some(&original),
            &committed,
            Some(candidate_store.as_ref()),
        )
        .expect("successful commit result");
        assert_eq!(saved, ProviderSettingsSaveOutcome::Saved);

        let credential = resolver
            .resolve_provider_credential(resolve_context())
            .await
            .expect("resolve updated route credential");

        assert_eq!(credential.api_key, "new-route-key");
        assert_ne!(credential.api_key, "old-route-key");

        let revoked = ProviderSettingsRecord {
            default_config_id: committed.default_config_id.clone(),
            configs: committed
                .configs
                .iter()
                .filter(|config| config.id != "route-config")
                .cloned()
                .collect(),
        };
        let revoked_outcome = compare_and_swap_provider_settings_with_candidate(
            &active_store,
            Some(&committed),
            &revoked,
            None,
        )
        .expect("revoke route config");
        assert_eq!(revoked_outcome, ProviderSettingsSaveOutcome::Saved);
        let error = resolver
            .resolve_provider_credential(resolve_context())
            .await
            .expect_err("revoked route credential must not remain available");
        assert!(matches!(error, ToolError::PermissionDenied(_)));
    }
}
