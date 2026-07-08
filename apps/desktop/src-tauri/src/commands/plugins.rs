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
use super::providers::*;
#[allow(unused_imports)]
use super::runtime::*;
#[allow(unused_imports)]
use super::skills::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;
use harness_contracts::PluginSelectionRecord;
use std::{collections::BTreeSet, path::Path};

pub async fn list_plugins_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListPluginsResponse, CommandErrorPayload> {
    let settings = state.plugin_store.load_record()?;
    let selection = project_plugin_selection_for_state(state)?;
    let allow_project_plugins = selection
        .as_ref()
        .map(|selection| selection.allow_project_plugins)
        .unwrap_or(settings.allow_project_plugins);
    if let Some(harness) = state.harness() {
        if let Some(registry) = harness.plugin_registry() {
            registry.discover().await.map_err(|error| {
                runtime_operation_failed(format!("plugin discovery failed: {error}"))
            })?;
            return Ok(ListPluginsResponse {
                allow_project_plugins,
                plugins: registry.product_snapshot(),
            });
        }
    }

    let global_plugin_store = global_plugin_store_for_project_state(state);
    let registry = build_plugin_registry(
        state.conversation_cwd(),
        state.project_workspace_root(),
        state.plugin_store.as_ref(),
        global_plugin_store.as_ref(),
    )?;
    registry
        .discover()
        .await
        .map_err(|error| runtime_operation_failed(format!("plugin discovery failed: {error}")))?;
    Ok(ListPluginsResponse {
        allow_project_plugins,
        plugins: registry.product_snapshot(),
    })
}

fn global_plugin_store_for_project_state(
    state: &DesktopRuntimeState,
) -> Option<DesktopPluginStore> {
    state.project_workspace_root()?;
    state
        .global_config_store
        .as_ref()
        .map(|store| DesktopPluginStore::global(store.layout().clone()))
}

fn project_plugin_selection_for_state(
    state: &DesktopRuntimeState,
) -> Result<Option<PluginSelectionRecord>, CommandErrorPayload> {
    state
        .project_config_store
        .as_ref()
        .map(ProjectConfigStore::load_project_plugin_selection_if_present)
        .transpose()
        .map(Option::flatten)
}

fn save_project_plugin_selection_for_state(
    state: &DesktopRuntimeState,
    selection: &PluginSelectionRecord,
) -> Result<(), CommandErrorPayload> {
    let project_config = state.project_config_store.as_ref().ok_or_else(|| {
        runtime_operation_failed("project plugin selection config store is unavailable".to_owned())
    })?;
    project_config.save_project_plugin_selection(selection)
}

pub async fn get_plugin_detail_with_runtime_state(
    request: GetPluginDetailRequest,
    state: &DesktopRuntimeState,
) -> Result<GetPluginDetailResponse, CommandErrorPayload> {
    if let Some(harness) = state.harness() {
        if let Some(registry) = harness.plugin_registry() {
            registry.discover().await.map_err(|error| {
                runtime_operation_failed(format!("plugin discovery failed: {error}"))
            })?;
            let plugin = registry
                .product_detail(&request.plugin_id)
                .map(redact_plugin_detail_config)
                .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?;
            return Ok(GetPluginDetailResponse { plugin });
        }
    }

    let global_plugin_store = global_plugin_store_for_project_state(state);
    let registry = build_plugin_registry(
        state.conversation_cwd(),
        state.project_workspace_root(),
        state.plugin_store.as_ref(),
        global_plugin_store.as_ref(),
    )?;
    registry
        .discover()
        .await
        .map_err(|error| runtime_operation_failed(format!("plugin discovery failed: {error}")))?;
    let plugin = registry
        .product_detail(&request.plugin_id)
        .map(redact_plugin_detail_config)
        .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?;
    Ok(GetPluginDetailResponse { plugin })
}

pub async fn validate_plugin_from_path_with_runtime_state(
    request: ValidatePluginFromPathRequest,
    _state: &DesktopRuntimeState,
) -> Result<PluginInstallReport, CommandErrorPayload> {
    let source_path = ensure_plugin_source_path(&request.source_path)?;
    validate_plugin_source_path(&source_path).await
}

