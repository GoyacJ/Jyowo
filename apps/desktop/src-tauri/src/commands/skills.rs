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
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
use super::evals::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::plugins::*;
#[allow(unused_imports)]
use super::providers::*;
#[allow(unused_imports)]
use super::runtime::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;
use harness_contracts::{SkillConfigEntry, SkillSecretMetadata, SkillSelectionRecord};
use harness_skill::{SkillConfigDecl, SkillParamType};
use jyowo_harness_sdk::skill_config::SecretString;
use std::collections::{BTreeMap, BTreeSet};

const SKILL_PACKAGE_INTEGRITY_ERROR: &str = "skill package content hash mismatch";

pub async fn get_skill_config_with_runtime_state(
    request: GetSkillConfigRequest,
    state: &DesktopRuntimeState,
) -> Result<GetSkillConfigResponse, CommandErrorPayload> {
    let view = resolve_skill_config_view(&request.skill_id, state).await?;
    let skill_id = view.summary.id;
    let snapshot = state.skill_config_store.load_snapshot()?;
    let mut config = SkillConfigEntry::default();
    for declaration in &view.config {
        if declaration.secret {
            config.secrets.insert(
                declaration.key.clone(),
                SkillSecretMetadata {
                    configured: snapshot
                        .secret_is_available_for(&skill_id, &declaration.key)
                        .map_err(skill_config_runtime_error)?,
                },
            );
        } else if let Some(value) = snapshot.value_for(&skill_id, &declaration.key) {
            config.values.insert(declaration.key.clone(), value.clone());
        }
    }
    let declarations = view
        .config
        .into_iter()
        .map(|declaration| SkillConfigDeclarationPayload {
            key: declaration.key,
            value_type: declaration.value_type,
            secret: declaration.secret,
            required: declaration.required,
            default: declaration.default,
            description: declaration.description,
        })
        .collect();
    Ok(GetSkillConfigResponse {
        config,
        declarations,
        skill_id,
    })
}

pub async fn set_skill_config_value_with_runtime_state(
    request: SetSkillConfigValueRequest,
    state: &DesktopRuntimeState,
) -> Result<SkillConfigMutationResponse, CommandErrorPayload> {
    let _settings_reload_guard = state.settings_reload_lock.lock().await;
    let view = resolve_skill_config_view(&request.skill_id, state).await?;
    let declaration = config_declaration(&view, &request.key)?;
    state
        .skill_config_store
        .set_public_value(&view.summary.id, &declaration, request.value)?;
    refresh_skill_config_snapshot(state)?;
    Ok(SkillConfigMutationResponse {
        skill_id: view.summary.id,
        key: request.key,
        configured: true,
    })
}

pub async fn set_skill_secret_with_runtime_state(
    request: SetSkillSecretRequest,
    state: &DesktopRuntimeState,
) -> Result<SkillConfigMutationResponse, CommandErrorPayload> {
    let _settings_reload_guard = state.settings_reload_lock.lock().await;
    let view = resolve_skill_config_view(&request.skill_id, state).await?;
    let declaration = config_declaration(&view, &request.key)?;
    state.skill_config_store.set_secret(
        &view.summary.id,
        &declaration,
        SecretString::from(request.value),
    )?;
    refresh_skill_config_snapshot(state)?;
    Ok(SkillConfigMutationResponse {
        skill_id: view.summary.id,
        key: request.key,
        configured: true,
    })
}

pub async fn clear_skill_secret_with_runtime_state(
    request: ClearSkillSecretRequest,
    state: &DesktopRuntimeState,
) -> Result<SkillConfigMutationResponse, CommandErrorPayload> {
    let _settings_reload_guard = state.settings_reload_lock.lock().await;
    let view = resolve_skill_config_view(&request.skill_id, state).await?;
    let declaration = config_declaration(&view, &request.key)?;
    state
        .skill_config_store
        .clear_secret(&view.summary.id, &declaration)?;
    refresh_skill_config_snapshot(state)?;
    Ok(SkillConfigMutationResponse {
        skill_id: view.summary.id,
        key: request.key,
        configured: false,
    })
}

fn refresh_skill_config_snapshot(state: &DesktopRuntimeState) -> Result<(), CommandErrorPayload> {
    let snapshot = state.skill_config_store.load_snapshot()?;
    let runtime = state.settings_runtime().ok_or_else(|| {
        runtime_unavailable("Skill configuration requires the runtime skill facade.")
    })?;
    runtime.replace_skill_config_snapshot(snapshot.clone());
    let skill_root = state
        .global_config_store
        .as_ref()
        .map(|store| store.layout().global_skills_root())
        .unwrap_or_else(|| state.skill_config_store.layout().global_skills_root());
    for shared_runtime in shared_skill_runtimes(&skill_root) {
        if Arc::ptr_eq(&shared_runtime, &runtime) {
            continue;
        }
        shared_runtime.replace_skill_config_snapshot(snapshot.clone());
    }
    Ok(())
}

