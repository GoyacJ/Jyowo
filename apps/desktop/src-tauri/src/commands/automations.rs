#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::artifacts::*;
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

pub async fn run_automation_now_with_runtime_state(
    id: String,
    state: &DesktopRuntimeState,
) -> Result<RunAutomationNowResponse, CommandErrorPayload> {
    ensure_automation_id(&id)?;
    let automation = {
        let _guard = state.automation_lock.lock().await;
        let automations = state.automation_store.load_automations()?;
        automations
            .iter()
            .find(|automation| automation.id == id)
            .cloned()
            .ok_or_else(|| not_found(format!("automation not found: {id}")))?
    };
    let record = run_automation_spec(&automation, state).await?;

    Ok(RunAutomationNowResponse { record })
}

pub async fn run_due_automations_once_with_runtime_state(
    now: DateTime<Utc>,
    state: &DesktopRuntimeState,
) -> Result<Vec<AutomationRunRecord>, CommandErrorPayload> {
    let due_automations = {
        let _guard = state.automation_lock.lock().await;
        let automations = state.automation_store.load_automations()?;
        let existing_runs = state.automation_store.load_run_records()?;
        automations
            .into_iter()
            .filter_map(|automation| {
                if !automation.enabled {
                    return None;
                }
                match automation_is_due(&automation, &existing_runs, now) {
                    Ok(true) => Some(Ok(automation)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                }
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut new_records = Vec::new();

    for automation in due_automations {
        new_records.push(run_automation_spec(&automation, state).await?);
    }

    Ok(new_records)
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

pub(crate) async fn run_automation_spec(
    automation: &AutomationSpec,
    state: &DesktopRuntimeState,
) -> Result<AutomationRunRecord, CommandErrorPayload> {
    ensure_automation_spec(automation)?;
    let started_at = Utc::now();
    let run_result = start_automation_conversation_run(automation, state).await;
    let record = match run_result {
        Ok(response) => AutomationRunRecord {
            automation_id: automation.id.clone(),
            completed_at: None,
            id: format!("automation-run-{}", RunId::new()),
            message: Some("Started".to_owned()),
            run_id: Some(response.run_id),
            started_at,
            status: AutomationRunStatus::Started,
        },
        Err(error) => {
            let redactor = DefaultRedactor::default();
            AutomationRunRecord {
                automation_id: automation.id.clone(),
                completed_at: Some(Utc::now()),
                id: format!("automation-run-{}", RunId::new()),
                message: Some(redacted_display(error.message, &redactor)),
                run_id: None,
                started_at,
                status: automation_run_status_for_error(&error.code),
            }
        }
    };
    {
        let _guard = state.automation_lock.lock().await;
        state.automation_store.append_run_record(&record)?;
    }
    Ok(record)
}

pub(crate) fn automation_run_status_for_error(code: &str) -> AutomationRunStatus {
    match code {
        "INVALID_PAYLOAD" | "RUNTIME_UNAVAILABLE" | "NOT_FOUND" => AutomationRunStatus::Rejected,
        _ => AutomationRunStatus::Failed,
    }
}

pub(crate) fn automation_is_due(
    automation: &AutomationSpec,
    existing_runs: &[AutomationRunRecord],
    now: DateTime<Utc>,
) -> Result<bool, CommandErrorPayload> {
    if automation.schedule.interval_minutes == 0 {
        return Err(invalid_payload(
            "automation schedule intervalMinutes must be greater than zero".to_owned(),
        ));
    }
    let last_started_at = existing_runs
        .iter()
        .filter(|record| record.automation_id == automation.id)
        .map(|record| record.started_at)
        .max();
    let base = last_started_at.unwrap_or(automation.created_at);
    if now <= base {
        return Ok(false);
    }
    let elapsed_minutes = now.signed_duration_since(base).num_minutes();
    let interval = i64::from(automation.schedule.interval_minutes);
    let elapsed_intervals = elapsed_minutes / interval;
    if elapsed_intervals <= 0 {
        return Ok(false);
    }
    if last_started_at.is_none()
        && elapsed_intervals > 1
        && automation.missed_run_policy == MissedRunPolicy::Skip
    {
        return Ok(false);
    }
    Ok(true)
}

pub(crate) async fn start_automation_conversation_run(
    automation: &AutomationSpec,
    state: &DesktopRuntimeState,
) -> Result<StartRunResponse, CommandErrorPayload> {
    let permission_mode = automation.permission_mode;
    ensure_start_run_permission_mode(permission_mode)?;
    let conversation_id = state.default_conversation_id().to_string();
    let request = StartRunRequest {
        attachments: None,
        client_message_id: None,
        context_references: None,
        conversation_id: conversation_id.clone(),
        permission_mode: Some(permission_mode),
        prompt: automation.prompt.clone(),
    };
    let session_id = parse_session_id(&conversation_id)?;
    let input = build_conversation_turn_input(&request, state).await?;
    let _start_run_guard = state.start_run_lock.lock().await;
    let (harness, mut options) =
        if let Some(model_config_id) = conversation_model_config_id(&session_id, state)? {
            let stream_permission_runtime =
                state.stream_permission_runtime.as_ref().ok_or_else(|| {
                    runtime_unavailable("Starting automation runs requires the desktop runtime.")
                })?;
            let (harness, model_id, protocol) = build_desktop_harness(
                &state.workspace_root,
                Arc::clone(stream_permission_runtime),
                Some(&model_config_id),
                Arc::clone(&state.provider_capability_routes),
            )
            .await?;
            (
                Arc::new(harness),
                state.conversation_session_options_for_model(session_id, model_id, protocol),
            )
        } else {
            let Some(runtime) = state.active_conversation_runtime(session_id) else {
                return Err(runtime_unavailable(
                    "Starting automation runs requires the runtime conversation facade.",
                ));
            };
            runtime
        };
    options = options
        .with_tool_profile(automation_effective_tool_profile(automation))
        .with_permission_mode(permission_mode);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .map_err(|error| runtime_operation_failed(format!("conversation open failed: {error}")))?;
    let after_event_id = conversation_tail_event_id(&harness, options.clone()).await?;
    let run_harness = Arc::clone(&harness);
    let run_options = options.clone();
    let mut run_task = tokio::spawn(async move {
        run_harness
            .submit_conversation_turn(ConversationTurnRequest {
                options: run_options,
                input,
                permission_mode_override: Some(permission_mode),
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

    Ok(StartRunResponse {
        run_id: run_id.to_string(),
        status: "started",
    })
}

pub(crate) fn automation_effective_tool_profile(automation: &AutomationSpec) -> ToolProfile {
    let mut denylist = BTreeSet::from([
        "Bash".to_owned(),
        "FileEdit".to_owned(),
        "FileWrite".to_owned(),
        "ProcessRead".to_owned(),
        "ProcessStart".to_owned(),
        "ProcessStop".to_owned(),
    ]);
    if let ToolProfile::Custom {
        denylist: configured,
        ..
    } = &automation.tool_profile
    {
        denylist.extend(configured.iter().cloned());
    }

    ToolProfile::Custom {
        allowlist: BTreeSet::new(),
        denylist,
        group_allowlist: vec![
            ToolGroup::Clarification,
            ToolGroup::Coordinator,
            ToolGroup::FileSystem,
            ToolGroup::Memory,
            ToolGroup::Meta,
            ToolGroup::Search,
        ],
        group_denylist: vec![ToolGroup::Network, ToolGroup::Shell],
        mcp_included: false,
        plugin_included: false,
    }
}