pub async fn install_plugin_from_path_with_runtime_state(
    request: InstallPluginFromPathRequest,
    state: &DesktopRuntimeState,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let source_path = ensure_plugin_source_path(&request.source_path)?;
    let report = validate_plugin_source_path(&source_path).await?;
    let Some(summary) = report.summary.clone() else {
        return Ok(PluginOperationResult {
            plugin_id: None,
            status: PluginOperationStatus::Rejected,
            summary: None,
            report: Some(report),
        });
    };
    if !report.valid {
        return Ok(PluginOperationResult {
            plugin_id: Some(summary.id.clone()),
            status: PluginOperationStatus::Rejected,
            summary: Some(summary),
            report: Some(report),
        });
    }

    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    let mut settings = state.plugin_store.load_record()?;
    let previous_settings = settings.clone();
    if settings
        .records
        .iter()
        .any(|record| record.name == summary.name || record.plugin_id == summary.id)
    {
        return Ok(PluginOperationResult {
            plugin_id: Some(summary.id.clone()),
            status: PluginOperationStatus::Rejected,
            summary: Some(summary.clone()),
            report: Some(PluginInstallReport {
                source_path: plugin_report_source_path(&source_path),
                valid: false,
                summary: Some(summary),
                warnings: Vec::new(),
                reason: Some("plugin is already installed".to_owned()),
            }),
        });
    }

    let package_dir = plugin_package_dir_name(&summary.id);
    let installed_hash = state
        .plugin_store
        .write_plugin_package(&package_dir, &source_path)?;
    let installed_package = state.plugin_store.package_root().join(&package_dir);
    let installed_report = validate_plugin_source_path(&installed_package).await?;
    let installed_summary = installed_report.summary.as_ref();
    let installed_matches_source = installed_report.valid
        && installed_summary.is_some_and(|installed_summary| {
            installed_summary.id == summary.id
                && installed_summary.name == summary.name
                && installed_summary.version == summary.version
        });
    if !installed_matches_source {
        let _ = state.plugin_store.delete_plugin_package(&package_dir);
        return Ok(PluginOperationResult {
            plugin_id: Some(summary.id.clone()),
            status: PluginOperationStatus::Rejected,
            summary: Some(summary.clone()),
            report: Some(PluginInstallReport {
                source_path: plugin_report_source_path(&source_path),
                valid: false,
                summary: Some(summary),
                warnings: Vec::new(),
                reason: Some(installed_report.reason.unwrap_or_else(|| {
                    "installed plugin package did not match validation".to_owned()
                })),
            }),
        });
    }
    let now = now().to_rfc3339();
    settings.records.push(PluginStoreRecord {
        plugin_id: summary.id.clone(),
        name: summary.name.clone(),
        version: summary.version.clone(),
        enabled: false,
        package_dir: package_dir.clone(),
        source_path: source_path.display().to_string(),
        content_hash: installed_hash,
        imported_at: now.clone(),
        updated_at: now,
        config: Value::Null,
        last_validation_error: None,
    });
    settings.records.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.version.cmp(&right.version))
            .then(left.plugin_id.cmp(&right.plugin_id))
    });
    if let Err(error) = state.plugin_store.save_record(&settings) {
        let _ = state.plugin_store.delete_plugin_package(&package_dir);
        return Err(error);
    }
    if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
        let _ = state.plugin_store.save_record(&previous_settings);
        let _ = state.plugin_store.delete_plugin_package(&package_dir);
        return Err(error);
    }

    Ok(PluginOperationResult {
        plugin_id: Some(summary.id.clone()),
        status: PluginOperationStatus::Installed,
        summary: Some(summary),
        report: Some(report),
    })
}

pub async fn set_plugin_enabled_with_runtime_state(
    request: SetPluginEnabledRequest,
    state: &DesktopRuntimeState,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    if let Some(mut selection) = project_plugin_selection_for_state(state)? {
        let previous_selection = selection.clone();
        let settings = state.plugin_store.load_record()?;
        let global_plugin_store = global_plugin_store_for_project_state(state);
        let global_settings = global_plugin_store
            .as_ref()
            .map(PluginStore::load_record)
            .transpose()?;
        let project_installed = settings
            .records
            .iter()
            .any(|record| record.plugin_id == request.plugin_id);
        let global_record = global_settings
            .iter()
            .flat_map(|settings| settings.records.iter())
            .find(|record| record.plugin_id == request.plugin_id);
        if !project_installed && global_record.is_none() {
            return Err(invalid_payload("plugin not found".to_owned()));
        }
        if request.enabled
            && !project_installed
            && global_record.is_some_and(|record| !record.enabled)
        {
            return Err(invalid_payload("plugin is disabled globally".to_owned()));
        }
        if request.enabled {
            if !selection
                .enabled
                .iter()
                .any(|id| id == &request.plugin_id.0)
            {
                selection.enabled.push(request.plugin_id.0.clone());
                selection.enabled.sort();
            }
        } else {
            selection.enabled.retain(|id| id != &request.plugin_id.0);
        }
        save_project_plugin_selection_for_state(state, &selection)?;
        if request.enabled {
            if let Err(error) = preflight_plugin_activation(state, &request.plugin_id).await {
                let _ = save_project_plugin_selection_for_state(state, &previous_selection);
                return Err(error);
            }
        }
        if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
            let _ = save_project_plugin_selection_for_state(state, &previous_selection);
            return Err(error);
        }
        let summary = plugin_summary_after_reload(state, &request.plugin_id).await?;
        return Ok(PluginOperationResult {
            plugin_id: Some(request.plugin_id),
            status: if request.enabled {
                PluginOperationStatus::Enabled
            } else {
                PluginOperationStatus::Disabled
            },
            summary,
            report: None,
        });
    }
    let mut settings = state.plugin_store.load_record()?;
    let previous_settings = settings.clone();
    let record = settings
        .records
        .iter_mut()
        .find(|record| record.plugin_id == request.plugin_id)
        .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?;
    record.enabled = request.enabled;
    record.updated_at = now().to_rfc3339();
    state.plugin_store.save_record(&settings)?;
    if request.enabled {
        if let Err(error) = preflight_plugin_activation(state, &request.plugin_id).await {
            let _ = state.plugin_store.save_record(&previous_settings);
            return Err(error);
        }
    }
    if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
        let _ = state.plugin_store.save_record(&previous_settings);
        return Err(error);
    }
    let summary = plugin_summary_after_reload(state, &request.plugin_id).await?;
    Ok(PluginOperationResult {
        plugin_id: Some(request.plugin_id),
        status: if request.enabled {
            PluginOperationStatus::Enabled
        } else {
            PluginOperationStatus::Disabled
        },
        summary,
        report: None,
    })
}