async fn resolve_skill_config_view(
    request_id: &str,
    state: &DesktopRuntimeState,
) -> Result<RuntimeSkillView, CommandErrorPayload> {
    let _skill_store_guard = state.skill_store_lock.lock().await;
    let runtime = state.settings_runtime().ok_or_else(|| {
        runtime_unavailable("Skill configuration requires the runtime skill facade.")
    })?;
    let mut records = state.skill_store.load_records()?;
    refresh_and_persist_skill_package_integrity(state, &mut records)?;
    if let Some(record) = records.iter().find(|record| record.id == request_id) {
        if record.last_validation_error.is_some() {
            return Err(invalid_payload("skill package is rejected".to_owned()));
        }
        if records
            .iter()
            .filter(|candidate| candidate.name == record.name)
            .count()
            != 1
        {
            return Err(invalid_payload(
                "managed skill identity is ambiguous".to_owned(),
            ));
        }
        let view = runtime
            .view_runtime_skill(&record.name, false)
            .map_err(skill_config_runtime_error)?
            .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
        let expected_id = format!("user:{}", record.name);
        if view.summary.id != expected_id {
            return Err(invalid_payload(
                "managed skill is shadowed by another skill source".to_owned(),
            ));
        }
        return Ok(view);
    }
    let runtime_skills = runtime
        .list_runtime_skills()
        .map_err(skill_config_runtime_error)?;
    let requested_name = runtime_skills
        .iter()
        .find(|skill| skill.id == request_id || skill.name == request_id)
        .map(|skill| skill.name.as_str())
        .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
    let view = runtime
        .view_runtime_skill(requested_name, false)
        .map_err(skill_config_runtime_error)?
        .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
    if records.iter().any(|record| {
        record.last_validation_error.is_some() && view.summary.id == format!("user:{}", record.name)
    }) {
        return Err(invalid_payload("skill package is rejected".to_owned()));
    }
    Ok(view)
}

fn skill_config_runtime_error(
    _error: jyowo_harness_sdk::SkillConfigStoreError,
) -> CommandErrorPayload {
    runtime_operation_failed("skill secret store operation failed".to_owned())
}

fn config_declaration(
    view: &RuntimeSkillView,
    key: &str,
) -> Result<SkillConfigDecl, CommandErrorPayload> {
    let declaration = view
        .config
        .iter()
        .find(|declaration| declaration.key == key)
        .ok_or_else(|| invalid_payload("skill config key is not declared".to_owned()))?;
    Ok(SkillConfigDecl {
        key: declaration.key.clone(),
        value_type: runtime_config_type(declaration)?,
        secret: declaration.secret,
        required: declaration.required,
        default: declaration.default.clone(),
        description: declaration.description.clone(),
    })
}

fn runtime_config_type(
    declaration: &RuntimeSkillConfig,
) -> Result<SkillParamType, CommandErrorPayload> {
    match declaration.value_type.as_str() {
        "string" => Ok(SkillParamType::String),
        "number" => Ok(SkillParamType::Number),
        "boolean" => Ok(SkillParamType::Boolean),
        "path" => Ok(SkillParamType::Path),
        "url" => Ok(SkillParamType::Url),
        _ => Err(runtime_operation_failed(
            "runtime skill config type is invalid".to_owned(),
        )),
    }
}
pub async fn list_skills_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListSkillsResponse, CommandErrorPayload> {
    let _skill_store_guard = state.skill_store_lock.lock().await;
    let records = load_fresh_skill_records(state).await?;
    let runtime = match state.settings_runtime() {
        Some(settings_runtime) => settings_runtime
            .list_runtime_skills()
            .map_err(skill_config_runtime_error)?,
        None => Vec::new(),
    };
    let enabled_ids = enabled_skill_ids_for_state(state)?;
    Ok(ListSkillsResponse {
        skills: skill_summaries_from_records_and_runtime(&records, &runtime, &enabled_ids),
    })
}

fn load_skill_selection_for_state(
    state: &DesktopRuntimeState,
) -> Result<SkillSelectionRecord, CommandErrorPayload> {
    if let Some(global_config) = &state.global_config_store {
        if let Some(selection) = global_config.load_global_skill_selection_if_present()? {
            return Ok(selection);
        }
        return skill_selection_from_store_records(state.skill_store.as_ref());
    }
    Ok(SkillSelectionRecord::default())
}

fn skill_selection_from_store_records(
    skill_store: &dyn SkillStore,
) -> Result<SkillSelectionRecord, CommandErrorPayload> {
    let mut enabled = skill_store
        .load_records()?
        .into_iter()
        .filter(|record| record.enabled)
        .map(|record| record.id)
        .collect::<Vec<_>>();
    enabled.sort();
    Ok(SkillSelectionRecord { enabled })
}

fn save_skill_selection_for_state(
    state: &DesktopRuntimeState,
    selection: &SkillSelectionRecord,
) -> Result<(), CommandErrorPayload> {
    let global_config = state.global_config_store.as_ref().ok_or_else(|| {
        runtime_operation_failed("skill selection config store is unavailable".to_owned())
    })?;
    global_config.save_global_skill_selection(selection)
}

pub(crate) fn enabled_skill_ids_for_state(
    state: &DesktopRuntimeState,
) -> Result<BTreeSet<String>, CommandErrorPayload> {
    Ok(load_skill_selection_for_state(state)?
        .enabled
        .into_iter()
        .collect())
}

pub async fn list_skill_catalog_sources_with_runtime_state(
) -> Result<ListSkillCatalogSourcesResponse, CommandErrorPayload> {
    Ok(list_catalog_sources_payload())
}

