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
use super::skills::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;
use harness_contracts::{
    BackgroundAgentId, BackgroundAgentState, ConversationAttachmentReference,
    ConversationContextReference, ConversationInspectorItemResponse, ManifestOriginRef,
    McpServerScope, MemberLeaveReason, PermissionActorSource, PermissionConfirmation,
    PermissionMode, PermissionReview, RedactPatternSet, RedactRules, RedactScope, Redactor,
    RoutingPolicyKind, RunModelSnapshot, SandboxMode, SandboxPolicySummary, SandboxScope,
    SubagentId, SubagentStatus, SubagentTerminationReason, TeamId, TeamTerminationReason,
    TopologyKind, UiSafeText,
};
use harness_journal::SqliteConversationReadModelStore;
use std::io::Write;

use crate::project_registry::ProjectRegistry;

pub async fn list_conversations_with_runtime_state(
    state: &DesktopRuntimeState,
) -> ListConversationsResponse {
    let runtime_summaries = if let Some(harness) = state.harness() {
        list_runtime_conversation_summaries(&harness, state).await
    } else {
        Vec::new()
    };
    let mut conversations: Vec<_> = runtime_summaries
        .into_iter()
        .map(conversation_summary_payload_from_read_model)
        .collect();
    let mut seen = conversations
        .iter()
        .map(|conversation| conversation.id.clone())
        .collect::<HashSet<_>>();
    let deleted = state.deleted_conversation_ids.lock().await;
    if let Ok(metadata) = state.conversation_metadata_store.load_record() {
        conversations.extend(
            metadata
                .conversations
                .into_values()
                .filter(|record| record.state == ConversationMetadataState::Draft)
                .filter(|record| {
                    SessionId::parse(&record.id)
                        .is_ok_and(|session_id| !deleted.contains(&session_id))
                })
                .filter(|record| seen.insert(record.id.clone()))
                .map(conversation_summary_payload_from_metadata),
        );
    }
    conversations.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

    ListConversationsResponse { conversations }
}

pub async fn list_project_conversation_groups_payload(
    project_registry: &ProjectRegistry,
) -> Result<ListProjectConversationGroupsResponse, CommandErrorPayload> {
    let mut groups = Vec::new();
    for project in project_registry.list_projects() {
        let runtime_root = PathBuf::from(&project.path).join(".jyowo").join("runtime");
        let conversations = list_project_conversations_from_runtime_root(runtime_root).await?;
        groups.push(ProjectConversationGroupPayload {
            project,
            conversations,
        });
    }

    Ok(ListProjectConversationGroupsResponse {
        active_path: project_registry.active_path(),
        groups,
    })
}

async fn list_project_conversations_from_runtime_root(
    runtime_root: PathBuf,
) -> Result<Vec<ConversationSummaryPayload>, CommandErrorPayload> {
    let read_model_path = runtime_root.join("conversation-read-model.sqlite");
    let runtime_summaries = if read_model_path.exists() {
        SqliteConversationReadModelStore::open(&read_model_path)
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("conversation read model open failed: {error}"))
            })?
            .list_summaries(TenantId::SINGLE, 50)
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("conversation summaries list failed: {error}"))
            })?
    } else {
        Vec::new()
    };

    let mut conversations: Vec<_> = runtime_summaries
        .into_iter()
        .map(conversation_summary_payload_from_read_model)
        .collect();
    let mut seen = conversations
        .iter()
        .map(|conversation| conversation.id.clone())
        .collect::<HashSet<_>>();
    if let Ok(metadata) =
        DesktopConversationMetadataStore::new_runtime_root(runtime_root).load_record()
    {
        conversations.extend(
            metadata
                .conversations
                .into_values()
                .filter(|record| record.state == ConversationMetadataState::Draft)
                .filter(|record| SessionId::parse(&record.id).is_ok())
                .filter(|record| seen.insert(record.id.clone()))
                .map(conversation_summary_payload_from_metadata),
        );
    }
    conversations.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    conversations.truncate(50);
    Ok(conversations)
}

pub async fn create_conversation_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<CreateConversationResponse, CommandErrorPayload> {
    let session_id = SessionId::new();
    let now = chrono::Utc::now().to_rfc3339();
    let record = ConversationMetadataRecord {
        id: session_id.to_string(),
        title: "New conversation".to_owned(),
        created_at: now.clone(),
        updated_at: now,
        default_model_config_id: None,
        state: ConversationMetadataState::Draft,
    };
    let _guard = state.conversation_metadata_lock.lock().await;
    let mut metadata = state.conversation_metadata_store.load_record()?;
    metadata
        .conversations
        .insert(session_id.to_string(), record.clone());
    state.conversation_metadata_store.save_record(&metadata)?;

    Ok(CreateConversationResponse {
        conversation: conversation_summary_payload_from_metadata(record),
    })
}

pub async fn create_default_conversation_with_runtime_handle(
    runtime_handle: &ManagedDesktopRuntime,
    project_registry: &ProjectRegistry,
) -> Result<CreateConversationResponse, CommandErrorPayload> {
    let next_runtime = runtime_state_for_no_workspace().await?;
    let response = create_conversation_with_runtime_state(&next_runtime).await?;
    project_registry.clear_active()?;
    *runtime_handle.write().await = next_runtime;
    Ok(response)
}

pub async fn create_project_conversation_payload(
    path: String,
    project_registry: &ProjectRegistry,
) -> Result<CreateConversationResponse, CommandErrorPayload> {
    let workspace_root = canonical_workspace_root(PathBuf::from(path), "project path".to_owned())?;
    if !project_registry.contains(&workspace_root) {
        return Err(not_found(format!(
            "project is not registered: {}",
            workspace_root.to_string_lossy()
        )));
    }
    let runtime_state = runtime_state_for_workspace(workspace_root).await?;
    create_conversation_with_runtime_state(&runtime_state).await
}

pub(crate) async fn list_runtime_conversation_summaries(
    harness: &Harness,
    state: &DesktopRuntimeState,
) -> Vec<harness_contracts::ConversationSummary> {
    let mut summaries = harness
        .list_conversation_summaries(TenantId::SINGLE, 50)
        .await
        .unwrap_or_default();

    let deleted = state.deleted_conversation_ids.lock().await;
    summaries.retain(|summary| {
        SessionId::parse(&summary.id).is_ok_and(|session_id| !deleted.contains(&session_id))
    });

    summaries
}

fn conversation_summary_payload_from_metadata(
    record: ConversationMetadataRecord,
) -> ConversationSummaryPayload {
    ConversationSummaryPayload {
        id: record.id,
        is_empty: record.state == ConversationMetadataState::Draft,
        last_message_preview: None,
        title: record.title,
        updated_at: record.updated_at,
    }
}

pub(crate) fn conversation_summary_payload_from_read_model(
    summary: harness_contracts::ConversationSummary,
) -> ConversationSummaryPayload {
    ConversationSummaryPayload {
        id: summary.id,
        is_empty: summary.is_empty,
        last_message_preview: summary
            .last_message_preview
            .map(|preview| preview.into_string()),
        title: summary.title.into_string(),
        updated_at: summary.updated_at.to_rfc3339(),
    }
}

async fn cleanup_no_workspace_conversation_runtime(
    state: &DesktopRuntimeState,
    session_id: SessionId,
) -> Result<(), CommandErrorPayload> {
    if state.project_workspace_root().is_some() {
        return Ok(());
    }

    let signer = crate::commands::runtime::desktop_integrity_signer(state.runtime_root())?;
    harness_permission::FileDecisionPersistence::new(
        TenantId::SINGLE,
        state.runtime_root().join("permission-decisions.json"),
        signer,
    )
    .remove_no_workspace_conversation_scope(session_id)
    .await
    .map_err(|error| {
        runtime_operation_failed(format!(
            "no-workspace permission decision cleanup failed: {error}"
        ))
    })?;
    cleanup_no_workspace_attachment_records(state.runtime_root(), session_id)?;
    AgentRuntimeStore::open_runtime_dir(state.runtime_root())
        .map_err(|error| {
            runtime_operation_failed(format!("background agent store open failed: {error}"))
        })?
        .delete_background_agents_for_conversation(&session_id.to_string())
        .map_err(|error| {
            runtime_operation_failed(format!("background agent cleanup failed: {error}"))
        })?;
    if let Some(harness) = state.harness() {
        harness
            .delete_thread_memory_settings(
                state.conversation_session_options(session_id)?,
                TenantId::SINGLE,
                session_id,
            )
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("memory thread settings cleanup failed: {error}"))
            })?;
    }
    remove_runtime_child_dir(
        state
            .runtime_root()
            .join("workdir")
            .join(session_id.to_string()),
        "no-workspace conversation workdir",
    )?;
    remove_runtime_child_dir(
        state
            .runtime_root()
            .join("exports")
            .join(session_id.to_string()),
        "no-workspace conversation exports",
    )?;
    Ok(())
}

fn remove_runtime_child_dir(path: PathBuf, label: &str) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(&path, label)?;
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(runtime_operation_failed(format!(
                "{label} metadata failed: {error}"
            )));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(runtime_operation_failed(format!(
            "{label} must not be a symlink"
        )));
    }
    if metadata.is_dir() {
        std::fs::remove_dir_all(&path).map_err(|error| {
            runtime_operation_failed(format!("{label} removal failed: {error}"))
        })?;
    } else {
        std::fs::remove_file(&path).map_err(|error| {
            runtime_operation_failed(format!("{label} removal failed: {error}"))
        })?;
    }
    Ok(())
}

pub async fn get_conversation_with_runtime_state(
    request: GetConversationRequest,
    state: &DesktopRuntimeState,
) -> Result<GetConversationResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }
    if let Some(record) = conversation_metadata_record(&session_id, state)?
        .filter(|record| record.state == ConversationMetadataState::Draft)
    {
        return Ok(GetConversationResponse {
            conversation: ConversationPayload {
                id: record.id,
                messages: Vec::new(),
                model_config_id: record.default_model_config_id,
                title: record.title,
                updated_at: record.updated_at,
            },
        });
    }
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading conversations requires the runtime conversation facade.",
        ));
    };
    let snapshot = harness
        .get_conversation_snapshot(&request.conversation_id, 200)
        .await
        .map_err(conversation_read_error)?
        .ok_or_else(|| {
            not_found(format!(
                "conversation not found: {}",
                request.conversation_id
            ))
        })?;

    Ok(GetConversationResponse {
        conversation: ConversationPayload {
            id: request.conversation_id,
            messages: snapshot
                .messages
                .into_iter()
                .map(conversation_message_payload_from_read_model)
                .collect(),
            model_config_id: conversation_model_config_id(&session_id, state)?
                .or(snapshot.model_config_id),
            title: snapshot.title.into_string(),
            updated_at: snapshot.updated_at.to_rfc3339(),
        },
    })
}

pub(crate) fn conversation_message_payload_from_read_model(
    message: harness_contracts::ConversationMessage,
) -> ConversationMessagePayload {
    ConversationMessagePayload {
        author: match message.author {
            ConversationMessageAuthor::User => "user",
            ConversationMessageAuthor::Assistant => "assistant",
        },
        body: message.body.into_string(),
        client_message_id: message.client_message_id,
        id: message.id,
        timestamp: message.timestamp.to_rfc3339(),
    }
}

pub(crate) fn conversation_model_config_id(
    session_id: &SessionId,
    state: &DesktopRuntimeState,
) -> Result<Option<String>, CommandErrorPayload> {
    Ok(conversation_metadata_record(session_id, state)?
        .and_then(|record| record.default_model_config_id))
}

/// Resolve the effective model config id for a run.
///
/// Precedence:
/// 1. Explicit `model_config_id` in the run request (non-empty) wins.
/// 2. Global provider selection from `~/.jyowo/config/provider-selection.json`.
/// Fails closed if no effective selection can be resolved.
pub(crate) fn resolve_effective_model_config_id(
    model_config_id: Option<&str>,
    state: &DesktopRuntimeState,
) -> Result<String, CommandErrorPayload> {
    // 1. Explicit request value wins.
    if let Some(id) = model_config_id {
        let id = id.trim();
        if !id.is_empty() {
            return Ok(id.to_owned());
        }
    }

    // 2. Global provider selection.
    if let Some(ref global_config) = state.global_config_store {
        let selection = global_config.load_global_provider_selection()?;
        if let Some(ref id) = selection.default_config_id {
            let id = id.trim();
            if !id.is_empty() {
                return Ok(id.to_owned());
            }
        }
    }

    Err(invalid_payload(
        "modelConfigId is required when no default provider is configured".to_owned(),
    ))
}

pub(crate) fn default_model_config_id_for_conversation_or_provider(
    session_id: &SessionId,
    state: &DesktopRuntimeState,
) -> Result<String, CommandErrorPayload> {
    if let Some(model_config_id) = conversation_model_config_id(session_id, state)? {
        return Ok(model_config_id);
    }
    // Delegate to the effective resolution chain (global selection → fail).
    resolve_effective_model_config_id(None, state)
}

fn provider_config_for_run(
    model_config_id: &str,
    state: &DesktopRuntimeState,
) -> Result<ProviderConfigRecord, CommandErrorPayload> {
    let record = state
        .provider_settings_store
        .load_record()?
        .ok_or_else(|| invalid_payload("provider config was not found".to_owned()))?;
    let config = provider_config_by_id(&record, model_config_id)?;
    ensure_provider_config_has_api_key(config)?;
    Ok(config.clone())
}

pub(crate) async fn runtime_for_model_config(
    session_id: SessionId,
    model_config_id: &str,
    state: &DesktopRuntimeState,
) -> Result<
    (
        Arc<Harness>,
        SessionOptions,
        String,
        ModelProtocol,
        harness_contracts::ModelRequestOptions,
    ),
    CommandErrorPayload,
> {
    let config = provider_config_for_run(model_config_id, state)?;
    let provider_config_fingerprint = provider_config_runtime_fingerprint(&config)?;
    if let Some((harness, options)) = state.active_conversation_runtime_for_model_config(
        session_id,
        model_config_id,
        provider_config_fingerprint,
    )? {
        return Ok((
            harness,
            options,
            config.model_id.clone(),
            config.protocol,
            config.model_options.clone(),
        ));
    }
    let stream_permission_runtime = state
        .stream_permission_runtime
        .as_ref()
        .ok_or_else(|| runtime_unavailable("Starting runs requires the desktop runtime."))?;
    let layout = if state.runtime_layout().workspace_root.is_some() {
        project_runtime_layout(state.workspace_root())
    } else {
        crate::commands::global_conversation_runtime_layout_with_runtime_root(
            session_id,
            state.runtime_root().to_path_buf(),
        )
    };
    let (harness, model_id, protocol, model_options) = build_desktop_harness(
        &layout,
        Arc::clone(stream_permission_runtime),
        Some(model_config_id),
        Arc::clone(&state.provider_capability_routes),
        Some(Arc::clone(&state.provider_settings_store)),
    )
    .await?;
    let options = state.conversation_session_options_for_model(
        session_id,
        model_id.clone(),
        protocol,
        model_options.clone(),
    )?;
    Ok((
        Arc::new(harness),
        options,
        model_id,
        protocol,
        model_options,
    ))
}

pub(crate) fn conversation_metadata_record(
    session_id: &SessionId,
    state: &DesktopRuntimeState,
) -> Result<Option<ConversationMetadataRecord>, CommandErrorPayload> {
    Ok(state
        .conversation_metadata_store
        .load_record()?
        .conversations
        .get(&session_id.to_string())
        .cloned())
}

pub(crate) async fn mark_conversation_metadata_active(
    session_id: SessionId,
    default_model_config_id: Option<String>,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let _guard = state.conversation_metadata_lock.lock().await;
    let mut metadata = state.conversation_metadata_store.load_record()?;
    let now = chrono::Utc::now().to_rfc3339();
    let record = metadata
        .conversations
        .entry(session_id.to_string())
        .or_insert_with(|| ConversationMetadataRecord {
            id: session_id.to_string(),
            title: "New conversation".to_owned(),
            created_at: now.clone(),
            updated_at: now.clone(),
            default_model_config_id: None,
            state: ConversationMetadataState::Active,
        });
    record.state = ConversationMetadataState::Active;
    record.updated_at = now;
    if let Some(model_config_id) = default_model_config_id {
        record.default_model_config_id = Some(model_config_id);
    }
    state.conversation_metadata_store.save_record(&metadata)
}

pub fn delete_conversation_payload(
    request: DeleteConversationRequest,
) -> Result<DeleteConversationResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let _session_id = parse_session_id(&request.conversation_id)?;

    Err(runtime_unavailable(
        "Deleting conversations requires the runtime conversation facade.",
    ))
}

pub async fn delete_conversation_with_runtime_state(
    request: DeleteConversationRequest,
    state: &DesktopRuntimeState,
) -> Result<DeleteConversationResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    let existing_metadata = {
        let _guard = state.conversation_metadata_lock.lock().await;
        state
            .conversation_metadata_store
            .load_record()?
            .conversations
            .get(&request.conversation_id)
            .cloned()
    };
    if existing_metadata
        .as_ref()
        .is_some_and(|record| record.state == ConversationMetadataState::Draft)
    {
        cleanup_no_workspace_conversation_runtime(state, session_id).await?;
        remove_conversation_metadata_record(state, &request.conversation_id).await?;
        state
            .deleted_conversation_ids
            .lock()
            .await
            .insert(session_id);
        return Ok(DeleteConversationResponse {
            conversation_id: request.conversation_id,
            status: "deleted",
        });
    }

    let deleted = if let Some(harness) = state.harness() {
        harness
            .delete_conversation_session(state.conversation_session_options(session_id)?)
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("conversation delete failed: {error}"))
            })?
    } else {
        false
    };
    if !deleted && existing_metadata.is_none() {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }

    cleanup_no_workspace_conversation_runtime(state, session_id).await?;
    if existing_metadata.is_some() {
        remove_conversation_metadata_record(state, &request.conversation_id).await?;
    }
    state
        .deleted_conversation_ids
        .lock()
        .await
        .insert(session_id);

    Ok(DeleteConversationResponse {
        conversation_id: request.conversation_id,
        status: "deleted",
    })
}

pub async fn delete_project_conversation_payload(
    path: String,
    conversation_id: String,
    project_registry: &ProjectRegistry,
) -> Result<DeleteConversationResponse, CommandErrorPayload> {
    let workspace_root = canonical_workspace_root(PathBuf::from(path), "project path".to_owned())?;
    if !project_registry.contains(&workspace_root) {
        return Err(not_found(format!(
            "project is not registered: {}",
            workspace_root.to_string_lossy()
        )));
    }
    let runtime_state = runtime_state_for_workspace(workspace_root).await?;
    delete_conversation_with_runtime_state(
        DeleteConversationRequest { conversation_id },
        &runtime_state,
    )
    .await
}

