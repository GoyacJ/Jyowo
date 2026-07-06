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
use super::providers::*;
#[allow(unused_imports)]
use super::runtime::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;

pub async fn list_skills_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListSkillsResponse, CommandErrorPayload> {
    let records = state.skill_store.load_records()?;
    let runtime = state
        .harness()
        .map(|harness| harness.list_runtime_skills())
        .unwrap_or_default();
    Ok(ListSkillsResponse {
        skills: skill_summaries_from_records_and_runtime(&records, &runtime),
    })
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
        .skill_catalog_install_tasks
        .read()
        .map_err(|_| {
            runtime_operation_failed("skill catalog install tasks unavailable".to_owned())
        })?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    tasks.sort_by(|left, right| {
        left.source_id
            .cmp(&right.source_id)
            .then(left.entry_id.cmp(&right.entry_id))
            .then(left.version.cmp(&right.version))
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
        let _skill_store_guard = state_for_task.skill_store_lock.lock().await;
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
    install_skill_from_catalog_with_progress(
        request,
        state,
        None::<SkillCatalogInstallProgressEmitter>,
    )
    .await
}

pub async fn install_skill_from_catalog_with_progress(
    request: InstallSkillFromCatalogRequest,
    state: &DesktopRuntimeState,
    emitter: Option<SkillCatalogInstallProgressEmitter>,
) -> Result<ImportSkillResponse, CommandErrorPayload> {
    validate_catalog_install_operation_id(&request)?;
    let result: Result<ImportSkillResponse, CommandErrorPayload> = async {
        emit_skill_catalog_install_progress(&emitter, &request, "preparing", 5, None);
        let catalog_progress = |stage: &str, percent: u8| {
            emit_skill_catalog_install_progress(&emitter, &request, stage, percent, None);
        };
        let materialized =
            materialize_skill_from_catalog_with_progress(request.clone(), Some(&catalog_progress))
                .await?;
        let response = install_skill_package_with_progress(
            materialized.package_path.clone(),
            Some(materialized.origin.clone()),
            state,
            Some((&emitter, &request)),
        )
        .await?;
        drop(materialized);
        emit_skill_catalog_install_progress(&emitter, &request, "completed", 100, None);
        Ok(response)
    }
    .await;

    if let Err(error) = &result {
        emit_skill_catalog_install_progress(
            &emitter,
            &request,
            "failed",
            100,
            Some(error.message.clone()),
        );
    }

    result
}

#[cfg(test)]
pub(crate) fn get_or_create_skill_catalog_install_task(
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
    let key = skill_catalog_install_task_key(request)?;
    let mut tasks = state.skill_catalog_install_tasks.write().map_err(|_| {
        runtime_operation_failed("skill catalog install tasks unavailable".to_owned())
    })?;
    if let Some(existing) = tasks.get(&key) {
        if existing.status == "running" {
            let request = InstallSkillFromCatalogRequest {
                operation_id: Some(existing.operation_id.clone()),
                ..request.clone()
            };
            return Ok((existing.clone(), request, false));
        }
    }

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
    tasks.insert(key, task.clone());
    let request = InstallSkillFromCatalogRequest {
        operation_id: Some(operation_id),
        ..request.clone()
    };
    Ok((task, request, true))
}

#[cfg(test)]
pub(crate) async fn record_skill_catalog_install_task_progress(
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
    let key = SkillCatalogInstallTaskKey {
        source_id: payload.source_id.clone(),
        entry_id: payload.entry_id.clone(),
        version: payload.version.clone(),
    };
    let mut tasks = state.skill_catalog_install_tasks.write().map_err(|_| {
        runtime_operation_failed("skill catalog install tasks unavailable".to_owned())
    })?;
    let now = now().to_rfc3339();
    let task = tasks
        .entry(key)
        .or_insert_with(|| SkillCatalogInstallTaskPayload {
            operation_id: payload.operation_id.clone(),
            source_id: payload.source_id.clone(),
            entry_id: payload.entry_id.clone(),
            version: payload.version.clone(),
            stage: "preparing".to_owned(),
            percent: 5,
            status: "running".to_owned(),
            message: None,
            started_at: now.clone(),
            updated_at: now.clone(),
        });
    task.operation_id = payload.operation_id;
    task.stage = payload.stage.to_owned();
    task.percent = payload.percent.min(100);
    task.status = match payload.stage {
        "completed" => "completed",
        "failed" => "failed",
        _ => "running",
    }
    .to_owned();
    task.message = payload.message;
    task.updated_at = now;
    Ok(task.clone())
}

pub(crate) fn skill_catalog_install_task_emitter(
    state: DesktopRuntimeState,
    request: InstallSkillFromCatalogRequest,
    emitter: Option<SkillCatalogInstallProgressEmitter>,
) -> SkillCatalogInstallProgressEmitter {
    Arc::new(move |payload| {
        let _ = record_skill_catalog_install_task_payload(&state, payload.clone());
        if payload.operation_id == request.operation_id.clone().unwrap_or_default() {
            if let Some(emitter) = &emitter {
                emitter(payload);
            }
        }
    })
}

pub(crate) fn skill_catalog_install_task_key(
    request: &InstallSkillFromCatalogRequest,
) -> Result<SkillCatalogInstallTaskKey, CommandErrorPayload> {
    ensure_non_empty("sourceId", &request.source_id)?;
    ensure_non_empty("entryId", &request.entry_id)?;
    if let Some(version) = request.version.as_deref() {
        ensure_non_empty("version", version)?;
    }
    Ok(SkillCatalogInstallTaskKey {
        source_id: request.source_id.clone(),
        entry_id: request.entry_id.clone(),
        version: request.version.clone(),
    })
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
) {
    let Some(operation_id) = request.operation_id.clone() else {
        return;
    };
    let Some(emitter) = emitter else {
        return;
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
    emitter(payload);
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
    let mut names = state
        .skill_store
        .load_records()?
        .into_iter()
        .filter(|record| record.enabled)
        .map(|record| record.name)
        .collect::<HashSet<_>>();
    if let Some(harness) = state.harness() {
        names.extend(
            harness
                .list_runtime_skills()
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
    let harness = state.harness().ok_or_else(|| {
        runtime_unavailable("Importing skills requires the runtime skill facade.")
    })?;
    if let Some((emitter, request)) = progress_context {
        emit_skill_catalog_install_progress(emitter, request, "validating", 65, None);
    }
    let entry_path = source_path.join(SKILL_PACKAGE_ENTRY_FILE);
    let bytes =
        read_regular_file_no_follow(&entry_path, "skill entry file", MAX_SKILL_MARKDOWN_BYTES)?;
    let markdown = String::from_utf8(bytes)
        .map_err(|_| invalid_payload("skill entry file must be valid UTF-8".to_owned()))?;
    let validated = harness
        .validate_workspace_skill_markdown(&markdown, Some(entry_path))
        .await
        .map_err(|error| invalid_payload(error.to_string()))?;
    if let Some((emitter, request)) = progress_context {
        emit_skill_catalog_install_progress(emitter, request, "validating", 72, None);
    }
    let mut records = state.skill_store.load_records()?;
    let previous_records = records.clone();
    if records
        .iter()
        .any(|record| record.enabled && record.name == validated.summary.name)
        || harness
            .list_runtime_skills()
            .iter()
            .any(|skill| skill.name == validated.summary.name)
    {
        return Err(invalid_payload(format!(
            "active skill name already exists: {}",
            validated.summary.name
        )));
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
        emit_skill_catalog_install_progress(emitter, request, "copying", 82, None);
    }
    record.content_hash = state
        .skill_store
        .write_skill_package(&record.id, true, &source_path)?;
    let copied_markdown = state.skill_store.read_skill_entry_file(&record)?;
    let copied_validation = harness
        .validate_workspace_skill_markdown(&copied_markdown, None)
        .await
        .map_err(|error| {
            let _ = state.skill_store.delete_skill_package(&record.id);
            invalid_payload(error.to_string())
        })?;
    record.name = copied_validation.summary.name;
    record.description = copied_validation.summary.description;
    record.tags = copied_validation.summary.tags;
    record.category = copied_validation.summary.category;
    if records
        .iter()
        .any(|existing| existing.enabled && existing.name == record.name)
        || harness
            .list_runtime_skills()
            .iter()
            .any(|skill| skill.name == record.name)
    {
        let _ = state.skill_store.delete_skill_package(&record.id);
        return Err(invalid_payload(format!(
            "active skill name already exists: {}",
            record.name
        )));
    }
    records.retain(|existing| existing.id != record.id);
    records.push(record.clone());
    records.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    if let Err(error) = state.skill_store.save_records(&records) {
        let _ = state.skill_store.delete_skill_package(&record.id);
        return Err(error);
    }
    if let Some((emitter, request)) = progress_context {
        emit_skill_catalog_install_progress(emitter, request, "reloading", 95, None);
    }
    if let Err(error) = reload_managed_skills(state, &harness).await {
        let _ = state.skill_store.delete_skill_package(&record.id);
        let _ = state.skill_store.save_records(&previous_records);
        let _ = reload_managed_skills(state, &harness).await;
        return Err(error);
    }

    Ok(ImportSkillResponse {
        skill: managed_skill_summary(&record, runtime_status_for_name(&harness, &record.name)),
    })
}

pub async fn get_skill_detail_with_runtime_state(
    request: GetSkillDetailRequest,
    state: &DesktopRuntimeState,
) -> Result<GetSkillDetailResponse, CommandErrorPayload> {
    ensure_skill_id(&request.id)?;
    let records = state.skill_store.load_records()?;
    let record = records.iter().find(|record| record.id == request.id);
    let harness = state.harness();

    let Some(record) = record else {
        let harness = harness
            .as_ref()
            .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
        let view = harness
            .view_runtime_skill(&request.id, false)
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

    let runtime_view = harness.as_ref().and_then(|harness| {
        record
            .enabled
            .then(|| harness.view_runtime_skill(&record.name, false))
            .flatten()
    });
    let files = state.skill_store.list_skill_package_files(record)?;
    let detail = if let Some(view) = runtime_view {
        let status = skill_status_string(&view.summary.status);
        skill_detail_from_runtime_view(
            managed_skill_summary(record, Some(status)),
            view,
            files,
            record.last_validation_error.clone(),
        )
    } else {
        SkillDetailPayload {
            summary: managed_skill_summary(record, None),
            parameters: Vec::new(),
            config_keys: Vec::new(),
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
    let harness = state.harness().ok_or_else(|| {
        runtime_unavailable("Changing skill state requires the runtime skill facade.")
    })?;
    let mut records = state.skill_store.load_records()?;
    let record_index = records
        .iter()
        .position(|record| record.id == request.id)
        .ok_or_else(|| invalid_payload("skill not found".to_owned()))?;
    let record_name = records[record_index].name.clone();
    if records[record_index].enabled != request.enabled {
        if request.enabled
            && (records.iter().any(|candidate| {
                candidate.enabled && candidate.name == record_name && candidate.id != request.id
            }) || harness
                .list_runtime_skills()
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
        records[record_index].enabled = request.enabled;
        records[record_index].updated_at = now().to_rfc3339();
        records[record_index].last_validation_error = None;
        state.skill_store.save_records(&records)?;
        reload_managed_skills(state, &harness).await?;
    }
    let record = records[record_index].clone();
    Ok(SetSkillEnabledResponse {
        skill: managed_skill_summary(&record, runtime_status_for_name(&harness, &record.name)),
    })
}

pub async fn delete_skill_with_runtime_state(
    request: DeleteSkillRequest,
    state: &DesktopRuntimeState,
) -> Result<DeleteSkillResponse, CommandErrorPayload> {
    ensure_skill_id(&request.id)?;
    let harness = state
        .harness()
        .ok_or_else(|| runtime_unavailable("Deleting skills requires the runtime skill facade."))?;
    let mut records = state.skill_store.load_records()?;
    let original_len = records.len();
    records.retain(|record| record.id != request.id);
    if records.len() == original_len {
        return Err(invalid_payload("skill not found".to_owned()));
    }
    state.skill_store.delete_skill_package(&request.id)?;
    state.skill_store.save_records(&records)?;
    reload_managed_skills(state, &harness).await?;
    Ok(DeleteSkillResponse {
        id: request.id,
        status: "deleted",
    })
}

pub(crate) async fn reload_managed_skills(
    state: &DesktopRuntimeState,
    harness: &Harness,
) -> Result<(), CommandErrorPayload> {
    harness
        .reload_workspace_managed_skills(state.skill_store.enabled_dir())
        .await
        .map_err(|error| runtime_operation_failed(format!("skill reload failed: {error}")))
}