pub async fn list_skill_catalog_entries_with_runtime_state(
    request: ListSkillCatalogEntriesRequest,
    state: &DesktopRuntimeState,
) -> Result<ListSkillCatalogEntriesResponse, CommandErrorPayload> {
    let installed_entry_ids = installed_catalog_entry_ids(state)?;
    list_catalog_entries_payload(request, &installed_entry_ids).await
}

pub async fn get_skill_catalog_entry_with_runtime_state(
    request: GetSkillCatalogEntryRequest,
    state: &DesktopRuntimeState,
) -> Result<GetSkillCatalogEntryResponse, CommandErrorPayload> {
    let installed_entry_ids = installed_catalog_entry_ids(state)?;
    let mut response = get_catalog_entry_payload(request, &installed_entry_ids).await?;
    if active_skill_names(state)?.contains(response.entry.name.as_str()) {
        mark_catalog_entry_name_conflict(&mut response);
    }
    Ok(response)
}

pub async fn get_skill_catalog_file_with_runtime_state(
    request: GetSkillCatalogFileRequest,
    _state: &DesktopRuntimeState,
) -> Result<GetSkillCatalogFileResponse, CommandErrorPayload> {
    get_catalog_file_payload(request).await
}

pub async fn list_skill_catalog_install_tasks_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListSkillCatalogInstallTasksResponse, CommandErrorPayload> {
    let mut tasks = state
        .skill_catalog_install_task_store
        .load()?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    tasks.sort_by(|left, right| {
        left.started_at
            .cmp(&right.started_at)
            .then(left.operation_id.cmp(&right.operation_id))
    });
    Ok(ListSkillCatalogInstallTasksResponse { tasks })
}

pub async fn install_skill_from_catalog_with_runtime_state(
    request: InstallSkillFromCatalogRequest,
    state: &DesktopRuntimeState,
) -> Result<InstallSkillFromCatalogResponse, CommandErrorPayload> {
    start_skill_catalog_install_task_with_runtime_state(request, state.clone(), None).await
}

pub async fn start_skill_catalog_install_task_with_runtime_state(
    request: InstallSkillFromCatalogRequest,
    state: DesktopRuntimeState,
    emitter: Option<SkillCatalogInstallProgressEmitter>,
) -> Result<InstallSkillFromCatalogResponse, CommandErrorPayload> {
    let (task, request, created) =
        get_or_create_skill_catalog_install_task_record(&state, &request)?;
    if !created {
        return Ok(InstallSkillFromCatalogResponse { task });
    }

    let state_for_task = state.clone();
    let request_for_task = request.clone();
    let recording_emitter = skill_catalog_install_task_emitter(state, request, emitter);
    tauri::async_runtime::spawn(async move {
        let _ = install_skill_from_catalog_with_progress(
            request_for_task,
            &state_for_task,
            Some(recording_emitter),
        )
        .await;
    });

    Ok(InstallSkillFromCatalogResponse { task })
}

pub async fn install_skill_from_catalog_package_with_runtime_state(
    request: InstallSkillFromCatalogRequest,
    state: &DesktopRuntimeState,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    install_skill_from_catalog_with_progress(request, state, None).await
}

pub async fn install_skill_from_catalog_with_progress(
    request: InstallSkillFromCatalogRequest,
    state: &DesktopRuntimeState,
    emitter: Option<SkillCatalogInstallProgressEmitter>,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    validate_catalog_install_operation_id(&request)?;
    let result: Result<ImportSkillResponse, CommandErrorPayload> = async {
        emit_skill_catalog_install_progress(&emitter, &request, "preparing", 5, None)?;
        let catalog_progress = |stage: &str, percent: u8| {
            emit_skill_catalog_install_progress(&emitter, &request, stage, percent, None)
        };
        if let Some(hook) = &state.catalog_download_hook {
            hook();
        }
        let (package_path, origin, materialized_guard) =
            if let Some(materialize) = &state.catalog_materialize_hook {
                let (package_path, origin) = materialize(&request)?;
                (package_path, origin, None)
            } else {
                let materialized = materialize_skill_from_catalog_with_progress(
                    request.clone(),
                    Some(&catalog_progress),
                )
                .await?;
                (
                    materialized.package_path.clone(),
                    materialized.origin.clone(),
                    Some(materialized),
                )
            };
        let response = install_skill_package_with_progress(
            package_path,
            Some(origin),
            state,
            Some((&emitter, &request)),
        )
        .await?;
        drop(materialized_guard);
        Ok(response)
    }
    .await;

    if let Err(error) = &result {
        if let Err(persistence_error) = emit_skill_catalog_install_progress(
            &emitter,
            &request,
            "failed",
            100,
            Some(error.message.clone()),
        ) {
            return Err(persistence_error);
        }
    }

    result
}

#[doc(hidden)]
pub fn get_or_create_skill_catalog_install_task(
    state: &DesktopRuntimeState,
    request: &InstallSkillFromCatalogRequest,
) -> Result<SkillCatalogInstallTaskPayload, CommandErrorPayload> {
    let (task, _, _) = get_or_create_skill_catalog_install_task_record(state, request)?;
    Ok(task)
}

pub(crate) fn get_or_create_skill_catalog_install_task_record(
    state: &DesktopRuntimeState,
    request: &InstallSkillFromCatalogRequest,
) -> Result<
    (
        SkillCatalogInstallTaskPayload,
        InstallSkillFromCatalogRequest,
        bool,
    ),
    CommandErrorPayload,
