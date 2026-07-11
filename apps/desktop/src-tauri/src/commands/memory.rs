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
use super::plugins::*;
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
use harness_contracts::ActionPlanId;

pub async fn list_memory_items_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListMemoryItemsResponse, CommandErrorPayload> {
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Listing memory requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    let mut items = settings_runtime
        .list_memory_items(options)
        .await
        .map_err(|_| memory_operation_failed("Memory items could not be loaded."))?
        .into_iter()
        .map(memory_item_summary_payload)
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then(left.id.cmp(&right.id))
    });

    Ok(ListMemoryItemsResponse { items })
}

pub async fn get_memory_item_with_runtime_state(
    request: GetMemoryItemRequest,
    state: &DesktopRuntimeState,
) -> Result<GetMemoryItemResponse, CommandErrorPayload> {
    let id = parse_memory_id(&request.id)?;
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Inspecting memory requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    let summary = settings_runtime
        .list_memory_items(options.clone())
        .await
        .ok()
        .and_then(|items| items.into_iter().find(|item| item.id == id))
        .ok_or_else(|| memory_operation_failed("Memory detail metadata could not be loaded."))?;
    let item = settings_runtime
        .get_memory_item(options, id)
        .await
        .map_err(|_| memory_operation_failed("Memory detail could not be loaded."))?;

    Ok(GetMemoryItemResponse {
        item: memory_item_payload_with_summary(item, &summary),
    })
}

pub async fn update_memory_item_with_runtime_state(
    request: UpdateMemoryItemRequest,
    state: &DesktopRuntimeState,
) -> Result<UpdateMemoryItemResponse, CommandErrorPayload> {
    let id = parse_memory_id(&request.id)?;
    let action_plan_id = parse_optional_action_plan_id(request.action_plan_id.as_deref())?;
    ensure_non_empty("content", &request.content)?;
    ensure_max_bytes("content", &request.content, MAX_MEMORY_CONTENT_BYTES)?;
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Editing memory requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    let item = settings_runtime
        .update_memory_item_content(options, id, request.content, action_plan_id)
        .await
        .map_err(|_| memory_operation_failed("Memory item could not be saved."))?;
    let summary = settings_runtime
        .list_memory_items(state.settings_session_options(state.default_conversation_id)?)
        .await
        .ok()
        .and_then(|items| items.into_iter().find(|item| item.id == id))
        .ok_or_else(|| memory_operation_failed("Memory item metadata could not be loaded."))?;

    Ok(UpdateMemoryItemResponse {
        item: memory_item_payload_with_summary(item, &summary),
    })
}

pub async fn delete_memory_item_with_runtime_state(
    request: DeleteMemoryItemRequest,
    state: &DesktopRuntimeState,
) -> Result<DeleteMemoryItemResponse, CommandErrorPayload> {
    let id = parse_memory_id(&request.id)?;
    let action_plan_id = parse_optional_action_plan_id(request.action_plan_id.as_deref())?;
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Deleting memory requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    settings_runtime
        .delete_memory_item(options, id, action_plan_id)
        .await
        .map_err(|_| memory_operation_failed("Memory item could not be deleted."))?;

    Ok(DeleteMemoryItemResponse {
        id: request.id,
        status: "deleted",
    })
}

pub async fn export_memory_items_with_runtime_state(
    request: ExportMemoryItemsRequest,
    state: &DesktopRuntimeState,
) -> Result<ExportMemoryItemsResponse, CommandErrorPayload> {
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Exporting memory requires the runtime memory facade.",
        ));
    };
    let options = state
        .settings_session_options(request.session_id.unwrap_or(state.default_conversation_id))?;
    let export = settings_runtime
        .export_memory_items(
            options,
            request.scope.as_str(),
            request.format.as_str(),
            request.include_raw_content,
            request.include_metadata,
            request.include_hashes,
            request.explicit_user_action,
        )
        .await
        .map_err(memory_export_error)?;

    Ok(ExportMemoryItemsResponse {
        exported_at: export.exported_at.to_rfc3339(),
        format: export.format,
        scope: export.scope,
        include_raw_content: export.include_raw_content,
        include_metadata: export.include_metadata,
        include_hashes: export.include_hashes,
        item_count: export.item_count,
        path: export.relative_path.to_string_lossy().into_owned(),
        audit_hash: export.audit_hash,
    })
}

