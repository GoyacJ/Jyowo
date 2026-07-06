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
    workspace_root: PathBuf,
}

impl DesktopProviderSettingsStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn settings_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("provider-settings.json")
    }
}

impl ProviderSettingsStore for DesktopProviderSettingsStore {
    fn load_record(&self) -> Result<Option<ProviderSettingsRecord>, CommandErrorPayload> {
        let settings_path = self.settings_path();
        read_secret_json_file_or_remove_invalid(&settings_path, "provider settings")
    }

    fn save_record(&self, record: &ProviderSettingsRecord) -> Result<(), CommandErrorPayload> {
        ensure_provider_settings_record(record)?;
        let settings_path = self.settings_path();
        write_secret_json_file_atomic(&settings_path, "provider settings", record)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecutionSettingsRecord {
    #[serde(default = "default_permission_mode")]
    pub permission_mode: PermissionMode,
    #[serde(default)]
    pub tool_profile: ToolProfile,
    #[serde(default = "default_context_compression_trigger_ratio")]
    pub context_compression_trigger_ratio: f32,
    #[serde(default)]
    pub subagents_enabled: bool,
    #[serde(default)]
    pub agent_teams_enabled: bool,
    #[serde(default)]
    pub background_agents_enabled: bool,
}

pub(crate) fn default_permission_mode() -> PermissionMode {
    PermissionMode::Default
}

pub(crate) fn default_context_compression_trigger_ratio() -> f32 {
    0.8
}

impl Default for ExecutionSettingsRecord {
    fn default() -> Self {
        Self {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: default_context_compression_trigger_ratio(),
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
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

    fn settings_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("provider-capability-routes.json")
    }
}

impl provider_capability_route_store_seal::Sealed for DesktopProviderCapabilityRouteStore {}

impl ProviderCapabilityRouteStore for DesktopProviderCapabilityRouteStore {
    fn load_record(&self) -> Result<Option<ProviderCapabilityRouteSettings>, CommandErrorPayload> {
        let settings_path = self.settings_path();
        read_secret_json_file_or_remove_invalid(&settings_path, "provider capability route")
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

#[derive(Clone)]
pub struct DesktopExecutionSettingsStore {
    workspace_root: PathBuf,
}

impl DesktopExecutionSettingsStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    fn settings_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("execution-settings.json")
    }

    pub fn load_record(&self) -> Result<ExecutionSettingsRecord, CommandErrorPayload> {
        let settings_path = self.settings_path();
        let Some(record) = read_json_file_or_remove_invalid(&settings_path, "execution settings")?
        else {
            return Ok(ExecutionSettingsRecord::default());
        };
        if ensure_execution_settings_structure(&record).is_err() {
            remove_invalid_json_file(&settings_path, "execution settings")?;
            return Ok(ExecutionSettingsRecord::default());
        }
        Ok(record)
    }

    pub fn save_record(
        &self,
        record: &ExecutionSettingsRecord,
        context: Option<&AgentCapabilityResolutionContext>,
    ) -> Result<(), CommandErrorPayload> {
        ensure_execution_settings_record(record, &self.workspace_root, context)?;
        let settings_path = self.settings_path();
        write_json_file_atomic(&settings_path, "execution settings", record)
    }
}

pub(crate) fn ensure_execution_settings_structure(
    record: &ExecutionSettingsRecord,
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

pub(crate) fn ensure_execution_settings_record(
    record: &ExecutionSettingsRecord,
    workspace_root: &Path,
    context: Option<&AgentCapabilityResolutionContext>,
) -> Result<(), CommandErrorPayload> {
    ensure_execution_settings_structure(record)?;

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
    record: &ExecutionSettingsRecord,
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
    workspace_root: PathBuf,
}

impl DesktopConversationMetadataStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn metadata_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("conversation-metadata.json")
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
    let permission_mode = effective_execution_settings_permission_mode(record.permission_mode);
    Ok(GetExecutionSettingsResponse {
        permission_mode,
        tool_profile: record.tool_profile.clone(),
        context_compression_trigger_ratio: record.context_compression_trigger_ratio,
        auto_mode_available: auto_mode_available(),
        agent_capabilities: agent_capabilities_payload(&record, store.workspace_root(), context),
    })
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
    ensure_execution_settings_record(
        &ExecutionSettingsRecord {
            permission_mode: request.permission_mode,
            tool_profile: request.tool_profile.clone(),
            context_compression_trigger_ratio: request.context_compression_trigger_ratio,
            subagents_enabled: request.subagents_enabled,
            agent_teams_enabled: request.agent_teams_enabled,
            background_agents_enabled: request.background_agents_enabled,
        },
        store.workspace_root(),
        context,
    )?;
    if request.permission_mode == PermissionMode::Auto && !auto_mode_available() {
        return Err(invalid_payload(
            "auto permission mode is unavailable in this desktop build".to_owned(),
        ));
    }
    let record = ExecutionSettingsRecord {
        permission_mode: request.permission_mode,
        tool_profile: request.tool_profile,
        context_compression_trigger_ratio: request.context_compression_trigger_ratio,
        subagents_enabled: request.subagents_enabled,
        agent_teams_enabled: request.agent_teams_enabled,
        background_agents_enabled: request.background_agents_enabled,
    };
    store.save_record(&record, context)?;
    Ok(SetExecutionSettingsResponse {
        permission_mode: record.permission_mode,
        tool_profile: record.tool_profile.clone(),
        context_compression_trigger_ratio: record.context_compression_trigger_ratio,
        auto_mode_available: auto_mode_available(),
        agent_capabilities: agent_capabilities_payload(&record, store.workspace_root(), context),
    })
}

pub async fn list_provider_settings_with_store(
    store: &dyn ProviderSettingsStore,
) -> Result<ListProviderSettingsResponse, CommandErrorPayload> {
    let record = store.load_record()?.unwrap_or_default();

    Ok(ListProviderSettingsResponse {
        default_config_id: record.default_config_id.clone(),
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
    _store: &dyn ProviderCapabilityRouteStore,
    provider_settings: &ProviderSettingsRecord,
    provider_catalog: &ModelProviderCatalogResponse,
    adapter_availability: &ProviderServiceAdapterAvailability,
) -> Result<ListProviderCapabilityRouteOptionsResponse, CommandErrorPayload> {
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
        Arc<DesktopProviderCapabilityRouteStore>,
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