> {
    validate_skill_catalog_install_request(request)?;
    let operation_id = match request.operation_id.as_deref() {
        Some(operation_id) => {
            ensure_non_empty("operationId", operation_id)?;
            operation_id.to_owned()
        }
        None => catalog_install_operation_id(),
    };
    let now = now().to_rfc3339();
    let task = SkillCatalogInstallTaskPayload {
        operation_id: operation_id.clone(),
        source_id: request.source_id.clone(),
        entry_id: request.entry_id.clone(),
        version: request.version.clone(),
        stage: "preparing".to_owned(),
        percent: 5,
        status: "running".to_owned(),
        message: None,
        started_at: now.clone(),
        updated_at: now,
    };
    let (task, created) = state
        .skill_catalog_install_task_store
        .create_running(task)?;
    let request = InstallSkillFromCatalogRequest {
        operation_id: Some(task.operation_id.clone()),
        ..request.clone()
    };
    Ok((task, request, created))
}

#[doc(hidden)]
pub async fn record_skill_catalog_install_task_progress(
    state: &DesktopRuntimeState,
    request: &InstallSkillFromCatalogRequest,
    stage: &str,
    percent: u8,
    message: Option<String>,
) -> Result<SkillCatalogInstallTaskPayload, CommandErrorPayload> {
    let operation_id = request
        .operation_id
        .as_deref()
        .ok_or_else(|| invalid_payload("operationId is required".to_owned()))?;
    let payload = SkillCatalogInstallProgressPayload {
        operation_id: operation_id.to_owned(),
        source_id: request.source_id.clone(),
        entry_id: request.entry_id.clone(),
        version: request.version.clone(),
        stage: skill_catalog_install_stage(stage),
        percent,
        message,
    };
    record_skill_catalog_install_task_payload(state, payload)
}

pub(crate) fn record_skill_catalog_install_task_payload(
    state: &DesktopRuntimeState,
    payload: SkillCatalogInstallProgressPayload,
) -> Result<SkillCatalogInstallTaskPayload, CommandErrorPayload> {
    state
        .skill_catalog_install_task_store
        .record_progress(payload)
}

pub(crate) fn skill_catalog_install_task_emitter(
    state: DesktopRuntimeState,
    request: InstallSkillFromCatalogRequest,
    emitter: Option<SkillCatalogInstallProgressEmitter>,
) -> SkillCatalogInstallProgressEmitter {
    Arc::new(move |payload| {
        let recorded = record_skill_catalog_install_task_payload(&state, payload)?;
        if recorded.operation_id == request.operation_id.clone().unwrap_or_default() {
            if let Some(emitter) = &emitter {
                let _ = emitter(SkillCatalogInstallProgressPayload {
                    operation_id: recorded.operation_id,
                    source_id: recorded.source_id,
                    entry_id: recorded.entry_id,
                    version: recorded.version,
                    stage: skill_catalog_install_stage(&recorded.stage),
                    percent: recorded.percent,
                    message: recorded.message,
                });
            }
        }
        Ok(())
    })
}

pub(crate) fn validate_skill_catalog_install_request(
    request: &InstallSkillFromCatalogRequest,
) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("sourceId", &request.source_id)?;
    ensure_non_empty("entryId", &request.entry_id)?;
    if let Some(version) = request.version.as_deref() {
        ensure_non_empty("version", version)?;
    }
    Ok(())
}

pub(crate) fn catalog_install_operation_id() -> String {
    format!("catalog-install-{}", skill_import_id())
}

pub(crate) fn validate_catalog_install_operation_id(
    request: &InstallSkillFromCatalogRequest,
) -> Result<(), CommandErrorPayload> {
    if let Some(operation_id) = request.operation_id.as_deref() {
        ensure_non_empty("operationId", operation_id)?;
    }
    Ok(())
}

pub(crate) fn emit_skill_catalog_install_progress(
    emitter: &Option<SkillCatalogInstallProgressEmitter>,
    request: &InstallSkillFromCatalogRequest,
    stage: &str,
    percent: u8,
    message: Option<String>,
) -> Result<(), CommandErrorPayload> {
    let Some(operation_id) = request.operation_id.clone() else {
        return Ok(());
    };
    let Some(emitter) = emitter else {
        return Ok(());
    };
    let stage = skill_catalog_install_stage(stage);
    let payload = SkillCatalogInstallProgressPayload {
        operation_id,
        source_id: request.source_id.clone(),
        entry_id: request.entry_id.clone(),
        version: request.version.clone(),
        stage,
        percent: percent.min(100),
        message,
    };
    // Progress events are UI telemetry. Failure to emit must not change install policy.
    emitter(payload)
}

pub(crate) fn skill_catalog_install_stage(stage: &str) -> &'static str {
    match stage {
        "preparing" => "preparing",
        "resolving" => "resolving",
        "checking" => "checking",
        "downloading" => "downloading",
        "validating" => "validating",
        "copying" => "copying",
        "reloading" => "reloading",
        "completed" => "completed",
        "failed" => "failed",
        "interrupted" => "interrupted",
        _ => "preparing",
    }
}

