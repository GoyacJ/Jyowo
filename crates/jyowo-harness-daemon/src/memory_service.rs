use std::path::{Path, PathBuf};

use chrono::Utc;
use harness_contracts::{
    ClientRequest, ContentHash, DaemonMemoryItem, DaemonMemoryItemSummary,
    DeleteMemoryItemResponse, ExportMemoryItemsResponse, GetMemoryItemResponse,
    GetMemoryRecallTraceResponse, GetMemorySettingsResponse, GetModelRequestPreviewResponse,
    GetThreadMemorySettingsResponse, ListMemoryCandidatesResponse, ListMemoryItemsResponse,
    ListMemoryRecallTracesResponse, MemoryActor, MemoryActorContext, MemoryCandidate,
    MemoryCandidateListItem, MemoryCandidateState, MemoryEvidence, MemoryEvidenceOrigin, MemoryId,
    MemoryModelRequestPreview, MemoryModelRequestPreviewSection, MemoryPermissionContext,
    MemoryPolicyDecision, MemorySource, MemoryVisibility, MergeMemoryCandidateResponse,
    ServerMessage, SessionId, TenantId, UpdateMemoryItemResponse, UpdateMemorySettingsResponse,
    UpdateThreadMemorySettingsResponse,
};
use harness_memory::{
    default_thread_settings, local::LocalMemoryProvider, MemoryInbox, MemoryListScope,
    MemoryMetadata, MemoryPolicyEngine, MemoryRecallTraceCollector, MemoryRecord,
    MemorySettingsStore, MemoryStore,
};
use thiserror::Error;

use crate::{RuntimeConfigError, RuntimeConfigResolver};

const MAX_MEMORY_CONTENT_BYTES: usize = 64 * 1024;

/// Owns every user-facing memory operation served by the daemon.
#[derive(Debug, Clone)]
pub struct MemoryService {
    runtime_config: RuntimeConfigResolver,
}

impl MemoryService {
    #[must_use]
    pub fn new(runtime_config: RuntimeConfigResolver) -> Self {
        Self { runtime_config }
    }

    pub fn database_path(
        &self,
        workspace_root: Option<&Path>,
    ) -> Result<PathBuf, RuntimeConfigError> {
        self.runtime_config
            .resolve_memory_database_path(workspace_root)
    }

