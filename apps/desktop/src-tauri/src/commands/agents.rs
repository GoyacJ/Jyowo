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

use jyowo_harness_sdk::{
    delete_agent_profile, list_agent_profiles, save_agent_profile, AgentProfileRegistryError,
    AgentRuntimeFacadeError, AgentRuntimeStoreError,
};

pub async fn list_agent_profiles_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListAgentProfilesResponse, CommandErrorPayload> {
    let profiles = list_agent_profiles(state.workspace_root()).map_err(map_agent_runtime_error)?;
    Ok(ListAgentProfilesResponse { profiles })
}

pub async fn save_agent_profile_with_runtime_state(
    profile: AgentProfile,
    state: &DesktopRuntimeState,
) -> Result<SaveAgentProfileResponse, CommandErrorPayload> {
    validate_agent_profile(&profile).map_err(|error| invalid_payload(error.to_string()))?;
    if profile.scope == AgentProfileScope::Builtin {
        return Err(invalid_payload(
            "builtin agent profiles are read-only".to_owned(),
        ));
    }

    let saved =
        save_agent_profile(state.workspace_root(), profile).map_err(map_agent_runtime_error)?;

    Ok(SaveAgentProfileResponse {
        profile: saved,
        status: "saved",
    })
}

pub async fn delete_agent_profile_with_runtime_state(
    request: DeleteAgentProfileRequest,
    state: &DesktopRuntimeState,
) -> Result<DeleteAgentProfileResponse, CommandErrorPayload> {
    ensure_non_empty("id", &request.id)?;
    delete_agent_profile(state.workspace_root(), request.id.trim())
        .map_err(map_agent_runtime_error)?;

    Ok(DeleteAgentProfileResponse {
        id: request.id,
        status: "deleted",
    })
}

pub(crate) fn map_agent_runtime_error(error: AgentRuntimeFacadeError) -> CommandErrorPayload {
    match error {
        AgentRuntimeFacadeError::Profiles(profile_error) => {
            map_profile_registry_error(profile_error)
        }
        AgentRuntimeFacadeError::Store(store_error) => map_runtime_store_error(store_error),
    }
}

fn map_profile_registry_error(error: AgentProfileRegistryError) -> CommandErrorPayload {
    match error {
        AgentProfileRegistryError::Validation(message) => invalid_payload(message),
        AgentProfileRegistryError::BuiltinReadOnly => {
            invalid_payload("builtin agent profiles are read-only".to_owned())
        }
        AgentProfileRegistryError::NotFound(id) => {
            not_found(format!("agent profile not found: {id}"))
        }
        AgentProfileRegistryError::Json(_) => {
            invalid_payload("agent profiles file is invalid and was quarantined".to_owned())
        }
        AgentProfileRegistryError::Io(_) | AgentProfileRegistryError::Sqlite(_) => {
            runtime_operation_failed("agent profile registry operation failed".to_owned())
        }
        AgentProfileRegistryError::Store(store_error) => map_runtime_store_error(store_error),
    }
}

fn map_runtime_store_error(error: AgentRuntimeStoreError) -> CommandErrorPayload {
    match error {
        AgentRuntimeStoreError::UnsupportedSchema(message) => runtime_init_failed(message),
        AgentRuntimeStoreError::LockPoisoned => {
            runtime_operation_failed("agent runtime store lock poisoned".to_owned())
        }
        AgentRuntimeStoreError::Io(_) | AgentRuntimeStoreError::Sqlite(_) => {
            runtime_operation_failed("agent runtime store operation failed".to_owned())
        }
    }
}