pub async fn set_project_plugins_enabled_with_runtime_state(
    request: SetProjectPluginsEnabledRequest,
    state: &DesktopRuntimeState,
) -> Result<SetProjectPluginsEnabledResponse, CommandErrorPayload> {
    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    if let Some(mut selection) = project_plugin_selection_for_state(state)? {
        let previous_selection = selection.clone();
        selection.allow_project_plugins = request.enabled;
        save_project_plugin_selection_for_state(state, &selection)?;
        if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
            let _ = save_project_plugin_selection_for_state(state, &previous_selection);
            return Err(error);
        }
        return Ok(SetProjectPluginsEnabledResponse {
            allow_project_plugins: selection.allow_project_plugins,
        });
    }
    let mut settings = state.plugin_store.load_record()?;
    let previous_settings = settings.clone();
    settings.allow_project_plugins = request.enabled;
    state.plugin_store.save_record(&settings)?;
    if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
        let _ = state.plugin_store.save_record(&previous_settings);
        return Err(error);
    }
    Ok(SetProjectPluginsEnabledResponse {
        allow_project_plugins: settings.allow_project_plugins,
    })
}

pub async fn update_plugin_config_with_runtime_state(
    request: UpdatePluginConfigRequest,
    state: &DesktopRuntimeState,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    let global_plugin_store = global_plugin_store_for_project_state(state);
    let mut settings = state.plugin_store.load_record()?;
    let previous_settings = settings.clone();
    let mut global_settings = global_plugin_store
        .as_ref()
        .map(PluginStore::load_record)
        .transpose()?;
    let owner = if let Some(index) = settings
        .records
        .iter()
        .position(|record| record.plugin_id == request.plugin_id)
    {
        PluginConfigOwner::Project(index)
    } else if let Some((index, _)) = global_settings.as_ref().and_then(|settings| {
        settings
            .records
            .iter()
            .enumerate()
            .find(|(_, record)| record.plugin_id == request.plugin_id)
    }) {
        PluginConfigOwner::Global(index)
    } else {
        return Err(invalid_payload("plugin not found".to_owned()));
    };
    let previous_global_settings = global_settings.clone();
    let current_config = match &owner {
        PluginConfigOwner::Project(index) => settings.records[*index].config.clone(),
        PluginConfigOwner::Global(index) => global_settings
            .as_ref()
            .and_then(|settings| settings.records.get(*index))
            .map(|record| record.config.clone())
            .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?,
    };
    let record_enabled = match &owner {
        PluginConfigOwner::Project(index) => settings.records[*index].enabled,
        PluginConfigOwner::Global(index) => global_settings
            .as_ref()
            .and_then(|settings| settings.records.get(*index))
            .map(|record| record.enabled)
            .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?,
    };
    let selection = project_plugin_selection_for_state(state)?;
    let currently_enabled = selection
        .as_ref()
        .map(|selection| {
            selection
                .enabled
                .iter()
                .any(|id| id == &request.plugin_id.0)
                && (matches!(&owner, PluginConfigOwner::Project(_)) || record_enabled)
        })
        .unwrap_or(record_enabled);
    let registry = build_plugin_registry(
        state.conversation_cwd(),
        state.project_workspace_root(),
        state.plugin_store.as_ref(),
        global_plugin_store.as_ref(),
    )?;
    let discovered = registry
        .discover()
        .await
        .map_err(|error| runtime_operation_failed(format!("plugin discovery failed: {error}")))?;
    ensure_no_secret_like_config_values(&request.values)?;
    let private_schema = discovered
        .iter()
        .find(|plugin| plugin.record.manifest.plugin_id() == request.plugin_id)
        .and_then(|plugin| {
            plugin
                .record
                .manifest
                .capabilities
                .configuration_schema
                .as_ref()
        });
    if registry.product_detail(&request.plugin_id).is_none() {
        return Err(invalid_payload("plugin not found".to_owned()));
    }
    let merged_config =
        merge_plugin_config_values(private_schema, &current_config, request.values.clone());
    let validation_config = redact_secret_config_values(private_schema, merged_config.clone());
    registry
        .validate_config_update(&PluginConfigUpdate {
            plugin_id: request.plugin_id.clone(),
            values: validation_config,
        })
        .map_err(|error| invalid_payload(format!("plugin config rejected: {error}")))?;
    match owner {
        PluginConfigOwner::Project(index) => {
            settings.records[index].config = merged_config;
            settings.records[index].updated_at = now().to_rfc3339();
            state.plugin_store.save_record(&settings)?;
        }
        PluginConfigOwner::Global(index) => {
            let global_settings = global_settings
                .as_mut()
                .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?;
            global_settings.records[index].config = merged_config;
            global_settings.records[index].updated_at = now().to_rfc3339();
            let global_plugin_store = global_plugin_store.as_ref().ok_or_else(|| {
                runtime_operation_failed("global plugin store is unavailable".to_owned())
            })?;
            global_plugin_store.save_record(global_settings)?;
        }
    }
    if currently_enabled {
        if let Err(error) = preflight_plugin_activation(state, &request.plugin_id).await {
            let _ = state.plugin_store.save_record(&previous_settings);
            if let (Some(global_plugin_store), Some(previous_global_settings)) = (
                global_plugin_store.as_ref(),
                previous_global_settings.as_ref(),
            ) {
                let _ = global_plugin_store.save_record(previous_global_settings);
            }
            return Err(error);
        }
    }
    if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
        let _ = state.plugin_store.save_record(&previous_settings);
        if let (Some(global_plugin_store), Some(previous_global_settings)) = (
            global_plugin_store.as_ref(),
            previous_global_settings.as_ref(),
        ) {
            let _ = global_plugin_store.save_record(previous_global_settings);
        }
        return Err(error);
    }
    let summary = plugin_summary_after_reload(state, &request.plugin_id).await?;
    Ok(PluginOperationResult {
        plugin_id: Some(request.plugin_id),
        status: PluginOperationStatus::Configured,
        summary,
        report: None,
    })
}