async fn remove_conversation_metadata_record(
    state: &DesktopRuntimeState,
    conversation_id: &str,
) -> Result<(), CommandErrorPayload> {
    let _guard = state.conversation_metadata_lock.lock().await;
    let mut metadata = state.conversation_metadata_store.load_record()?;
    if metadata.conversations.remove(conversation_id).is_some() {
        state.conversation_metadata_store.save_record(&metadata)?;
    }
    Ok(())
}

pub fn start_run_payload(
    request: StartRunRequest,
) -> Result<StartRunResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let _session_id = parse_session_id(&request.conversation_id)?;
    if let Some(ref model_config_id) = request.model_config_id {
        ensure_non_empty("modelConfigId", model_config_id)?;
    }
    ensure_non_empty("prompt", &request.prompt)?;
    if let Some(client_message_id) = request.client_message_id.as_deref() {
        validate_client_message_id(client_message_id)?;
    }
    if let Some(permission_mode) = request.permission_mode {
        ensure_start_run_permission_mode(permission_mode)?;
    }
    validate_context_reference_payloads(request.context_references.as_deref())?;
    validate_attachment_reference_payloads(request.attachments.as_deref())?;

    Err(runtime_unavailable(
        "Starting runs requires the runtime conversation facade.",
    ))
}

pub async fn start_run_with_runtime_state(
    request: StartRunRequest,
    state: &DesktopRuntimeState,
) -> Result<StartRunResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    if let Some(ref model_config_id) = request.model_config_id {
        ensure_non_empty("modelConfigId", model_config_id)?;
    }
    ensure_non_empty("prompt", &request.prompt)?;
    if let Some(client_message_id) = request.client_message_id.as_deref() {
        validate_client_message_id(client_message_id)?;
    }
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }

    // Resolve effective model config id before any run activation.
    // Falls back to global selection → fail closed.
    let model_config_id =
        resolve_effective_model_config_id(request.model_config_id.as_deref(), state)?;

    let permission_mode = resolve_start_run_permission_mode(request.permission_mode, state)?;
    let agent_policy = resolve_start_run_agent_policy(&request, state)?;
    let input = build_conversation_turn_input(&request, state).await?;
    let _start_run_guard = state.start_run_lock.lock().await;
    let (harness, options, model_id, protocol, _model_options) =
        runtime_for_model_config(session_id, &model_config_id, state).await?;
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .map_err(|error| runtime_operation_failed(format!("conversation open failed: {error}")))?;
    let after_event_id = conversation_tail_event_id(&harness, options.clone()).await?;
    let run_harness = Arc::clone(&harness);
    let run_session_options = options.clone();
    let run_agent_options = agent_policy.options;
    let mut run_options = ConversationRunOptions::from_session_options(&run_session_options)
        .with_model_config_id(model_config_id.clone())
        .with_model_id(model_id)
        .with_protocol(protocol)
        .with_permission_mode(permission_mode);
    run_options.agent_tool_policy = Some(run_agent_options);
    let mut run_task = tokio::spawn(async move {
        run_harness
            .submit_conversation_turn(ConversationTurnRequest {
                options: run_session_options,
                run_options,
                input,
                permission_actor_source: None,
            })
            .await
    });
    let run_id =
        match wait_for_started_conversation_run(&harness, options, after_event_id, &mut run_task)
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => {
                run_task.abort();
                return Err(error);
            }
        };
    drop(run_task);
    mark_conversation_metadata_active(session_id, Some(model_config_id), state).await?;

    Ok(StartRunResponse {
        run_id: run_id.to_string(),
        status: "started",
    })
}

pub(crate) fn safe_background_supervisor_input(
    input: &ConversationTurnInput,
    redactor: &dyn harness_contracts::Redactor,
) -> ConversationTurnInput {
    let mut safe = input.clone();
    safe.prompt = safe_redacted_string(&input.prompt, redactor);
    safe.client_message_id = input
        .client_message_id
        .as_deref()
        .map(|value| safe_redacted_string(value, redactor));
    safe.context_references = input
        .context_references
        .iter()
        .map(|reference| safe_background_context_reference(reference, redactor))
        .collect();
    safe.attachments = input
        .attachments
        .iter()
        .map(|attachment| safe_background_attachment_reference(attachment, redactor))
        .collect();
    safe
}

fn safe_background_context_reference(
    reference: &ConversationContextReference,
    redactor: &dyn harness_contracts::Redactor,
) -> ConversationContextReference {
    match reference {
        ConversationContextReference::WorkspaceFile { path, label } => {
            ConversationContextReference::WorkspaceFile {
                path: safe_redacted_string(path, redactor),
                label: safe_redacted_string(label, redactor),
            }
        }
        ConversationContextReference::Artifact { id, label } => {
            ConversationContextReference::Artifact {
                id: safe_redacted_string(id, redactor),
                label: safe_redacted_string(label, redactor),
            }
        }
        ConversationContextReference::Conversation { id, label } => {
            ConversationContextReference::Conversation {
                id: safe_redacted_string(id, redactor),
                label: safe_redacted_string(label, redactor),
            }
        }
        ConversationContextReference::Memory {
            id,
            label,
            resolved_content,
        } => ConversationContextReference::Memory {
            id: safe_redacted_string(id, redactor),
            label: safe_redacted_string(label, redactor),
            resolved_content: resolved_content
                .as_ref()
                .map(|content| safe_redacted_string(content, redactor)),
        },
        ConversationContextReference::Skill { id, label } => ConversationContextReference::Skill {
            id: safe_redacted_string(id, redactor),
            label: safe_redacted_string(label, redactor),
        },
        ConversationContextReference::Tool { id, label } => ConversationContextReference::Tool {
            id: safe_redacted_string(id, redactor),
            label: safe_redacted_string(label, redactor),
        },
        ConversationContextReference::McpServer { id, label } => {
            ConversationContextReference::McpServer {
                id: safe_redacted_string(id, redactor),
                label: safe_redacted_string(label, redactor),
            }
        }
    }
}

fn safe_background_attachment_reference(
    attachment: &ConversationAttachmentReference,
    redactor: &dyn harness_contracts::Redactor,
) -> ConversationAttachmentReference {
    let mut safe = attachment.clone();
    safe.id = safe_redacted_string(&attachment.id, redactor);
    safe.name = safe_redacted_string(&attachment.name, redactor);
    safe.mime_type = safe_redacted_string(&attachment.mime_type, redactor);
    safe.blob_ref.content_type = attachment
        .blob_ref
        .content_type
        .as_deref()
        .map(|value| safe_redacted_string(value, redactor));
    safe
}

fn safe_redacted_string(value: &str, redactor: &dyn harness_contracts::Redactor) -> String {
    harness_contracts::UiSafeText::from_redacted_display(value, redactor).into_string()
}

pub fn resolve_start_run_agent_policy(
    request: &StartRunRequest,
    state: &DesktopRuntimeState,
) -> Result<ResolvedAgentToolPolicy, CommandErrorPayload> {
    let settings = state.effective_execution_settings(None)?;
    let capability_context = agent_capability_resolution_context_for_state(state);
    let policy_root = state
        .project_workspace_root()
        .unwrap_or_else(|| state.conversation_cwd());
    let capabilities_payload = if state.project_workspace_root().is_some() {
        agent_capabilities_payload(&settings, policy_root, capability_context.as_ref())
    } else {
        let conversation_id = parse_session_id(&request.conversation_id)?;
        no_workspace_agent_capabilities_payload_for_conversation(
            &settings,
            state.runtime_root(),
            Some(conversation_id),
            capability_context.as_ref(),
        )
    };
    let capabilities = AgentCapabilitiesInput {
        subagents_available: capabilities_payload.subagents_available,
        agent_teams_available: capabilities_payload.agent_teams_available,
        background_agents_available: capabilities_payload.background_agents_available,
    };
    let settings_input = ExecutionSettingsAgentInput {
        subagents_enabled: settings.subagents_enabled,
        agent_teams_enabled: settings.agent_teams_enabled,
        background_agents_enabled: settings.background_agents_enabled,
    };
    let profiles = list_global_agent_profiles_with_builtin(state)?;
    let profile_ids: Vec<String> = profiles.into_iter().map(|profile| profile.id).collect();

    resolve_agent_runtime_policy(
        policy_root,
        &settings_input,
        None,
        &capabilities,
        &profile_ids,
        &request.conversation_id,
    )
    .map_err(map_agent_runtime_policy_error)
}

fn agent_capability_resolution_context_for_state(
    state: &DesktopRuntimeState,
) -> Option<AgentCapabilityResolutionContext> {
    Some(AgentCapabilityResolutionContext {
        stream_permission_runtime_available: state.stream_permission_runtime.is_some(),
    })
}

fn map_agent_runtime_policy_error(error: AgentRuntimePolicyError) -> CommandErrorPayload {
    invalid_payload(error.to_string())
}

pub fn create_attachment_from_path_payload(
    request: CreateAttachmentFromPathRequest,
) -> Result<CreateAttachmentFromPathResponse, CommandErrorPayload> {
    ensure_non_empty("path", &request.path)?;
    if let Some(conversation_id) = request.conversation_id.as_deref() {
        ensure_non_empty("conversationId", conversation_id)?;
        let _ = parse_session_id(conversation_id)?;
    }

    Err(runtime_unavailable(
        "Creating attachments requires the runtime workspace state.",
    ))
}

pub fn list_reference_candidates_payload(
) -> Result<ListReferenceCandidatesResponse, CommandErrorPayload> {
    Err(runtime_unavailable(
        "Listing reference candidates requires the runtime workspace state.",
    ))
}

pub async fn create_attachment_from_path_with_runtime_state(
    request: CreateAttachmentFromPathRequest,
    state: &DesktopRuntimeState,
) -> Result<CreateAttachmentFromPathResponse, CommandErrorPayload> {
    ensure_non_empty("path", &request.path)?;
    let no_workspace_session_id =
        no_workspace_attachment_conversation_session_id(state, request.conversation_id.as_deref())?;
    let no_workspace_conversation_cwd = no_workspace_session_id.map(|session_id| {
        state
            .runtime_root()
            .join("workdir")
            .join(session_id.to_string())
    });
    let file_access_root = state.project_workspace_root().unwrap_or_else(|| {
        no_workspace_conversation_cwd
            .as_deref()
            .unwrap_or_else(|| state.conversation_cwd())
    });
    let requested_path = Path::new(&request.path);
    let candidate_path = if requested_path.is_absolute() {
        if requested_path.strip_prefix(file_access_root).is_err() {
            let Some(parent) = requested_path.parent() else {
                return Err(invalid_payload(
                    "attachment path must stay inside the active workspace file access root"
                        .to_owned(),
                ));
            };
            let Ok(parent) = parent.canonicalize() else {
                return Err(invalid_payload(
                    "attachment path must stay inside the active workspace file access root"
                        .to_owned(),
                ));
            };
            if workspace_relative_path(&parent, file_access_root).is_none() {
                return Err(invalid_payload(
                    "attachment path must stay inside the active workspace file access root"
                        .to_owned(),
                ));
            }
            let Some(file_name) = requested_path.file_name() else {
                return Err(invalid_payload("path must point to a file".to_owned()));
            };
            parent.join(file_name)
        } else {
            requested_path.to_path_buf()
        }
    } else {
        file_access_root.join(requested_path)
    };
    let source_path = canonicalize_existing_file(&candidate_path, "path")?;
    if workspace_relative_path(&source_path, file_access_root).is_none() {
        return Err(invalid_payload(
            "attachment path must stay inside the active workspace file access root".to_owned(),
        ));
    }
    let metadata = source_path.metadata().map_err(|error| {
        runtime_operation_failed(format!("attachment metadata failed: {error}"))
    })?;
    if !metadata.is_file() {
        return Err(invalid_payload("path must point to a file".to_owned()));
    }
    if metadata.len() > MAX_ATTACHMENT_BYTES {
        return Err(invalid_payload(format!(
            "attachment must be at most {} MB",
            MAX_ATTACHMENT_BYTES / 1024 / 1024
        )));
    }

    let name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("attachment")
        .to_owned();
    let id = attachment_id(&source_path, metadata.len());
    let mime_type = infer_mime_type(&source_path);
    let bytes = std::fs::read(&source_path)
        .map_err(|error| runtime_operation_failed(format!("attachment read failed: {error}")))?;
    let hash = blake3::hash(&bytes);
    let blob_store = FileBlobStore::open(state.runtime_root().join("blobs")).map_err(|error| {
        runtime_operation_failed(format!("attachment store unavailable: {error}"))
    })?;
    let blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            Bytes::from(bytes),
            BlobMeta {
                content_type: Some(mime_type.clone()),
                size: metadata.len(),
                content_hash: *hash.as_bytes(),
                created_at: Utc::now(),
                retention: BlobRetention::TenantScoped,
            },
        )
        .await
        .map_err(|error| {
            runtime_operation_failed(format!("attachment blob write failed: {error}"))
        })?;
    let attachment = AttachmentReferencePayload {
        id: id.clone(),
        name,
        mime_type,
        size_bytes: metadata.len(),
        blob_ref: attachment_blob_ref_payload(&blob_ref),
    };

    write_attachment_record(
        state.runtime_root(),
        &AttachmentRecord {
            attachment: attachment.clone(),
            blob_ref,
        },
    )?;
    if let Err(error) =
        record_no_workspace_attachment_owner(state, no_workspace_session_id, &attachment.id)
    {
        let _ = std::fs::remove_file(attachment_record_path(state.runtime_root(), &attachment.id));
        return Err(error);
    }

    Ok(CreateAttachmentFromPathResponse { attachment })
}

pub async fn list_reference_candidates_with_runtime_state(
    request: ListReferenceCandidatesRequest,
    state: &DesktopRuntimeState,
) -> Result<ListReferenceCandidatesResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    ensure_reference_conversation_exists(session_id, state).await?;
    let files = state
        .project_workspace_root()
        .map(context_files_from_workspace)
        .unwrap_or_default()
        .into_iter()
        .map(|file| ReferenceCandidatePayload {
            id: None,
            label: file.label.clone(),
            path: Some(file.label),
        })
        .collect();
    let artifacts = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: request.conversation_id.clone(),
        },
        state,
    )
    .await?
    .artifacts
    .into_iter()
    .map(|artifact| ReferenceCandidatePayload {
        id: Some(artifact.id),
        label: artifact.title,
        path: None,
    })
    .collect();
    let conversations = list_conversations_with_runtime_state(state)
        .await
        .conversations
        .into_iter()
        .map(|conversation| ReferenceCandidatePayload {
            id: Some(conversation.id),
            label: conversation.title,
            path: None,
        })
        .collect();
    let memories = match list_memory_items_with_runtime_state(state).await {
        Ok(payload) => payload
            .items
            .into_iter()
            .map(|item| ReferenceCandidatePayload {
                id: Some(item.id),
                label: item.content_preview,
                path: None,
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    let skills = match list_skills_with_runtime_state(state).await {
        Ok(payload) => payload
            .skills
            .into_iter()
            .map(|skill| ReferenceCandidatePayload {
                id: Some(skill.id),
                label: skill.name,
                path: None,
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    let tools = state.harness().map_or_else(Vec::new, |harness| {
        let mut tools = harness
            .tool_registry()
            .snapshot()
            .as_descriptors()
            .into_iter()
            .map(|descriptor| ReferenceCandidatePayload {
                id: Some(descriptor.name.clone()),
                label: descriptor.display_name.clone(),
                path: None,
            })
            .collect::<Vec<_>>();
        tools.sort_by(|left, right| left.label.cmp(&right.label).then(left.id.cmp(&right.id)));
        tools
    });
    let mcp_servers = match list_mcp_servers_with_runtime_state(state).await {
        Ok(payload) => payload
            .servers
            .into_iter()
            .map(|server| ReferenceCandidatePayload {
                id: Some(server.id),
                label: server.display_name,
                path: None,
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    Ok(ListReferenceCandidatesResponse {
        artifacts,
        conversations,
        files,
        memories,
        mcp_servers,
        skills,
        tools,
    })
}

pub(crate) async fn ensure_reference_conversation_exists(
    session_id: SessionId,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Listing reference candidates requires the runtime conversation facade.",
        ));
    };

    ensure_existing_conversation_session_with_harness(session_id, state, &harness).await
}

pub(crate) async fn ensure_existing_conversation_session_with_harness(
    session_id: SessionId,
    state: &DesktopRuntimeState,
    harness: &Harness,
) -> Result<(), CommandErrorPayload> {
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!("conversation not found: {session_id}")));
    }
    if conversation_metadata_record(&session_id, state)?.is_some() {
        return Ok(());
    }
    if session_id == state.default_conversation_id() {
        harness
            .open_or_create_conversation_session(state.conversation_session_options(session_id)?)
            .await
            .map_err(|error| runtime_operation_failed(error.to_string()))?;
        return Ok(());
    }
    if harness
        .conversation_session_exists(state.conversation_session_options(session_id)?)
        .await
        .map_err(|error| runtime_operation_failed(error.to_string()))?
    {
        return Ok(());
    }

    Err(not_found(format!("conversation not found: {session_id}")))
}

pub fn cancel_run_payload(
    request: CancelRunRequest,
) -> Result<CancelRunResponse, CommandErrorPayload> {
    ensure_non_empty("runId", &request.run_id)?;

    Err(runtime_unavailable(
        "Cancelling runs requires the runtime conversation facade.",
    ))
}

pub async fn cancel_run_with_runtime_state(
    request: CancelRunRequest,
    state: &DesktopRuntimeState,
) -> Result<CancelRunResponse, CommandErrorPayload> {
    ensure_non_empty("runId", &request.run_id)?;
    let run_id = parse_run_id(&request.run_id)?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Cancelling runs requires the runtime conversation facade.",
        ));
    };
    harness
        .cancel_conversation_run(run_id)
        .await
        .map_err(|error| runtime_operation_failed(error.to_string()))?;

    Ok(CancelRunResponse {
        run_id: request.run_id,
        status: "cancelled",
    })
}

pub fn resolve_permission_payload(
    request: ResolvePermissionRequest,
) -> Result<ResolvePermissionResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let _session_id = parse_session_id(&request.conversation_id)?;
    ensure_non_empty("requestId", &request.request_id)?;
    let _request_id = parse_request_id(&request.request_id)?;
    ensure_non_empty("optionId", &request.option_id)?;
    let _option_id = parse_permission_option_id(&request.option_id)?;

    Err(runtime_unavailable(
        "Permission decisions require the runtime PermissionBroker.",
    ))
}