pub(crate) fn installed_catalog_entry_ids(
    state: &DesktopRuntimeState,
) -> Result<HashSet<String>, CommandErrorPayload> {
    Ok(state
        .skill_store
        .load_records()?
        .into_iter()
        .filter_map(|record| record.origin.map(|origin| origin.entry_id))
        .collect())
}

pub(crate) fn active_skill_names(
    state: &DesktopRuntimeState,
) -> Result<HashSet<String>, CommandErrorPayload> {
    let enabled_ids = enabled_skill_ids_for_state(state)?;
    let mut names = state
        .skill_store
        .load_records()?
        .into_iter()
        .filter(|record| enabled_ids.contains(&record.id))
        .map(|record| record.name)
        .collect::<HashSet<_>>();
    if let Some(settings_runtime) = state.settings_runtime() {
        names.extend(
            settings_runtime
                .list_runtime_skills()
                .map_err(skill_config_runtime_error)?
                .into_iter()
                .map(|skill| skill.name),
        );
    }
    Ok(names)
}

pub async fn import_skill_with_runtime_state(
    request: ImportSkillRequest,
    state: &DesktopRuntimeState,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    let source_path = ensure_import_skill_source_path(&request.source_path)?;
    install_skill_package_with_runtime_state(source_path, None, state).await
}

pub(crate) async fn install_skill_package_with_runtime_state(
    source_path: PathBuf,
    origin: Option<SkillInstallOriginRecord>,
    state: &DesktopRuntimeState,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    install_skill_package_with_progress(source_path, origin, state, None).await
}

pub(crate) async fn install_skill_package_with_progress(
    source_path: PathBuf,
    origin: Option<SkillInstallOriginRecord>,
    state: &DesktopRuntimeState,
    progress_context: Option<(
        &Option<SkillCatalogInstallProgressEmitter>,
        &InstallSkillFromCatalogRequest,
    )>,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    let settings_runtime = state.settings_runtime().ok_or_else(|| {
        runtime_unavailable("Importing skills requires the runtime skill facade.")
    })?;
    if let Some((emitter, request)) = progress_context {
        emit_skill_catalog_install_progress(emitter, request, "validating", 65, None)?;
    }
    let entry_path = source_path.join(SKILL_PACKAGE_ENTRY_FILE);
    let bytes =
        read_regular_file_no_follow(&entry_path, "skill entry file", MAX_SKILL_MARKDOWN_BYTES)?;
    let markdown = String::from_utf8(bytes)
        .map_err(|_| invalid_payload("skill entry file must be valid UTF-8".to_owned()))?;
    let validated = settings_runtime
        .validate_workspace_skill_markdown(&markdown, Some(entry_path))
        .await
        .map_err(|error| invalid_payload(error.to_string()))?;
    if let Some((emitter, request)) = progress_context {
        emit_skill_catalog_install_progress(emitter, request, "validating", 72, None)?;
    }
    let id = skill_import_id();
    let now = now().to_rfc3339();
    let mut record = SkillStoreRecord {
        id: id.clone(),
        name: validated.summary.name.clone(),
        description: validated.summary.description.clone(),
        enabled: true,
        content_hash: String::new(),
        package_dir: id.clone(),
        file_name: String::new(),
        imported_at: now.clone(),
        updated_at: now,
        tags: validated.summary.tags.clone(),
        category: validated.summary.category.clone(),
        last_validation_error: None,
        origin,
    };
    if let Some((emitter, request)) = progress_context {
        emit_skill_catalog_install_progress(emitter, request, "copying", 82, None)?;
    }
    record.content_hash = state
        .skill_store
        .stage_skill_package(&record.id, &source_path)?;
    let copied_markdown = match state.skill_store.read_staged_skill_entry_file(&record.id) {
        Ok(markdown) => markdown,
        Err(error) => {
            return Err(discard_staged_skill_after_error(state, &record.id, error));
        }
    };
    let copied_validation = settings_runtime
        .validate_workspace_skill_markdown(&copied_markdown, None)
        .await
        .map_err(|error| {
            discard_staged_skill_after_error(state, &record.id, invalid_payload(error.to_string()))
        })?;
    record.name = copied_validation.summary.name;
    record.description = copied_validation.summary.description;
    record.tags = copied_validation.summary.tags;
    record.category = copied_validation.summary.category;

    let _settings_reload_guard = state.settings_reload_lock.lock().await;
    let _skill_store_guard = state.skill_store_lock.lock().await;
    let mut records = match state.skill_store.load_records() {
        Ok(records) => records,
        Err(error) => {
            return Err(discard_staged_skill_after_error(state, &record.id, error));
        }
    };
    let previous_records = records.clone();
    let previous_selection = match load_skill_selection_for_state(state) {
        Ok(selection) => selection,
        Err(error) => {
            return Err(discard_staged_skill_after_error(state, &record.id, error));
        }
    };
    let enabled_ids: BTreeSet<String> = previous_selection.enabled.iter().cloned().collect();
    let runtime_skills = match settings_runtime
        .list_runtime_skills()
        .map_err(skill_config_runtime_error)
    {
        Ok(skills) => skills,
        Err(error) => {
            return Err(discard_staged_skill_after_error(state, &record.id, error));
        }
    };
    if records
        .iter()
        .any(|existing| enabled_ids.contains(&existing.id) && existing.name == record.name)
        || runtime_skills.iter().any(|skill| skill.name == record.name)
    {
        let error = invalid_payload(format!("active skill name already exists: {}", record.name));
        return Err(discard_staged_skill_after_error(state, &record.id, error));
    }
    if let Err(error) = state.skill_store.commit_staged_skill_package(&record.id) {
        return Err(discard_staged_skill_after_error(state, &record.id, error));
    }
    records.retain(|existing| existing.id != record.id);
    records.push(record.clone());
    records.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    let mut selection = previous_selection.clone();
    selection.enabled.retain(|id| id != &record.id);
    selection.enabled.push(record.id.clone());
    selection.enabled.sort();
    let commit_result: Result<ImportSkillResponse, CommandErrorPayload> = async {
        state.skill_store.save_records(&records)?;
        save_skill_selection_for_state(state, &selection)?;
        if let Some((emitter, request)) = progress_context {
            emit_skill_catalog_install_progress(emitter, request, "reloading", 95, None)?;
        }
        reload_managed_skills(state, &settings_runtime).await?;
        let runtime_status = runtime_status_for_name(&settings_runtime, &record.name)
            .map_err(skill_config_runtime_error)?;
        if let Some((emitter, request)) = progress_context {
            emit_skill_catalog_install_progress(emitter, request, "completed", 100, None)?;
        }
        Ok(ImportSkillResponse {
            skill: managed_skill_summary(&record, true, runtime_status),
        })
    }
    .await;

    match commit_result {
        Ok(response) => Ok(response),
        Err(error) => {
            match rollback_committed_skill_install(
                state,
                &settings_runtime,
                &record.id,
                &previous_records,
                &previous_selection,
            )
            .await
            {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(CommandErrorPayload {
                    code: "SKILL_INSTALL_COMMIT_INDETERMINATE",
                    message: format!(
                        "skill install failed: {}; rollback failed: {}",
                        error.message, rollback_error.message
                    ),
                }),
            }
        }
    }
}