    pub async fn handle(
        &self,
        request: ClientRequest,
    ) -> Result<ServerMessage, MemoryServiceError> {
        ensure_single_tenant_request(&request)?;
        let workspace_root = memory_workspace_root(&request)?;
        let db_path = self.database_path(workspace_root.as_deref())?;
        match request {
            ClientRequest::ListMemoryItems { .. } => {
                let provider = local_provider(&db_path, TenantId::SINGLE)?;
                let mut items = provider
                    .list(MemoryListScope::ForActor(management_actor(
                        TenantId::SINGLE,
                        None,
                    )))
                    .await?;
                items.sort_by(|left, right| {
                    right
                        .updated_at
                        .cmp(&left.updated_at)
                        .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
                });
                Ok(ServerMessage::MemoryItems(ListMemoryItemsResponse {
                    items: items.into_iter().map(summary_payload).collect(),
                }))
            }
            ClientRequest::GetMemoryItem { memory_id, .. } => {
                let provider = local_provider(&db_path, TenantId::SINGLE)?;
                let item = memory_item(
                    &provider,
                    memory_id,
                    &management_actor(TenantId::SINGLE, None),
                )
                .await?;
                Ok(ServerMessage::MemoryItem(GetMemoryItemResponse { item }))
            }
            ClientRequest::UpdateMemoryItem {
                memory_id,
                content,
                action_plan_id,
                ..
            } => {
                if content.trim().is_empty() {
                    return Err(MemoryServiceError::Invalid(
                        "memory content must not be empty".to_owned(),
                    ));
                }
                if content.len() > MAX_MEMORY_CONTENT_BYTES {
                    return Err(MemoryServiceError::Invalid(format!(
                        "memory content must not exceed {MAX_MEMORY_CONTENT_BYTES} bytes"
                    )));
                }
                let provider = local_provider(&db_path, TenantId::SINGLE)?;
                ensure_memory_visible(
                    &provider,
                    memory_id,
                    &management_actor(TenantId::SINGLE, None),
                )
                .await?;
                let mut record = provider.get(memory_id).await?;
                authorize_memory_write(
                    &db_path,
                    action_plan_id,
                    mutation_evidence(action_plan_id, "memory-item-update", &content),
                    &record.visibility,
                )?;
                record.content = content;
                record.updated_at = Utc::now();
                provider.upsert(record).await?;
                let item = memory_item(
                    &provider,
                    memory_id,
                    &management_actor(TenantId::SINGLE, None),
                )
                .await?;
                Ok(ServerMessage::MemoryUpdated(UpdateMemoryItemResponse {
                    item,
                }))
            }
            ClientRequest::DeleteMemoryItem {
                memory_id,
                action_plan_id,
                ..
            } => {
                let provider = local_provider(&db_path, TenantId::SINGLE)?;
                ensure_memory_visible(
                    &provider,
                    memory_id,
                    &management_actor(TenantId::SINGLE, None),
                )
                .await?;
                authorize_memory_delete(&db_path, action_plan_id, None)?;
                provider.forget(memory_id).await?;
                Ok(ServerMessage::MemoryDeleted(DeleteMemoryItemResponse {
                    memory_id,
                }))
            }
            ClientRequest::ExportMemoryItems { request, .. } => {
                if !request.explicit_user_action
                    || request.scope != "visible"
                    || request.format != "json"
                {
                    return Err(MemoryServiceError::Invalid(
                        "memory export requires explicit visible JSON export".to_owned(),
                    ));
                }
                let provider = local_provider(&db_path, TenantId::SINGLE)?;
                let summaries = provider
                    .list(MemoryListScope::ForActor(management_actor(
                        TenantId::SINGLE,
                        request.session_id,
                    )))
                    .await?;
                let mut values = Vec::with_capacity(summaries.len());
                for summary in &summaries {
                    let record = provider.get(summary.id).await?;
                    let mut value = serde_json::Map::new();
                    value.insert("id".to_owned(), serde_json::json!(record.id));
                    value.insert(
                        "kind".to_owned(),
                        serde_json::json!(kind_name(&record.kind)),
                    );
                    value.insert(
                        "visibility".to_owned(),
                        serde_json::json!(visibility_name(&record.visibility)),
                    );
                    if request.include_raw_content {
                        value.insert("content".to_owned(), serde_json::json!(record.content));
                    }
                    if request.include_metadata {
                        value.insert("tags".to_owned(), serde_json::json!(record.metadata.tags));
                        value.insert(
                            "source".to_owned(),
                            serde_json::json!(source_name(&record.metadata.source)),
                        );
                    }
                    if request.include_hashes {
                        value.insert(
                            "contentHash".to_owned(),
                            serde_json::json!(content_hash_hex(&summary.content_hash)),
                        );
                    }
                    values.push(serde_json::Value::Object(value));
                }
                let exported_at = Utc::now();
                let bytes = serde_json::to_vec_pretty(&values)?;
                let audit_hash = blake3::hash(&bytes).to_hex().to_string();
                let export_dir = self
                    .runtime_config
                    .resolve_memory_export_directory(workspace_root.as_deref())?;
                let path = export_dir.join(format!(
                    "memory-{}.json",
                    exported_at.format("%Y%m%dT%H%M%S%.fZ")
                ));
                std::fs::write(&path, bytes)?;
                Ok(ServerMessage::MemoryExported(ExportMemoryItemsResponse {
                    exported_at,
                    format: request.format,
                    scope: request.scope,
                    include_raw_content: request.include_raw_content,
                    include_metadata: request.include_metadata,
                    include_hashes: request.include_hashes,
                    item_count: summaries.len() as u32,
                    path: path.to_string_lossy().into_owned(),
                    audit_hash,
                }))
            }
            ClientRequest::ListMemoryCandidates { request, .. } => {
                let inbox = MemoryInbox::open(&db_path.to_string_lossy(), request.tenant_id)
                    .map_err(MemoryServiceError::Store)?;
                let candidates = inbox
                    .list(request.state)
                    .map_err(MemoryServiceError::Store)?
                    .into_iter()
                    .filter(|candidate| {
                        request.session_id.is_none()
                            || candidate.evidence.session_id == request.session_id
                    })
                    .take(request.limit.max(1) as usize)
                    .map(candidate_list_item)
                    .collect();
                Ok(ServerMessage::MemoryCandidates(
                    ListMemoryCandidatesResponse {
                        candidates,
                        next_cursor: None,
                    },
                ))
            }
            ClientRequest::ApproveMemoryCandidate { request, .. } => {
                let inbox = MemoryInbox::open(&db_path.to_string_lossy(), request.tenant_id)
                    .map_err(MemoryServiceError::Store)?;
                let candidate = find_candidate(&inbox, request.candidate_id)?;
                authorize_memory_write(
                    &db_path,
                    request.action_plan_id,
                    candidate.evidence.clone(),
                    &candidate.proposed_record.visibility,
                )?;
                let (candidate, memory_id) = inbox
                    .promote_into_memory(request.candidate_id)
                    .map_err(MemoryServiceError::Store)?;
                Ok(ServerMessage::MemoryCandidateApproved(
                    harness_contracts::ApproveMemoryCandidateResponse {
                        candidate,
                        memory_id,
                    },
                ))
            }
            ClientRequest::RejectMemoryCandidate { request, .. } => {
                if request.reason.trim().is_empty() {
                    return Err(MemoryServiceError::Invalid(
                        "candidate rejection reason must not be empty".into(),
                    ));
                }
                let inbox = MemoryInbox::open(&db_path.to_string_lossy(), request.tenant_id)
                    .map_err(MemoryServiceError::Store)?;
                let candidate = inbox
                    .reject(request.candidate_id)
                    .map_err(MemoryServiceError::Store)?;
                Ok(ServerMessage::MemoryCandidateRejected(
                    harness_contracts::RejectMemoryCandidateResponse { candidate },
                ))
            }
            ClientRequest::MergeMemoryCandidate { request, .. } => {
                if request.candidate_ids.len() < 2 {
                    return Err(MemoryServiceError::Invalid(
                        "at least two candidates are required".into(),
                    ));
                }
                if request
                    .candidate_ids
                    .iter()
                    .enumerate()
                    .any(|(index, id)| request.candidate_ids[..index].contains(id))
                {
                    return Err(MemoryServiceError::Invalid(
                        "candidate IDs must be distinct".into(),
                    ));
                }
                let inbox = MemoryInbox::open(&db_path.to_string_lossy(), request.tenant_id)
                    .map_err(MemoryServiceError::Store)?;
                for id in &request.candidate_ids {
                    let candidate = find_candidate(&inbox, *id)?;
                    if candidate.state != MemoryCandidateState::Proposed {
                        return Err(MemoryServiceError::Invalid(format!(
                            "candidate is not proposed: {id}"
                        )));
                    }
                }
                authorize_memory_write(
                    &db_path,
                    request.action_plan_id,
                    request.evidence.clone(),
                    &request.merged_record.visibility,
                )?;
                let now = Utc::now();
                let record = MemoryRecord {
                    id: MemoryId::new(),
                    tenant_id: request.tenant_id,
                    kind: request.merged_record.kind,
                    visibility: request.merged_record.visibility,
                    content: request.merged_record.content,
                    metadata: MemoryMetadata {
                        tags: request.merged_record.metadata.tags,
                        source: request.evidence.source.clone(),
                        evidence: Some(request.evidence),
                        confidence: request.merged_record.metadata.source_trust.clamp(0.0, 1.0)
                            as f32,
                        access_count: 0,
                        last_accessed_at: None,
                        recall_score: 0.0,
                        recall_score_breakdown: None,
                        ttl: request.merged_record.metadata.ttl,
                        redacted_segments: 0,
                    },
                    created_at: now,
                    updated_at: now,
                };
                let memory_id = inbox
                    .merge_into_memory(&request.candidate_ids, &record)
                    .map_err(MemoryServiceError::Store)?;
                Ok(ServerMessage::MemoryCandidatesMerged(
                    MergeMemoryCandidateResponse {
                        candidate_ids: request.candidate_ids,
                        memory_id,
                    },
                ))
            }
            ClientRequest::ListMemoryRecallTraces { request, .. } => {
                let collector = MemoryRecallTraceCollector::open(&db_path.to_string_lossy())
                    .map_err(MemoryServiceError::Store)?;
                let traces = collector
                    .list_summaries(request.tenant_id, request.session_id, request.run_id)
                    .into_iter()
                    .take(request.limit.max(1) as usize)
                    .collect();
                Ok(ServerMessage::MemoryRecallTraces(
                    ListMemoryRecallTracesResponse {
                        traces,
                        next_cursor: None,
                    },
                ))
            }
            ClientRequest::GetMemoryRecallTrace { request, .. } => {
                let collector = MemoryRecallTraceCollector::open(&db_path.to_string_lossy())
                    .map_err(MemoryServiceError::Store)?;
                let trace = collector
                    .get(request.tenant_id, request.trace_id)
                    .ok_or_else(|| MemoryServiceError::NotFound("memory recall trace".into()))?;
                Ok(ServerMessage::MemoryRecallTrace(
                    GetMemoryRecallTraceResponse { trace },
                ))
            }
            ClientRequest::GetModelRequestPreview { request, .. } => {
                let collector = MemoryRecallTraceCollector::open(&db_path.to_string_lossy())
                    .map_err(MemoryServiceError::Store)?;
                let preview = collector
                    .get_model_request_preview(
                        request.tenant_id,
                        request.session_id,
                        request.run_id,
                        request.trace_id,
                    )
                    .unwrap_or_else(|| {
                        let trace = request
                            .trace_id
                            .and_then(|trace_id| collector.get(request.tenant_id, trace_id))
                            .or_else(|| {
                                collector
                                    .for_run(request.tenant_id, request.session_id, request.run_id)
                                    .into_iter()
                                    .max_by_key(|trace| trace.at)
                            });
                        trace.map_or_else(
                            || {
                                empty_model_request_preview(
                                    request.session_id,
                                    request.run_id,
                                    request.trace_id,
                                )
                            },
                            model_request_preview_from_trace,
                        )
                    });
                Ok(ServerMessage::ModelRequestPreview(
                    GetModelRequestPreviewResponse { preview },
                ))
            }
            ClientRequest::GetMemorySettings { request, .. } => {
                let settings = MemorySettingsStore::open(&db_path.to_string_lossy())
                    .map_err(MemoryServiceError::Store)?
                    .get_global(request.tenant_id)
                    .map_err(MemoryServiceError::Store)?;
                Ok(ServerMessage::MemorySettings(GetMemorySettingsResponse {
                    settings,
                }))
            }
            ClientRequest::UpdateMemorySettings { request, .. } => {
                validate_global_settings(&request.settings)?;
                let settings = MemorySettingsStore::open(&db_path.to_string_lossy())
                    .map_err(MemoryServiceError::Store)?
                    .update_global(request.tenant_id, request.settings)
                    .map_err(MemoryServiceError::Store)?;
                Ok(ServerMessage::MemorySettingsUpdated(
                    UpdateMemorySettingsResponse { settings },
                ))
            }
            ClientRequest::GetThreadMemorySettings { request, .. } => {
                let settings = MemorySettingsStore::open(&db_path.to_string_lossy())
                    .map_err(MemoryServiceError::Store)?
                    .get_thread(request.tenant_id, request.session_id)
                    .map_err(MemoryServiceError::Store)?;
                Ok(ServerMessage::ThreadMemorySettings(
                    GetThreadMemorySettingsResponse { settings },
                ))
            }
            ClientRequest::UpdateThreadMemorySettings { request, .. } => {
                let settings = MemorySettingsStore::open(&db_path.to_string_lossy())
                    .map_err(MemoryServiceError::Store)?
                    .update_thread(request.tenant_id, request.settings)
                    .map_err(MemoryServiceError::Store)?;
                Ok(ServerMessage::ThreadMemorySettingsUpdated(
                    UpdateThreadMemorySettingsResponse { settings },
                ))
            }
            _ => Err(MemoryServiceError::Invalid(
                "request is not a memory operation".to_owned(),
            )),
        }
    }
}