pub async fn list_memory_candidates_with_runtime_state(
    request: ListMemoryCandidatesRequest,
    state: &DesktopRuntimeState,
) -> Result<ListMemoryCandidatesResponse, CommandErrorPayload> {
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Listing memory candidates requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    settings_runtime
        .list_memory_candidates(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory candidates could not be loaded."))
}

pub async fn approve_memory_candidate_with_runtime_state(
    request: ApproveMemoryCandidateRequest,
    state: &DesktopRuntimeState,
) -> Result<ApproveMemoryCandidateResponse, CommandErrorPayload> {
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Approving memory candidates requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    settings_runtime
        .approve_memory_candidate(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory candidate could not be approved."))
}

pub async fn reject_memory_candidate_with_runtime_state(
    request: RejectMemoryCandidateRequest,
    state: &DesktopRuntimeState,
) -> Result<RejectMemoryCandidateResponse, CommandErrorPayload> {
    ensure_non_empty("reason", &request.reason)?;
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Rejecting memory candidates requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    settings_runtime
        .reject_memory_candidate(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory candidate could not be rejected."))
}

pub async fn merge_memory_candidate_with_runtime_state(
    request: MergeMemoryCandidateRequest,
    state: &DesktopRuntimeState,
) -> Result<MergeMemoryCandidateResponse, CommandErrorPayload> {
    if request.candidate_ids.len() < 2 {
        return Err(invalid_payload(
            "candidate_ids must contain at least two candidates".to_owned(),
        ));
    }
    let distinct_candidate_ids = request
        .candidate_ids
        .iter()
        .map(ToString::to_string)
        .collect::<std::collections::HashSet<_>>();
    if distinct_candidate_ids.len() != request.candidate_ids.len() {
        return Err(invalid_payload(
            "candidate_ids must contain distinct candidates".to_owned(),
        ));
    }
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Merging memory candidates requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    settings_runtime
        .merge_memory_candidate(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory candidates could not be merged."))
}

pub async fn list_memory_recall_traces_with_runtime_state(
    request: ListMemoryRecallTracesRequest,
    state: &DesktopRuntimeState,
) -> Result<ListMemoryRecallTracesResponse, CommandErrorPayload> {
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Listing memory recall traces requires the runtime memory facade.",
        ));
    };
    let options = state
        .settings_session_options(request.session_id.unwrap_or(state.default_conversation_id))?;
    settings_runtime
        .list_memory_recall_traces(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory recall traces could not be loaded."))
}

pub async fn get_memory_recall_trace_with_runtime_state(
    request: GetMemoryRecallTraceRequest,
    state: &DesktopRuntimeState,
) -> Result<GetMemoryRecallTraceResponse, CommandErrorPayload> {
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Loading memory recall traces requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    settings_runtime
        .get_memory_recall_trace(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory recall trace could not be loaded."))
}

pub async fn get_model_request_preview_with_runtime_state(
    request: GetModelRequestPreviewRequest,
    state: &DesktopRuntimeState,
) -> Result<GetModelRequestPreviewResponse, CommandErrorPayload> {
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Building model request preview requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(request.session_id)?;
    settings_runtime
        .get_model_request_preview(options, request)
        .await
        .map_err(|_| memory_operation_failed("Model request preview could not be built."))
}

pub(crate) fn parse_memory_id(value: &str) -> Result<MemoryId, CommandErrorPayload> {
    ensure_non_empty("id", value)?;
    let value = value.trim();
    let id = MemoryId::parse(value)
        .map_err(|_| invalid_payload("id must be a valid memory id".to_owned()))?;

    if id.to_string() != value {
        return Err(invalid_payload(
            "id must be a canonical memory id".to_owned(),
        ));
    }

    Ok(id)
}

pub(crate) fn parse_action_plan_id(value: &str) -> Result<ActionPlanId, CommandErrorPayload> {
    ensure_non_empty("actionPlanId", value)?;
    let value = value.trim();
    let id = ActionPlanId::parse(value)
        .map_err(|_| invalid_payload("actionPlanId must be a valid action plan id".to_owned()))?;

    if id.to_string() != value {
        return Err(invalid_payload(
            "actionPlanId must be a canonical action plan id".to_owned(),
        ));
    }

    Ok(id)
}

pub(crate) fn parse_optional_action_plan_id(
    value: Option<&str>,
) -> Result<Option<ActionPlanId>, CommandErrorPayload> {
    value.map(parse_action_plan_id).transpose()
}

pub(crate) fn memory_item_summary_payload(summary: MemorySummary) -> MemoryItemSummaryPayload {
    MemoryItemSummaryPayload {
        content_hash: content_hash_payload(&summary.content_hash),
        content_preview: summary.content_preview,
        deleted: summary.deleted,
        expires_at: summary.expires_at.map(|at| at.to_rfc3339()),
        id: summary.id.to_string(),
        kind: memory_kind_payload(&summary.kind).to_owned(),
        last_accessed_at: summary.metadata.last_accessed_at.map(|at| at.to_rfc3339()),
        provider_id: summary.provider_id,
        source: memory_source_payload(&summary.metadata.source).to_owned(),
        tags: summary.metadata.tags,
        updated_at: summary.updated_at.to_rfc3339(),
        visibility: memory_visibility_payload(&summary.visibility).to_owned(),
    }
}

pub(crate) fn memory_item_payload_with_summary(
    record: MemoryRecord,
    summary: &MemorySummary,
) -> MemoryItemPayload {
    MemoryItemPayload {
        access_count: record.metadata.access_count,
        confidence: record.metadata.confidence,
        content: record.content,
        content_hash: content_hash_payload(&summary.content_hash),
        created_at: record.created_at.to_rfc3339(),
        deleted: summary.deleted,
        expires_at: summary.expires_at.map(|at| at.to_rfc3339()),
        id: record.id.to_string(),
        kind: memory_kind_payload(&record.kind).to_owned(),
        last_accessed_at: summary.metadata.last_accessed_at.map(|at| at.to_rfc3339()),
        provider_id: summary.provider_id.clone(),
        source: memory_source_payload(&record.metadata.source).to_owned(),
        tags: record.metadata.tags,
        updated_at: record.updated_at.to_rfc3339(),
        visibility: memory_visibility_payload(&record.visibility).to_owned(),
    }
}

fn content_hash_payload(hash: &harness_contracts::ContentHash) -> String {
    hash.0.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn memory_kind_payload(kind: &MemoryKind) -> &'static str {
    match kind {
        MemoryKind::UserPreference => "user_preference",
        MemoryKind::Feedback => "feedback",
        MemoryKind::ProjectFact => "project_fact",
        MemoryKind::Reference => "reference",
        MemoryKind::AgentSelfNote => "agent_self_note",
        MemoryKind::Custom(_) => "custom",
        _ => "custom",
    }
}

pub(crate) fn memory_visibility_payload(visibility: &MemoryVisibility) -> &'static str {
    match visibility {
        MemoryVisibility::Private { .. } => "private",
        MemoryVisibility::User { .. } => "user",
        MemoryVisibility::Team { .. } => "team",
        MemoryVisibility::Tenant => "tenant",
        _ => "tenant",
    }
}

pub(crate) fn memory_source_payload(source: &MemorySource) -> &'static str {
    match source {
        MemorySource::UserInput => "user_input",
        MemorySource::AgentDerived => "agent_derived",
        MemorySource::SubagentDerived { .. } => "subagent_derived",
        MemorySource::ExternalRetrieval => "external_retrieval",
        MemorySource::Imported => "imported",
        MemorySource::Consolidated { .. } => "consolidated",
        _ => "imported",
    }
}
