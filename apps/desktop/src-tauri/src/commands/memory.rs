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

pub async fn list_memory_items_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListMemoryItemsResponse, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Listing memory requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id);
    let mut items = harness
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
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Inspecting memory requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id);
    let item = harness
        .get_memory_item(options, id)
        .await
        .map_err(|_| memory_operation_failed("Memory detail could not be loaded."))?;

    Ok(GetMemoryItemResponse {
        item: memory_item_payload(item),
    })
}

pub async fn update_memory_item_with_runtime_state(
    request: UpdateMemoryItemRequest,
    state: &DesktopRuntimeState,
) -> Result<UpdateMemoryItemResponse, CommandErrorPayload> {
    let id = parse_memory_id(&request.id)?;
    ensure_non_empty("content", &request.content)?;
    ensure_max_bytes("content", &request.content, MAX_MEMORY_CONTENT_BYTES)?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Editing memory requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id);
    let item = harness
        .update_memory_item_content(options, id, request.content)
        .await
        .map_err(|_| memory_operation_failed("Memory item could not be saved."))?;

    Ok(UpdateMemoryItemResponse {
        item: memory_item_payload(item),
    })
}

pub async fn delete_memory_item_with_runtime_state(
    request: DeleteMemoryItemRequest,
    state: &DesktopRuntimeState,
) -> Result<DeleteMemoryItemResponse, CommandErrorPayload> {
    let id = parse_memory_id(&request.id)?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Deleting memory requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id);
    harness
        .delete_memory_item(options, id)
        .await
        .map_err(|_| memory_operation_failed("Memory item could not be deleted."))?;

    Ok(DeleteMemoryItemResponse {
        id: request.id,
        status: "deleted",
    })
}

pub async fn export_memory_items_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ExportMemoryItemsResponse, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Exporting memory requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id);
    let records = harness
        .export_memory_items(options)
        .await
        .map_err(|_| memory_operation_failed("Memory export could not be prepared."))?;
    let item_count = records.len().min(u32::MAX as usize) as u32;
    let items = records
        .into_iter()
        .map(memory_item_payload)
        .collect::<Vec<_>>();
    let content = serde_json::to_string_pretty(&items)
        .map_err(|_| memory_operation_failed("Memory export could not be prepared."))?;
    let exported_at = jyowo_harness_sdk::ext::now();
    let file_name = format!("memory-{}.json", exported_at.format("%Y%m%dT%H%M%S%.3fZ"));
    let relative_path = PathBuf::from(".jyowo")
        .join("runtime")
        .join("exports")
        .join(file_name);
    let export_path = state.workspace_root.join(&relative_path);
    write_memory_export_file(&export_path, &content)?;

    Ok(ExportMemoryItemsResponse {
        exported_at: exported_at.to_rfc3339(),
        format: "json",
        item_count,
        path: relative_path.to_string_lossy().into_owned(),
    })
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

pub(crate) fn memory_item_summary_payload(summary: MemorySummary) -> MemoryItemSummaryPayload {
    MemoryItemSummaryPayload {
        content_preview: summary.content_preview,
        id: summary.id.to_string(),
        kind: memory_kind_payload(&summary.kind).to_owned(),
        source: memory_source_payload(&summary.metadata.source).to_owned(),
        tags: summary.metadata.tags,
        updated_at: summary.updated_at.to_rfc3339(),
        visibility: memory_visibility_payload(&summary.visibility).to_owned(),
    }
}

pub(crate) fn memory_item_payload(record: MemoryRecord) -> MemoryItemPayload {
    MemoryItemPayload {
        access_count: record.metadata.access_count,
        confidence: record.metadata.confidence,
        content: record.content,
        created_at: record.created_at.to_rfc3339(),
        id: record.id.to_string(),
        kind: memory_kind_payload(&record.kind).to_owned(),
        source: memory_source_payload(&record.metadata.source).to_owned(),
        tags: record.metadata.tags,
        updated_at: record.updated_at.to_rfc3339(),
        visibility: memory_visibility_payload(&record.visibility).to_owned(),
    }
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