pub async fn resolve_permission_with_runtime_state(
    request: ResolvePermissionRequest,
    state: &DesktopRuntimeState,
) -> Result<ResolvePermissionResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("requestId", &request.request_id)?;

    let session_id = parse_session_id(&request.conversation_id)?;
    let request_id = parse_request_id(&request.request_id)?;
    let option_id = parse_permission_option_id(&request.option_id)?;
    let submitted_decision = submitted_permission_decision(request.decision);
    let Some(resolver) = state.permission_resolver.as_ref() else {
        return Err(runtime_unavailable(
            "Permission decisions require the runtime PermissionBroker.",
        ));
    };

    let Some(pending) = pending_permission_request(state, request_id).await else {
        return Err(not_found(format!(
            "permission request not found: {}",
            request.request_id
        )));
    };
    if pending.request.session_id != session_id {
        return Err(invalid_payload(
            "permission request does not belong to conversationId".to_owned(),
        ));
    }

    let resolved_decision = resolver
        .resolve_permission_option(
            request_id,
            pending.request.tenant_id,
            pending.request.session_id,
            option_id,
            submitted_decision,
            request.confirmation_text.as_deref(),
        )
        .await?;
    let supervisor_scope = agent_supervisor_scope_for_state(state);
    let _ = crate::agent_supervisor::wake_agent_supervisor_scope(&supervisor_scope).await;

    Ok(ResolvePermissionResponse {
        decision: permission_decision_from_resolved(resolved_decision)?,
        request_id: request.request_id,
        status: "resolved",
    })
}

async fn pending_permission_request(
    state: &DesktopRuntimeState,
    request_id: RequestId,
) -> Option<jyowo_harness_sdk::ext::PendingPermissionRequest> {
    const ATTEMPTS: usize = 25;
    const DELAY_MS: u64 = 10;

    for attempt in 0..ATTEMPTS {
        if let Some(pending) = state
            .pending_permission_requests()
            .into_iter()
            .find(|pending| pending.request.request_id == request_id)
        {
            return Some(pending);
        }
        if attempt + 1 < ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(DELAY_MS)).await;
        }
    }

    None
}

pub async fn resolve_permission_for_window_with_runtime_state(
    request: ResolvePermissionRequest,
    window_label: String,
    state: &DesktopRuntimeState,
) -> Result<ResolvePermissionResponse, CommandErrorPayload> {
    ensure_non_empty("windowLabel", &window_label)?;
    ensure_window_subscribed_to_conversation(state, &window_label, &request.conversation_id)
        .await?;
    resolve_permission_with_runtime_state(request, state).await
}

pub(crate) async fn ensure_window_subscribed_to_conversation(
    state: &DesktopRuntimeState,
    window_label: &str,
    conversation_id: &str,
) -> Result<(), CommandErrorPayload> {
    let subscriptions = state.conversation_event_subscriptions.lock().await;
    if subscriptions.values().any(|subscription| {
        subscription.window_label == window_label && subscription.conversation_id == conversation_id
    }) {
        return Ok(());
    }

    Err(invalid_payload(
        "permission request is not visible in this window".to_owned(),
    ))
}

pub fn list_activity_payload(
    request: ListActivityRequest,
) -> Result<ListActivityResponse, CommandErrorPayload> {
    ensure_optional("conversationId", request.conversation_id.as_deref())?;
    ensure_optional("runId", request.run_id.as_deref())?;
    require_conversation_id_for_activity(request.conversation_id.as_deref())?;

    Ok(ListActivityResponse { events: Vec::new() })
}

pub async fn list_activity_with_runtime_state(
    request: ListActivityRequest,
    state: &DesktopRuntimeState,
) -> Result<ListActivityResponse, CommandErrorPayload> {
    ensure_optional("conversationId", request.conversation_id.as_deref())?;
    ensure_optional("runId", request.run_id.as_deref())?;
    require_conversation_id_for_activity(request.conversation_id.as_deref())?;
    if let Some(conversation_id) = request.conversation_id.as_deref() {
        let session_id = parse_session_id(conversation_id)?;
        if conversation_metadata_record(&session_id, state)?
            .is_some_and(|record| record.state == ConversationMetadataState::Draft)
        {
            return Ok(ListActivityResponse { events: Vec::new() });
        }
    }

    let mut events = read_activity_replay_events(&request, state).await?;
    events.retain(|event| event.event_type != "assistant.thinking.delta");

    Ok(ListActivityResponse { events })
}

pub async fn get_replay_timeline_with_runtime_state(
    request: ReplayTimelineRequest,
    state: &DesktopRuntimeState,
) -> Result<ReplayTimelineResponse, CommandErrorPayload> {
    ensure_optional("conversationId", request.conversation_id.as_deref())?;
    ensure_optional("runId", request.run_id.as_deref())?;
    require_conversation_id_for_replay(request.conversation_id.as_deref())?;

    let events = read_replay_run_events(
        ListActivityRequest {
            conversation_id: request.conversation_id,
            run_id: request.run_id,
        },
        state,
    )
    .await?;

    Ok(ReplayTimelineResponse {
        events,
        replayed: true,
    })
}

pub async fn page_conversation_timeline_with_runtime_state(
    request: PageConversationTimelineRequest,
    state: &DesktopRuntimeState,
) -> Result<PageConversationTimelineResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading conversation timeline requires the runtime conversation facade.",
        ));
    };
    let page = harness
        .page_conversation_timeline(
            &request.conversation_id,
            request.after_cursor,
            request
                .limit
                .unwrap_or(CONVERSATION_SUBSCRIPTION_BATCH_LIMIT),
        )
        .await
        .map_err(conversation_read_error)?;
    let events = page
        .events
        .into_iter()
        .map(run_event_payload_from_read_model)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PageConversationTimelineResponse {
        events,
        cursor: page.cursor,
        gap: page.gap,
    })
}

pub async fn page_conversation_worktree_with_runtime_state(
    request: PageConversationWorktreeRequest,
    state: &DesktopRuntimeState,
) -> Result<ConversationWorktreePage, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading conversation worktree requires the runtime conversation facade.",
        ));
    };
    harness
        .page_conversation_worktree(
            &request.conversation_id,
            request.page_cursor,
            request.direction.into(),
            request.limit.unwrap_or(50),
        )
        .await
        .map_err(conversation_read_error)
}

pub async fn get_conversation_inspector_item_with_runtime_state(
    request: GetConversationInspectorItemRequest,
    state: &DesktopRuntimeState,
) -> Result<ConversationInspectorItemResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading conversation inspector items requires the runtime conversation facade.",
        ));
    };
    harness
        .get_conversation_inspector_item(&request.conversation_id, request.selection)
        .await
        .map_err(conversation_read_error)
}

pub async fn subscribe_conversation_events_with_runtime_state(
    request: SubscribeConversationEventsRequest,
    state: &DesktopRuntimeState,
) -> Result<SubscribeConversationEventsResponse, CommandErrorPayload> {
    subscribe_conversation_events_for_window_with_runtime_state(
        request,
        "default".to_owned(),
        Arc::new(|_batch| Ok(())),
        state,
    )
    .await
}

pub async fn subscribe_conversation_events_for_window_with_runtime_state(
    request: SubscribeConversationEventsRequest,
    window_label: String,
    emitter: ConversationEventBatchEmitter,
    state: &DesktopRuntimeState,
) -> Result<SubscribeConversationEventsResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("windowLabel", &window_label)?;
    let session_id = parse_session_id(&request.conversation_id)?;
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!(
            "conversation not found: {}",
            request.conversation_id
        )));
    }

    let replay_page = match page_conversation_timeline_with_runtime_state(
        PageConversationTimelineRequest {
            conversation_id: request.conversation_id.clone(),
            after_cursor: request.after_cursor,
            limit: Some(CONVERSATION_SUBSCRIPTION_BATCH_LIMIT),
        },
        state,
    )
    .await
    {
        Ok(page) => page,
        Err(error) if is_conversation_cursor_mismatch(&error) => {
            resync_conversation_subscription_page(&request.conversation_id, state).await?
        }
        Err(error) => return Err(error),
    };
    let cursor = replay_page.cursor;
    let replay_events = replay_page.events;
    let gap = replay_page.gap;
    let subscription_id = format!("subscription-{}", EventId::new());

    let handle = spawn_conversation_event_subscription(
        subscription_id.clone(),
        request.conversation_id.clone(),
        cursor.clone(),
        window_label.clone(),
        Arc::clone(&emitter),
        state.clone(),
    );
    state.conversation_event_subscriptions.lock().await.insert(
        subscription_id.clone(),
        ConversationSubscriptionHandle {
            conversation_id: request.conversation_id.clone(),
            task: handle,
            window_label,
        },
    );

    Ok(SubscribeConversationEventsResponse {
        subscription_id,
        conversation_id: request.conversation_id,
        replay_events,
        cursor,
        gap,
    })
}

pub async fn unsubscribe_conversation_events_with_runtime_state(
    request: UnsubscribeConversationEventsRequest,
    state: &DesktopRuntimeState,
) -> Result<UnsubscribeConversationEventsResponse, CommandErrorPayload> {
    unsubscribe_conversation_events_for_window_with_runtime_state(
        request,
        "default".to_owned(),
        state,
    )
    .await
}

pub async fn unsubscribe_conversation_events_for_window_with_runtime_state(
    request: UnsubscribeConversationEventsRequest,
    window_label: String,
    state: &DesktopRuntimeState,
) -> Result<UnsubscribeConversationEventsResponse, CommandErrorPayload> {
    ensure_non_empty("subscriptionId", &request.subscription_id)?;
    ensure_non_empty("windowLabel", &window_label)?;
    let mut subscriptions = state.conversation_event_subscriptions.lock().await;
    let removed = match subscriptions.get(&request.subscription_id) {
        Some(subscription) if subscription.window_label != window_label => {
            return Err(invalid_payload(
                "subscription does not belong to this window".to_owned(),
            ));
        }
        Some(_) => subscriptions.remove(&request.subscription_id),
        None => None,
    };
    drop(subscriptions);

    if let Some(subscription) = removed {
        let _ = &subscription.conversation_id;
        subscription.task.abort();
        return Ok(UnsubscribeConversationEventsResponse {
            subscription_id: request.subscription_id,
            status: "unsubscribed",
        });
    }

    Ok(UnsubscribeConversationEventsResponse {
        subscription_id: request.subscription_id,
        status: "alreadyClosed",
    })
}

pub fn unsubscribe_conversation_events_payload(
    request: UnsubscribeConversationEventsRequest,
) -> Result<UnsubscribeConversationEventsResponse, CommandErrorPayload> {
    ensure_non_empty("subscriptionId", &request.subscription_id)?;

    Ok(UnsubscribeConversationEventsResponse {
        subscription_id: request.subscription_id,
        status: "alreadyClosed",
    })
}

pub(crate) fn spawn_conversation_event_subscription(
    subscription_id: String,
    conversation_id: String,
    initial_cursor: Option<ConversationCursor>,
    window_label: String,
    emitter: ConversationEventBatchEmitter,
    state: DesktopRuntimeState,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut cursor = initial_cursor;

        loop {
            tokio::time::sleep(CONVERSATION_SUBSCRIPTION_POLL_INTERVAL).await;
            let page = match page_conversation_timeline_with_runtime_state(
                PageConversationTimelineRequest {
                    conversation_id: conversation_id.clone(),
                    after_cursor: cursor.clone(),
                    limit: Some(CONVERSATION_SUBSCRIPTION_BATCH_LIMIT),
                },
                &state,
            )
            .await
            {
                Ok(page) => page,
                Err(error) if is_conversation_cursor_mismatch(&error) => {
                    match resync_conversation_subscription_page(&conversation_id, &state).await {
                        Ok(page) => {
                            cursor = page.cursor.clone();
                            let _ = emitter(ConversationEventBatchPayload {
                                subscription_id: subscription_id.clone(),
                                conversation_id: conversation_id.clone(),
                                events: Vec::new(),
                                cursor: cursor.clone(),
                                gap: true,
                                phase: "live",
                            });
                            continue;
                        }
                        Err(_) => {
                            let _ = emitter(ConversationEventBatchPayload {
                                subscription_id: subscription_id.clone(),
                                conversation_id: conversation_id.clone(),
                                events: Vec::new(),
                                cursor: None,
                                gap: true,
                                phase: "live",
                            });
                            break;
                        }
                    }
                }
                Err(_) => {
                    let _ = emitter(ConversationEventBatchPayload {
                        subscription_id: subscription_id.clone(),
                        conversation_id: conversation_id.clone(),
                        events: Vec::new(),
                        cursor: None,
                        gap: true,
                        phase: "live",
                    });
                    break;
                }
            };

            if page.events.is_empty() {
                cursor = page.cursor.or(cursor);
                continue;
            }

            let mut emit_failed = false;
            for chunk in page.events.chunks(CONVERSATION_SUBSCRIPTION_BATCH_LIMIT) {
                cursor = page.cursor.clone();
                let batch = ConversationEventBatchPayload {
                    subscription_id: subscription_id.clone(),
                    conversation_id: conversation_id.clone(),
                    events: chunk.to_vec(),
                    cursor: cursor.clone(),
                    gap: page.gap,
                    phase: "live",
                };
                if emitter(batch).is_err() {
                    emit_failed = true;
                    break;
                }
            }
            if emit_failed {
                break;
            }
        }

        state
            .conversation_event_subscriptions
            .lock()
            .await
            .remove(&subscription_id);
        let _ = window_label;
    })
}

fn is_conversation_cursor_mismatch(error: &CommandErrorPayload) -> bool {
    error.code == "RUNTIME_OPERATION_FAILED"
        && error.message.contains("conversation cursor is unknown")
}

async fn resync_conversation_subscription_page(
    conversation_id: &str,
    state: &DesktopRuntimeState,
) -> Result<PageConversationTimelineResponse, CommandErrorPayload> {
    let worktree = page_conversation_worktree_with_runtime_state(
        PageConversationWorktreeRequest {
            conversation_id: conversation_id.to_owned(),
            page_cursor: None,
            direction: PageConversationWorktreeDirection::Before,
            limit: Some(1),
        },
        state,
    )
    .await?;

    Ok(PageConversationTimelineResponse {
        events: Vec::new(),
        cursor: worktree.event_cursor,
        gap: true,
    })
}

pub async fn export_support_bundle_with_runtime_state(
    request: ExportSupportBundleRequest,
    state: &DesktopRuntimeState,
) -> Result<ExportSupportBundleResponse, CommandErrorPayload> {
    ensure_optional("conversationId", request.conversation_id.as_deref())?;
    ensure_optional("runId", request.run_id.as_deref())?;
    require_conversation_id_for_replay(request.conversation_id.as_deref())?;

    let events = read_replay_run_events(
        ListActivityRequest {
            conversation_id: request.conversation_id.clone(),
            run_id: request.run_id.clone(),
        },
        state,
    )
    .await
    .map_err(support_bundle_read_error)?;
    let event_count = events.len().min(u32::MAX as usize) as u32;
    let exported_at = now();
    let stamp = exported_at.format("%Y%m%dT%H%M%S%.3fZ");
    let export_id = RunId::new();
    let export_dir = export_response_dir(state, request.conversation_id.as_deref());
    let jsonl_path = export_dir.join(format!("events-{stamp}-{export_id}.jsonl"));
    let markdown_path = export_dir.join(format!("support-report-{stamp}-{export_id}.md"));
    let bundle_path = export_dir.join(format!("support-bundle-{stamp}-{export_id}.json"));
    let safe_events = events
        .iter()
        .map(support_bundle_safe_event)
        .collect::<Vec<_>>();
    let jsonl = safe_events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| support_bundle_operation_failed())?
        .join("\n");
    let markdown = support_bundle_markdown(&request, exported_at.to_rfc3339(), event_count);
    let bundle = json!({
        "conversationId": request.conversation_id,
        "runId": request.run_id,
        "exportedAt": exported_at.to_rfc3339(),
        "eventCount": event_count,
        "redacted": true,
        "events": safe_events,
    });
    let bundle = serde_json::to_string(&bundle).map_err(|_| support_bundle_operation_failed())?;

    write_support_bundle_file(&export_absolute_path(state, &jsonl_path), &jsonl)?;
    write_support_bundle_file(&export_absolute_path(state, &markdown_path), &markdown)?;
    write_support_bundle_file(&export_absolute_path(state, &bundle_path), &bundle)?;

    Ok(ExportSupportBundleResponse {
        bundle_path: bundle_path.to_string_lossy().into_owned(),
        event_count,
        exported_at: exported_at.to_rfc3339(),
        jsonl_path: jsonl_path.to_string_lossy().into_owned(),
        markdown_path: markdown_path.to_string_lossy().into_owned(),
        redacted: true,
    })
}

fn export_response_dir(state: &DesktopRuntimeState, conversation_id: Option<&str>) -> PathBuf {
    if state.project_workspace_root().is_some() {
        return PathBuf::from(".jyowo").join("runtime").join("exports");
    }

    match conversation_id {
        Some(conversation_id) => PathBuf::from("exports").join(conversation_id),
        None => PathBuf::from("exports"),
    }
}

fn evidence_export_response_path(
    state: &DesktopRuntimeState,
    conversation_id: &str,
    kind: &str,
) -> String {
    let file_name = format!("evidence-{kind}-{}.txt", RunId::new());
    let path = if state.project_workspace_root().is_some() {
        PathBuf::from(".jyowo")
            .join("runtime")
            .join("exports")
            .join(file_name)
    } else {
        PathBuf::from("exports")
            .join(conversation_id)
            .join(file_name)
    };
    path.to_string_lossy().into_owned()
}

fn export_absolute_path(state: &DesktopRuntimeState, relative_path: &Path) -> PathBuf {
    if let Some(workspace_root) = state.project_workspace_root() {
        workspace_root.join(relative_path)
    } else {
        state.runtime_root().join(relative_path)
    }
}

fn support_bundle_safe_event(event: &RunEventPayload) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("id".to_owned(), json!(event.id));
    object.insert("type".to_owned(), json!(event.event_type));
    object.insert("runId".to_owned(), json!(event.run_id));
    object.insert("source".to_owned(), json!(event.source));
    object.insert("timestamp".to_owned(), json!(event.timestamp));
    object.insert("visibility".to_owned(), json!(event.visibility));

    let identifiers = support_bundle_identifiers(&event.payload);
    if !identifiers.is_empty() {
        object.insert("identifiers".to_owned(), Value::Object(identifiers));
    }

    let summary = support_bundle_summary(event);
    if !summary.is_empty() {
        object.insert("summary".to_owned(), Value::Object(summary));
    }

    Value::Object(object)
}