fn validate_global_settings(
    settings: &harness_contracts::MemoryGlobalSettings,
) -> Result<(), MemoryServiceError> {
    if settings.max_memory_bytes == 0 {
        return Err(MemoryServiceError::Invalid(
            "max_memory_bytes must be greater than zero".into(),
        ));
    }
    if settings.max_recall_records_per_turn == 0 {
        return Err(MemoryServiceError::Invalid(
            "max_recall_records_per_turn must be greater than zero".into(),
        ));
    }
    if settings.max_recall_chars_per_turn == 0 {
        return Err(MemoryServiceError::Invalid(
            "max_recall_chars_per_turn must be greater than zero".into(),
        ));
    }
    Ok(())
}

fn ensure_single_tenant_request(request: &ClientRequest) -> Result<(), MemoryServiceError> {
    let tenant_id = match request {
        ClientRequest::ListMemoryCandidates { request, .. } => Some(request.tenant_id),
        ClientRequest::ApproveMemoryCandidate { request, .. } => Some(request.tenant_id),
        ClientRequest::RejectMemoryCandidate { request, .. } => Some(request.tenant_id),
        ClientRequest::MergeMemoryCandidate { request, .. } => Some(request.tenant_id),
        ClientRequest::ListMemoryRecallTraces { request, .. } => Some(request.tenant_id),
        ClientRequest::GetMemoryRecallTrace { request, .. } => Some(request.tenant_id),
        ClientRequest::GetModelRequestPreview { request, .. } => Some(request.tenant_id),
        ClientRequest::GetMemorySettings { request, .. } => Some(request.tenant_id),
        ClientRequest::UpdateMemorySettings { request, .. } => Some(request.tenant_id),
        ClientRequest::GetThreadMemorySettings { request, .. } => Some(request.tenant_id),
        ClientRequest::UpdateThreadMemorySettings { request, .. } => Some(request.tenant_id),
        _ => None,
    };
    if tenant_id.is_some_and(|tenant_id| tenant_id != TenantId::SINGLE) {
        return Err(MemoryServiceError::Invalid(
            "daemon memory requests require the single-user tenant".into(),
        ));
    }
    Ok(())
}

