#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
use super::contracts::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::error::*;
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
use super::skills::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;

pub async fn list_agent_profiles_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListAgentProfilesResponse, CommandErrorPayload> {
    let profiles = list_global_agent_profiles_with_builtin(state)?;
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

    let saved = save_global_agent_profile(state, profile)?;

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
    delete_global_agent_profile(state, request.id.trim())?;

    Ok(DeleteAgentProfileResponse {
        id: request.id,
        status: "deleted",
    })
}

pub(crate) fn list_global_agent_profiles_with_builtin(
    state: &DesktopRuntimeState,
) -> Result<Vec<AgentProfile>, CommandErrorPayload> {
    let mut profiles = jyowo_harness_sdk::builtin_agent_profiles();
    profiles.extend(global_config_store(state)?.load_global_agent_profiles()?);
    Ok(profiles)
}

fn save_global_agent_profile(
    state: &DesktopRuntimeState,
    profile: AgentProfile,
) -> Result<AgentProfile, CommandErrorPayload> {
    let store = global_config_store(state)?;
    let mut profiles = store.load_global_agent_profiles()?;
    if let Some(existing) = profiles.iter_mut().find(|entry| entry.id == profile.id) {
        *existing = profile.clone();
    } else {
        profiles.push(profile.clone());
    }
    store.save_global_agent_profiles(&profiles)?;
    Ok(profile)
}

fn delete_global_agent_profile(
    state: &DesktopRuntimeState,
    profile_id: &str,
) -> Result<(), CommandErrorPayload> {
    if jyowo_harness_sdk::builtin_agent_profiles()
        .iter()
        .any(|profile| profile.id == profile_id)
    {
        return Err(invalid_payload(
            "builtin agent profiles are read-only".to_owned(),
        ));
    }
    let store = global_config_store(state)?;
    let mut profiles = store.load_global_agent_profiles()?;
    let original_len = profiles.len();
    profiles.retain(|profile| profile.id != profile_id);
    if profiles.len() == original_len {
        return Err(not_found(format!("agent profile not found: {profile_id}")));
    }
    store.save_global_agent_profiles(&profiles)
}

fn global_config_store(
    state: &DesktopRuntimeState,
) -> Result<&GlobalConfigStore, CommandErrorPayload> {
    state
        .global_config_store
        .as_ref()
        .ok_or_else(|| runtime_unavailable("global configuration store is unavailable"))
}