fn support_bundle_identifiers(payload: &Value) -> serde_json::Map<String, Value> {
    let mut identifiers = serde_json::Map::new();
    for key in [
        "artifactId",
        "backgroundAgentId",
        "messageId",
        "noticeId",
        "requestId",
        "sessionId",
        "subagentId",
        "teamId",
        "toolUseId",
        "triggerToolUseId",
    ] {
        support_bundle_copy_payload_key(payload, &mut identifiers, key);
    }
    identifiers
}

fn support_bundle_summary(event: &RunEventPayload) -> serde_json::Map<String, Value> {
    let mut summary = serde_json::Map::new();
    match event.event_type {
        "run.started" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "permissionMode");
        }
        "run.ended" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "reason");
            support_bundle_copy_payload_key(&event.payload, &mut summary, "usage");
        }
        "permission.requested" => {
            for key in [
                "actorSource",
                "autoResolved",
                "exposure",
                "operation",
                "reason",
                "severity",
                "target",
            ] {
                support_bundle_copy_payload_key(&event.payload, &mut summary, key);
            }
        }
        "permission.resolved"
        | "background.permission.resolved"
        | "subagent.permission.resolved" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "decision");
        }
        "tool.requested" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "toolName");
            summary.insert(
                "arguments".to_owned(),
                json!("withheld from support bundle"),
            );
        }
        "tool.completed" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "durationMs");
            summary.insert("output".to_owned(), json!("withheld from support bundle"));
        }
        "tool.failed" | "engine.failed" | "plugin.failed" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "message");
        }
        "artifact.created" | "artifact.updated" => {
            for key in ["kind", "source", "status"] {
                support_bundle_copy_payload_key(&event.payload, &mut summary, key);
            }
        }
        "subagent.spawned" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "depth");
            summary.insert(
                "taskSummary".to_owned(),
                json!("withheld from support bundle"),
            );
        }
        "subagent.announced" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "status");
            support_bundle_copy_payload_key(&event.payload, &mut summary, "redacted");
            summary.insert(
                "resultSummary".to_owned(),
                json!("withheld from support bundle"),
            );
        }
        "subagent.terminated" | "team.terminated" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "reason");
        }
        "team.created" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "topologyKind");
        }
        "team.task.updated" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "status");
        }
        "background.permission.requested" => {
            support_bundle_copy_payload_key(&event.payload, &mut summary, "reason");
        }
        _ => {}
    }
    summary
}

fn permission_actor_source_payload(
    actor_source: &PermissionActorSource,
    redactor: &dyn Redactor,
) -> PermissionActorSourceRunEventPayload {
    match actor_source {
        PermissionActorSource::ParentRun => PermissionActorSourceRunEventPayload::ParentRun,
        PermissionActorSource::Subagent {
            subagent_id,
            parent_session_id,
            parent_run_id,
            team_id,
            team_member_profile_id,
        } => PermissionActorSourceRunEventPayload::Subagent {
            subagent_id: subagent_id.to_string(),
            parent_session_id: parent_session_id.to_string(),
            parent_run_id: parent_run_id.to_string(),
            team_id: team_id.map(|id| id.to_string()),
            team_member_profile_id: team_member_profile_id
                .as_ref()
                .map(|profile_id| public_text_display(profile_id.clone(), redactor)),
        },
        PermissionActorSource::TeamMember {
            team_id,
            agent_id,
            role,
            parent_run_id,
        } => PermissionActorSourceRunEventPayload::TeamMember {
            team_id: team_id.to_string(),
            agent_id: agent_id.to_string(),
            role: public_text_display(role.clone(), redactor),
            parent_run_id: parent_run_id.map(|id| id.to_string()),
        },
        PermissionActorSource::BackgroundAgent {
            background_agent_id,
            conversation_id,
            attempt_id,
        } => PermissionActorSourceRunEventPayload::BackgroundAgent {
            background_agent_id: background_agent_id.to_string(),
            conversation_id: conversation_id.to_string(),
            attempt_id: attempt_id.map(|id| id.to_string()),
        },
        PermissionActorSource::Automation {
            automation_id,
            conversation_id,
            run_id,
        } => PermissionActorSourceRunEventPayload::Automation {
            automation_id: public_text_display(automation_id.clone(), redactor),
            conversation_id: conversation_id.to_string(),
            run_id: run_id.map(|id| id.to_string()),
        },
        PermissionActorSource::McpServer {
            server_id,
            origin,
            scope,
        } => PermissionActorSourceRunEventPayload::McpServer {
            server_id: server_id.0.clone(),
            origin: manifest_origin_run_event_payload(origin, redactor),
            scope: mcp_server_scope_run_event_payload(scope),
        },
    }
}

fn manifest_origin_run_event_payload(
    origin: &ManifestOriginRef,
    redactor: &dyn Redactor,
) -> ManifestOriginRunEventPayload {
    match origin {
        ManifestOriginRef::File { path } => ManifestOriginRunEventPayload::File {
            path: public_text_display(path.clone(), redactor),
        },
        ManifestOriginRef::CargoExtension { binary } => {
            ManifestOriginRunEventPayload::CargoExtension {
                binary: public_text_display(binary.clone(), redactor),
            }
        }
        ManifestOriginRef::RemoteRegistry { endpoint } => {
            ManifestOriginRunEventPayload::RemoteRegistry {
                endpoint: public_text_display(endpoint.clone(), redactor),
            }
        }
        _ => ManifestOriginRunEventPayload::Unknown,
    }
}

fn mcp_server_scope_run_event_payload(scope: &McpServerScope) -> McpServerScopeRunEventPayload {
    match scope {
        McpServerScope::Global => McpServerScopeRunEventPayload::Global,
        McpServerScope::Session(session_id) => McpServerScopeRunEventPayload::Session {
            conversation_id: session_id.to_string(),
        },
        McpServerScope::Agent(agent_id) => McpServerScopeRunEventPayload::Agent {
            agent_id: agent_id.to_string(),
        },
        _ => McpServerScopeRunEventPayload::Unknown,
    }
}

fn support_bundle_copy_payload_key(
    payload: &Value,
    target: &mut serde_json::Map<String, Value>,
    key: &str,
) {
    if let Some(value) = payload.get(key) {
        target.insert(key.to_owned(), value.clone());
    }
}

pub async fn get_context_snapshot_with_runtime_state(
    request: GetContextSnapshotRequest,
    state: &DesktopRuntimeState,
) -> Result<GetContextSnapshotResponse, CommandErrorPayload> {
    ensure_optional("conversationId", request.conversation_id.as_deref())?;
    ensure_optional("runId", request.run_id.as_deref())?;
    let session_id = match request.conversation_id.as_deref() {
        Some(conversation_id) => parse_session_id(conversation_id)?,
        None => state.default_conversation_id(),
    };
    let run_id = request.run_id.as_deref().map(parse_run_id).transpose()?;
    let Some(_harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading context snapshot requires the runtime conversation facade.",
        ));
    };
    let redactor = DefaultRedactor::default();
    let mut active_artifact = None;
    let mut next_actions = Vec::new();

    // Context snapshot is display-only metadata. If a selected conversation has no event stream
    // yet, keep the workspace metadata visible instead of failing the UI.
    if let Ok(payload) = collect_artifacts_from_runtime_state(state, session_id).await {
        active_artifact = payload
            .artifacts
            .into_iter()
            .find(|artifact| {
                run_id
                    .as_ref()
                    .is_none_or(|run_id| artifact.source_run_id == run_id.to_string())
            })
            .map(|artifact| artifact.title);
    }

    if let Some(title) = active_artifact.as_ref() {
        next_actions.push(format!("Review {title}"));
    }
    let decisions =
        context_decisions_from_pending_requests(state, session_id, run_id.as_ref(), &redactor);
    if !decisions.is_empty() {
        next_actions.push("Resolve pending runtime decisions".to_owned());
    }
    if next_actions.is_empty() {
        next_actions.push("Continue the conversation".to_owned());
    }

    let (files, path, project) = if let Some(workspace_root) = state.project_workspace_root() {
        (
            context_files_from_workspace(workspace_root),
            "workspace://local".to_owned(),
            redacted_display(workspace_project_name(workspace_root), &redactor),
        )
    } else {
        (
            Vec::new(),
            "runtime://global-conversation".to_owned(),
            "No workspace".to_owned(),
        )
    };

    Ok(GetContextSnapshotResponse {
        active_artifact,
        decisions,
        files,
        next_actions,
        path,
        project,
    })
}

pub(crate) async fn conversation_tail_event_id(
    harness: &Harness,
    options: SessionOptions,
) -> Result<Option<EventId>, CommandErrorPayload> {
    let mut after_event_id = None;
    let mut tail_event_id = None;

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: options.clone(),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("conversation event page failed: {error}"))
            })?;
        let Some(next_event_id) = page.next_event_id else {
            return Ok(tail_event_id);
        };

        tail_event_id = Some(next_event_id);
        after_event_id = Some(next_event_id);
    }
}

pub(crate) async fn wait_for_started_conversation_run(
    harness: &Harness,
    options: SessionOptions,
    mut after_event_id: Option<EventId>,
    run_task: &mut tokio::task::JoinHandle<
        Result<jyowo_harness_sdk::ConversationTurnReceipt, jyowo_harness_sdk::HarnessError>,
    >,
) -> Result<RunId, CommandErrorPayload> {
    let deadline = tokio::time::Instant::now() + START_RUN_STARTED_TIMEOUT;

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: options.clone(),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|error| {
                runtime_operation_failed(format!("conversation event page failed: {error}"))
            })?;

        for envelope in &page.events {
            if let Event::RunStarted(started) = &envelope.payload {
                if started.session_id == options.session_id
                    && started.tenant_id == options.tenant_id
                {
                    return Ok(started.run_id);
                }
            }
        }

        if let Some(next_event_id) = page.next_event_id {
            after_event_id = Some(next_event_id);
        }

        if run_task.is_finished() {
            let receipt = run_task.await.map_err(|error| {
                runtime_operation_failed(format!("conversation run task failed: {error}"))
            })?;
            return receipt.map(|receipt| receipt.run_id).map_err(|error| {
                runtime_operation_failed(format!("conversation run failed: {error}"))
            });
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(runtime_operation_failed(
                "conversation run did not emit RunStarted before timeout".to_owned(),
            ));
        }

        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

pub(crate) fn parse_request_id(value: &str) -> Result<RequestId, CommandErrorPayload> {
    let request_id = RequestId::parse(value).map_err(|error| {
        invalid_payload(format!(
            "requestId must be a valid permission request id: {error}"
        ))
    })?;

    if request_id.to_string() != value {
        return Err(invalid_payload(
            "requestId must be a canonical permission request id".to_owned(),
        ));
    }

    Ok(request_id)
}

pub(crate) fn parse_permission_option_id(
    value: &str,
) -> Result<PermissionOptionId, CommandErrorPayload> {
    let option_id = PermissionOptionId::parse(value).map_err(|error| {
        invalid_payload(format!(
            "optionId must be a valid permission option id: {error}"
        ))
    })?;

    if option_id.to_string() != value {
        return Err(invalid_payload(
            "optionId must be a canonical permission option id".to_owned(),
        ));
    }

    Ok(option_id)
}

pub(crate) fn parse_session_id(value: &str) -> Result<SessionId, CommandErrorPayload> {
    let session_id = SessionId::parse(value).map_err(|error| {
        invalid_payload(format!(
            "conversationId must be a valid conversation session id: {error}"
        ))
    })?;

    if session_id.to_string() != value {
        return Err(invalid_payload(
            "conversationId must be a canonical conversation session id".to_owned(),
        ));
    }

    Ok(session_id)
}

pub(crate) fn validate_client_message_id(value: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("clientMessageId", value)?;
    if is_uuid_v4_like(value) {
        return Ok(());
    }

    Err(invalid_payload(
        "clientMessageId must be a UUID generated by the desktop client".to_owned(),
    ))
}

pub(crate) fn is_uuid_v4_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }

    for index in [8, 13, 18, 23] {
        if bytes[index] != b'-' {
            return false;
        }
    }
    if bytes[14] != b'4' || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B') {
        return false;
    }

    bytes
        .iter()
        .enumerate()
        .filter(|(index, _)| !matches!(index, 8 | 13 | 18 | 23))
        .all(|(_, byte)| byte.is_ascii_hexdigit())
}

fn submitted_permission_decision(decision: PermissionDecision) -> Decision {
    match decision {
        PermissionDecision::Approve => Decision::AllowOnce,
        PermissionDecision::Deny => Decision::DenyOnce,
    }
}

fn permission_decision_from_resolved(
    decision: Decision,
) -> Result<PermissionDecision, CommandErrorPayload> {
    match decision {
        Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent => {
            Ok(PermissionDecision::Approve)
        }
        Decision::DenyOnce | Decision::DenyPermanent => Ok(PermissionDecision::Deny),
        Decision::Escalate => Err(runtime_operation_failed(
            "resolved permission option cannot be represented as approve or deny".to_owned(),
        )),
        _ => Err(runtime_operation_failed(
            "resolved permission option uses an unsupported decision".to_owned(),
        )),
    }
}

pub(crate) fn run_event_payload_from_read_model(
    event: harness_contracts::ConversationTimelineEvent,
) -> Result<RunEventPayload, CommandErrorPayload> {
    Ok(RunEventPayload {
        id: event.id,
        conversation_sequence: event.cursor.conversation_sequence,
        payload: event.payload,
        run_id: event.run_id,
        sequence: event.sequence,
        source: run_event_source_label(&event.source)?,
        timestamp: event.timestamp.to_rfc3339(),
        event_type: run_event_type_label(&event.event_type)?,
        visibility: run_event_visibility_label(&event.visibility)?,
    })
}

pub(crate) fn run_event_source_label(value: &str) -> Result<&'static str, CommandErrorPayload> {
    match value {
        "user" => Ok("user"),
        "assistant" => Ok("assistant"),
        "tool" => Ok("tool"),
        "engine" => Ok("engine"),
        "policy" => Ok("policy"),
        "agent" => Ok("agent"),
        "background" => Ok("background"),
        "plugin" => Ok("plugin"),
        _ => Err(runtime_operation_failed(
            "conversation timeline source is invalid".to_owned(),
        )),
    }
}

pub(crate) fn run_event_visibility_label(value: &str) -> Result<&'static str, CommandErrorPayload> {
    match value {
        "public" => Ok("public"),
        "redacted" => Ok("redacted"),
        "withheld" => Ok("withheld"),
        _ => Err(runtime_operation_failed(
            "conversation timeline visibility is invalid".to_owned(),
        )),
    }
}

pub(crate) fn run_event_type_label(value: &str) -> Result<&'static str, CommandErrorPayload> {
    match value {
        "run.started" => Ok("run.started"),
        "run.ended" => Ok("run.ended"),
        "user.message.appended" => Ok("user.message.appended"),
        "assistant.delta" => Ok("assistant.delta"),
        "assistant.thinking.delta" => Ok("assistant.thinking.delta"),
        "assistant.completed" => Ok("assistant.completed"),
        "assistant.review.requested" => Ok("assistant.review.requested"),
        "assistant.clarification.requested" => Ok("assistant.clarification.requested"),
        "assistant.notice" => Ok("assistant.notice"),
        "tool.requested" => Ok("tool.requested"),
        "tool.approved" => Ok("tool.approved"),
        "tool.denied" => Ok("tool.denied"),
        "tool.completed" => Ok("tool.completed"),
        "tool.failed" => Ok("tool.failed"),
        "permission.requested" => Ok("permission.requested"),
        "permission.resolved" => Ok("permission.resolved"),
        "subagent.spawned" => Ok("subagent.spawned"),
        "subagent.announced" => Ok("subagent.announced"),
        "subagent.terminated" => Ok("subagent.terminated"),
        "subagent.stalled" => Ok("subagent.stalled"),
        "subagent.permission.forwarded" => Ok("subagent.permission.forwarded"),
        "subagent.permission.resolved" => Ok("subagent.permission.resolved"),
        "team.created" => Ok("team.created"),
        "team.member.joined" => Ok("team.member.joined"),
        "team.member.left" => Ok("team.member.left"),
        "team.member.stalled" => Ok("team.member.stalled"),
        "team.task.updated" => Ok("team.task.updated"),
        "agent.message.sent" => Ok("agent.message.sent"),
        "agent.message.routed" => Ok("agent.message.routed"),
        "team.turn.completed" => Ok("team.turn.completed"),
        "team.terminated" => Ok("team.terminated"),
        "background.started" => Ok("background.started"),
        "background.state.changed" => Ok("background.state.changed"),
        "background.input.requested" => Ok("background.input.requested"),
        "background.input.submitted" => Ok("background.input.submitted"),
        "background.permission.requested" => Ok("background.permission.requested"),
        "background.permission.resolved" => Ok("background.permission.resolved"),
        "background.cancelled" => Ok("background.cancelled"),
        "background.completed" => Ok("background.completed"),
        "background.failed" => Ok("background.failed"),
        "background.interrupted" => Ok("background.interrupted"),
        "background.archived" => Ok("background.archived"),
        "background.deleted" => Ok("background.deleted"),
        "artifact.created" => Ok("artifact.created"),
        "artifact.updated" => Ok("artifact.updated"),
        "engine.failed" => Ok("engine.failed"),
        _ => Err(runtime_operation_failed(
            "conversation timeline event type is invalid".to_owned(),
        )),
    }
}

pub(crate) fn permission_requested_run_event(
    event_id: String,
    event: &Event,
    sequence: u64,
    redactor: &dyn Redactor,
) -> RunEventPayload {
    let Event::PermissionRequested(event) = event else {
        unreachable!("permission activity must be built from PermissionRequested events");
    };
    let subject = permission_subject_display(&event.subject, redactor);
    let reason = if event.auto_resolved {
        "已按本次授权模式自动允许。"
    } else {
        "需要批准后才能继续。"
    };

    RunEventPayload {
        id: event_id,
        conversation_sequence: sequence,
        payload: serde_json::to_value(PermissionRequestedRunEventPayload {
            actor_source: permission_actor_source_payload(&event.actor_source, redactor),
            action_plan_hash: event.action_plan_hash.to_string(),
            auto_resolved: event.auto_resolved,
            decision_options: permission_decision_options_run_event_payload(
                &event.presented_options,
                redactor,
            ),
            decision_scope: decision_scope_display(&event.scope_hint, redactor),
            effective_mode: permission_mode_payload(event.effective_mode),
            exposure: subject.exposure,
            operation: subject.operation,
            reason: reason.to_owned(),
            review: permission_review_run_event_payload(&event.review, redactor),
            request_id: event.request_id.to_string(),
            sandbox_policy: sandbox_policy_run_event_payload(&event.sandbox_policy, redactor),
            severity: severity_display(event.severity),
            target: subject.target,
            tool_use_id: event.tool_use_id.to_string(),
            workspace_boundary: "current workspace".to_owned(),
        })
        .unwrap_or_else(|_| json!({})),
        run_id: event.run_id.to_string(),
        sequence,
        source: "policy",
        timestamp: event.at.to_rfc3339(),
        event_type: "permission.requested",
        visibility: "public",
    }
}

