#[allow(unused_imports)]
use super::agents::*;
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
use super::skills::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;

pub async fn list_background_agents_with_runtime_state(
    request: ListBackgroundAgentsRequest,
    state: &DesktopRuntimeState,
) -> Result<ListBackgroundAgentsResponse, CommandErrorPayload> {
    ensure_background_agents_enabled(state)?;
    let conversation_id = parse_optional_conversation_id(request.conversation_id.as_deref())?;
    let manager = background_manager_for_session(
        state,
        conversation_id.unwrap_or_else(|| state.default_conversation_id()),
    )?;
    let mut agents = manager
        .list(request.include_archived)
        .map_err(map_background_agent_error)?;
    if let Some(conversation_id) = conversation_id {
        let conversation_id = conversation_id.to_string();
        agents.retain(|agent| agent.conversation_id == conversation_id);
    }

    Ok(ListBackgroundAgentsResponse {
        agents: agents.into_iter().map(background_agent_payload).collect(),
    })
}

pub async fn get_background_agent_with_runtime_state(
    request: GetBackgroundAgentRequest,
    state: &DesktopRuntimeState,
) -> Result<GetBackgroundAgentResponse, CommandErrorPayload> {
    ensure_background_agents_enabled(state)?;
    ensure_background_agent_id(&request.background_agent_id)?;
    let expected_conversation_id =
        parse_optional_conversation_id(request.conversation_id.as_deref())?;
    let manager = background_manager_for_session(state, state.default_conversation_id())?;
    let agent = manager
        .get(&request.background_agent_id)
        .map_err(map_background_agent_error)?;
    ensure_background_agent_conversation(&agent, expected_conversation_id)?;

    Ok(GetBackgroundAgentResponse {
        agent: background_agent_payload(agent),
    })
}

pub async fn pause_background_agent_with_runtime_state(
    request: BackgroundAgentIdRequest,
    state: &DesktopRuntimeState,
) -> Result<BackgroundAgentActionResponse, CommandErrorPayload> {
    let background_agent_id = request.background_agent_id.clone();
    let response = with_background_agent_manager(request, state, |manager, background_agent_id| {
        Box::pin(async move { manager.pause(&background_agent_id, "paused by user").await })
    })
    .await?;
    let supervisor_scope = agent_supervisor_scope_for_state(state);
    let _ = crate::agent_supervisor::pause_background_agent_run_scope(
        &supervisor_scope,
        &background_agent_id,
    )
    .await;
    Ok(response)
}

pub async fn resume_background_agent_with_runtime_state(
    request: BackgroundAgentIdRequest,
    state: &DesktopRuntimeState,
) -> Result<BackgroundAgentActionResponse, CommandErrorPayload> {
    let background_agent_id = request.background_agent_id.clone();
    let response = with_background_agent_manager(request, state, |manager, background_agent_id| {
        Box::pin(async move {
            manager
                .resume(&background_agent_id, "resumed by user")
                .await
        })
    })
    .await?;
    let supervisor_scope = agent_supervisor_scope_for_state(state);
    let _ = crate::agent_supervisor::requeue_background_agent_supervisor_payload_scope(
        &supervisor_scope,
        &background_agent_id,
    );
    let _ = crate::agent_supervisor::wake_agent_supervisor_scope(&supervisor_scope).await;
    Ok(response)
}

pub async fn cancel_background_agent_with_runtime_state(
    request: BackgroundAgentIdRequest,
    state: &DesktopRuntimeState,
) -> Result<BackgroundAgentActionResponse, CommandErrorPayload> {
    let background_agent_id = request.background_agent_id.clone();
    let response = with_background_agent_manager(request, state, |manager, background_agent_id| {
        Box::pin(async move {
            manager
                .cancel(&background_agent_id, "cancelled by user")
                .await
        })
    })
    .await?;
    let supervisor_scope = agent_supervisor_scope_for_state(state);
    let _ = crate::agent_supervisor::cancel_background_agent_run_scope(
        &supervisor_scope,
        &background_agent_id,
    )
    .await;
    Ok(response)
}

