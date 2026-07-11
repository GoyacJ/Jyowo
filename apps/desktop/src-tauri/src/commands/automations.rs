#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::artifacts::*;
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

pub async fn list_automations_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListAutomationsResponse, CommandErrorPayload> {
    let _guard = state.automation_lock.lock().await;
    Ok(ListAutomationsResponse {
        automations: state.automation_store.load_automations()?,
    })
}

pub async fn save_automation_with_runtime_state(
    request: SaveAutomationRequest,
    state: &DesktopRuntimeState,
) -> Result<SaveAutomationResponse, CommandErrorPayload> {
    ensure_automation_spec(&request.automation)?;
    let _guard = state.automation_lock.lock().await;
    let mut automations = state.automation_store.load_automations()?;
    automations.retain(|record| record.id != request.automation.id);
    automations.push(request.automation.clone());
    state.automation_store.save_automations(&automations)?;

    Ok(SaveAutomationResponse {
        automation: request.automation,
        status: "saved",
    })
}

pub async fn set_automation_enabled_with_runtime_state(
    request: SetAutomationEnabledRequest,
    state: &DesktopRuntimeState,
) -> Result<SetAutomationEnabledResponse, CommandErrorPayload> {
    ensure_automation_id(&request.id)?;
    let _guard = state.automation_lock.lock().await;
    let mut automations = state.automation_store.load_automations()?;
    let Some(automation) = automations
        .iter_mut()
        .find(|automation| automation.id == request.id)
    else {
        return Err(not_found(format!("automation not found: {}", request.id)));
    };
    automation.enabled = request.enabled;
    automation.updated_at = Utc::now();
    let automation = automation.clone();
    state.automation_store.save_automations(&automations)?;

    Ok(SetAutomationEnabledResponse {
        automation,
        status: "saved",
    })
}

pub async fn delete_automation_with_runtime_state(
    id: String,
    state: &DesktopRuntimeState,
) -> Result<DeleteAutomationResponse, CommandErrorPayload> {
    ensure_automation_id(&id)?;
    let _guard = state.automation_lock.lock().await;
    let mut automations = state.automation_store.load_automations()?;
    automations.retain(|automation| automation.id != id);
    state.automation_store.save_automations(&automations)?;

    Ok(DeleteAutomationResponse {
        id,
        status: "deleted",
    })
}

pub async fn list_automation_runs_with_runtime_state(
    automation_id: Option<String>,
    state: &DesktopRuntimeState,
) -> Result<ListAutomationRunsResponse, CommandErrorPayload> {
    if let Some(automation_id) = automation_id.as_deref() {
        ensure_automation_id(automation_id)?;
    }
    let _guard = state.automation_lock.lock().await;
    let mut runs = state.automation_store.load_run_records()?;
    if let Some(automation_id) = automation_id {
        runs.retain(|record| record.automation_id == automation_id);
    }
    runs.sort_by(|left, right| right.started_at.cmp(&left.started_at));
    Ok(ListAutomationRunsResponse { runs })
}