fn permission_decision_options_run_event_payload(
    options: &[harness_contracts::PermissionDecisionOption],
    _redactor: &dyn Redactor,
) -> Vec<Value> {
    options
        .iter()
        .filter_map(|option| {
            let decision = match option.decision {
                Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent => {
                    "approve"
                }
                Decision::DenyOnce | Decision::DenyPermanent => "deny",
                Decision::Escalate => return None,
                _ => return None,
            };

            Some(json!({
                "id": option.option_id.to_string(),
                "decision": decision,
                "label": permission_decision_option_label(option.decision.clone(), option.lifetime),
                "lifetime": decision_lifetime_run_event_payload(option.lifetime),
                "matcher": {
                    "kind": decision_matcher_kind_run_event_payload(option.matcher_summary.kind),
                    "label": decision_matcher_label_run_event_payload(option.matcher_summary.kind),
                },
                "requiresConfirmation": option.requires_confirmation,
            }))
        })
        .collect()
}

fn permission_decision_option_label(
    decision: Decision,
    lifetime: harness_contracts::DecisionLifetime,
) -> &'static str {
    match (decision, lifetime) {
        (Decision::AllowOnce, _) => "Approve once",
        (Decision::AllowSession, _) => "Approve for session",
        (Decision::AllowPermanent, _) => "Approve permanently",
        (Decision::DenyOnce, _) => "Deny once",
        (Decision::DenyPermanent, _) => "Deny permanently",
        (_, harness_contracts::DecisionLifetime::Once) => "Decide once",
        (_, harness_contracts::DecisionLifetime::Run) => "Decide for run",
        (_, harness_contracts::DecisionLifetime::Session) => "Decide for session",
        (_, harness_contracts::DecisionLifetime::Persisted) => "Decide persistently",
    }
}

fn decision_lifetime_run_event_payload(
    lifetime: harness_contracts::DecisionLifetime,
) -> &'static str {
    match lifetime {
        harness_contracts::DecisionLifetime::Once => "once",
        harness_contracts::DecisionLifetime::Run => "run",
        harness_contracts::DecisionLifetime::Session => "session",
        harness_contracts::DecisionLifetime::Persisted => "persisted",
    }
}

fn decision_matcher_kind_run_event_payload(
    kind: harness_contracts::DecisionMatcherKind,
) -> &'static str {
    match kind {
        harness_contracts::DecisionMatcherKind::ExactCommand => "exactCommand",
        harness_contracts::DecisionMatcherKind::ExactArgs => "exactArgs",
        harness_contracts::DecisionMatcherKind::ToolName => "toolName",
        harness_contracts::DecisionMatcherKind::Category => "category",
        harness_contracts::DecisionMatcherKind::PathPrefix => "pathPrefix",
        harness_contracts::DecisionMatcherKind::GlobPattern => "globPattern",
        harness_contracts::DecisionMatcherKind::ExecuteCodeScript => "executeCodeScript",
        harness_contracts::DecisionMatcherKind::Any => "any",
    }
}

fn decision_matcher_label_run_event_payload(
    kind: harness_contracts::DecisionMatcherKind,
) -> &'static str {
    match kind {
        harness_contracts::DecisionMatcherKind::ExactCommand => "this exact command",
        harness_contracts::DecisionMatcherKind::ExactArgs => "these exact command arguments",
        harness_contracts::DecisionMatcherKind::ToolName => "this tool",
        harness_contracts::DecisionMatcherKind::Category => "this tool category",
        harness_contracts::DecisionMatcherKind::PathPrefix => "this workspace path scope",
        harness_contracts::DecisionMatcherKind::GlobPattern => "this workspace glob",
        harness_contracts::DecisionMatcherKind::ExecuteCodeScript => "execute code script",
        harness_contracts::DecisionMatcherKind::Any => "any matching operation",
    }
}

fn permission_review_run_event_payload(
    review: &PermissionReview,
    redactor: &dyn Redactor,
) -> Value {
    json!({
        "summary": public_text_display(review.summary.clone(), redactor),
        "details": review
            .details
            .iter()
            .map(|detail| {
                json!({
                    "label": public_text_display(detail.label.clone(), redactor),
                    "value": public_text_display(detail.value.clone(), redactor),
                    "redacted": detail.redacted,
                })
            })
            .collect::<Vec<_>>(),
        "confirmation": permission_confirmation_run_event_payload(&review.confirmation, redactor),
        "redacted": review.redacted,
    })
}

fn permission_confirmation_run_event_payload(
    confirmation: &PermissionConfirmation,
    redactor: &dyn Redactor,
) -> Value {
    match confirmation {
        PermissionConfirmation::None => json!({ "type": "none" }),
        PermissionConfirmation::ExplicitButton { label } => json!({
            "type": "explicitButton",
            "label": public_text_display(label.clone(), redactor),
        }),
        PermissionConfirmation::TypeToConfirm { expected } => json!({
            "type": "typeToConfirm",
            "expected": public_text_display(expected.clone(), redactor),
        }),
        _ => json!({ "type": "none" }),
    }
}

fn sandbox_policy_run_event_payload(
    policy: &SandboxPolicySummary,
    redactor: &dyn Redactor,
) -> Value {
    json!({
        "mode": sandbox_mode_run_event_payload(&policy.mode),
        "scope": sandbox_scope_run_event_payload(&policy.scope, redactor),
        "network": serde_json::to_value(&policy.network).unwrap_or(Value::Null),
        "resourceLimits": {
            "maxMemoryBytes": policy.resource_limits.max_memory_bytes,
            "maxCpuCores": policy.resource_limits.max_cpu_cores,
            "maxPids": policy.resource_limits.max_pids,
            "maxWallClockMs": policy.resource_limits.max_wall_clock_ms,
            "maxOpenFiles": policy.resource_limits.max_open_files,
        },
    })
}

fn sandbox_mode_run_event_payload(mode: &SandboxMode) -> Value {
    match mode {
        SandboxMode::None => json!("none"),
        SandboxMode::OsLevel(tag) => json!({ "osLevel": tag }),
        SandboxMode::Container => json!("container"),
        SandboxMode::Remote => json!("remote"),
        _ => json!("unknown"),
    }
}

fn sandbox_scope_run_event_payload(scope: &SandboxScope, redactor: &dyn Redactor) -> Value {
    match scope {
        SandboxScope::WorkspaceOnly => json!("workspace_only"),
        SandboxScope::WorkspacePlus(paths) => json!({
            "workspacePlus": paths
                .iter()
                .map(|path| safe_path_label(path.as_path(), redactor))
                .collect::<Vec<_>>(),
        }),
        SandboxScope::Unrestricted => json!("unrestricted"),
        _ => json!("unknown"),
    }
}

fn permission_mode_payload(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Default => "default",
        PermissionMode::Plan => "plan",
        PermissionMode::AcceptEdits => "accept_edits",
        PermissionMode::BypassPermissions => "bypass_permissions",
        PermissionMode::DontAsk => "dont_ask",
        PermissionMode::Auto => "auto",
        _ => "default",
    }
}

pub(crate) struct PermissionSubjectDisplay {
    exposure: String,
    operation: String,
    target: String,
}

pub(crate) fn permission_subject_display(
    subject: &PermissionSubject,
    redactor: &dyn Redactor,
) -> PermissionSubjectDisplay {
    match subject {
        PermissionSubject::CommandExec { command, .. } => PermissionSubjectDisplay {
            exposure: "Can execute a command inside the workspace boundary.".to_owned(),
            operation: "Execute command".to_owned(),
            target: safe_command_label(command, redactor),
        },
        PermissionSubject::ToolInvocation { tool, .. } => PermissionSubjectDisplay {
            exposure: "Can invoke a runtime tool.".to_owned(),
            operation: "Use tool".to_owned(),
            target: public_text_display(tool.clone(), redactor),
        },
        PermissionSubject::FileWrite { path, .. } => PermissionSubjectDisplay {
            exposure: "Can write a file in the workspace.".to_owned(),
            operation: "Write file".to_owned(),
            target: safe_path_label(path, redactor),
        },
        PermissionSubject::FileDelete { path } => PermissionSubjectDisplay {
            exposure: "Can delete a file in the workspace.".to_owned(),
            operation: "Delete file".to_owned(),
            target: safe_path_label(path, redactor),
        },
        PermissionSubject::NetworkAccess { host, port } => PermissionSubjectDisplay {
            exposure: "Can access a network endpoint.".to_owned(),
            operation: "Access network".to_owned(),
            target: public_text_display(
                port.map_or_else(|| host.clone(), |port| format!("{host}:{port}")),
                redactor,
            ),
        },
        PermissionSubject::DangerousCommand { command, .. } => PermissionSubjectDisplay {
            exposure: "Can execute a dangerous command.".to_owned(),
            operation: "Execute dangerous command".to_owned(),
            target: safe_command_label(command, redactor),
        },
        PermissionSubject::McpToolCall { server, tool, .. } => PermissionSubjectDisplay {
            exposure: "Can invoke an MCP tool.".to_owned(),
            operation: "Use MCP tool".to_owned(),
            target: public_text_display(format!("{server}/{tool}"), redactor),
        },
        PermissionSubject::Custom { kind, .. } => PermissionSubjectDisplay {
            exposure: "Can perform a custom permission-gated operation.".to_owned(),
            operation: "Review custom operation".to_owned(),
            target: public_text_display(kind.clone(), redactor),
        },
        _ => PermissionSubjectDisplay {
            exposure: "Can continue a permission-gated operation.".to_owned(),
            operation: "Review permission".to_owned(),
            target: "runtime operation".to_owned(),
        },
    }
}

pub(crate) fn decision_scope_display(scope: &DecisionScope, redactor: &dyn Redactor) -> String {
    match scope {
        DecisionScope::ExactCommand { .. } => "this exact command".to_owned(),
        DecisionScope::ExactArgs(_) => "these exact command arguments".to_owned(),
        DecisionScope::ToolName(_) => "this tool".to_owned(),
        DecisionScope::Category(_) => "this tool category".to_owned(),
        DecisionScope::PathPrefix(_) => "this workspace path scope".to_owned(),
        DecisionScope::GlobPattern(_) => "this workspace glob".to_owned(),
        DecisionScope::ExecuteCodeScript { .. } => "execute code script".to_owned(),
        DecisionScope::Any => "any matching operation".to_owned(),
        _ => public_text_display("current operation".to_owned(), redactor),
    }
}

pub(crate) fn safe_command_label(command: &str, redactor: &dyn Redactor) -> String {
    let executable_token = command.split_whitespace().next().unwrap_or(command);
    let executable = Path::new(executable_token)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(executable_token);
    public_text_display(executable.to_owned(), redactor)
}

pub(crate) fn safe_path_label(path: &Path, redactor: &dyn Redactor) -> String {
    let label = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(
            || "workspace file".to_owned(),
            |name| format!("workspace file: {name}"),
        );
    public_text_display(label, redactor)
}

pub(crate) async fn read_replay_run_events(
    request: ListActivityRequest,
    state: &DesktopRuntimeState,
) -> Result<Vec<RunEventPayload>, CommandErrorPayload> {
    read_replay_run_events_after(request, state, None).await
}

pub(crate) async fn read_replay_run_events_after(
    request: ListActivityRequest,
    state: &DesktopRuntimeState,
    after_cursor: Option<String>,
) -> Result<Vec<RunEventPayload>, CommandErrorPayload> {
    let session_id = match request.conversation_id.as_deref() {
        Some(conversation_id) => parse_session_id(conversation_id)?,
        None => state.default_conversation_id(),
    };
    let run_id = request.run_id.as_deref().map(parse_run_id).transpose()?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Reading replay events requires the runtime conversation facade.",
        ));
    };
    let redactor = DefaultRedactor::default();
    let mut mapper = RunEventMapper::default();
    let mut after_event_id = after_cursor
        .as_deref()
        .map(EventId::parse)
        .transpose()
        .map_err(|error| invalid_payload(format!("conversation cursor is invalid: {error}")))?;
    let mut conversation_sequence = 0;
    let mut run_sequences: HashMap<String, u64> = HashMap::new();
    if let Some(cursor_event_id) = after_event_id {
        let seed = seed_run_event_mapper_until_cursor(
            &harness,
            state.conversation_session_options(session_id)?,
            session_id,
            cursor_event_id,
            &mut mapper,
            &redactor,
        )
        .await?;
        conversation_sequence = seed.conversation_sequence;
        run_sequences = seed.run_sequences;
    }
    let mut events = Vec::new();

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: state.conversation_session_options(session_id)?,
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|error| runtime_operation_failed(format!("replay read failed: {error}")))?;
        if page.events.is_empty() {
            break;
        }

        for envelope in page.events {
            let Some(event) = mapper.map(
                envelope.event_id.to_string(),
                envelope.payload,
                session_id,
                &redactor,
            ) else {
                continue;
            };
            if run_id
                .as_ref()
                .is_some_and(|run_id| event.run_id != run_id.to_string())
            {
                continue;
            }
            let event_conversation_sequence = conversation_sequence + 1;
            conversation_sequence += 1;
            let run_sequence = run_sequences.entry(event.run_id.clone()).or_insert(0);
            events.push(RunEventPayload {
                conversation_sequence: event_conversation_sequence,
                sequence: *run_sequence + 1,
                ..event
            });
            *run_sequence += 1;
        }

        after_event_id = page.next_event_id;
    }

    Ok(events)
}

pub(crate) struct RunEventMapperSeed {
    conversation_sequence: u64,
    run_sequences: HashMap<String, u64>,
}

pub(crate) async fn seed_run_event_mapper_until_cursor(
    harness: &Harness,
    options: SessionOptions,
    session_id: SessionId,
    cursor_event_id: EventId,
    mapper: &mut RunEventMapper,
    redactor: &dyn Redactor,
) -> Result<RunEventMapperSeed, CommandErrorPayload> {
    let mut after_event_id = None;
    let mut conversation_sequence = 0;
    let mut run_sequences: HashMap<String, u64> = HashMap::new();

    loop {
        let page = harness
            .page_conversation_events(ConversationEventsPageRequest {
                options: options.clone(),
                after_event_id,
                limit: 200,
            })
            .await
            .map_err(|error| runtime_operation_failed(format!("replay read failed: {error}")))?;
        if page.events.is_empty() {
            return Err(invalid_payload("conversation cursor is unknown".to_owned()));
        }

        for envelope in page.events {
            let event_id = envelope.event_id;
            if let Some(event) =
                mapper.map(event_id.to_string(), envelope.payload, session_id, redactor)
            {
                conversation_sequence += 1;
                *run_sequences.entry(event.run_id).or_insert(0) += 1;
            }
            if event_id == cursor_event_id {
                return Ok(RunEventMapperSeed {
                    conversation_sequence,
                    run_sequences,
                });
            }
            after_event_id = Some(event_id);
        }
    }
}

pub(crate) async fn read_activity_replay_events(
    request: &ListActivityRequest,
    state: &DesktopRuntimeState,
) -> Result<Vec<RunEventPayload>, CommandErrorPayload> {
    read_replay_run_events(request.clone(), state).await
}

pub(crate) fn message_content_display(content: &MessageContent, redactor: &dyn Redactor) -> String {
    let value = match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Structured(value) => value.to_string(),
        MessageContent::Multimodal(parts) => parts
            .iter()
            .filter_map(|part| match part {
                MessagePart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };

    public_text_display(value, redactor)
}

pub(crate) fn redact_private_absolute_paths(value: String) -> String {
    redact_unsafe_display_text(&value)
}

pub(crate) fn public_text_display(value: String, redactor: &dyn Redactor) -> String {
    redact_unsafe_display_text(&redacted_display(value, redactor))
}

pub(crate) fn public_ui_safe_text_display(value: &UiSafeText, redactor: &dyn Redactor) -> String {
    redact_unsafe_display_text(
        &UiSafeText::from_redacted_display(value.as_str(), redactor).into_string(),
    )
}

pub(crate) fn redact_unsafe_display_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut index = 0;

    while index < value.len() {
        if unsafe_url_starts_at(value, index) {
            output.push_str("[REDACTED]");
            index = unsafe_url_token_end(value, index);
            continue;
        }
        if local_unsafe_path_starts_at(value, index) {
            output.push_str("[REDACTED]");
            index = unsafe_token_end(value, index);
            continue;
        }

        let ch = value[index..]
            .chars()
            .next()
            .expect("index is within string bounds");
        output.push(ch);
        index += ch.len_utf8();
    }

    output
}

pub(crate) fn token_starts_at(value: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    value[..index]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace() || (!ch.is_alphanumeric() && ch != '_'))
}

pub(crate) fn unsafe_url_starts_at(value: &str, index: usize) -> bool {
    if unsafe_opaque_url_starts_at(value, index) {
        return true;
    }

    let tail = &value[index..];
    let Some(separator) = tail.find("://") else {
        return false;
    };
    if separator == 0 {
        return false;
    }
    tail[..separator]
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

pub(crate) fn unsafe_opaque_url_starts_at(value: &str, index: usize) -> bool {
    const SCHEMES: &[&str] = &["blob:", "data:", "file:", "javascript:", "mailto:"];
    let tail = &value[index..];
    ascii_token_starts_at(value, index)
        && SCHEMES.iter().any(|scheme| {
            tail.get(..scheme.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        })
}

pub(crate) fn ascii_token_starts_at(value: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    value[..index]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace() || (!ch.is_ascii_alphanumeric() && ch != '_'))
}

pub(crate) fn local_unsafe_path_starts_at(value: &str, index: usize) -> bool {
    let tail = &value[index..];
    if tail.starts_with("~/")
        || tail.starts_with("~\\")
        || starts_with_jyowo_path(tail)
        || starts_with_known_unix_absolute_root(tail)
    {
        return true;
    }
    token_starts_at(value, index)
        && (is_probable_unix_absolute_path_start(tail) || is_windows_absolute_path_start(tail))
}

pub(crate) fn starts_with_jyowo_path(value: &str) -> bool {
    value
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(".jyowo"))
        && value
            .as_bytes()
            .get(6)
            .is_some_and(|byte| matches!(byte, b'/' | b'\\'))
}