enum PluginConfigOwner {
    Project(usize),
    Global(usize),
}

pub async fn uninstall_plugin_with_runtime_state(
    request: UninstallPluginRequest,
    state: &DesktopRuntimeState,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    let mut settings = state.plugin_store.load_record()?;
    let previous_settings = settings.clone();
    let previous_selection = project_plugin_selection_for_state(state)?;
    let original_len = settings.records.len();
    let mut package_dirs = Vec::new();
    settings.records.retain(|record| {
        if record.plugin_id == request.plugin_id {
            package_dirs.push(record.package_dir.clone());
            false
        } else {
            true
        }
    });
    if settings.records.len() == original_len {
        return Err(invalid_payload("plugin not found".to_owned()));
    }
    state.plugin_store.save_record(&settings)?;
    if let Some(mut selection) = previous_selection.clone() {
        selection.enabled.retain(|id| id != &request.plugin_id.0);
        if let Err(error) = save_project_plugin_selection_for_state(state, &selection) {
            let _ = state.plugin_store.save_record(&previous_settings);
            return Err(error);
        }
    }
    if let Err(error) = reload_desktop_harness_after_plugin_change_locked(state).await {
        let _ = state.plugin_store.save_record(&previous_settings);
        if let Some(selection) = previous_selection {
            let _ = save_project_plugin_selection_for_state(state, &selection);
        }
        return Err(error);
    }
    for package_dir in &package_dirs {
        if let Err(error) = state.plugin_store.delete_plugin_package(package_dir) {
            let _ = state.plugin_store.save_record(&previous_settings);
            let _ = reload_desktop_harness_after_plugin_change_locked(state).await;
            return Err(error);
        }
    }
    Ok(PluginOperationResult {
        plugin_id: Some(request.plugin_id),
        status: PluginOperationStatus::Uninstalled,
        summary: None,
        report: None,
    })
}

pub async fn reload_plugin_with_runtime_state(
    request: ReloadPluginRequest,
    state: &DesktopRuntimeState,
) -> Result<PluginOperationResult, CommandErrorPayload> {
    let _start_run_guard = state.start_run_lock.lock().await;
    let _plugin_store_guard = state.plugin_store_lock.lock().await;
    let settings = state.plugin_store.load_record()?;
    let selection = project_plugin_selection_for_state(state)?;
    let global_plugin_store = global_plugin_store_for_project_state(state);
    let global_settings = global_plugin_store
        .as_ref()
        .map(PluginStore::load_record)
        .transpose()?;
    let project_record = settings
        .records
        .iter()
        .find(|record| record.plugin_id == request.plugin_id);
    let global_record = global_settings
        .iter()
        .flat_map(|settings| settings.records.iter())
        .find(|record| record.plugin_id == request.plugin_id);
    let installed_record = project_record
        .or(global_record)
        .ok_or_else(|| invalid_payload("plugin not found".to_owned()))?;
    let enabled = selection
        .as_ref()
        .map(|selection| {
            selection
                .enabled
                .iter()
                .any(|id| id == &request.plugin_id.0)
                && (project_record.is_some() || global_record.is_some_and(|record| record.enabled))
        })
        .unwrap_or(installed_record.enabled);
    if enabled {
        preflight_plugin_activation(state, &request.plugin_id).await?;
    }
    reload_desktop_harness_after_plugin_change_locked(state).await?;
    let summary = plugin_summary_after_reload(state, &request.plugin_id).await?;
    Ok(PluginOperationResult {
        plugin_id: Some(request.plugin_id),
        status: PluginOperationStatus::Reloaded,
        summary,
        report: None,
    })
}

