use std::collections::{BTreeMap, BTreeSet, HashMap};

use harness_contracts::{
    CapabilityRouteKind, DeferPolicy, ExecutionDefaultsRecord, RuntimeExecutionStatus,
    ToolDescriptor, ToolGroup, ToolOrigin, ToolProfile, ToolRuntimeSettings,
};
use harness_tool::{
    effective_tool_runtime_settings, validate_tool_runtime_settings, ToolPoolFilter,
};

use super::{
    CommandErrorPayload, DesktopRuntimeState, ListRuntimeToolsResponse, ManagedDesktopRuntime,
    ResetRuntimeToolConfigRequest, RuntimeToolServiceBindingSummary, RuntimeToolSummary,
    SetRuntimeToolEnabledRequest, SettingsScope, UpdateRuntimeToolConfigRequest,
};

const EMPTY_TOOL_ALLOWLIST_SENTINEL: &str = "__jyowo_no_runtime_tools__";

pub fn get_runtime_execution_status_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<RuntimeExecutionStatus, CommandErrorPayload> {
    let runtime = settings_runtime(state)?;
    Ok(runtime.runtime_execution_status())
}

#[tauri::command]
pub async fn get_runtime_execution_status(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<RuntimeExecutionStatus, CommandErrorPayload> {
    let state = runtime_handle.read().await;
    get_runtime_execution_status_with_runtime_state(&state)
}

pub fn list_runtime_tools_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload> {
    let runtime = settings_runtime(state)?;
    let snapshot = runtime.tool_registry().snapshot();
    let generation = snapshot.generation();
    let (execution_defaults, scope, profile_customized, configuration_customized) =
        effective_execution_defaults(state)?;
    let tool_profile = execution_defaults.tool_profile.clone();
    let filter = ToolPoolFilter::from_profile(&tool_profile);
    let execution_status = runtime.runtime_execution_status();
    let tool_statuses = execution_status
        .tools
        .iter()
        .map(|status| (status.tool_name.as_str(), status))
        .collect::<HashMap<_, _>>();
    let mut descriptors = snapshot
        .as_descriptors()
        .into_iter()
        .cloned()
        .map(|descriptor| (descriptor.name.clone(), descriptor))
        .collect::<BTreeMap<_, _>>();
    for descriptor in runtime.runtime_appended_tool_descriptors() {
        descriptors
            .entry(descriptor.name.clone())
            .or_insert(descriptor);
    }
    let mut tools = descriptors
        .values()
        .map(|descriptor| {
            let configured_enabled = filter.allows_descriptor(&descriptor);
            let (available, unavailable_reason) = runtime_tool_availability(
                &descriptor,
                &execution_defaults,
                &execution_status,
                &tool_statuses,
            );
            let configured_settings = execution_defaults.tool_settings.get(&descriptor.name);
            runtime_tool_summary_from_descriptor(
                &descriptor,
                configured_enabled,
                available,
                unavailable_reason,
                configured_settings,
                configuration_customized.contains(&descriptor.name),
            )
        })
        .collect::<Vec<_>>();

    tools.sort_by(|left, right| {
        left.group_label
            .cmp(&right.group_label)
            .then_with(|| left.display_name.cmp(&right.display_name))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(ListRuntimeToolsResponse {
        generation,
        scope,
        customized: profile_customized || !configuration_customized.is_empty(),
        tools,
    })
}

#[tauri::command]
pub async fn list_runtime_tools(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload> {
    let state = runtime_handle.read().await;
    list_runtime_tools_with_runtime_state(&state)
}

pub fn set_runtime_tool_enabled_with_runtime_state(
    state: &DesktopRuntimeState,
    request: SetRuntimeToolEnabledRequest,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload> {
    let runtime = settings_runtime(state)?;
    let descriptors = runtime_tool_catalog(&runtime);
    if !descriptors
        .iter()
        .any(|descriptor| descriptor.name == request.name)
    {
        return Err(super::invalid_payload(format!(
            "runtime tool `{}` is not registered",
            request.name
        )));
    }

    let (execution_defaults, _, _, _) = effective_execution_defaults(state)?;
    let next_profile =
        tool_profile_with_override(&execution_defaults.tool_profile, &descriptors, &request);
    save_tool_profile(state, Some(next_profile))?;
    list_runtime_tools_with_runtime_state(state)
}

fn tool_profile_with_override(
    profile: &ToolProfile,
    descriptors: &[ToolDescriptor],
    request: &SetRuntimeToolEnabledRequest,
) -> ToolProfile {
    if let ToolProfile::Full = profile {
        let denylist = (!request.enabled)
            .then(|| BTreeSet::from([request.name.clone()]))
            .unwrap_or_default();
        return unrestricted_custom_profile(BTreeSet::new(), denylist);
    }

    if let ToolProfile::Custom {
        allowlist,
        denylist,
        group_allowlist,
        group_denylist,
        mcp_included,
        plugin_included,
    } = profile
    {
        if allowlist.is_empty()
            && group_allowlist.is_empty()
            && group_denylist.is_empty()
            && *mcp_included
            && *plugin_included
        {
            let mut next_denylist = denylist.clone();
            if request.enabled {
                next_denylist.remove(&request.name);
            } else {
                next_denylist.insert(request.name.clone());
            }
            return unrestricted_custom_profile(BTreeSet::new(), next_denylist);
        }
    }

    let filter = ToolPoolFilter::from_profile(profile);
    let mut allowlist = descriptors
        .iter()
        .filter(|descriptor| filter.allows_descriptor(descriptor))
        .map(|descriptor| descriptor.name.clone())
        .collect::<BTreeSet<_>>();
    if request.enabled {
        allowlist.insert(request.name.clone());
    } else {
        allowlist.remove(&request.name);
    }
    if allowlist.is_empty() {
        allowlist.insert(EMPTY_TOOL_ALLOWLIST_SENTINEL.to_owned());
    }
    unrestricted_custom_profile(allowlist, BTreeSet::new())
}

fn unrestricted_custom_profile(
    allowlist: BTreeSet<String>,
    denylist: BTreeSet<String>,
) -> ToolProfile {
    ToolProfile::Custom {
        allowlist,
        denylist,
        group_allowlist: Vec::new(),
        group_denylist: Vec::new(),
        mcp_included: true,
        plugin_included: true,
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_runtime_tool_enabled(
    name: String,
    enabled: bool,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload> {
    let state = runtime_handle.read().await;
    let _settings_guard = state.execution_settings_lock.lock().await;
    set_runtime_tool_enabled_with_runtime_state(
        &state,
        SetRuntimeToolEnabledRequest { name, enabled },
    )
}

pub fn update_runtime_tool_config_with_runtime_state(
    state: &DesktopRuntimeState,
    request: UpdateRuntimeToolConfigRequest,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload> {
    let runtime = settings_runtime(state)?;
    let descriptors = runtime_tool_catalog(&runtime);
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor.name == request.name)
        .ok_or_else(|| {
            super::invalid_payload(format!("runtime tool `{}` is not registered", request.name))
        })?;
    let settings = ToolRuntimeSettings {
        timeout_ms: request.timeout_ms,
        parameters: request.parameters,
    };
    validate_tool_runtime_settings(descriptor, &settings).map_err(super::invalid_payload)?;
    save_tool_settings(state, &request.name, Some(settings))?;
    list_runtime_tools_with_runtime_state(state)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_runtime_tool_config(
    name: String,
    timeout_ms: u64,
    parameters: serde_json::Value,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload> {
    let state = runtime_handle.read().await;
    let _settings_guard = state.execution_settings_lock.lock().await;
    update_runtime_tool_config_with_runtime_state(
        &state,
        UpdateRuntimeToolConfigRequest {
            name,
            timeout_ms,
            parameters,
        },
    )
}

pub fn reset_runtime_tool_config_with_runtime_state(
    state: &DesktopRuntimeState,
    request: ResetRuntimeToolConfigRequest,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload> {
    let runtime = settings_runtime(state)?;
    if !runtime_tool_catalog(&runtime)
        .iter()
        .any(|descriptor| descriptor.name == request.name)
    {
        return Err(super::invalid_payload(format!(
            "runtime tool `{}` is not registered",
            request.name
        )));
    }
    save_tool_settings(state, &request.name, None)?;
    list_runtime_tools_with_runtime_state(state)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn reset_runtime_tool_config(
    name: String,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload> {
    let state = runtime_handle.read().await;
    let _settings_guard = state.execution_settings_lock.lock().await;
    reset_runtime_tool_config_with_runtime_state(&state, ResetRuntimeToolConfigRequest { name })
}

pub fn reset_runtime_tools_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload> {
    settings_runtime(state)?;
    if let Some(project_store) = &state.project_config_store {
        let mut overrides = project_store.load_execution_overrides()?;
        overrides.tool_profile = None;
        overrides.tool_settings.clear();
        project_store.save_execution_overrides(&overrides)?;
    } else {
        let global_store =
            state
                .global_config_store
                .as_ref()
                .ok_or_else(|| CommandErrorPayload {
                    code: "RUNTIME_NOT_READY",
                    message: "global configuration store is not initialized".to_owned(),
                })?;
        let mut defaults = global_store.load_execution_defaults()?;
        defaults.tool_profile = ToolProfile::Full;
        defaults.tool_settings.clear();
        global_store.save_execution_defaults(&defaults)?;
    }
    list_runtime_tools_with_runtime_state(state)
}

#[tauri::command]
pub async fn reset_runtime_tools(
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload> {
    let state = runtime_handle.read().await;
    let _settings_guard = state.execution_settings_lock.lock().await;
    reset_runtime_tools_with_runtime_state(&state)
}

fn settings_runtime(
    state: &DesktopRuntimeState,
) -> Result<std::sync::Arc<jyowo_harness_sdk::DesktopSettingsRuntime>, CommandErrorPayload> {
    state.settings_runtime().ok_or_else(|| CommandErrorPayload {
        code: "RUNTIME_NOT_READY",
        message: "desktop settings runtime is not initialized".to_owned(),
    })
}

fn effective_execution_defaults(
    state: &DesktopRuntimeState,
) -> Result<
    (
        ExecutionDefaultsRecord,
        SettingsScope,
        bool,
        BTreeSet<String>,
    ),
    CommandErrorPayload,
> {
    let mut defaults = state
        .global_config_store
        .as_ref()
        .map(|store| store.load_execution_defaults())
        .transpose()?
        .unwrap_or_default();

    if let Some(project_store) = &state.project_config_store {
        let overrides = project_store.load_execution_overrides()?;
        let profile_customized = overrides.tool_profile.is_some();
        let configuration_customized = overrides.tool_settings.keys().cloned().collect();
        if let Some(value) = overrides.tool_profile {
            defaults.tool_profile = value;
        }
        defaults.tool_settings.extend(overrides.tool_settings);
        if let Some(value) = overrides.subagents_enabled {
            defaults.subagents_enabled = value;
        }
        if let Some(value) = overrides.agent_teams_enabled {
            defaults.agent_teams_enabled = value;
        }
        if let Some(value) = overrides.background_agents_enabled {
            defaults.background_agents_enabled = value;
        }
        return Ok((
            defaults,
            SettingsScope::Project,
            profile_customized,
            configuration_customized,
        ));
    }

    let profile_customized = defaults.tool_profile != ToolProfile::Full;
    let configuration_customized = defaults.tool_settings.keys().cloned().collect();
    Ok((
        defaults,
        SettingsScope::Global,
        profile_customized,
        configuration_customized,
    ))
}

fn save_tool_profile(
    state: &DesktopRuntimeState,
    profile: Option<ToolProfile>,
) -> Result<(), CommandErrorPayload> {
    if let Some(project_store) = &state.project_config_store {
        let mut overrides = project_store.load_execution_overrides()?;
        overrides.tool_profile = profile;
        return project_store.save_execution_overrides(&overrides);
    }

    let global_store = state
        .global_config_store
        .as_ref()
        .ok_or_else(|| CommandErrorPayload {
            code: "RUNTIME_NOT_READY",
            message: "global configuration store is not initialized".to_owned(),
        })?;
    let mut defaults = global_store.load_execution_defaults()?;
    defaults.tool_profile = profile.unwrap_or(ToolProfile::Full);
    global_store.save_execution_defaults(&defaults)
}

fn save_tool_settings(
    state: &DesktopRuntimeState,
    name: &str,
    settings: Option<ToolRuntimeSettings>,
) -> Result<(), CommandErrorPayload> {
    if let Some(project_store) = &state.project_config_store {
        let mut overrides = project_store.load_execution_overrides()?;
        match settings {
            Some(settings) => {
                overrides.tool_settings.insert(name.to_owned(), settings);
            }
            None => {
                overrides.tool_settings.remove(name);
            }
        }
        return project_store.save_execution_overrides(&overrides);
    }

    let global_store = state
        .global_config_store
        .as_ref()
        .ok_or_else(|| CommandErrorPayload {
            code: "RUNTIME_NOT_READY",
            message: "global configuration store is not initialized".to_owned(),
        })?;
    let mut defaults = global_store.load_execution_defaults()?;
    match settings {
        Some(settings) => {
            defaults.tool_settings.insert(name.to_owned(), settings);
        }
        None => {
            defaults.tool_settings.remove(name);
        }
    }
    global_store.save_execution_defaults(&defaults)
}

fn runtime_tool_catalog(
    runtime: &jyowo_harness_sdk::DesktopSettingsRuntime,
) -> Vec<ToolDescriptor> {
    let mut descriptors = runtime
        .tool_registry()
        .snapshot()
        .as_descriptors()
        .into_iter()
        .cloned()
        .map(|descriptor| (descriptor.name.clone(), descriptor))
        .collect::<BTreeMap<_, _>>();
    for descriptor in runtime.runtime_appended_tool_descriptors() {
        descriptors
            .entry(descriptor.name.clone())
            .or_insert(descriptor);
    }
    descriptors.into_values().collect()
}

fn runtime_tool_availability(
    descriptor: &ToolDescriptor,
    execution_defaults: &ExecutionDefaultsRecord,
    execution_status: &RuntimeExecutionStatus,
    tool_statuses: &HashMap<&str, &harness_contracts::ToolRuntimeStatus>,
) -> (bool, Option<String>) {
    let enabled = match descriptor.name.as_str() {
        "agent" => execution_defaults.subagents_enabled,
        "agent_team" => {
            execution_defaults.subagents_enabled && execution_defaults.agent_teams_enabled
        }
        "background_agent" => {
            execution_defaults.subagents_enabled && execution_defaults.background_agents_enabled
        }
        _ => true,
    };
    if !enabled {
        return (
            false,
            Some("Required Agent capability is disabled in execution settings".to_owned()),
        );
    }
    if let Some(status) = tool_statuses.get(descriptor.name.as_str()) {
        return (status.available, status.unavailable_reason.clone());
    }

    if runtime_tool_execution_channel(descriptor) == "httpBroker"
        && !execution_status.http_broker.available
    {
        return (
            false,
            execution_status.http_broker.denied_reasons.first().cloned(),
        );
    }

    // The registry snapshot only contains materialized tools. Channels with a
    // dedicated preflight override this value above; the remaining registered
    // descriptors have no additional runtime prerequisite to report here.
    (true, None)
}

fn runtime_tool_summary_from_descriptor(
    descriptor: &ToolDescriptor,
    configured_enabled: bool,
    available: bool,
    unavailable_reason: Option<String>,
    configured_settings: Option<&ToolRuntimeSettings>,
    configuration_customized: bool,
) -> RuntimeToolSummary {
    let (origin_kind, origin_id) = runtime_tool_origin(&descriptor.origin);
    let defaults = effective_tool_runtime_settings(descriptor, None);
    let effective = effective_tool_runtime_settings(descriptor, configured_settings);

    RuntimeToolSummary {
        name: descriptor.name.clone(),
        display_name: descriptor.display_name.clone(),
        description: descriptor.description.clone(),
        category: descriptor.category.clone(),
        group: runtime_tool_group(&descriptor.group),
        group_label: runtime_tool_group_label(&descriptor.group),
        origin_kind,
        origin_id,
        access: runtime_tool_access(descriptor),
        execution_channel: runtime_tool_execution_channel(descriptor),
        required_capabilities: descriptor
            .required_capabilities
            .iter()
            .map(ToString::to_string)
            .collect(),
        defer_policy: runtime_tool_defer_policy(descriptor.properties.defer_policy),
        long_running: descriptor.properties.long_running.is_some(),
        service_binding: descriptor.service_binding.as_ref().map(|binding| {
            RuntimeToolServiceBindingSummary {
                provider_id: binding.provider_id.clone(),
                operation_id: binding.operation_id.clone(),
                route_kind: runtime_tool_route_kind(binding.route_kind),
            }
        }),
        configured_enabled,
        available,
        unavailable_reason,
        default_timeout_ms: defaults.timeout_ms,
        timeout_ms: effective.timeout_ms,
        configuration_schema: descriptor
            .metadata
            .configuration
            .as_ref()
            .map(|configuration| configuration.schema.clone()),
        default_parameters: defaults.parameters,
        parameters: effective.parameters,
        configuration_customized,
    }
}

fn runtime_tool_access(descriptor: &ToolDescriptor) -> String {
    if descriptor.properties.is_destructive {
        "destructive".to_owned()
    } else if descriptor.properties.is_read_only {
        "readOnly".to_owned()
    } else {
        "mutating".to_owned()
    }
}

fn runtime_tool_origin(origin: &ToolOrigin) -> (String, Option<String>) {
    match origin {
        ToolOrigin::Builtin => ("builtin".to_owned(), None),
        ToolOrigin::Plugin { plugin_id, .. } => ("plugin".to_owned(), Some(plugin_id.0.clone())),
        ToolOrigin::Mcp(mcp) => ("mcp".to_owned(), Some(mcp.server_id.0.clone())),
        ToolOrigin::Skill(skill) => ("skill".to_owned(), Some(skill.skill_id.0.clone())),
        _ => ("custom".to_owned(), None),
    }
}

fn runtime_tool_execution_channel(descriptor: &ToolDescriptor) -> String {
    if matches!(
        descriptor.origin,
        ToolOrigin::Plugin { .. } | ToolOrigin::Mcp(_) | ToolOrigin::Skill(_)
    ) {
        return "externalCapability".to_owned();
    }
    if descriptor.service_binding.is_some() {
        return "httpBroker".to_owned();
    }
    if descriptor.name == "WebFetch" || descriptor.name.starts_with("MiniMax") {
        return "httpBroker".to_owned();
    }
    if descriptor.required_capabilities.iter().any(|capability| {
        capability.to_string() == "custom:jyowo.builtin.brokered_platform_runtime"
    }) {
        return "externalCapability".to_owned();
    }
    match descriptor.name.as_str() {
        "Bash" | "Diagnostics" | "ProcessStart" | "ExecuteCode" => "processSandbox".to_owned(),
        "SendMessage" | "WebSearch" => "externalCapability".to_owned(),
        _ => "directAuthorizedRust".to_owned(),
    }
}

fn runtime_tool_defer_policy(policy: DeferPolicy) -> String {
    match policy {
        DeferPolicy::AlwaysLoad => "alwaysLoad".to_owned(),
        DeferPolicy::AutoDefer => "autoDefer".to_owned(),
        DeferPolicy::ForceDefer => "forceDefer".to_owned(),
        _ => "autoDefer".to_owned(),
    }
}

fn runtime_tool_route_kind(kind: CapabilityRouteKind) -> String {
    match kind {
        CapabilityRouteKind::ImageGeneration => "imageGeneration".to_owned(),
        CapabilityRouteKind::VideoGeneration => "videoGeneration".to_owned(),
        CapabilityRouteKind::ThreeDGeneration => "threeDGeneration".to_owned(),
        CapabilityRouteKind::EmbeddingGeneration => "embeddingGeneration".to_owned(),
        CapabilityRouteKind::FileOperation => "fileOperation".to_owned(),
        CapabilityRouteKind::TextToSpeech => "textToSpeech".to_owned(),
        CapabilityRouteKind::SpeechToText => "speechToText".to_owned(),
        CapabilityRouteKind::MusicGeneration => "musicGeneration".to_owned(),
        CapabilityRouteKind::Moderation => "moderation".to_owned(),
        CapabilityRouteKind::FileManagement => "fileManagement".to_owned(),
        CapabilityRouteKind::VectorStoreManagement => "vectorStoreManagement".to_owned(),
        CapabilityRouteKind::BatchJob => "batchJob".to_owned(),
        CapabilityRouteKind::FineTuningJob => "fineTuningJob".to_owned(),
        CapabilityRouteKind::EvalRun => "evalRun".to_owned(),
        CapabilityRouteKind::ContainerSession => "containerSession".to_owned(),
        CapabilityRouteKind::RealtimeSession => "realtimeSession".to_owned(),
        CapabilityRouteKind::AdminOperation => "adminOperation".to_owned(),
        CapabilityRouteKind::WebhookVerification => "webhookVerification".to_owned(),
    }
}

fn runtime_tool_group(group: &ToolGroup) -> String {
    match group {
        ToolGroup::FileSystem => "fileSystem".to_owned(),
        ToolGroup::Search => "search".to_owned(),
        ToolGroup::Network => "network".to_owned(),
        ToolGroup::Shell => "shell".to_owned(),
        ToolGroup::Git => "git".to_owned(),
        ToolGroup::Worktree => "worktree".to_owned(),
        ToolGroup::Session => "session".to_owned(),
        ToolGroup::Artifact => "artifact".to_owned(),
        ToolGroup::Browser => "browser".to_owned(),
        ToolGroup::Computer => "computer".to_owned(),
        ToolGroup::Image => "image".to_owned(),
        ToolGroup::Notebook => "notebook".to_owned(),
        ToolGroup::Lsp => "lsp".to_owned(),
        ToolGroup::Automation => "automation".to_owned(),
        ToolGroup::Workflow => "workflow".to_owned(),
        ToolGroup::Agent => "agent".to_owned(),
        ToolGroup::Coordinator => "coordinator".to_owned(),
        ToolGroup::Memory => "memory".to_owned(),
        ToolGroup::Clarification => "clarification".to_owned(),
        ToolGroup::Meta => "meta".to_owned(),
        ToolGroup::Custom(value) => value.clone(),
        _ => "custom".to_owned(),
    }
}

fn runtime_tool_group_label(group: &ToolGroup) -> String {
    match group {
        ToolGroup::FileSystem => "File system".to_owned(),
        ToolGroup::Search => "Search".to_owned(),
        ToolGroup::Network => "Network".to_owned(),
        ToolGroup::Shell => "Shell".to_owned(),
        ToolGroup::Git => "Git".to_owned(),
        ToolGroup::Worktree => "Worktree".to_owned(),
        ToolGroup::Session => "Session".to_owned(),
        ToolGroup::Artifact => "Artifact".to_owned(),
        ToolGroup::Browser => "Browser".to_owned(),
        ToolGroup::Computer => "Computer".to_owned(),
        ToolGroup::Image => "Image".to_owned(),
        ToolGroup::Notebook => "Notebook".to_owned(),
        ToolGroup::Lsp => "LSP".to_owned(),
        ToolGroup::Automation => "Automation".to_owned(),
        ToolGroup::Workflow => "Workflow".to_owned(),
        ToolGroup::Agent => "Agent".to_owned(),
        ToolGroup::Coordinator => "Coordinator".to_owned(),
        ToolGroup::Memory => "Memory".to_owned(),
        ToolGroup::Clarification => "Clarification".to_owned(),
        ToolGroup::Meta => "Meta".to_owned(),
        ToolGroup::Custom(value) => value.clone(),
        _ => "Custom".to_owned(),
    }
}