pub(crate) fn is_windows_absolute_path_start(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

pub(crate) fn is_probable_unix_absolute_path_start(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('/') else {
        return false;
    };
    let Some(first) = rest.chars().next() else {
        return false;
    };
    if first.is_whitespace() || matches!(first, '/' | '\\') {
        return false;
    }

    value[..unsafe_token_end(value, 0)]
        .as_bytes()
        .get(1..)
        .is_some_and(|bytes| bytes.contains(&b'/') || bytes.contains(&b'\\'))
}

pub(crate) fn starts_with_known_unix_absolute_root(value: &str) -> bool {
    const ROOTS: &[&str] = &[
        "/Applications",
        "/Library",
        "/System",
        "/Users",
        "/Volumes",
        "/dev",
        "/etc",
        "/home",
        "/media",
        "/mnt",
        "/opt",
        "/private",
        "/root",
        "/run",
        "/tmp",
        "/usr",
        "/var",
    ];

    ROOTS.iter().any(|root| {
        value
            .strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\'))
    })
}

pub(crate) fn unsafe_url_token_end(value: &str, start: usize) -> usize {
    if starts_with_unsafe_opaque_scheme(value, start, "data:")
        || starts_with_unsafe_opaque_scheme(value, start, "javascript:")
    {
        return unsafe_data_url_token_end(value, start);
    }

    unsafe_token_end(value, start)
}

pub(crate) fn starts_with_unsafe_opaque_scheme(value: &str, start: usize, scheme: &str) -> bool {
    ascii_token_starts_at(value, start)
        && value[start..]
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
}

pub(crate) fn unsafe_data_url_token_end(value: &str, start: usize) -> usize {
    value[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            (matches!(
                ch,
                '"' | '\''
                    | '`'
                    | '，'
                    | '。'
                    | '；'
                    | '、'
                    | '）'
                    | '】'
                    | '」'
                    | '》'
                    | '！'
                    | '？'
            ))
            .then_some(start + offset)
        })
        .unwrap_or(value.len())
}

pub(crate) fn unsafe_token_end(value: &str, start: usize) -> usize {
    value[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            (ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\''
                        | '`'
                        | ')'
                        | ']'
                        | '}'
                        | ','
                        | ';'
                        | '<'
                        | '>'
                        | '，'
                        | '。'
                        | '；'
                        | '、'
                        | '）'
                        | '】'
                        | '」'
                        | '》'
                        | '！'
                        | '？'
                ))
            .then_some(start + offset)
        })
        .unwrap_or(value.len())
}