pub(crate) async fn preflight_plugin_activation(
    state: &DesktopRuntimeState,
    plugin_id: &PluginId,
) -> Result<(), CommandErrorPayload> {
    let global_plugin_store = global_plugin_store_for_project_state(state);
    let registry = build_plugin_registry(
        state.conversation_cwd(),
        state.project_workspace_root(),
        state.plugin_store.as_ref(),
        global_plugin_store.as_ref(),
    )?;
    let discovered = registry
        .discover()
        .await
        .map_err(|error| runtime_operation_failed(format!("plugin discovery failed: {error}")))?;
    if let Some(plugin) = discovered
        .iter()
        .find(|plugin| plugin.record.manifest.plugin_id() == *plugin_id)
    {
        if matches!(plugin.record.origin, ManifestOrigin::CargoExtension { .. }) {
            return Ok(());
        }
        return Err(invalid_payload(format!(
            "plugin cannot be enabled: {LOCAL_PLUGIN_SIDECAR_REQUIRED_REASON}"
        )));
    }
    let reason = registry
        .state_detail(plugin_id)
        .and_then(|detail| detail.rejection_reason)
        .map(|reason| plugin_rejection_report_reason(&reason))
        .unwrap_or_else(|| "plugin was not discovered".to_owned());
    Err(invalid_payload(format!(
        "plugin cannot be enabled: {reason}"
    )))
}

pub(crate) async fn plugin_summary_after_reload(
    state: &DesktopRuntimeState,
    plugin_id: &PluginId,
) -> Result<Option<PluginSummary>, CommandErrorPayload> {
    Ok(list_plugins_with_runtime_state(state)
        .await?
        .plugins
        .into_iter()
        .find(|plugin| &plugin.id == plugin_id))
}

pub(crate) async fn validate_plugin_source_path(
    source_path: &Path,
) -> Result<PluginInstallReport, CommandErrorPayload> {
    let loader = FileManifestLoader;
    let load_report = loader
        .load_package_report(source_path)
        .await
        .map_err(|error| {
            runtime_operation_failed(format!("plugin manifest load failed: {error}"))
        })?;
    if let Some(failure) = load_report.failures.first() {
        return Ok(PluginInstallReport {
            source_path: plugin_report_source_path(source_path),
            valid: false,
            summary: None,
            warnings: Vec::new(),
            reason: Some(plugin_manifest_validation_failure_report_reason(
                &failure.failure,
            )),
        });
    }
    let Some(record) = load_report.records.first() else {
        return Ok(PluginInstallReport {
            source_path: plugin_report_source_path(source_path),
            valid: false,
            summary: None,
            warnings: Vec::new(),
            reason: Some("plugin manifest not found".to_owned()),
        });
    };
    if record.manifest.trust_level != TrustLevel::UserControlled {
        return Ok(PluginInstallReport {
            source_path: plugin_report_source_path(source_path),
            valid: false,
            summary: None,
            warnings: Vec::new(),
            reason: Some("local user plugin must declare user_controlled trust".to_owned()),
        });
    }
    if !matches!(record.origin, ManifestOrigin::CargoExtension { .. }) {
        return Ok(PluginInstallReport {
            source_path: plugin_report_source_path(source_path),
            valid: false,
            summary: None,
            warnings: Vec::new(),
            reason: Some(LOCAL_PLUGIN_SIDECAR_REQUIRED_REASON.to_owned()),
        });
    }

    let registry = PluginRegistry::builder()
        .with_source(DiscoverySource::Inline)
        .with_manifest_loader(Arc::new(InlineManifestLoader::new(vec![record.clone()])))
        .build()
        .map_err(|error| runtime_operation_failed(format!("plugin registry failed: {error}")))?;
    let discovered = registry
        .discover()
        .await
        .map_err(|error| runtime_operation_failed(format!("plugin discovery failed: {error}")))?;
    let plugin_id = record.manifest.plugin_id();
    let summary = registry
        .product_snapshot()
        .into_iter()
        .find(|summary| summary.id == plugin_id);
    let valid = discovered
        .iter()
        .any(|plugin| plugin.record.manifest.plugin_id() == plugin_id);
    let reason = if valid {
        None
    } else {
        registry
            .state_detail(&plugin_id)
            .and_then(|detail| detail.rejection_reason)
            .map(|reason| plugin_rejection_report_reason(&reason))
            .or_else(|| Some("plugin rejected".to_owned()))
    };
    let warnings = summary
        .as_ref()
        .map(|summary| summary.warnings.clone())
        .unwrap_or_default();
    Ok(PluginInstallReport {
        source_path: plugin_report_source_path(source_path),
        valid,
        summary,
        warnings,
        reason,
    })
}