fn discard_staged_skill_after_error(
    state: &DesktopRuntimeState,
    skill_id: &str,
    error: CommandErrorPayload,
) -> CommandErrorPayload {
    match state.skill_store.discard_staged_skill_package(skill_id) {
        Ok(()) => error,
        Err(cleanup_error) => CommandErrorPayload {
            code: "SKILL_INSTALL_STAGING_CLEANUP_FAILED",
            message: format!(
                "skill install failed: {}; staging cleanup failed: {}",
                error.message, cleanup_error.message
            ),
        },
    }
}

async fn rollback_committed_skill_install(
    state: &DesktopRuntimeState,
    settings_runtime: &DesktopSettingsRuntime,
    skill_id: &str,
    previous_records: &[SkillStoreRecord],
    previous_selection: &SkillSelectionRecord,
) -> Result<(), CommandErrorPayload> {
    let mut failures = Vec::new();
    if let Err(error) = state.skill_store.delete_skill_package(skill_id) {
        failures.push(error.message);
    }
    if let Err(error) = state.skill_store.save_records(previous_records) {
        failures.push(error.message);
    }
    if let Err(error) = save_skill_selection_for_state(state, previous_selection) {
        failures.push(error.message);
    }
    if let Err(error) = reload_managed_skills(state, settings_runtime).await {
        failures.push(error.message);
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(runtime_operation_failed(failures.join("; ")))
    }
}

pub async fn get_skill_detail_with_runtime_state(
    request: GetSkillDetailRequest,
    state: &DesktopRuntimeState,
) -> Result<GetSkillDetailResponse, CommandErrorPayload> {
    ensure_skill_id(&request.id)?;
    let _skill_store_guard = state.skill_store_lock.lock().await;
    let records = load_fresh_skill_records(state).await?;
    let record = records.iter().find(|record| record.id == request.id);
    let enabled_ids = enabled_skill_ids_for_state(state)?;
    let settings_runtime = state.settings_runtime();

    let Some(record) = record else {
        let settings_runtime = settings_runtime
            .as_ref()
            .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
        let view = settings_runtime
            .view_runtime_skill(&request.id, false)
            .map_err(skill_config_runtime_error)?
            .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
        return Ok(GetSkillDetailResponse {
            skill: skill_detail_from_runtime_view(
                runtime_skill_summary_payload(&view.summary),
                view,
                Vec::new(),
                None,
            ),
        });
    };

    let enabled = enabled_ids.contains(&record.id);
    let runtime_view = match (enabled, settings_runtime.as_ref()) {
        (true, Some(settings_runtime)) => settings_runtime
            .view_runtime_skill(&record.name, false)
            .map_err(skill_config_runtime_error)?,
        _ => None,
    };
    let files = state.skill_store.list_skill_package_files(record)?;
    let detail = if let Some(view) = runtime_view {
        let status = skill_status_string(&view.summary.status);
        skill_detail_from_runtime_view(
            managed_skill_summary(record, enabled, Some(status)),
            view,
            files,
            record.last_validation_error.clone(),
        )
    } else {
        SkillDetailPayload {
            summary: managed_skill_summary(record, enabled, None),
            parameters: Vec::new(),
            config_keys: Vec::new(),
            scripts: Vec::new(),
            prerequisites: SkillPrerequisitePayload::default(),
            files,
            body_preview: String::new(),
            validation_error: record.last_validation_error.clone(),
        }
    };
    Ok(GetSkillDetailResponse { skill: detail })
}