pub(crate) fn truncate_utf8(value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

pub(crate) fn context_decisions_from_pending_requests(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    run_id: Option<&RunId>,
    redactor: &dyn Redactor,
) -> Vec<ContextDecisionPayload> {
    let mut pending_requests = state.pending_permission_requests();
    pending_requests.sort_by_key(|pending| {
        (
            pending.request.created_at,
            pending.request.request_id.to_string(),
        )
    });

    pending_requests
        .into_iter()
        .filter(|pending| {
            pending.request.session_id == session_id
                && run_id.is_none_or(|run_id| pending.context.run_id == Some(*run_id))
        })
        .map(|pending| ContextDecisionPayload {
            detail: format!(
                "{} permission is waiting for decision {}.",
                severity_display(pending.request.severity),
                pending.request.request_id
            ),
            request_id: Some(pending.request.request_id.to_string()),
            title: format!(
                "Approve {}",
                public_text_display(pending.request.tool_name, redactor)
            ),
        })
        .collect()
}

pub(crate) fn context_files_from_workspace(workspace_root: &Path) -> Vec<ContextFilePayload> {
    [
        "apps/desktop/src/main.tsx",
        "apps/desktop/src/routes/index.tsx",
        "apps/desktop/src/shared/tauri/commands.ts",
        "apps/desktop/src-tauri/src/commands/mod.rs",
        "crates/jyowo-harness-sdk/src/lib.rs",
    ]
    .into_iter()
    .filter_map(|label| {
        workspace_root
            .join(label)
            .is_file()
            .then(|| ContextFilePayload {
                label: label.to_owned(),
                state: Some("ready"),
            })
    })
    .take(5)
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachmentRecord {
    pub(crate) attachment: AttachmentReferencePayload,
    pub(crate) blob_ref: BlobRef,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentConversationIndex {
    attachment_ids: BTreeSet<String>,
}

pub(crate) fn canonicalize_existing_file(
    path: &Path,
    field: &'static str,
) -> Result<PathBuf, CommandErrorPayload> {
    path.canonicalize()
        .map_err(|error| invalid_payload(format!("{field} is invalid: {error}")))
}

pub(crate) fn workspace_relative_path(path: &Path, workspace_root: &Path) -> Option<String> {
    let workspace_root = workspace_root.canonicalize().ok()?;
    path.strip_prefix(workspace_root)
        .ok()
        .map(path_to_workspace_string)
}

pub(crate) fn path_to_workspace_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn attachment_id(path: &Path, size_bytes: u64) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(&size_bytes.to_le_bytes());
    if let Ok(metadata) = path.metadata() {
        if let Ok(modified_at) = metadata.modified() {
            if let Ok(duration) = modified_at.duration_since(std::time::UNIX_EPOCH) {
                hasher.update(&duration.as_nanos().to_le_bytes());
            }
        }
    }
    format!("attachment-{}", hasher.finalize().to_hex())
}

pub(crate) fn attachment_record_path(runtime_root: &Path, attachment_id: &str) -> PathBuf {
    runtime_root
        .join("attachments")
        .join("records")
        .join(format!("{attachment_id}.json"))
}

fn attachment_conversation_index_path(runtime_root: &Path, session_id: SessionId) -> PathBuf {
    runtime_root
        .join("attachments")
        .join("conversations")
        .join(format!("{session_id}.json"))
}

fn no_workspace_attachment_conversation_session_id(
    state: &DesktopRuntimeState,
    requested_conversation_id: Option<&str>,
) -> Result<Option<SessionId>, CommandErrorPayload> {
    if state.project_workspace_root().is_some() {
        return Ok(None);
    }
    if let Some(conversation_id) = requested_conversation_id {
        return SessionId::parse(conversation_id)
            .map(Some)
            .map_err(|_| invalid_payload("conversationId must be a valid session id".to_owned()));
    }
    let Some(value) = state
        .conversation_cwd()
        .file_name()
        .and_then(|value| value.to_str())
    else {
        return Err(runtime_operation_failed(
            "no-workspace conversation id is unavailable".to_owned(),
        ));
    };
    SessionId::parse(value)
        .map(Some)
        .map_err(|_| runtime_operation_failed("no-workspace conversation id is invalid".to_owned()))
}

fn read_attachment_conversation_index(
    path: &Path,
) -> Result<AttachmentConversationIndex, CommandErrorPayload> {
    read_json_file_invalid_payload(path, "attachment ownership index")
        .map(|index| index.unwrap_or_default())
}

fn write_attachment_conversation_index(
    path: &Path,
    index: &AttachmentConversationIndex,
) -> Result<(), CommandErrorPayload> {
    write_json_file_atomic(path, "attachment ownership index", index)
}

fn record_no_workspace_attachment_owner(
    state: &DesktopRuntimeState,
    session_id: Option<SessionId>,
    attachment_id: &str,
) -> Result<(), CommandErrorPayload> {
    let Some(session_id) = session_id else {
        return Ok(());
    };
    ensure_attachment_id(attachment_id)?;
    let index_path = attachment_conversation_index_path(state.runtime_root(), session_id);
    let mut index = read_attachment_conversation_index(&index_path)?;
    index.attachment_ids.insert(attachment_id.to_owned());
    write_attachment_conversation_index(&index_path, &index)
}

pub(crate) fn no_workspace_attachment_belongs_to_conversation(
    runtime_root: &Path,
    session_id: SessionId,
    attachment_id: &str,
) -> Result<bool, CommandErrorPayload> {
    ensure_attachment_id(attachment_id)?;
    let index_path = attachment_conversation_index_path(runtime_root, session_id);
    let index = read_attachment_conversation_index(&index_path)?;
    Ok(index.attachment_ids.contains(attachment_id))
}

fn cleanup_no_workspace_attachment_records(
    runtime_root: &Path,
    session_id: SessionId,
) -> Result<(), CommandErrorPayload> {
    let index_path = attachment_conversation_index_path(runtime_root, session_id);
    let index = read_attachment_conversation_index(&index_path)?;
    for attachment_id in index.attachment_ids {
        ensure_attachment_id(&attachment_id)?;
        let record_path = attachment_record_path(runtime_root, &attachment_id);
        ensure_no_symlink_components(&record_path, "no-workspace attachment record")?;
        match std::fs::remove_file(&record_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "no-workspace attachment record removal failed: {error}"
                )));
            }
        }
    }
    ensure_no_symlink_components(&index_path, "no-workspace attachment ownership index")?;
    match std::fs::remove_file(&index_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(runtime_operation_failed(format!(
                "no-workspace attachment ownership index removal failed: {error}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn write_attachment_record(
    runtime_root: &Path,
    record: &AttachmentRecord,
) -> Result<(), CommandErrorPayload> {
    let path = attachment_record_path(runtime_root, &record.attachment.id);
    write_json_file_atomic(&path, "attachment record", record)
}

pub(crate) fn read_attachment_record(
    runtime_root: &Path,
    attachment_id: &str,
) -> Result<AttachmentRecord, CommandErrorPayload> {
    ensure_attachment_id(attachment_id)?;
    let path = attachment_record_path(runtime_root, attachment_id);
    let record = read_json_file_invalid_payload(&path, "attachment record").map_err(|error| {
        if error.message.contains("symlink") {
            invalid_payload(error.message)
        } else {
            error
        }
    })?;
    record.ok_or_else(|| invalid_payload("attachment reference does not exist".to_owned()))
}

pub(crate) fn infer_mime_type(path: &Path) -> String {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "css" => "text/css",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "md" | "markdown" => "text/markdown",
        "rs" | "tsx" | "ts" | "js" | "jsx" | "txt" | "toml" | "yaml" | "yml" => "text/plain",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        _ => "application/octet-stream",
    }
    .to_owned()
}

pub(crate) fn workspace_project_name(workspace_root: &Path) -> String {
    workspace_root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Local workspace")
        .to_owned()
}

#[derive(Default)]
pub(crate) struct RunEventMapper {
    allowed_run_ids: HashSet<RunId>,
    current_run_id: Option<RunId>,
    background_agent_run_ids: HashMap<BackgroundAgentId, RunId>,
    permission_run_ids: HashMap<RequestId, RunId>,
    subagent_run_ids: HashMap<SubagentId, RunId>,
    team_run_ids: HashMap<TeamId, RunId>,
    tool_run_ids: HashMap<ToolUseId, RunId>,
}

impl RunEventMapper {
    fn is_allowed_run(&self, run_id: &RunId) -> bool {
        self.allowed_run_ids.contains(run_id)
    }

    pub(crate) fn map(
        &mut self,
        event_id: String,
        event: Event,
        requested_session_id: SessionId,
        redactor: &dyn Redactor,
    ) -> Option<RunEventPayload> {
        match event {
            Event::RunStarted(event) => {
                if event.session_id != requested_session_id {
                    return None;
                }

                self.allowed_run_ids.insert(event.run_id);
                self.current_run_id = Some(event.run_id);
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "model": run_model_payload(&event.model, redactor),
                        "permissionMode": event.permission_mode,
                        "sessionId": event.session_id.to_string(),
                    }),
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "engine",
                    timestamp: event.started_at.to_rfc3339(),
                    event_type: "run.started",
                    visibility: "public",
                })
            }
            Event::RunEnded(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                let mut payload = json!({ "reason": run_end_reason_display(&event.reason, redactor) });
                if let Some(usage) = event.usage {
                    payload["usage"] = json!({
                        "cacheReadTokens": usage.cache_read_tokens,
                        "cacheWriteTokens": usage.cache_write_tokens,
                        "costMicros": usage.cost_micros,
                        "inputTokens": usage.input_tokens,
                        "outputTokens": usage.output_tokens,
                        "toolCalls": usage.tool_calls,
                    });
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "engine",
                    timestamp: event.ended_at.to_rfc3339(),
                    event_type: "run.ended",
                    visibility: "public",
                })
            }
            Event::UserMessageAppended(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                let mut payload = json!({
                    "messageId": event.message_id.to_string(),
                    "body": message_content_display(&event.content, redactor),
                });
                if let Some(client_message_id) = event
                    .metadata
                    .labels
                    .get("clientMessageId")
                    .filter(|client_message_id| is_uuid_v4_like(client_message_id))
                {
                    payload["clientMessageId"] = json!(client_message_id);
                }
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "user",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "user.message.appended",
                    visibility: "public",
                })
            }
            Event::AssistantDeltaProduced(event) => match event.delta {
                DeltaChunk::Text(text) => {
                    if !self.is_allowed_run(&event.run_id) {
                        return None;
                    }

                    Some(RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "messageId": event.message_id.to_string(),
                            "text": public_text_display(text, redactor),
                        }),
                        run_id: event.run_id.to_string(),
                        sequence: 0,
                        source: "assistant",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "assistant.delta",
                        visibility: "public",
                    })
                }
                DeltaChunk::Thought(_) => {
                    if !self.is_allowed_run(&event.run_id) {
                        return None;
                    }

                    Some(RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "status": "running",
                        }),
                        run_id: event.run_id.to_string(),
                        sequence: 0,
                        source: "assistant",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "assistant.thinking.delta",
                        visibility: "public",
                    })
                }
                DeltaChunk::ReasoningSummary(summary) => {
                    if !self.is_allowed_run(&event.run_id) {
                        return None;
                    }

                    Some(RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "safeSummaryDelta": public_text_display(summary.text, redactor),
                            "status": "running",
                        }),
                        run_id: event.run_id.to_string(),
                        sequence: 0,
                        source: "assistant",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "assistant.thinking.delta",
                        visibility: "public",
                    })
                }
                DeltaChunk::ToolUseStart { .. }
                | DeltaChunk::ToolUseInputDelta { .. }
                | DeltaChunk::ToolUseEnd { .. } => None,
                _ => None,
            },
            Event::AssistantMessageCompleted(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "messageId": event.message_id.to_string(),
                        "body": message_content_display(&event.content, redactor),
                        "toolUses": event.tool_uses.iter().map(|tool_use| {
                            json!({
                                "toolUseId": tool_use.tool_use_id.to_string(),
                                "toolName": public_text_display(tool_use.tool_name.clone(), redactor),
                            })
                        }).collect::<Vec<_>>(),
                    }),
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "assistant",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "assistant.completed",
                    visibility: "public",
                })
            }
            Event::ArtifactCreated(event) => {
                if event.session_id != requested_session_id || !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                let artifact_kind = event.kind;
                let mut payload = json!({
                    "artifactId": event.artifact_id,
                    "kind": public_text_display(artifact_kind.clone(), redactor),
                    "status": artifact_status_label(event.status),
                    "source": artifact_source_label(event.source),
                    "title": public_text_display(event.title, redactor),
                });
                if let Some(preview) = event.preview {
                    payload["summary"] = json!(public_text_display(preview, redactor));
                }
                if let Some(media) = artifact_media_payload(event.blob_ref.as_ref(), &artifact_kind) {
                    payload["media"] = media;
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "engine",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "artifact.created",
                    visibility: "public",
                })
            }
            Event::ArtifactUpdated(event) => {
                if event.session_id != requested_session_id || !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                let mut payload = json!({ "artifactId": event.artifact_id });
                payload["source"] = json!(artifact_source_label(event.source));
                if let Some(title) = event.title.as_ref() {
                    payload["title"] = json!(public_text_display(title.clone(), redactor));
                }
                if let Some(kind) = event.kind.as_ref() {
                    payload["kind"] = json!(public_text_display(kind.clone(), redactor));
                    if let Some(media) = artifact_media_payload(event.blob_ref.as_ref(), kind) {
                        payload["media"] = media;
                    }
                }
                if let Some(status) = event.status {
                    payload["status"] = json!(artifact_status_label(status));
                }
                if let Some(preview) = event.preview.as_ref() {
                    payload["summary"] = json!(public_text_display(preview.clone(), redactor));
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "engine",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "artifact.updated",
                    visibility: "public",
                })
            }
            Event::AssistantReviewRequested(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                let mut payload = json!({
                    "requestId": event.request_id.to_string(),
                    "title": public_ui_safe_text_display(&event.title, redactor),
                });
                if let Some(body) = event.body.as_ref() {
                    payload["body"] = json!(public_ui_safe_text_display(body, redactor));
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "assistant",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "assistant.review.requested",
                    visibility: "public",
                })
            }
            Event::AssistantClarificationRequested(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "requestId": event.request_id.to_string(),
                        "prompt": public_ui_safe_text_display(&event.prompt, redactor),
                    }),
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "assistant",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "assistant.clarification.requested",
                    visibility: "public",
                })
            }
            Event::AssistantNotice(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "noticeId": event.notice_id.to_string(),
                        "body": public_ui_safe_text_display(&event.body, redactor),
                    }),
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "assistant",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "assistant.notice",
                    visibility: "public",
                })
            }
            Event::ToolUseRequested(event) => {
                if !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                self.tool_run_ids.insert(event.tool_use_id, event.run_id);
                let mut payload = json!({
                    "argumentsSummary": "Input withheld from conversation timeline.",
                    "toolName": public_text_display(event.tool_name.clone(), redactor),
                    "toolUseId": event.tool_use_id.to_string(),
                });
                if let Some(command) =
                    safe_tool_command_preview(&event.tool_name, &event.input, redactor)
                {
                    payload["command"] = json!(command);
                }
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload,
                    run_id: event.run_id.to_string(),
                    sequence: 0,
                    source: "tool",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "tool.requested",
                    visibility: "redacted",
                })
            }
            Event::ToolUseApproved(event) => self.tool_run_ids.get(&event.tool_use_id).map(|run_id| {
                RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({ "toolUseId": event.tool_use_id.to_string() }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "tool",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "tool.approved",
                    visibility: "public",
                }
            }),
            Event::ToolUseDenied(event) => self.tool_run_ids.get(&event.tool_use_id).map(|run_id| {
                RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({ "toolUseId": event.tool_use_id.to_string() }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "tool",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "tool.denied",
                    visibility: "public",
                }
            }),
            Event::ToolUseCompleted(event) => {
                self.tool_run_ids.get(&event.tool_use_id).map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "durationMs": event.duration_ms,
                        "outputSummary": tool_result_summary(event.result),
                        "toolUseId": event.tool_use_id.to_string(),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "tool",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "tool.completed",
                    visibility: "redacted",
                })
            }
            Event::ToolUseFailed(event) => self.tool_run_ids.get(&event.tool_use_id).map(|run_id| {
                RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "code": "tool_error",
                        "message": "Tool error withheld from conversation timeline.",
                        "toolUseId": event.tool_use_id.to_string(),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "tool",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "tool.failed",
                    visibility: "redacted",
                }
            }),
            Event::PermissionRequested(event) => {
                if event.session_id != requested_session_id || !self.is_allowed_run(&event.run_id) {
                    return None;
                }

                self.permission_run_ids.insert(event.request_id, event.run_id);
                Some(permission_requested_run_event(
                    event_id,
                    &Event::PermissionRequested(event),
                    0,
                    redactor,
                ))
            }
            Event::PermissionResolved(event) => self
                .permission_run_ids
                .get(&event.request_id)
                .map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "actionPlanHash": event.action_plan_hash.to_string(),
                        "autoResolved": event.auto_resolved,
                        "decision": permission_decision_payload(event.decision),
                        "decisionId": event.decision_id.to_string(),
                        "requestId": event.request_id.to_string(),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "policy",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "permission.resolved",
                    visibility: "public",
                }),
            Event::SubagentSpawned(event) => {
                if event.parent_session_id != requested_session_id
                    || !self.is_allowed_run(&event.parent_run_id)
                {
                    return None;
                }
                self.subagent_run_ids
                    .insert(event.subagent_id, event.parent_run_id);
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "subagentId": event.subagent_id.to_string(),
                        "role": public_text_display(event.agent_ref.name, redactor),
                        "taskSummary": "Subagent task details withheld from conversation timeline.",
                        "depth": event.depth,
                        "triggerToolUseId": event.trigger_tool_use_id.map(|id| id.to_string()),
                    }),
                    run_id: event.parent_run_id.to_string(),
                    sequence: 0,
                    source: "agent",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "subagent.spawned",
                    visibility: "public",
                })
            }
            Event::SubagentAnnounced(event) => {
                self.subagent_run_ids
                    .get(&event.subagent_id)
                    .map(|run_id| {
                        let safe_summary = public_text_display(event.summary, redactor);
                        let redacted = safe_summary.contains("[REDACTED]");
                        RunEventPayload {
                            id: event_id,
                            conversation_sequence: 0,
                            payload: json!({
                                "subagentId": event.subagent_id.to_string(),
                                "status": subagent_status_payload(&event.status),
                                "resultSummary": if redacted {
                                    "Subagent result withheld from conversation timeline.".to_owned()
                                } else {
                                    safe_summary
                                },
                                "redacted": redacted,
                            }),
                            run_id: run_id.to_string(),
                            sequence: 0,
                            source: "agent",
                            timestamp: event.at.to_rfc3339(),
                            event_type: "subagent.announced",
                            visibility: if redacted { "redacted" } else { "public" },
                        }
                    })
            }
            Event::SubagentTerminated(event) => {
                self.subagent_run_ids
                    .get(&event.subagent_id)
                    .map(|run_id| RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "subagentId": event.subagent_id.to_string(),
                            "reason": subagent_termination_reason_payload(&event.reason),
                        }),
                        run_id: run_id.to_string(),
                        sequence: 0,
                        source: "agent",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "subagent.terminated",
                        visibility: "public",
                    })
            }
            Event::SubagentStalled(event) => {
                if !self.is_allowed_run(&event.parent_run_id) {
                    return None;
                }
                self.subagent_run_ids
                    .insert(event.subagent_id, event.parent_run_id);
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({ "subagentId": event.subagent_id.to_string() }),
                    run_id: event.parent_run_id.to_string(),
                    sequence: 0,
                    source: "agent",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "subagent.stalled",
                    visibility: "public",
                })
            }
            Event::SubagentPermissionForwarded(event) => {
                self.subagent_run_ids
                    .get(&event.subagent_id)
                    .map(|run_id| RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "subagentId": event.subagent_id.to_string(),
                            "requestId": event.original_request_id.to_string(),
                            "reason": "Subagent permission forwarded to parent.",
                        }),
                        run_id: run_id.to_string(),
                        sequence: 0,
                        source: "policy",
                        timestamp: event.forwarded_at.to_rfc3339(),
                        event_type: "subagent.permission.forwarded",
                        visibility: "public",
                    })
            }
            Event::SubagentPermissionResolved(event) => {
                self.subagent_run_ids
                    .get(&event.subagent_id)
                    .map(|run_id| RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "subagentId": event.subagent_id.to_string(),
                            "requestId": event.original_request_id.to_string(),
                            "decision": permission_decision_payload(event.decision),
                        }),
                        run_id: run_id.to_string(),
                        sequence: 0,
                        source: "policy",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "subagent.permission.resolved",
                        visibility: "public",
                    })
            }
            Event::TeamCreated(event) => {
                let run_id = self.current_run_id?;
                if !self.is_allowed_run(&run_id) {
                    return None;
                }
                self.team_run_ids.insert(event.team_id, run_id);
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "teamId": event.team_id.to_string(),
                        "name": public_text_display(event.name, redactor),
                        "topologyKind": topology_kind_payload(&event.topology_kind),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "agent",
                    timestamp: event.created_at.to_rfc3339(),
                    event_type: "team.created",
                    visibility: "public",
                })
            }
            Event::TeamMemberJoined(event) => {
                self.team_run_ids.get(&event.team_id).map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "teamId": event.team_id.to_string(),
                        "agentId": event.agent_id.to_string(),
                        "role": public_text_display(event.role, redactor),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "agent",
                    timestamp: event.joined_at.to_rfc3339(),
                    event_type: "team.member.joined",
                    visibility: "public",
                })
            }
            Event::TeamMemberLeft(event) => {
                self.team_run_ids.get(&event.team_id).map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "teamId": event.team_id.to_string(),
                        "agentId": event.agent_id.to_string(),
                        "reason": member_leave_reason_payload(&event.reason),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "agent",
                    timestamp: event.left_at.to_rfc3339(),
                    event_type: "team.member.left",
                    visibility: "public",
                })
            }
            Event::TeamMemberStalled(event) => {
                self.team_run_ids.get(&event.team_id).map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "teamId": event.team_id.to_string(),
                        "agentId": event.agent_id.to_string(),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "agent",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "team.member.stalled",
                    visibility: "public",
                })
            }
            Event::AgentMessageSent(event) => {
                self.team_run_ids.get(&event.team_id).map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "teamId": event.team_id.to_string(),
                        "messageId": event.message_id.to_string(),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "agent",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "agent.message.sent",
                    visibility: "public",
                })
            }
            Event::AgentMessageRouted(event) => {
                self.team_run_ids.get(&event.team_id).map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "teamId": event.team_id.to_string(),
                        "messageId": event.message_id.to_string(),
                        "resolvedRecipients": event
                            .resolved_recipients
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>(),
                        "routingPolicy": routing_policy_payload(&event.routing_policy),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "agent",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "agent.message.routed",
                    visibility: "public",
                })
            }
            Event::TeamTurnCompleted(event) => {
                self.team_run_ids.get(&event.team_id).map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "teamId": event.team_id.to_string(),
                        "turnId": event.turn_id.to_string(),
                        "participatingAgents": event
                            .participating_agents
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>(),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "agent",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "team.turn.completed",
                    visibility: "public",
                })
            }
            Event::TeamTaskUpdated(event) => {
                self.team_run_ids.get(&event.team_id).map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "teamId": event.team_id.to_string(),
                        "taskId": public_text_display(event.task_id, redactor),
                        "title": public_text_display(event.title, redactor),
                        "status": public_text_display(event.status, redactor),
                        "assigneeProfileId": event
                            .assignee_profile_id
                            .map(|value| public_text_display(value, redactor)),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "agent",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "team.task.updated",
                    visibility: "public",
                })
            }
            Event::TeamTerminated(event) => {
                self.team_run_ids.get(&event.team_id).map(|run_id| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "teamId": event.team_id.to_string(),
                        "reason": team_termination_reason_payload(&event.reason),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "agent",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "team.terminated",
                    visibility: "public",
                })
            }
            Event::BackgroundAgentStarted(event) => {
                if event.conversation_id != requested_session_id {
                    return None;
                }
                self.background_agent_run_ids
                    .insert(event.background_agent_id, event.attempt_id);
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "backgroundAgentId": event.background_agent_id.to_string(),
                        "title": public_ui_safe_text_display(&event.title, redactor),
                    }),
                    run_id: event.attempt_id.to_string(),
                    sequence: 0,
                    source: "background",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "background.started",
                    visibility: "public",
                })
            }
            Event::BackgroundAgentStateChanged(event) => {
                let run_id = event.attempt_id.or_else(|| {
                    self.background_agent_run_ids
                        .get(&event.background_agent_id)
                        .copied()
                })?;
                self.background_agent_run_ids
                    .insert(event.background_agent_id, run_id);
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "backgroundAgentId": event.background_agent_id.to_string(),
                        "from": background_agent_state_payload(event.from),
                        "to": background_agent_state_payload(event.to),
                        "reason": event
                            .reason
                            .as_ref()
                            .map(|reason| public_ui_safe_text_display(reason, redactor)),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "background",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "background.state.changed",
                    visibility: "public",
                })
            }
            Event::BackgroundAgentInputRequested(event) => {
                self.background_agent_run_ids
                    .get(&event.background_agent_id)
                    .map(|run_id| RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "backgroundAgentId": event.background_agent_id.to_string(),
                            "requestId": event.request_id.to_string(),
                            "prompt": public_ui_safe_text_display(&event.prompt, redactor),
                        }),
                        run_id: run_id.to_string(),
                        sequence: 0,
                        source: "background",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "background.input.requested",
                        visibility: "public",
                    })
            }
            Event::BackgroundAgentInputSubmitted(event) => {
                self.background_agent_run_ids
                    .get(&event.background_agent_id)
                    .map(|run_id| RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "backgroundAgentId": event.background_agent_id.to_string(),
                            "requestId": event.request_id.to_string(),
                        }),
                        run_id: run_id.to_string(),
                        sequence: 0,
                        source: "background",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "background.input.submitted",
                        visibility: "public",
                    })
            }
            Event::BackgroundAgentPermissionRequested(event) => {
                let run_id = event.attempt_id.or_else(|| {
                    self.background_agent_run_ids
                        .get(&event.background_agent_id)
                        .copied()
                })?;
                if event.conversation_id != requested_session_id && !self.is_allowed_run(&run_id) {
                    return None;
                }
                self.background_agent_run_ids
                    .insert(event.background_agent_id, run_id);
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "backgroundAgentId": event.background_agent_id.to_string(),
                        "requestId": event.request_id.to_string(),
                        "reason": public_ui_safe_text_display(&event.reason, redactor),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "policy",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "background.permission.requested",
                    visibility: "public",
                })
            }
            Event::BackgroundAgentPermissionResolved(event) => {
                let run_id = event.attempt_id.or_else(|| {
                    self.background_agent_run_ids
                        .get(&event.background_agent_id)
                        .copied()
                })?;
                if event.conversation_id != requested_session_id && !self.is_allowed_run(&run_id) {
                    return None;
                }
                Some(RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({
                        "backgroundAgentId": event.background_agent_id.to_string(),
                        "requestId": event.request_id.to_string(),
                        "decision": permission_decision_payload(event.decision),
                    }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "policy",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "background.permission.resolved",
                    visibility: "public",
                })
            }
            Event::BackgroundAgentCancelled(event) => {
                self.background_agent_run_ids
                    .get(&event.background_agent_id)
                    .map(|run_id| RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "backgroundAgentId": event.background_agent_id.to_string(),
                            "reason": event
                                .reason
                                .as_ref()
                                .map(|reason| public_ui_safe_text_display(reason, redactor)),
                        }),
                        run_id: run_id.to_string(),
                        sequence: 0,
                        source: "background",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "background.cancelled",
                        visibility: "public",
                    })
            }
            Event::BackgroundAgentCompleted(event) => {
                self.background_agent_run_ids
                    .get(&event.background_agent_id)
                    .map(|run_id| RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "backgroundAgentId": event.background_agent_id.to_string(),
                            "summary": event
                                .summary
                                .as_ref()
                                .map(|summary| public_ui_safe_text_display(summary, redactor)),
                        }),
                        run_id: run_id.to_string(),
                        sequence: 0,
                        source: "background",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "background.completed",
                        visibility: "public",
                    })
            }
            Event::BackgroundAgentFailed(event) => {
                self.background_agent_run_ids
                    .get(&event.background_agent_id)
                    .map(|run_id| RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "backgroundAgentId": event.background_agent_id.to_string(),
                            "error": public_ui_safe_text_display(&event.error, redactor),
                        }),
                        run_id: run_id.to_string(),
                        sequence: 0,
                        source: "background",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "background.failed",
                        visibility: "public",
                    })
            }
            Event::BackgroundAgentInterrupted(event) => {
                self.background_agent_run_ids
                    .get(&event.background_agent_id)
                    .map(|run_id| RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "backgroundAgentId": event.background_agent_id.to_string(),
                            "reason": public_ui_safe_text_display(&event.reason, redactor),
                        }),
                        run_id: run_id.to_string(),
                        sequence: 0,
                        source: "background",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "background.interrupted",
                        visibility: "public",
                    })
            }
            Event::BackgroundAgentArchived(event) => {
                self.background_agent_run_ids
                    .get(&event.background_agent_id)
                    .map(|run_id| RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "backgroundAgentId": event.background_agent_id.to_string(),
                        }),
                        run_id: run_id.to_string(),
                        sequence: 0,
                        source: "background",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "background.archived",
                        visibility: "public",
                    })
            }
            Event::BackgroundAgentDeleted(event) => {
                self.background_agent_run_ids
                    .get(&event.background_agent_id)
                    .map(|run_id| RunEventPayload {
                        id: event_id,
                        conversation_sequence: 0,
                        payload: json!({
                            "backgroundAgentId": event.background_agent_id.to_string(),
                        }),
                        run_id: run_id.to_string(),
                        sequence: 0,
                        source: "background",
                        timestamp: event.at.to_rfc3339(),
                        event_type: "background.deleted",
                        visibility: "public",
                    })
            }
            Event::PluginLoaded(event) => Some(RunEventPayload {
                id: event_id,
                conversation_sequence: 0,
                payload: json!({
                    "capabilityCount": plugin_capability_count(&event.capabilities),
                    "pluginId": public_text_display(event.plugin_id.0, redactor),
                    "pluginName": public_text_display(event.plugin_name, redactor),
                    "trustLevel": plugin_trust_level_payload(event.trust_level),
                }),
                run_id: PLUGIN_RUNTIME_RUN_ID.to_owned(),
                sequence: 0,
                source: "plugin",
                timestamp: event.at.to_rfc3339(),
                event_type: "plugin.loaded",
                visibility: "redacted",
            }),
            Event::PluginRejected(event) => Some(RunEventPayload {
                id: event_id,
                conversation_sequence: 0,
                payload: json!({
                    "pluginId": public_text_display(event.plugin_id.0, redactor),
                    "pluginName": public_text_display(event.plugin_name, redactor),
                    "reason": plugin_rejection_reason_payload(&event.reason),
                    "trustLevel": plugin_trust_level_payload(event.trust_level),
                }),
                run_id: PLUGIN_RUNTIME_RUN_ID.to_owned(),
                sequence: 0,
                source: "plugin",
                timestamp: event.at.to_rfc3339(),
                event_type: "plugin.rejected",
                visibility: "redacted",
            }),
            Event::PluginFailed(event) => Some(RunEventPayload {
                id: event_id,
                conversation_sequence: 0,
                payload: json!({
                    "message": PLUGIN_FAILURE_WITHHELD_MESSAGE,
                    "pluginId": public_text_display(event.plugin_id.0, redactor),
                    "pluginName": public_text_display(event.plugin_name, redactor),
                    "trustLevel": plugin_trust_level_payload(event.trust_level),
                }),
                run_id: PLUGIN_RUNTIME_RUN_ID.to_owned(),
                sequence: 0,
                source: "plugin",
                timestamp: event.at.to_rfc3339(),
                event_type: "plugin.failed",
                visibility: "redacted",
            }),
            Event::EngineFailed(event) => event.run_id.and_then(|run_id| {
                self.is_allowed_run(&run_id).then(|| RunEventPayload {
                    id: event_id,
                    conversation_sequence: 0,
                    payload: json!({ "message": "Engine error withheld from conversation timeline." }),
                    run_id: run_id.to_string(),
                    sequence: 0,
                    source: "engine",
                    timestamp: event.at.to_rfc3339(),
                    event_type: "engine.failed",
                    visibility: "redacted",
                })
            }),
            _ => None,
        }
    }
}

fn run_model_payload(model: &RunModelSnapshot, redactor: &dyn Redactor) -> Value {
    json!({
        "modelConfigId": model
            .model_config_id
            .as_ref()
            .map(|value| public_text_display(value.clone(), redactor)),
        "providerId": public_text_display(model.provider_id.clone(), redactor),
        "modelId": public_text_display(model.model_id.clone(), redactor),
        "displayName": public_text_display(model.display_name.clone(), redactor),
        "protocol": model.protocol,
    })
}

pub(crate) fn tool_result_summary(_result: impl Serialize) -> String {
    "Output withheld from conversation timeline.".to_owned()
}

pub(crate) fn subagent_status_payload(status: &SubagentStatus) -> &'static str {
    match status {
        SubagentStatus::Completed => "completed",
        SubagentStatus::Cancelled => "cancelled",
        SubagentStatus::Failed => "failed",
        SubagentStatus::Stalled => "stalled",
        SubagentStatus::MaxIterationsReached => "max_iterations_reached",
        SubagentStatus::MaxBudget(_) => "max_budget",
        _ => "failed",
    }
}

pub(crate) fn subagent_termination_reason_payload(
    reason: &SubagentTerminationReason,
) -> &'static str {
    match reason {
        SubagentTerminationReason::NaturalCompletion => "natural_completion",
        SubagentTerminationReason::ParentCancelled => "parent_cancelled",
        SubagentTerminationReason::AdminInterrupted { .. } => "admin_interrupted",
        SubagentTerminationReason::Stalled { .. } => "stalled",
        SubagentTerminationReason::BridgeBroken => "bridge_broken",
        SubagentTerminationReason::Failed { .. } => "failed",
        _ => "failed",
    }
}