pub(crate) fn plugin_report_source_path(_source_path: &Path) -> String {
    PLUGIN_REPORT_SOURCE_PATH_WITHHELD.to_owned()
}

pub(crate) fn plugin_manifest_validation_failure_report_reason(
    failure: &harness_contracts::ManifestValidationFailure,
) -> String {
    match failure {
        harness_contracts::ManifestValidationFailure::RemoteIntegrityMismatch { .. } => {
            "plugin manifest integrity check failed".to_owned()
        }
        _ => "plugin manifest is invalid.".to_owned(),
    }
}

pub(crate) fn plugin_rejection_report_reason(reason: &RejectionReason) -> String {
    match reason {
        RejectionReason::SignatureInvalid { .. } => "plugin signature is invalid".to_owned(),
        RejectionReason::UnknownSigner { .. } => "plugin signer is unknown".to_owned(),
        RejectionReason::SignerRevoked { .. } => "plugin signer is revoked".to_owned(),
        RejectionReason::TrustMismatch { .. } => "plugin trust level is not allowed".to_owned(),
        RejectionReason::NamespaceConflict { .. } => "plugin namespace is not allowed".to_owned(),
        RejectionReason::DependencyUnsatisfied { .. } => {
            "plugin dependency is not satisfied".to_owned()
        }
        RejectionReason::DependencyCycle { .. } => "plugin dependency cycle detected".to_owned(),
        RejectionReason::HarnessVersionMismatch { .. } => {
            "plugin requires an mismatched harness version".to_owned()
        }
        RejectionReason::SlotOccupied { .. } => "plugin capability slot is occupied".to_owned(),
        RejectionReason::AdmissionDenied { .. } => "plugin rejected by policy".to_owned(),
        _ => "plugin rejected by policy".to_owned(),
    }
}

pub(crate) fn ensure_plugin_source_path(value: &str) -> Result<PathBuf, CommandErrorPayload> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(invalid_payload(
            "plugin source path must be absolute".to_owned(),
        ));
    }
    ensure_no_symlink_components(&path, "plugin source directory")?;
    let path = path.canonicalize().map_err(|error| {
        runtime_operation_failed(format!("plugin source path unavailable: {error}"))
    })?;
    ensure_no_symlink_components(&path, "plugin source directory")?;
    ensure_no_world_writable_ancestors(&path, "plugin source directory")?;
    ensure_not_world_writable_path(&path, "plugin source directory")?;
    if !path.is_dir() {
        return Err(invalid_payload(
            "plugin source path must point to a directory".to_owned(),
        ));
    }
    if !["plugin.json", "plugin.yaml", "plugin.yml"]
        .iter()
        .any(|name| path.join(name).is_file())
    {
        return Err(invalid_payload(
            "plugin package must contain plugin.json, plugin.yaml, or plugin.yml".to_owned(),
        ));
    }
    Ok(path)
}