pub async fn send_background_agent_input_with_runtime_state(
    request: SendBackgroundAgentInputRequest,
    state: &DesktopRuntimeState,
) -> Result<BackgroundAgentActionResponse, CommandErrorPayload> {
    ensure_background_agents_enabled(state)?;
    ensure_background_agent_id(&request.background_agent_id)?;
    ensure_non_empty("requestId", &request.request_id)?;
    ensure_non_empty("input", &request.input)?;
    let request_id = parse_request_id(&request.request_id)?;
    let expected_conversation_id =
        parse_optional_conversation_id(request.conversation_id.as_deref())?;
    let existing = get_background_agent_record(state, &request.background_agent_id)?;
    ensure_background_agent_conversation(&existing, expected_conversation_id)?;
    let session_id = parse_session_id(&existing.conversation_id)?;
    let manager = background_manager_for_session(state, session_id)?;
    let agent = manager
        .send_input(&request.background_agent_id, request_id, &request.input)
        .await
        .map_err(map_background_agent_error)?;
    let supervisor_scope = agent_supervisor_scope_for_state(state);
    let _ = crate::agent_supervisor::requeue_background_agent_supervisor_payload_scope(
        &supervisor_scope,
        &request.background_agent_id,
    );
    let _ = crate::agent_supervisor::wake_agent_supervisor_scope(&supervisor_scope).await;

    Ok(BackgroundAgentActionResponse {
        agent: background_agent_payload(agent),
    })
}

pub async fn archive_background_agent_with_runtime_state(
    request: BackgroundAgentIdRequest,
    state: &DesktopRuntimeState,
) -> Result<BackgroundAgentActionResponse, CommandErrorPayload> {
    with_background_agent_manager(request, state, |manager, background_agent_id| {
        Box::pin(async move { manager.archive(&background_agent_id).await })
    })
    .await
}

pub async fn delete_background_agent_with_runtime_state(
    request: BackgroundAgentIdRequest,
    state: &DesktopRuntimeState,
) -> Result<BackgroundAgentDeleteResponse, CommandErrorPayload> {
    ensure_background_agents_enabled(state)?;
    ensure_background_agent_id(&request.background_agent_id)?;
    let expected_conversation_id =
        parse_optional_conversation_id(request.conversation_id.as_deref())?;
    let existing = get_background_agent_record(state, &request.background_agent_id)?;
    ensure_background_agent_conversation(&existing, expected_conversation_id)?;
    let session_id = parse_session_id(&existing.conversation_id)?;
    let manager = background_manager_for_session(state, session_id)?;
    manager
        .delete_archived(&request.background_agent_id)
        .await
        .map_err(map_background_agent_error)?;

    Ok(BackgroundAgentDeleteResponse {
        background_agent_id: request.background_agent_id,
        status: "deleted",
    })
}

fn with_background_agent_manager<'a, F>(
    request: BackgroundAgentIdRequest,
    state: &'a DesktopRuntimeState,
    operation: F,
) -> BoxFuture<'a, Result<BackgroundAgentActionResponse, CommandErrorPayload>>
where
    F: FnOnce(
            BackgroundAgentManager,
            String,
        ) -> BoxFuture<
            'static,
            Result<
                jyowo_harness_sdk::BackgroundAgentRecord,
                jyowo_harness_sdk::BackgroundAgentTransitionError,
            >,
        > + Send
        + 'a,
{
    Box::pin(async move {
        ensure_background_agents_enabled(state)?;
        ensure_background_agent_id(&request.background_agent_id)?;
        let expected_conversation_id =
            parse_optional_conversation_id(request.conversation_id.as_deref())?;
        let existing = get_background_agent_record(state, &request.background_agent_id)?;
        ensure_background_agent_conversation(&existing, expected_conversation_id)?;
        let session_id = parse_session_id(&existing.conversation_id)?;
        let manager = background_manager_for_session(state, session_id)?;
        let agent = operation(manager, request.background_agent_id)
            .await
            .map_err(map_background_agent_error)?;
        Ok(BackgroundAgentActionResponse {
            agent: background_agent_payload(agent),
        })
    })
}

fn background_manager_for_session(
    state: &DesktopRuntimeState,
    session_id: SessionId,
) -> Result<BackgroundAgentManager, CommandErrorPayload> {
    let harness = state.harness().ok_or_else(|| {
        runtime_unavailable("Background agent commands require the runtime conversation facade.")
    })?;
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(state.runtime_root()).map_err(|error| {
            runtime_operation_failed(format!("background store open failed: {error}"))
        })?,
    );

    Ok(BackgroundAgentManager::new(
        store,
        harness.event_store(),
        TenantId::SINGLE,
        session_id,
        Arc::new(DefaultRedactor::default()),
    ))
}