pub(crate) fn topology_kind_payload(topology: &TopologyKind) -> String {
    match topology {
        TopologyKind::CoordinatorWorker => "coordinator_worker".to_owned(),
        TopologyKind::PeerToPeer => "peer_to_peer".to_owned(),
        TopologyKind::RoleRouted => "role_routed".to_owned(),
        TopologyKind::Custom(_) => "custom".to_owned(),
        _ => "custom".to_owned(),
    }
}

pub(crate) fn member_leave_reason_payload(reason: &MemberLeaveReason) -> &'static str {
    match reason {
        MemberLeaveReason::GoalAchieved => "goal_achieved",
        MemberLeaveReason::QuotaExceeded => "quota_exceeded",
        MemberLeaveReason::Interrupted => "interrupted",
        MemberLeaveReason::Error(_) => "error",
        MemberLeaveReason::Removed => "removed",
        MemberLeaveReason::StalledRemoved => "stalled_removed",
        _ => "error",
    }
}

pub(crate) fn team_termination_reason_payload(reason: &TeamTerminationReason) -> &'static str {
    match reason {
        TeamTerminationReason::Completed => "completed",
        TeamTerminationReason::Cancelled => "cancelled",
        TeamTerminationReason::Error(_) => "error",
        TeamTerminationReason::MemberFailed => "member_failed",
        TeamTerminationReason::IdleTimeout => "idle_timeout",
        TeamTerminationReason::Timeout => "timeout",
        _ => "error",
    }
}

pub(crate) fn routing_policy_payload(policy: &RoutingPolicyKind) -> &'static str {
    match policy {
        RoutingPolicyKind::Direct => "direct",
        RoutingPolicyKind::Role => "role",
        RoutingPolicyKind::Broadcast => "broadcast",
        RoutingPolicyKind::Coordinator => "coordinator",
        RoutingPolicyKind::Custom(_) => "custom",
        _ => "custom",
    }
}

pub(crate) fn background_agent_state_payload(state: BackgroundAgentState) -> &'static str {
    match state {
        BackgroundAgentState::Queued => "queued",
        BackgroundAgentState::Running => "running",
        BackgroundAgentState::WaitingForPermission => "waiting_for_permission",
        BackgroundAgentState::WaitingForInput => "waiting_for_input",
        BackgroundAgentState::Paused => "paused",
        BackgroundAgentState::Cancelling => "cancelling",
        BackgroundAgentState::Cancelled => "cancelled",
        BackgroundAgentState::Succeeded => "succeeded",
        BackgroundAgentState::Failed => "failed",
        BackgroundAgentState::Interrupted => "interrupted",
        BackgroundAgentState::Recoverable => "recoverable",
        BackgroundAgentState::Archived => "archived",
    }
}

pub(crate) fn plugin_capability_count(capabilities: &PluginCapabilitiesSummary) -> u64 {
    u64::from(capabilities.tools)
        + u64::from(capabilities.hooks)
        + u64::from(capabilities.mcp_servers)
        + u64::from(capabilities.skills)
        + if capabilities.steering { 1 } else { 0 }
        + if capabilities.memory_provider { 1 } else { 0 }
        + if capabilities.coordinator { 1 } else { 0 }
}

pub(crate) fn plugin_trust_level_payload(trust_level: TrustLevel) -> &'static str {
    match trust_level {
        TrustLevel::AdminTrusted => "admin_trusted",
        TrustLevel::UserControlled => "user_controlled",
        _ => "user_controlled",
    }
}

pub(crate) fn plugin_rejection_reason_payload(reason: &RejectionReason) -> &'static str {
    match reason {
        RejectionReason::SignatureInvalid { .. } => "SignatureInvalid",
        RejectionReason::UnknownSigner { .. } => "UnknownSigner",
        RejectionReason::SignerRevoked { .. } => "SignerRevoked",
        RejectionReason::TrustMismatch { .. } => "TrustMismatch",
        RejectionReason::NamespaceConflict { .. } => "NamespaceConflict",
        RejectionReason::DependencyUnsatisfied { .. } => "DependencyUnsatisfied",
        RejectionReason::DependencyCycle { .. } => "DependencyCycle",
        RejectionReason::HarnessVersionMismatch { .. } => "HarnessVersionMismatch",
        RejectionReason::SlotOccupied { .. } => "SlotOccupied",
        RejectionReason::AdmissionDenied { .. } => "AdmissionDenied",
        _ => "AdmissionDenied",
    }
}

pub(crate) fn safe_tool_command_preview(
    tool_name: &str,
    input: &Value,
    redactor: &dyn Redactor,
) -> Option<String> {
    if !is_command_tool_name(tool_name) {
        return None;
    }
    let command = input.get("command").and_then(Value::as_str)?.trim();
    if command.is_empty() || contains_obvious_secret(command) {
        return None;
    }
    Some(truncate_utf8(
        redact_private_absolute_paths(redacted_display(command.to_owned(), redactor)),
        1_200,
    ))
}

pub(crate) fn is_command_tool_name(tool_name: &str) -> bool {
    let normalized = tool_name.to_ascii_lowercase();
    normalized == "bash" || normalized.contains("shell")
}

pub(crate) fn contains_obvious_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("authorization:")
        || lower.contains("bearer ")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.contains("sk-")
        || lower.contains("ghp_")
        || lower.contains("xoxb-")
}

pub(crate) fn permission_decision_payload(decision: Decision) -> &'static str {
    match decision {
        Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent => "approve",
        Decision::DenyOnce | Decision::DenyPermanent | Decision::Escalate => "deny",
        _ => "deny",
    }
}

pub(crate) fn run_end_reason_display(reason: &EndReason, redactor: &dyn Redactor) -> String {
    if matches!(reason, EndReason::Error(_)) {
        return "Run error withheld from conversation timeline.".to_owned();
    }

    let value = match reason {
        EndReason::Completed => "completed".to_owned(),
        EndReason::MaxIterationsReached => "max iterations reached".to_owned(),
        EndReason::TokenBudgetExhausted => "token budget exhausted".to_owned(),
        EndReason::BudgetExhausted(_) => "budget exhausted".to_owned(),
        EndReason::Interrupted => "interrupted".to_owned(),
        EndReason::Cancelled { .. } => "cancelled".to_owned(),
        EndReason::Error(_) => unreachable!("error reasons return before redaction"),
        EndReason::Compacted => "compacted".to_owned(),
        _ => "ended".to_owned(),
    };

    let value = redacted_display(value, redactor);
    if value.trim().is_empty() {
        "error".to_owned()
    } else {
        value
    }
}

pub(crate) fn parse_run_id(value: &str) -> Result<RunId, CommandErrorPayload> {
    ensure_non_empty("runId", value)?;
    let run_id = RunId::parse(value)
        .map_err(|_| invalid_payload("runId must be a valid run id".to_owned()))?;

    if run_id.to_string() != value {
        return Err(invalid_payload(
            "runId must be a canonical run id".to_owned(),
        ));
    }

    Ok(run_id)
}

pub(crate) fn severity_display(severity: Severity) -> &'static str {
    match severity {
        Severity::Info | Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
        _ => "medium",
    }
}

pub(crate) fn redacted_display(value: String, redactor: &dyn Redactor) -> String {
    redactor.redact(
        &value,
        &RedactRules {
            scope: RedactScope::EventBody,
            replacement: "[REDACTED]".to_owned(),
            pattern_set: RedactPatternSet::Default,
        },
    )
}

// ── Evidence fetch handlers ──
//
// These commands validate conversation ownership and delegate typed evidence
// reads to the SDK facade.

const DEFAULT_EVIDENCE_READ_MAX_BYTES: usize = 64 * 1024;
const MAX_EVIDENCE_READ_BYTES: usize = 64 * 1024;

fn evidence_read_window(
    cursor: Option<String>,
    max_bytes: Option<u64>,
) -> Result<(Option<String>, usize), CommandErrorPayload> {
    let requested = max_bytes
        .unwrap_or(DEFAULT_EVIDENCE_READ_MAX_BYTES as u64)
        .clamp(1, MAX_EVIDENCE_READ_BYTES as u64);
    Ok((
        cursor,
        usize::try_from(requested)
            .map_err(|_| invalid_payload("maxBytes is too large".to_owned()))?,
    ))
}

pub async fn get_conversation_command_output_with_runtime_state(
    request: GetConversationCommandOutputRequest,
    state: &DesktopRuntimeState,
) -> Result<GetConversationCommandOutputResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("fullOutputRef", &request.full_output_ref)?;
    ensure_conversation_exists(&request.conversation_id, state).await?;

    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable("harness not available"));
    };

    let ref_id = harness_contracts::EvidenceRefId::new(&request.full_output_ref);
    let (cursor, max_bytes) = evidence_read_window(request.cursor, request.max_bytes)?;
    let result = harness
        .read_command_output_evidence_window(
            TenantId::SINGLE,
            &request.conversation_id,
            &ref_id,
            cursor,
            max_bytes,
        )
        .await
        .map_err(|e| runtime_unavailable(&format!("Evidence read failed: {e}")))?;

    Ok(GetConversationCommandOutputResponse {
        ref_id: ref_id.to_string(),
        kind: "command-output".to_owned(),
        output: String::from_utf8_lossy(&result.bytes).into_owned(),
        content_type: result.content_type,
        byte_length: result.content_bytes,
        content_bytes: result.content_bytes,
        offset_bytes: result.offset_bytes,
        limit_bytes: result.limit_bytes,
        total_bytes: result.total_bytes,
        returned_bytes: result.returned_bytes,
        max_bytes: result.max_bytes,
        truncated: result.truncated,
        has_more: result.has_more,
        next_cursor: result.next_cursor,
        content_hash: result.content_hash,
        hash_algorithm: result.hash_algorithm,
        redaction_state: format!("{:?}", result.redaction_state).to_lowercase(),
    })
}

pub async fn get_conversation_diff_patch_with_runtime_state(
    request: GetConversationDiffPatchRequest,
    state: &DesktopRuntimeState,
) -> Result<GetConversationDiffPatchResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("fullPatchRef", &request.full_patch_ref)?;
    ensure_conversation_exists(&request.conversation_id, state).await?;

    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable("harness not available"));
    };

    let ref_id = harness_contracts::EvidenceRefId::new(&request.full_patch_ref);
    let (cursor, max_bytes) = evidence_read_window(request.cursor, request.max_bytes)?;
    let result = harness
        .read_diff_patch_evidence_window(
            TenantId::SINGLE,
            &request.conversation_id,
            &ref_id,
            cursor,
            max_bytes,
        )
        .await
        .map_err(|e| runtime_unavailable(&format!("Evidence read failed: {e}")))?;

    Ok(GetConversationDiffPatchResponse {
        ref_id: ref_id.to_string(),
        kind: "diff-patch".to_owned(),
        patch: String::from_utf8_lossy(&result.bytes).into_owned(),
        content_type: result.content_type,
        byte_length: result.content_bytes,
        content_bytes: result.content_bytes,
        offset_bytes: result.offset_bytes,
        limit_bytes: result.limit_bytes,
        total_bytes: result.total_bytes,
        returned_bytes: result.returned_bytes,
        max_bytes: result.max_bytes,
        truncated: result.truncated,
        has_more: result.has_more,
        next_cursor: result.next_cursor,
        content_hash: result.content_hash,
        hash_algorithm: result.hash_algorithm,
        redaction_state: format!("{:?}", result.redaction_state).to_lowercase(),
    })
}

pub async fn get_artifact_revision_content_with_runtime_state(
    request: GetArtifactRevisionContentRequest,
    state: &DesktopRuntimeState,
) -> Result<GetArtifactRevisionContentResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("contentRef", &request.content_ref)?;
    ensure_conversation_exists(&request.conversation_id, state).await?;

    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable("harness not available"));
    };

    let ref_id = harness_contracts::EvidenceRefId::new(&request.content_ref);
    let (cursor, max_bytes) = evidence_read_window(request.cursor, request.max_bytes)?;
    let result = harness
        .read_artifact_revision_content_window(
            TenantId::SINGLE,
            &request.conversation_id,
            &ref_id,
            cursor,
            max_bytes,
        )
        .await
        .map_err(|_| runtime_unavailable("artifact content unavailable"))?;

    Ok(GetArtifactRevisionContentResponse {
        ref_id: ref_id.to_string(),
        kind: "artifact-content".to_owned(),
        content: String::from_utf8_lossy(&result.bytes).into_owned(),
        content_type: result.content_type,
        byte_length: result.content_bytes,
        content_bytes: result.content_bytes,
        offset_bytes: result.offset_bytes,
        limit_bytes: result.limit_bytes,
        total_bytes: result.total_bytes,
        returned_bytes: result.returned_bytes,
        max_bytes: result.max_bytes,
        truncated: result.truncated,
        has_more: result.has_more,
        next_cursor: result.next_cursor,
        content_hash: result.content_hash,
        hash_algorithm: result.hash_algorithm,
        redaction_state: format!("{:?}", result.redaction_state).to_lowercase(),
        artifact_id: None,
        revision_id: None,
    })
}

pub async fn export_conversation_evidence_with_runtime_state(
    request: ExportConversationEvidenceRequest,
    state: &DesktopRuntimeState,
) -> Result<ExportConversationEvidenceResponse, CommandErrorPayload> {
    ensure_non_empty("conversationId", &request.conversation_id)?;
    ensure_non_empty("refId", &request.ref_id)?;
    ensure_conversation_exists(&request.conversation_id, state).await?;

    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable("harness not available"));
    };

    let kind = validated_evidence_export_kind(&request.kind)?;
    let ref_id = harness_contracts::EvidenceRefId::new(&request.ref_id);
    let exported_at = chrono::Utc::now().to_rfc3339();
    let relative_path = evidence_export_response_path(state, &request.conversation_id, kind);
    let export_result = write_evidence_export_windows(
        &export_absolute_path(state, Path::new(&relative_path)),
        &harness,
        kind,
        &request.conversation_id,
        &ref_id,
    )
    .await?;

    Ok(ExportConversationEvidenceResponse {
        ref_id: ref_id.to_string(),
        kind: kind.to_owned(),
        path: relative_path,
        content_type: export_result.content_type,
        byte_length: export_result.byte_length,
        exported_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_contracts::ProviderSelectionRecord;

    #[test]
    fn resolve_effective_model_config_id_uses_global_selection_with_project_config_present() {
        let temp = tempfile::tempdir().expect("workspace tempdir");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let state = DesktopRuntimeState::with_workspace_for_test(workspace)
            .expect("runtime state should initialize");
        state
            .global_config_store
            .as_ref()
            .expect("global config store")
            .save_global_provider_selection(&ProviderSelectionRecord {
                default_config_id: Some("global-config".to_owned()),
            })
            .expect("save global selection");
        state
            .project_config_store
            .as_ref()
            .expect("project config store")
            .save_project_provider_selection(&ProviderSelectionRecord {
                default_config_id: Some("project-config".to_owned()),
            })
            .expect("save stale project selection");

        let resolved =
            resolve_effective_model_config_id(None, &state).expect("resolve default model config");

        assert_eq!(resolved, "global-config");
    }
}

struct EvidenceExportWriteResult {
    content_type: String,
    byte_length: u64,
}

async fn write_evidence_export_windows(
    path: &std::path::Path,
    harness: &jyowo_harness_sdk::Harness,
    kind: &str,
    conversation_id: &str,
    ref_id: &harness_contracts::EvidenceRefId,
) -> Result<EvidenceExportWriteResult, CommandErrorPayload> {
    let Some(parent) = path.parent() else {
        return Err(support_bundle_operation_failed());
    };
    ensure_no_symlink_components(parent, "evidence export directory")
        .map_err(|_| support_bundle_operation_failed())?;
    std::fs::create_dir_all(parent).map_err(|_| support_bundle_operation_failed())?;
    ensure_no_symlink_components(parent, "evidence export directory")
        .map_err(|_| support_bundle_operation_failed())?;
    ensure_no_symlink_components(path, "evidence export file")
        .map_err(|_| support_bundle_operation_failed())?;

    let temp_path = path.with_file_name(format!(
        "{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("evidence-export"),
        RunId::new()
    ));
    ensure_no_symlink_components(&temp_path, "evidence export temp file")
        .map_err(|_| support_bundle_operation_failed())?;

    let mut temp_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|_| support_bundle_operation_failed())?;

    let mut cursor = None;
    let mut content_type = None;
    let mut byte_length = 0_u64;
    loop {
        let page = match kind {
            "command-output" => {
                harness
                    .read_command_output_evidence_window(
                        TenantId::SINGLE,
                        conversation_id,
                        ref_id,
                        cursor,
                        MAX_EVIDENCE_READ_BYTES,
                    )
                    .await
            }
            "diff-patch" => {
                harness
                    .read_diff_patch_evidence_window(
                        TenantId::SINGLE,
                        conversation_id,
                        ref_id,
                        cursor,
                        MAX_EVIDENCE_READ_BYTES,
                    )
                    .await
            }
            "artifact-content" => {
                harness
                    .read_artifact_revision_content_window(
                        TenantId::SINGLE,
                        conversation_id,
                        ref_id,
                        cursor,
                        MAX_EVIDENCE_READ_BYTES,
                    )
                    .await
            }
            _ => unreachable!("validated_evidence_export_kind must reject unknown kinds"),
        }
        .map_err(|e| runtime_unavailable(&format!("Evidence export failed: {e}")))?;

        if content_type.is_none() {
            content_type = Some(page.content_type.clone());
            byte_length = page.content_bytes;
        }
        if temp_file.write_all(&page.bytes).is_err() {
            let _ = std::fs::remove_file(&temp_path);
            return Err(support_bundle_operation_failed());
        }
        if !page.has_more {
            break;
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            let _ = std::fs::remove_file(&temp_path);
            return Err(support_bundle_operation_failed());
        }
    }

    if temp_file.sync_all().is_err() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(support_bundle_operation_failed());
    }
    drop(temp_file);
    ensure_no_symlink_components(path, "evidence export file")
        .map_err(|_| support_bundle_operation_failed())?;
    std::fs::rename(&temp_path, path).map_err(|_| {
        let _ = std::fs::remove_file(&temp_path);
        support_bundle_operation_failed()
    })?;

    Ok(EvidenceExportWriteResult {
        content_type: content_type.unwrap_or_else(|| "application/octet-stream".to_owned()),
        byte_length,
    })
}

fn validated_evidence_export_kind(kind: &str) -> Result<&'static str, CommandErrorPayload> {
    match kind {
        "command-output" => Ok("command-output"),
        "diff-patch" => Ok("diff-patch"),
        "artifact-content" => Ok("artifact-content"),
        _ => Err(invalid_payload(
            "evidence export kind is invalid".to_owned(),
        )),
    }
}