#[cfg(unix)]
pub(crate) fn ensure_not_world_writable_path(
    path: &Path,
    label: &str,
) -> Result<(), CommandErrorPayload> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        runtime_operation_failed(format!("{label} metadata unavailable: {error}"))
    })?;
    if metadata.permissions().mode() & 0o002 != 0 {
        return Err(invalid_payload(format!(
            "{label} must not be world-writable"
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn ensure_not_world_writable_path(
    _path: &Path,
    _label: &str,
) -> Result<(), CommandErrorPayload> {
    Ok(())
}

#[cfg(unix)]
pub(crate) fn ensure_no_world_writable_ancestors(
    path: &Path,
    label: &str,
) -> Result<(), CommandErrorPayload> {
    use std::os::unix::fs::PermissionsExt;

    for ancestor in path.ancestors().skip(1) {
        let metadata = std::fs::symlink_metadata(ancestor).map_err(|error| {
            runtime_operation_failed(format!("{label} ancestor metadata unavailable: {error}"))
        })?;
        let mode = metadata.permissions().mode();
        let world_writable = mode & 0o002 != 0;
        let sticky = mode & 0o1000 != 0;
        if world_writable && !sticky {
            return Err(invalid_payload(format!(
                "{label} ancestors must not be world-writable"
            )));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn ensure_no_world_writable_ancestors(
    _path: &Path,
    _label: &str,
) -> Result<(), CommandErrorPayload> {
    Ok(())
}

pub(crate) fn plugin_package_dir_name(plugin_id: &PluginId) -> String {
    plugin_id
        .0
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn redact_plugin_detail_config(mut detail: PluginDetail) -> PluginDetail {
    detail.config =
        redact_plugin_detail_config_values(detail.configuration_schema.as_ref(), detail.config);
    detail
}

pub(crate) fn redact_plugin_detail_config_values(schema: Option<&Value>, values: Value) -> Value {
    let Some(schema) = schema else {
        return Value::Null;
    };
    strip_secret_config_value_for_detail(schema, &values).unwrap_or(Value::Null)
}

pub(crate) fn redact_secret_config_values(schema: Option<&Value>, values: Value) -> Value {
    let Some(schema) = schema else {
        return values;
    };
    strip_secret_config_value(schema, &values).unwrap_or(Value::Null)
}

fn strip_secret_config_value_for_detail(schema: &Value, value: &Value) -> Option<Value> {
    if schema
        .get("secret")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    match value {
        Value::Object(object) => {
            let properties = schema.get("properties").and_then(Value::as_object)?;
            Some(Value::Object(
                object
                    .iter()
                    .filter_map(|(key, value)| {
                        let field_schema = properties.get(key)?;
                        strip_secret_config_value_for_detail(field_schema, value)
                            .map(|value| (key.clone(), value))
                    })
                    .collect(),
            ))
        }
        Value::Array(values) => {
            let item_schema = schema.get("items")?;
            Some(Value::Array(
                values
                    .iter()
                    .filter_map(|value| strip_secret_config_value_for_detail(item_schema, value))
                    .collect(),
            ))
        }
        value => Some(value.clone()),
    }
}

pub(crate) fn strip_secret_config_value(schema: &Value, value: &Value) -> Option<Value> {
    if schema
        .get("secret")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    match value {
        Value::Object(object) => {
            let properties = schema.get("properties").and_then(Value::as_object);
            Some(Value::Object(
                object
                    .iter()
                    .filter_map(|(key, value)| {
                        let field_schema = properties.and_then(|properties| properties.get(key));
                        match field_schema {
                            Some(field_schema) => strip_secret_config_value(field_schema, value)
                                .map(|value| (key.clone(), value)),
                            None => Some((key.clone(), value.clone())),
                        }
                    })
                    .collect(),
            ))
        }
        Value::Array(values) => {
            let Some(item_schema) = schema.get("items") else {
                return Some(value.clone());
            };
            Some(Value::Array(
                values
                    .iter()
                    .filter_map(|value| strip_secret_config_value(item_schema, value))
                    .collect(),
            ))
        }
        value => Some(value.clone()),
    }
}

pub(crate) fn merge_plugin_config_values(
    schema: Option<&Value>,
    current: &Value,
    update: Value,
) -> Value {
    let update = redact_secret_config_values(schema, update);
    match update {
        Value::Object(update_object) => {
            let mut merged = current.as_object().cloned().unwrap_or_default();
            for (key, value) in update_object {
                merged.insert(key, value);
            }
            Value::Object(merged)
        }
        value => value,
    }
}

pub(crate) fn ensure_no_secret_like_config_values(
    value: &Value,
) -> Result<(), CommandErrorPayload> {
    fn visit(value: &Value) -> bool {
        match value {
            Value::Object(object) => object
                .iter()
                .any(|(key, value)| is_secret_like_key(key) || visit(value)),
            Value::Array(values) => values.iter().any(visit),
            Value::String(value) => is_secret_like_value(value),
            _ => false,
        }
    }
    if visit(value) {
        return Err(invalid_payload(
            "plugin config contains a secret-like field; secrets must be managed by the secure store"
                .to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn is_secret_like_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "secret",
        "token",
        "apikey",
        "credential",
        "password",
        "privatekey",
        "bearer",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

pub(crate) fn is_secret_like_value(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("sk-")
        || trimmed.starts_with("Bearer ")
        || trimmed.starts_with("ghp_")
        || trimmed.starts_with("gho_")
        || trimmed.starts_with("ghu_")
        || trimmed.starts_with("github_pat_")
}

pub(crate) fn ensure_import_skill_source_path(value: &str) -> Result<PathBuf, CommandErrorPayload> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(invalid_payload(
            "skill source path must be absolute".to_owned(),
        ));
    }
    ensure_no_symlink_components(&path, "skill source directory")?;
    let path = path.canonicalize().map_err(|error| {
        runtime_operation_failed(format!("skill source path unavailable: {error}"))
    })?;
    ensure_no_symlink_components(&path, "skill source directory")?;
    if !path.is_dir() {
        return Err(invalid_payload(
            "skill source path must point to a directory".to_owned(),
        ));
    }
    let entry_path = path.join(SKILL_PACKAGE_ENTRY_FILE);
    ensure_no_symlink_components(&entry_path, "skill entry file")?;
    if !entry_path.is_file() {
        return Err(invalid_payload(
            "skill package must contain SKILL.md".to_owned(),
        ));
    }
    Ok(path)
}

pub(crate) fn skill_import_id() -> String {
    RunId::new().to_string().to_ascii_lowercase()
}

pub(crate) fn skill_summaries_from_records_and_runtime(
    records: &[SkillStoreRecord],
    runtime: &[RuntimeSkillSummary],
    enabled_ids: &BTreeSet<String>,
) -> Vec<SkillSummaryPayload> {
    let managed_names = records
        .iter()
        .map(|record| record.name.as_str())
        .collect::<HashSet<_>>();
    let mut skills = records
        .iter()
        .map(|record| {
            let enabled = enabled_ids.contains(&record.id);
            let status = enabled
                .then(|| {
                    runtime
                        .iter()
                        .find(|skill| skill.name == record.name)
                        .map(|skill| skill_status_string(&skill.status))
                })
                .flatten();
            managed_skill_summary(record, enabled, status)
        })
        .collect::<Vec<_>>();
    skills.extend(
        runtime
            .iter()
            .filter(|skill| !managed_names.contains(skill.name.as_str()))
            .map(runtime_skill_summary_payload),
    );
    skills.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    skills
}

pub(crate) fn managed_skill_summary(
    record: &SkillStoreRecord,
    enabled: bool,
    runtime_status: Option<&'static str>,
) -> SkillSummaryPayload {
    let status = if record.last_validation_error.is_some() {
        "rejected"
    } else if !enabled {
        "disabled"
    } else {
        runtime_status.unwrap_or("ready")
    };
    SkillSummaryPayload {
        id: record.id.clone(),
        name: record.name.clone(),
        description: record.description.clone(),
        source_kind: "workspace".to_owned(),
        enabled,
        manageable: true,
        status: status.to_owned(),
        tags: record.tags.clone(),
        category: record.category.clone(),
        imported_at: Some(record.imported_at.clone()),
        updated_at: Some(record.updated_at.clone()),
        origin: record.origin.clone(),
        source_plugin_id: None,
    }
}

pub(crate) fn runtime_skill_summary_payload(skill: &RuntimeSkillSummary) -> SkillSummaryPayload {
    SkillSummaryPayload {
        id: skill.name.clone(),
        name: skill.name.clone(),
        description: skill.description.clone(),
        source_kind: skill_source_string(&skill.source).to_owned(),
        enabled: true,
        manageable: false,
        status: skill_status_string(&skill.status).to_owned(),
        tags: skill.tags.clone(),
        category: skill.category.clone(),
        imported_at: None,
        updated_at: None,
        origin: None,
        source_plugin_id: skill_source_plugin_id(&skill.source),
    }
}

pub(crate) fn skill_detail_from_runtime_view(
    summary: SkillSummaryPayload,
    view: RuntimeSkillView,
    files: Vec<SkillFilePayload>,
    validation_error: Option<String>,
) -> SkillDetailPayload {
    SkillDetailPayload {
        summary,
        parameters: view
            .parameters
            .into_iter()
            .map(|parameter| SkillParameterPayload {
                name: parameter.name,
                param_type: parameter.param_type,
                required: parameter.required,
                default: parameter.default,
                description: parameter.description,
            })
            .collect(),
        config_keys: view.config_keys,
        files,
        body_preview: view.body_preview,
        validation_error,
    }
}

pub(crate) fn runtime_status_for_name(harness: &Harness, name: &str) -> Option<&'static str> {
    harness
        .list_runtime_skills()
        .iter()
        .find(|skill| skill.name == name)
        .map(|skill| skill_status_string(&skill.status))
}

pub(crate) fn skill_status_string(status: &jyowo_harness_sdk::ext::SkillStatus) -> &'static str {
    match status {
        jyowo_harness_sdk::ext::SkillStatus::Ready => "ready",
        jyowo_harness_sdk::ext::SkillStatus::PrerequisiteMissing { .. } => "prerequisite_missing",
    }
}

pub(crate) fn skill_source_string(
    source: &jyowo_harness_sdk::ext::SkillSourceKind,
) -> &'static str {
    match source {
        jyowo_harness_sdk::ext::SkillSourceKind::Bundled => "bundled",
        jyowo_harness_sdk::ext::SkillSourceKind::Workspace => "workspace",
        jyowo_harness_sdk::ext::SkillSourceKind::User => "user",
        jyowo_harness_sdk::ext::SkillSourceKind::Plugin(_) => "plugin",
        jyowo_harness_sdk::ext::SkillSourceKind::Mcp(_) => "mcp",
        _ => "workspace",
    }
}

pub(crate) fn skill_source_plugin_id(
    source: &jyowo_harness_sdk::ext::SkillSourceKind,
) -> Option<String> {
    match source {
        jyowo_harness_sdk::ext::SkillSourceKind::Plugin(plugin_id) => Some(plugin_id.0.clone()),
        _ => None,
    }
}