fn get_background_agent_record(
    state: &DesktopRuntimeState,
    background_agent_id: &str,
) -> Result<jyowo_harness_sdk::BackgroundAgentRecord, CommandErrorPayload> {
    let store = AgentRuntimeStore::open_runtime_dir(state.runtime_root()).map_err(|error| {
        runtime_operation_failed(format!("background store open failed: {error}"))
    })?;
    store
        .get_background_agent(background_agent_id)
        .map_err(|error| {
            runtime_operation_failed(format!("background agent lookup failed: {error}"))
        })?
        .map(Into::into)
        .ok_or_else(|| not_found(format!("background agent not found: {background_agent_id}")))
}

fn ensure_background_agents_enabled(
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let settings = state.effective_execution_settings(None)?;
    let capabilities = if let Some(workspace_root) = state.project_workspace_root() {
        let context = state.agent_capability_resolution_context();
        agent_capabilities_payload(&settings, workspace_root, Some(&context))
    } else {
        let context = state.agent_capability_resolution_context();
        no_workspace_agent_capabilities_payload(&settings, state.runtime_root(), Some(&context))
    };
    if !capabilities.background_agents_available {
        return Err(invalid_payload(
            "background agents are unavailable in this desktop runtime".to_owned(),
        ));
    }
    if !capabilities.background_agents_enabled {
        return Err(invalid_payload(
            "background agents are disabled in execution settings".to_owned(),
        ));
    }
    Ok(())
}

fn parse_optional_conversation_id(
    value: Option<&str>,
) -> Result<Option<SessionId>, CommandErrorPayload> {
    match value {
        Some(value) => {
            ensure_non_empty("conversationId", value)?;
            Ok(Some(parse_session_id(value)?))
        }
        None => Ok(None),
    }
}

fn ensure_background_agent_id(value: &str) -> Result<(), CommandErrorPayload> {
    ensure_non_empty("backgroundAgentId", value)
}

fn ensure_background_agent_conversation(
    agent: &jyowo_harness_sdk::BackgroundAgentRecord,
    expected: Option<SessionId>,
) -> Result<(), CommandErrorPayload> {
    if let Some(expected) = expected {
        if agent.conversation_id != expected.to_string() {
            return Err(invalid_payload(
                "background agent does not belong to conversationId".to_owned(),
            ));
        }
    }
    Ok(())
}

fn background_agent_payload(
    agent: jyowo_harness_sdk::BackgroundAgentRecord,
) -> BackgroundAgentPayload {
    let pending_input_request_id = background_recovery_request_id(&agent.payload_json, "input");
    let pending_permission_request_id =
        background_recovery_request_id(&agent.payload_json, "permission");

    BackgroundAgentPayload {
        background_agent_id: agent.background_agent_id,
        conversation_id: agent.conversation_id,
        parent_run_id: agent.run_id,
        pending_input_request_id,
        pending_permission_request_id,
        state: agent.state,
        title: agent.title,
        created_at: agent.created_at,
        updated_at: agent.updated_at,
    }
}

fn background_recovery_request_id(payload_json: &str, expected_kind: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(payload_json)
        .ok()
        .and_then(|payload| {
            let recovery = payload.get("backgroundRecovery")?;
            let kind = recovery.get("kind")?.as_str()?;
            if kind != expected_kind {
                return None;
            }
            recovery.get("requestId")?.as_str().map(ToOwned::to_owned)
        })
}

fn map_background_agent_error(
    error: jyowo_harness_sdk::BackgroundAgentTransitionError,
) -> CommandErrorPayload {
    match error {
        jyowo_harness_sdk::BackgroundAgentTransitionError::NotFound(id) => {
            not_found(format!("background agent not found: {id}"))
        }
        jyowo_harness_sdk::BackgroundAgentTransitionError::InvalidTransition {
            operation,
            state,
        } => invalid_payload(format!(
            "invalid background agent transition {operation} from {state:?}"
        )),
        jyowo_harness_sdk::BackgroundAgentTransitionError::InvalidBackgroundAgentId(id) => {
            invalid_payload(format!("invalid background agent id: {id}"))
        }
        jyowo_harness_sdk::BackgroundAgentTransitionError::Store(error) => {
            runtime_operation_failed(format!("background agent store failed: {error}"))
        }
        jyowo_harness_sdk::BackgroundAgentTransitionError::Journal(error) => {
            runtime_operation_failed(format!("background agent journal failed: {error}"))
        }
    }
}