pub async fn get_skill_file_with_runtime_state(
    request: GetSkillFileRequest,
    state: &DesktopRuntimeState,
) -> Result<GetSkillFileResponse, CommandErrorPayload> {
    ensure_skill_id(&request.id)?;
    let records = state.skill_store.load_records()?;
    let record = records
        .iter()
        .find(|record| record.id == request.id)
        .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
    let files = state.skill_store.list_skill_package_files(record)?;
    if !files
        .iter()
        .any(|file| file.kind == "file" && file.path == request.path)
    {
        return Err(invalid_payload("skill file not found".to_owned()));
    }
    Ok(GetSkillFileResponse {
        file: state
            .skill_store
            .read_skill_package_file(record, &request.path)?,
    })
}

pub async fn set_skill_enabled_with_runtime_state(
    request: SetSkillEnabledRequest,
    state: &DesktopRuntimeState,
) -> Result<SetSkillEnabledResponse, CommandErrorPayload> {
    ensure_skill_id(&request.id)?;
    let _settings_reload_guard = state.settings_reload_lock.lock().await;
    let _skill_store_guard = state.skill_store_lock.lock().await;
    let settings_runtime = state.settings_runtime().ok_or_else(|| {
        runtime_unavailable("Changing skill state requires the runtime skill facade.")
    })?;
    let mut records = state.skill_store.load_records()?;
    refresh_and_persist_skill_package_integrity(state, &mut records)?;
    let previous_selection = load_skill_selection_for_state(state)?;
    let enabled_ids: BTreeSet<String> = previous_selection.enabled.iter().cloned().collect();
    let record_index = records
        .iter()
        .position(|record| record.id == request.id)
        .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
    let record_name = records[record_index].name.clone();
    let currently_enabled = enabled_ids.contains(&request.id);
    if currently_enabled != request.enabled {
        if request.enabled
            && (records.iter().any(|candidate| {
                enabled_ids.contains(&candidate.id)
                    && candidate.name == record_name
                    && candidate.id != request.id
            }) || settings_runtime
                .list_runtime_skills()
                .map_err(skill_config_runtime_error)?
                .iter()
                .any(|skill| skill.name == record_name))
        {
            return Err(invalid_payload(format!(
                "active skill name already exists: {}",
                record_name
            )));
        }
        state
            .skill_store
            .move_skill_package(&request.id, request.enabled)?;
        let previous_records = records.clone();
        records[record_index].enabled = request.enabled;
        records[record_index].updated_at = now().to_rfc3339();
        records[record_index].last_validation_error = None;
        let mut selection = previous_selection.clone();
        if request.enabled {
            if !selection.enabled.iter().any(|id| id == &request.id) {
                selection.enabled.push(request.id.clone());
                selection.enabled.sort();
            }
        } else {
            selection.enabled.retain(|id| id != &request.id);
        }
        if let Err(error) = state.skill_store.save_records(&records) {
            let _ = state
                .skill_store
                .move_skill_package(&request.id, currently_enabled);
            let _ = state.skill_store.save_records(&previous_records);
            return Err(error);
        }
        if let Err(error) = save_skill_selection_for_state(state, &selection) {
            let _ = save_skill_selection_for_state(state, &previous_selection);
            let _ = state
                .skill_store
                .move_skill_package(&request.id, currently_enabled);
            let _ = state.skill_store.save_records(&previous_records);
            return Err(error);
        }
        if let Err(error) = reload_managed_skills(state, &settings_runtime).await {
            let _ = save_skill_selection_for_state(state, &previous_selection);
            let _ = state
                .skill_store
                .move_skill_package(&request.id, currently_enabled);
            let _ = state.skill_store.save_records(&previous_records);
            let _ = reload_managed_skills(state, &settings_runtime).await;
            return Err(error);
        }
    } else {
        reload_managed_skills(state, &settings_runtime).await?;
    }
    let record = state
        .skill_store
        .load_records()?
        .into_iter()
        .find(|record| record.id == request.id)
        .ok_or_else(|| {
            runtime_operation_failed("skill record disappeared after reload".to_owned())
        })?;
    let runtime_status = if request.enabled {
        runtime_status_for_name(&settings_runtime, &record.name)
            .map_err(skill_config_runtime_error)?
    } else {
        None
    };
    Ok(SetSkillEnabledResponse {
        skill: managed_skill_summary(&record, request.enabled, runtime_status),
    })
}