fn authorize_memory_write(
    db_path: &Path,
    action_plan_id: Option<harness_contracts::ActionPlanId>,
    evidence: MemoryEvidence,
    visibility: &MemoryVisibility,
) -> Result<(), MemoryServiceError> {
    let (engine, thread) = memory_policy(db_path, evidence.session_id)?;
    authorize_policy_decision(engine.evaluate_write(
        &thread,
        &MemoryActor::User { user_label: None },
        &evidence,
        &manual_memory_permission(action_plan_id),
        visibility,
    ))
}

fn authorize_memory_delete(
    db_path: &Path,
    action_plan_id: Option<harness_contracts::ActionPlanId>,
    session_id: Option<SessionId>,
) -> Result<(), MemoryServiceError> {
    let (engine, thread) = memory_policy(db_path, session_id)?;
    authorize_policy_decision(engine.evaluate_delete(
        &thread,
        &MemoryActor::User { user_label: None },
        &manual_memory_permission(action_plan_id),
    ))
}

fn memory_policy(
    db_path: &Path,
    session_id: Option<SessionId>,
) -> Result<(MemoryPolicyEngine, harness_contracts::MemoryThreadSettings), MemoryServiceError> {
    let store =
        MemorySettingsStore::open(&db_path.to_string_lossy()).map_err(MemoryServiceError::Store)?;
    let global = store
        .get_global(TenantId::SINGLE)
        .map_err(MemoryServiceError::Store)?;
    let thread = match session_id {
        Some(session_id) => store
            .get_thread(TenantId::SINGLE, session_id)
            .map_err(MemoryServiceError::Store)?,
        None => default_thread_settings(SessionId::new()),
    };
    Ok((MemoryPolicyEngine::new(global), thread))
}

fn manual_memory_permission(
    action_plan_id: Option<harness_contracts::ActionPlanId>,
) -> MemoryPermissionContext {
    MemoryPermissionContext {
        explicit_user_instruction: true,
        include_raw_content: false,
        action_plan_id,
        authorization_ticket_id: None,
        non_interactive_policy_grant: false,
    }
}

fn mutation_evidence(
    action_plan_id: Option<harness_contracts::ActionPlanId>,
    operation: &str,
    content: &str,
) -> MemoryEvidence {
    MemoryEvidence {
        source: MemorySource::UserInput,
        origin: MemoryEvidenceOrigin::Imported {
            importer: operation.to_owned(),
            import_id: action_plan_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| operation.to_owned()),
        },
        content_hash: ContentHash(*blake3::hash(content.as_bytes()).as_bytes()),
        session_id: None,
        run_id: None,
        message_id: None,
        tool_use_id: None,
    }
}

fn authorize_policy_decision(decision: MemoryPolicyDecision) -> Result<(), MemoryServiceError> {
    match decision {
        MemoryPolicyDecision::Allow => Ok(()),
        denied => Err(MemoryServiceError::PolicyDenied(format!("{denied:?}"))),
    }
}

#[derive(Debug, Error)]
pub enum MemoryServiceError {
    #[error("runtime configuration failed: {0}")]
    RuntimeConfig(#[from] RuntimeConfigError),
    #[error("memory operation failed: {0}")]
    Memory(#[from] harness_contracts::MemoryError),
    #[error("memory store failed: {0}")]
    Store(String),
    #[error("invalid memory request: {0}")]
    Invalid(String),
    #[error("memory mutation denied by policy: {0}")]
    PolicyDenied(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("memory export I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("memory export serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

fn memory_workspace_root(request: &ClientRequest) -> Result<Option<PathBuf>, MemoryServiceError> {
    let root = match request {
        ClientRequest::ListMemoryItems { workspace_root }
        | ClientRequest::GetMemoryItem { workspace_root, .. }
        | ClientRequest::UpdateMemoryItem { workspace_root, .. }
        | ClientRequest::DeleteMemoryItem { workspace_root, .. }
        | ClientRequest::ExportMemoryItems { workspace_root, .. }
        | ClientRequest::ListMemoryCandidates { workspace_root, .. }
        | ClientRequest::ApproveMemoryCandidate { workspace_root, .. }
        | ClientRequest::RejectMemoryCandidate { workspace_root, .. }
        | ClientRequest::MergeMemoryCandidate { workspace_root, .. }
        | ClientRequest::ListMemoryRecallTraces { workspace_root, .. }
        | ClientRequest::GetMemoryRecallTrace { workspace_root, .. }
        | ClientRequest::GetModelRequestPreview { workspace_root, .. }
        | ClientRequest::GetMemorySettings { workspace_root, .. }
        | ClientRequest::UpdateMemorySettings { workspace_root, .. }
        | ClientRequest::GetThreadMemorySettings { workspace_root, .. }
        | ClientRequest::UpdateThreadMemorySettings { workspace_root, .. } => workspace_root,
        _ => {
            return Err(MemoryServiceError::Invalid(
                "request is not a memory operation".to_owned(),
            ));
        }
    };
    Ok(root.as_ref().map(PathBuf::from))
}

fn empty_model_request_preview(
    session_id: harness_contracts::SessionId,
    run_id: harness_contracts::RunId,
    trace_id: Option<harness_contracts::MemoryTraceId>,
) -> MemoryModelRequestPreview {
    MemoryModelRequestPreview {
        session_id,
        run_id,
        trace_id,
        sections: Vec::new(),
        redacted_count: 0,
        token_estimate: 0,
        tool_names: Vec::new(),
        policy_decisions: Vec::new(),
        content_hash: content_hash(&[]),
    }
}

fn model_request_preview_from_trace(
    trace: harness_contracts::MemoryRecallTrace,
) -> MemoryModelRequestPreview {
    let policy_decisions = trace
        .candidates
        .iter()
        .map(|candidate| format!("{:?}", candidate.policy_decision))
        .collect();
    let sections = trace
        .injected
        .into_iter()
        .map(|injected| MemoryModelRequestPreviewSection {
            source: harness_contracts::MemorySource::ExternalRetrieval,
            provider_id: Some(injected.provider_id),
            memory_ids: vec![injected.memory_id],
            redacted_content: format!(
                "[redacted memory context: hash={:?}, chars={}]",
                injected.content_hash, injected.injected_chars
            ),
        })
        .collect::<Vec<_>>();
    let token_estimate = sections
        .iter()
        .map(|section| section.redacted_content.len() as u64)
        .map(|chars| chars.saturating_add(3) / 4)
        .sum();
    let content_hash = content_hash(&sections);
    MemoryModelRequestPreview {
        session_id: trace.session_id,
        run_id: trace.run_id,
        trace_id: Some(trace.trace_id),
        redacted_count: sections.len() as u32,
        sections,
        token_estimate,
        tool_names: Vec::new(),
        policy_decisions,
        content_hash,
    }
}

fn content_hash(sections: &[MemoryModelRequestPreviewSection]) -> ContentHash {
    let mut hasher = blake3::Hasher::new();
    for section in sections {
        hasher.update(format!("{:?}", section.source).as_bytes());
        hasher.update(section.redacted_content.as_bytes());
    }
    ContentHash(*hasher.finalize().as_bytes())
}

fn local_provider(
    db_path: &Path,
    tenant_id: TenantId,
) -> Result<LocalMemoryProvider, MemoryServiceError> {
    LocalMemoryProvider::open(&db_path.to_string_lossy(), tenant_id).map_err(Into::into)
}

async fn memory_item(
    provider: &LocalMemoryProvider,
    memory_id: MemoryId,
    actor: &MemoryActorContext,
) -> Result<DaemonMemoryItem, MemoryServiceError> {
    let summary = provider
        .list(MemoryListScope::ForActor(actor.clone()))
        .await?
        .into_iter()
        .find(|summary| summary.id == memory_id)
        .ok_or_else(|| MemoryServiceError::NotFound("memory item".into()))?;
    let record = provider.get(memory_id).await?;
    Ok(DaemonMemoryItem {
        id: record.id,
        provider_id: summary.provider_id,
        kind: kind_name(&record.kind).to_owned(),
        visibility: visibility_name(&record.visibility).to_owned(),
        content: record.content,
        content_hash: content_hash_hex(&summary.content_hash),
        source: source_name(&record.metadata.source).to_owned(),
        tags: record.metadata.tags,
        confidence: record.metadata.confidence,
        access_count: record.metadata.access_count,
        last_accessed_at: record.metadata.last_accessed_at,
        expires_at: summary.expires_at,
        deleted: summary.deleted,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

fn management_actor(
    tenant_id: TenantId,
    session_id: Option<harness_contracts::SessionId>,
) -> MemoryActorContext {
    MemoryActorContext {
        tenant_id,
        user_id: None,
        team_id: None,
        session_id,
    }
}

async fn ensure_memory_visible(
    provider: &LocalMemoryProvider,
    memory_id: MemoryId,
    actor: &MemoryActorContext,
) -> Result<(), MemoryServiceError> {
    if provider
        .list(MemoryListScope::ForActor(actor.clone()))
        .await?
        .into_iter()
        .any(|summary| summary.id == memory_id)
    {
        Ok(())
    } else {
        Err(MemoryServiceError::NotFound("memory item".into()))
    }
}

fn summary_payload(summary: harness_memory::MemorySummary) -> DaemonMemoryItemSummary {
    DaemonMemoryItemSummary {
        id: summary.id,
        provider_id: summary.provider_id,
        kind: kind_name(&summary.kind).to_owned(),
        visibility: visibility_name(&summary.visibility).to_owned(),
        content_preview: summary.content_preview,
        content_hash: content_hash_hex(&summary.content_hash),
        source: source_name(&summary.metadata.source).to_owned(),
        tags: summary.metadata.tags,
        last_accessed_at: summary.metadata.last_accessed_at,
        expires_at: summary.expires_at,
        deleted: summary.deleted,
        updated_at: summary.updated_at,
    }
}

fn find_candidate(
    inbox: &MemoryInbox,
    id: harness_contracts::MemoryCandidateId,
) -> Result<MemoryCandidate, MemoryServiceError> {
    inbox
        .list(None)
        .map_err(MemoryServiceError::Store)?
        .into_iter()
        .find(|candidate| candidate.id == id)
        .ok_or_else(|| MemoryServiceError::NotFound("memory candidate".into()))
}

fn candidate_list_item(candidate: MemoryCandidate) -> MemoryCandidateListItem {
    MemoryCandidateListItem {
        id: candidate.id,
        state: candidate.state,
        operation: candidate.operation,
        proposed_record: candidate.proposed_record,
        evidence: candidate.evidence,
        created_at: candidate.created_at,
        expires_at: candidate.expires_at,
    }
}

fn content_hash_hex(hash: &harness_contracts::ContentHash) -> String {
    hash.0.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn kind_name(kind: &harness_contracts::MemoryKind) -> &'static str {
    match kind {
        harness_contracts::MemoryKind::UserPreference => "user_preference",
        harness_contracts::MemoryKind::Feedback => "feedback",
        harness_contracts::MemoryKind::ProjectFact => "project_fact",
        harness_contracts::MemoryKind::Reference => "reference",
        harness_contracts::MemoryKind::AgentSelfNote => "agent_self_note",
        _ => "custom",
    }
}

fn visibility_name(visibility: &harness_contracts::MemoryVisibility) -> &'static str {
    match visibility {
        harness_contracts::MemoryVisibility::Private { .. } => "private",
        harness_contracts::MemoryVisibility::User { .. } => "user",
        harness_contracts::MemoryVisibility::Team { .. } => "team",
        _ => "tenant",
    }
}

fn source_name(source: &harness_contracts::MemorySource) -> &'static str {
    match source {
        harness_contracts::MemorySource::UserInput => "user_input",
        harness_contracts::MemorySource::AgentDerived => "agent_derived",
        harness_contracts::MemorySource::SubagentDerived { .. } => "subagent_derived",
        harness_contracts::MemorySource::ExternalRetrieval => "external_retrieval",
        harness_contracts::MemorySource::Imported => "imported",
        harness_contracts::MemorySource::Consolidated { .. } => "consolidated",
        _ => "imported",
    }
}