pub async fn delete_skill_with_runtime_state(
    request: DeleteSkillRequest,
    state: &DesktopRuntimeState,
) -> Result<DeleteSkillResponse, CommandErrorPayload> {
    ensure_skill_id(&request.id)?;
    let _settings_reload_guard = state.settings_reload_lock.lock().await;
    let _skill_store_guard = state.skill_store_lock.lock().await;
    let settings_runtime = state
        .settings_runtime()
        .ok_or_else(|| runtime_unavailable("Deleting skills requires the runtime skill facade."))?;
    let mut records = state.skill_store.load_records()?;
    let previous_selection = load_skill_selection_for_state(state)?;
    let previous_records = records.clone();
    let original_len = records.len();
    records.retain(|record| record.id != request.id);
    if records.len() == original_len {
        return Err(invalid_payload("skill not found".to_owned()));
    }
    let mut selection = previous_selection.clone();
    selection.enabled.retain(|id| id != &request.id);
    if let Err(error) = save_skill_selection_for_state(state, &selection) {
        let _ = save_skill_selection_for_state(state, &previous_selection);
        return Err(error);
    }
    if let Err(error) = state.skill_store.save_records(&records) {
        let _ = save_skill_selection_for_state(state, &previous_selection);
        return Err(error);
    }
    if let Err(error) = reload_managed_skills(state, &settings_runtime).await {
        let _ = state.skill_store.save_records(&previous_records);
        let _ = save_skill_selection_for_state(state, &previous_selection);
        let _ = reload_managed_skills(state, &settings_runtime).await;
        return Err(error);
    }
    if let Err(error) = state.skill_store.delete_skill_package(&request.id) {
        let _ = state.skill_store.save_records(&previous_records);
        let _ = save_skill_selection_for_state(state, &previous_selection);
        let _ = reload_managed_skills(state, &settings_runtime).await;
        return Err(error);
    }
    Ok(DeleteSkillResponse {
        id: request.id,
        status: "deleted",
    })
}

pub(crate) async fn reload_managed_skills(
    state: &DesktopRuntimeState,
    settings_runtime: &DesktopSettingsRuntime,
) -> Result<(), CommandErrorPayload> {
    if let Some(global_config) = &state.global_config_store {
        let global_skill_store = DesktopSkillStore::global(global_config.layout().clone());
        let enabled_ids = global_config
            .load_global_skill_selection_if_present()?
            .map(|selection| selection.enabled.into_iter().collect::<BTreeSet<_>>());
        let mut records = global_skill_store.load_records()?;
        let previous_records = records.clone();
        refresh_skill_package_integrity(&global_skill_store, &mut records);
        if records != previous_records {
            global_skill_store.save_records(&records)?;
        }
        let expected_package_hashes = expected_package_hashes(&records, enabled_ids.as_ref());
        let skill_root = global_config.layout().global_skills_root();
        settings_runtime
            .reload_user_managed_skills_with_expected_package_hashes(
                global_skill_store.enabled_dir(),
                expected_package_hashes.clone(),
            )
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("global skill reload failed: {error}"))
            })?;
        let mut runtimes = shared_skill_runtimes(&skill_root);
        for runtime in runtimes.drain(..) {
            if std::ptr::eq(runtime.as_ref(), settings_runtime) {
                continue;
            }
            runtime
                .reload_user_managed_skills_with_expected_package_hashes(
                    global_skill_store.enabled_dir(),
                    expected_package_hashes.clone(),
                )
                .await
                .map_err(|error| {
                    runtime_operation_failed(format!("global skill reload failed: {error}"))
                })?;
        }
    }

    Ok(())
}

pub(crate) fn expected_package_hashes(
    records: &[SkillStoreRecord],
    enabled_ids: Option<&BTreeSet<String>>,
) -> BTreeMap<String, String> {
    records
        .iter()
        .filter(|record| {
            enabled_ids
                .map(|enabled_ids| enabled_ids.contains(&record.id))
                .unwrap_or(record.enabled)
        })
        .map(|record| (record.id.clone(), record.content_hash.clone()))
        .collect()
}

async fn load_fresh_skill_records(
    state: &DesktopRuntimeState,
) -> Result<Vec<SkillStoreRecord>, CommandErrorPayload> {
    let mut records = state.skill_store.load_records()?;
    let integrity_changed = refresh_and_persist_skill_package_integrity(state, &mut records)?;
    let has_integrity_rejection = records.iter().any(|record| {
        record
            .last_validation_error
            .as_deref()
            .is_some_and(is_skill_package_integrity_error)
    });
    if integrity_changed || has_integrity_rejection {
        if let Some(settings_runtime) = state.settings_runtime() {
            reload_managed_skills(state, &settings_runtime).await?;
        }
    }
    Ok(records)
}

fn refresh_and_persist_skill_package_integrity(
    state: &DesktopRuntimeState,
    records: &mut [SkillStoreRecord],
) -> Result<bool, CommandErrorPayload> {
    if !refresh_skill_package_integrity(state.skill_store.as_ref(), records) {
        return Ok(false);
    }
    state.skill_store.save_records(records)?;
    Ok(true)
}

fn refresh_skill_package_integrity(
    store: &dyn SkillStore,
    records: &mut [SkillStoreRecord],
) -> bool {
    let mut changed = false;
    for record in records {
        let previous_error = record.last_validation_error.clone();
        let integrity_error = match store.current_package_hash(record) {
            Ok(None) => continue,
            Ok(Some(current_hash)) if current_hash == record.content_hash => None,
            Ok(Some(_)) => Some(SKILL_PACKAGE_INTEGRITY_ERROR.to_owned()),
            Err(error) => Some(format!(
                "skill package integrity check failed: {}",
                error.message
            )),
        };
        match integrity_error {
            Some(error) => record.last_validation_error = Some(error),
            None if record
                .last_validation_error
                .as_deref()
                .is_some_and(is_skill_package_integrity_error) =>
            {
                record.last_validation_error = None;
            }
            None => {}
        }
        changed |= record.last_validation_error != previous_error;
    }
    changed
}

fn is_skill_package_integrity_error(error: &str) -> bool {
    error == SKILL_PACKAGE_INTEGRITY_ERROR
        || error.starts_with("skill package integrity check failed:")
}
