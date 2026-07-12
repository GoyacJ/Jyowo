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

pub async fn list_mcp_servers_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListMcpServersResponse, CommandErrorPayload> {
    let mut servers = BTreeMap::new();
    let records = state.mcp_server_store.load_records()?;
    let records_by_id = records
        .iter()
        .map(|record| (record.id.clone(), record.clone()))
        .collect::<BTreeMap<_, _>>();
    let last_diagnostics =
        mcp_last_diagnostics_by_server(&state.mcp_diagnostic_store.load_records()?);

    for record in &records {
        let mut summary = mcp_server_summary_from_record(record);
        apply_mcp_last_diagnostic(&mut summary, last_diagnostics.get(&record.id));
        servers.insert(record.id.clone(), summary);
    }

    if let Some(settings_runtime) = state.settings_runtime() {
        if let Some(config) = settings_runtime.mcp_config() {
            for server_id in config.registry.server_ids().await {
                if let Some(summary) =
                    mcp_server_summary_from_registry(&config.registry, &server_id).await
                {
                    let mut summary = summary;
                    if let Some(record) = records_by_id.get(&server_id.0) {
                        summary.enabled = record.enabled;
                        summary.manageable = true;
                        if !record.enabled {
                            summary.status = "disabled";
                            summary.exposed_tool_count = 0;
                        }
                    }
                    apply_mcp_last_diagnostic(&mut summary, last_diagnostics.get(&server_id.0));
                    servers.insert(server_id.0.clone(), summary);
                }
            }
        }
    }

    Ok(ListMcpServersResponse {
        servers: servers.into_values().collect(),
    })
}

pub async fn save_mcp_server_with_store(
    request: SaveMcpServerRequest,
    store: &dyn McpServerStore,
) -> Result<SaveMcpServerResponse, CommandErrorPayload> {
    ensure_mcp_server_request(&request)?;
    let id = request.id.trim().to_owned();
    let existing = store
        .load_records()?
        .into_iter()
        .find(|record| record.id == id);
    let record = mcp_server_record_from_save_request(request, existing.as_ref())?;

    store.save_record(&record)?;

    Ok(SaveMcpServerResponse {
        server: mcp_server_summary_from_record(&record),
    })
}

pub async fn save_mcp_server_with_runtime_state(
    request: SaveMcpServerRequest,
    state: &DesktopRuntimeState,
) -> Result<SaveMcpServerResponse, CommandErrorPayload> {
    ensure_mcp_server_request(&request)?;
    let id = request.id.trim().to_owned();
    let existing = state
        .mcp_server_store
        .load_records()?
        .into_iter()
        .find(|record| record.id == id);
    let record = mcp_server_record_from_save_request(request, existing.as_ref())?;

    save_mcp_server_record_with_runtime_state(record, state).await
}

async fn save_mcp_server_record_with_runtime_state(
    record: McpServerConfigRecord,
    state: &DesktopRuntimeState,
) -> Result<SaveMcpServerResponse, CommandErrorPayload> {
    let _ = mcp_server_spec_from_record(&record, mcp_workdir_root_for_state(state))?;
    state.mcp_server_store.save_record(&record)?;

    let Some(settings_runtime) = state.settings_runtime() else {
        return Ok(SaveMcpServerResponse {
            server: mcp_server_summary_from_record(&record),
        });
    };
    remove_mcp_server_from_settings_runtime(&settings_runtime, &record.id).await?;
    if !record.enabled {
        return Ok(SaveMcpServerResponse {
            server: mcp_server_summary_from_record(&record),
        });
    }
    let server = register_mcp_record_with_settings_runtime(
        &record,
        &settings_runtime,
        state.default_conversation_id,
        state,
    )
    .await?;

    Ok(SaveMcpServerResponse { server })
}

fn mcp_server_record_from_save_request(
    request: SaveMcpServerRequest,
    existing: Option<&McpServerConfigRecord>,
) -> Result<McpServerConfigRecord, CommandErrorPayload> {
    let record = McpServerConfigRecord {
        enabled: request.enabled,
        display_name: request.display_name.trim().to_owned(),
        id: request.id.trim().to_owned(),
        scope: request.scope,
        transport: mcp_transport_from_save_transport(request.transport, existing)?,
    };
    ensure_mcp_server_record(&record)?;
    Ok(record)
}

fn mcp_transport_from_save_transport(
    transport: SaveMcpServerTransportConfig,
    existing: Option<&McpServerConfigRecord>,
) -> Result<McpServerTransportConfig, CommandErrorPayload> {
    match transport {
        SaveMcpServerTransportConfig::Stdio {
            command,
            args,
            env,
            inherit_env,
            working_dir,
        } => {
            let existing_env = match existing.map(|record| &record.transport) {
                Some(McpServerTransportConfig::Stdio { env, .. }) => Some(env.as_slice()),
                _ => None,
            };
            Ok(McpServerTransportConfig::Stdio {
                command,
                args,
                env: mcp_name_values_from_save_records("transport.env", env, existing_env)?,
                inherit_env,
                working_dir,
            })
        }
        SaveMcpServerTransportConfig::Http {
            url,
            bearer_token_env_var,
            headers,
            headers_from_env,
        } => {
            let existing_headers = match existing.map(|record| &record.transport) {
                Some(McpServerTransportConfig::Http { headers, .. }) => Some(headers.as_slice()),
                _ => None,
            };
            Ok(McpServerTransportConfig::Http {
                url,
                bearer_token_env_var,
                headers: mcp_name_values_from_save_records(
                    "transport.headers",
                    headers,
                    existing_headers,
                )?,
                headers_from_env,
            })
        }
        SaveMcpServerTransportConfig::InProcess => Ok(McpServerTransportConfig::InProcess),
    }
}

fn mcp_name_values_from_save_records(
    field: &'static str,
    records: Vec<McpNameValueSaveRecord>,
    existing: Option<&[McpNameValueRecord]>,
) -> Result<Vec<McpNameValueRecord>, CommandErrorPayload> {
    records
        .into_iter()
        .map(|record| mcp_name_value_from_save_record(field, record, existing))
        .collect()
}

fn mcp_name_value_from_save_record(
    field: &'static str,
    record: McpNameValueSaveRecord,
    existing: Option<&[McpNameValueRecord]>,
) -> Result<McpNameValueRecord, CommandErrorPayload> {
    let key = record.key.trim().to_owned();
    if record.preserve_existing {
        if record.value.is_some() {
            return Err(invalid_payload(format!(
                "{field}.preserveExisting must not include a replacement value"
            )));
        }
        let Some(existing_value) = existing
            .unwrap_or_default()
            .iter()
            .find(|existing| existing.key == key)
            .map(|existing| existing.value.clone())
        else {
            return Err(invalid_payload(format!(
                "{field}.preserveExisting could not find an existing value"
            )));
        };
        return Ok(McpNameValueRecord {
            key,
            value: existing_value,
        });
    }

    let Some(value) = record.value else {
        return Err(invalid_payload(format!("{field}.value must not be empty")));
    };
    if value.trim().is_empty() {
        return Err(invalid_payload(format!("{field}.value must not be empty")));
    }
    Ok(McpNameValueRecord { key, value })
}

pub async fn list_browser_mcp_presets_with_store(
    store: &dyn McpServerStore,
) -> Result<ListBrowserMcpPresetsResponse, CommandErrorPayload> {
    let records = store.load_records()?;
    let presets = browser_mcp_preset_ids()
        .iter()
        .map(|preset_id| browser_mcp_preset_summary(*preset_id, &records))
        .collect();

    Ok(ListBrowserMcpPresetsResponse { presets })
}

pub async fn list_browser_mcp_presets_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListBrowserMcpPresetsResponse, CommandErrorPayload> {
    list_browser_mcp_presets_with_store(state.mcp_server_store.as_ref()).await
}

pub async fn save_browser_mcp_preset_with_store(
    request: SaveBrowserMcpPresetRequest,
    store: &dyn McpServerStore,
) -> Result<SaveBrowserMcpPresetResponse, CommandErrorPayload> {
    let record = browser_mcp_preset_record(request.preset_id, request.enabled);
    ensure_mcp_server_record(&record)?;
    store.save_record(&record)?;

    Ok(SaveBrowserMcpPresetResponse {
        preset: browser_mcp_preset_summary(request.preset_id, &[record.clone()]),
        server: mcp_server_summary_from_record(&record),
    })
}

pub async fn save_browser_mcp_preset_with_runtime_state(
    request: SaveBrowserMcpPresetRequest,
    state: &DesktopRuntimeState,
) -> Result<SaveBrowserMcpPresetResponse, CommandErrorPayload> {
    let record = browser_mcp_preset_record(request.preset_id, request.enabled);
    let preset_id = request.preset_id;
    ensure_mcp_server_record(&record)?;
    let response = save_mcp_server_record_with_runtime_state(record, state).await?;

    Ok(SaveBrowserMcpPresetResponse {
        preset: browser_mcp_preset_summary_from_enabled(preset_id, response.server.enabled),
        server: response.server,
    })
}

pub async fn get_mcp_server_config_with_store(
    request: GetMcpServerConfigRequest,
    store: &dyn McpServerStore,
) -> Result<GetMcpServerConfigResponse, CommandErrorPayload> {
    ensure_mcp_server_id(&request.id)?;
    let id = request.id.trim();
    let record = store
        .load_records()?
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| not_found(format!("mcp server not found: {id}")))?;
    ensure_mcp_server_record(&record)?;

    Ok(GetMcpServerConfigResponse {
        server: mcp_server_config_payload_from_record(&record),
    })
}

pub async fn get_mcp_server_config_with_runtime_state(
    request: GetMcpServerConfigRequest,
    state: &DesktopRuntimeState,
) -> Result<GetMcpServerConfigResponse, CommandErrorPayload> {
    get_mcp_server_config_with_store(request, state.mcp_server_store.as_ref()).await
}

pub async fn delete_mcp_server_with_store(
    request: DeleteMcpServerRequest,
    store: &dyn McpServerStore,
) -> Result<DeleteMcpServerResponse, CommandErrorPayload> {
    ensure_mcp_server_id(&request.id)?;
    store.delete_record(request.id.trim())?;

    Ok(DeleteMcpServerResponse {
        id: request.id.trim().to_owned(),
        status: "deleted",
    })
}

pub async fn delete_mcp_server_with_runtime_state(
    request: DeleteMcpServerRequest,
    state: &DesktopRuntimeState,
) -> Result<DeleteMcpServerResponse, CommandErrorPayload> {
    ensure_mcp_server_id(&request.id)?;
    let id = request.id.trim();
    state.mcp_server_store.delete_record(id)?;
    if let Some(settings_runtime) = state.settings_runtime() {
        remove_mcp_server_from_settings_runtime(&settings_runtime, id).await?;
    }

    Ok(DeleteMcpServerResponse {
        id: id.to_owned(),
        status: "deleted",
    })
}

pub async fn set_mcp_server_enabled_with_runtime_state(
    request: SetMcpServerEnabledRequest,
    state: &DesktopRuntimeState,
) -> Result<SetMcpServerEnabledResponse, CommandErrorPayload> {
    ensure_mcp_server_id(&request.id)?;
    let id = request.id.trim();
    let mut records = state.mcp_server_store.load_records()?;
    let Some(record) = records.iter_mut().find(|record| record.id == id) else {
        return Err(not_found(format!("mcp server not found: {id}")));
    };
    record.enabled = request.enabled;
    ensure_mcp_server_record(record)?;
    let record = record.clone();
    if record.enabled {
        let _ = mcp_server_spec_from_record(&record, mcp_workdir_root_for_state(state))?;
    }
    state.mcp_server_store.save_record(&record)?;

    let Some(settings_runtime) = state.settings_runtime() else {
        return Ok(SetMcpServerEnabledResponse {
            server: mcp_server_summary_from_record(&record),
        });
    };

    remove_mcp_server_from_settings_runtime(&settings_runtime, &record.id).await?;
    if !record.enabled {
        return Ok(SetMcpServerEnabledResponse {
            server: mcp_server_summary_from_record(&record),
        });
    }

    let server = register_mcp_record_with_settings_runtime(
        &record,
        &settings_runtime,
        state.default_conversation_id,
        state,
    )
    .await?;
    Ok(SetMcpServerEnabledResponse { server })
}

pub async fn restart_mcp_server_with_runtime_state(
    request: RestartMcpServerRequest,
    state: &DesktopRuntimeState,
) -> Result<RestartMcpServerResponse, CommandErrorPayload> {
    ensure_mcp_server_id(&request.id)?;
    let id = request.id.trim();
    let record = state
        .mcp_server_store
        .load_records()?
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| not_found(format!("mcp server not found: {id}")))?;
    ensure_mcp_server_record(&record)?;

    let Some(settings_runtime) = state.settings_runtime() else {
        return Ok(RestartMcpServerResponse {
            server: mcp_server_summary_from_record(&record),
        });
    };

    remove_mcp_server_from_settings_runtime(&settings_runtime, &record.id).await?;
    if !record.enabled {
        return Ok(RestartMcpServerResponse {
            server: mcp_server_summary_from_record(&record),
        });
    }

    let server = register_mcp_record_with_settings_runtime(
        &record,
        &settings_runtime,
        state.default_conversation_id,
        state,
    )
    .await?;
    Ok(RestartMcpServerResponse { server })
}

pub async fn list_mcp_diagnostics_with_store(
    server_id: Option<String>,
    store: &dyn McpDiagnosticStore,
) -> Result<ListMcpDiagnosticsResponse, CommandErrorPayload> {
    if let Some(server_id) = server_id.as_deref() {
        ensure_mcp_server_id(server_id)?;
    }
    let events = store
        .load_records()?
        .into_iter()
        .filter(|record| {
            server_id
                .as_deref()
                .is_none_or(|server_id| record.server_id == server_id)
        })
        .collect();
    Ok(ListMcpDiagnosticsResponse { events })
}

pub async fn list_mcp_diagnostics_with_runtime_state(
    request: ListMcpDiagnosticsRequest,
    state: &DesktopRuntimeState,
) -> Result<ListMcpDiagnosticsResponse, CommandErrorPayload> {
    list_mcp_diagnostics_with_store(request.server_id, state.mcp_diagnostic_store.as_ref()).await
}

pub async fn clear_mcp_diagnostics_with_runtime_state(
    request: ClearMcpDiagnosticsRequest,
    state: &DesktopRuntimeState,
) -> Result<ClearMcpDiagnosticsResponse, CommandErrorPayload> {
    if let Some(server_id) = request.server_id.as_deref() {
        ensure_mcp_server_id(server_id)?;
    }
    state
        .mcp_diagnostic_store
        .clear_records(request.server_id.as_deref())?;
    Ok(ClearMcpDiagnosticsResponse { status: "cleared" })
}

pub async fn subscribe_mcp_diagnostics_with_runtime_state(
    request: SubscribeMcpDiagnosticsRequest,
    state: &DesktopRuntimeState,
) -> Result<SubscribeMcpDiagnosticsResponse, CommandErrorPayload> {
    subscribe_mcp_diagnostics_for_window_with_runtime_state(
        request,
        "default".to_owned(),
        Arc::new(|_batch| Ok(())),
        state,
    )
    .await
}

pub async fn subscribe_mcp_diagnostics_for_window_with_runtime_state(
    request: SubscribeMcpDiagnosticsRequest,
    window_label: String,
    emitter: McpDiagnosticBatchEmitter,
    state: &DesktopRuntimeState,
) -> Result<SubscribeMcpDiagnosticsResponse, CommandErrorPayload> {
    ensure_non_empty("windowLabel", &window_label)?;
    if let Some(server_id) = request.server_id.as_deref() {
        ensure_mcp_server_id(server_id)?;
    }
    let replay_events = list_mcp_diagnostics_with_store(
        request.server_id.clone(),
        state.mcp_diagnostic_store.as_ref(),
    )
    .await?
    .events;
    let subscription_id = format!("mcp-diagnostic-subscription-{}", EventId::new());
    let handle = spawn_mcp_diagnostic_subscription(
        subscription_id.clone(),
        request.server_id.clone(),
        replay_events.iter().map(|event| event.id.clone()).collect(),
        window_label.clone(),
        Arc::clone(&emitter),
        state.clone(),
    );
    state.mcp_diagnostic_subscriptions.lock().await.insert(
        subscription_id.clone(),
        McpDiagnosticSubscriptionHandle {
            task: handle,
            window_label,
        },
    );

    Ok(SubscribeMcpDiagnosticsResponse {
        subscription_id,
        server_id: request.server_id,
        replay_events,
    })
}

pub async fn unsubscribe_mcp_diagnostics_with_runtime_state(
    request: UnsubscribeMcpDiagnosticsRequest,
    state: &DesktopRuntimeState,
) -> Result<UnsubscribeMcpDiagnosticsResponse, CommandErrorPayload> {
    unsubscribe_mcp_diagnostics_for_window_with_runtime_state(request, "default".to_owned(), state)
        .await
}

pub async fn unsubscribe_mcp_diagnostics_for_window_with_runtime_state(
    request: UnsubscribeMcpDiagnosticsRequest,
    window_label: String,
    state: &DesktopRuntimeState,
) -> Result<UnsubscribeMcpDiagnosticsResponse, CommandErrorPayload> {
    ensure_non_empty("subscriptionId", &request.subscription_id)?;
    ensure_non_empty("windowLabel", &window_label)?;
    let mut subscriptions = state.mcp_diagnostic_subscriptions.lock().await;
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
        subscription.task.abort();
        return Ok(UnsubscribeMcpDiagnosticsResponse {
            subscription_id: request.subscription_id,
            status: "unsubscribed",
        });
    }

    Ok(UnsubscribeMcpDiagnosticsResponse {
        subscription_id: request.subscription_id,
        status: "alreadyClosed",
    })
}

pub(crate) fn spawn_mcp_diagnostic_subscription(
    subscription_id: String,
    server_id: Option<String>,
    mut seen_ids: HashSet<String>,
    window_label: String,
    emitter: McpDiagnosticBatchEmitter,
    state: DesktopRuntimeState,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(MCP_DIAGNOSTIC_SUBSCRIPTION_POLL_INTERVAL).await;
            let records = match state.mcp_diagnostic_store.load_records() {
                Ok(records) => records,
                Err(_) => break,
            };
            let events = records
                .into_iter()
                .filter(|record| {
                    server_id
                        .as_deref()
                        .is_none_or(|server_id| record.server_id == server_id)
                        && !seen_ids.contains(&record.id)
                })
                .collect::<Vec<_>>();
            if events.is_empty() {
                continue;
            }

            let mut emit_failed = false;
            for chunk in events.chunks(MCP_DIAGNOSTIC_SUBSCRIPTION_BATCH_LIMIT) {
                for event in chunk {
                    seen_ids.insert(event.id.clone());
                }
                let batch = McpDiagnosticBatchPayload {
                    subscription_id: subscription_id.clone(),
                    server_id: server_id.clone(),
                    events: chunk.to_vec(),
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
            .mcp_diagnostic_subscriptions
            .lock()
            .await
            .remove(&subscription_id);
        let _ = window_label;
    })
}

pub(crate) async fn mcp_config_from_records(
    records: Vec<McpServerConfigRecord>,
    default_session_id: SessionId,
    default_agent_id: AgentId,
    diagnostic_store: Arc<dyn McpDiagnosticStore>,
    authorization_service: Arc<harness_execution::AuthorizationService>,
    workdir_root: &Path,
) -> Result<McpConfig, CommandErrorPayload> {
    let registry = McpRegistry::new();
    let mut server_ids_to_inject = Vec::new();

    for record in records {
        if ensure_mcp_server_record_identity(&record).is_err() {
            continue;
        }
        if !record.enabled {
            continue;
        }
        let server_id = register_mcp_record_with_registry(
            &record,
            &registry,
            default_session_id,
            default_agent_id,
            Arc::clone(&diagnostic_store),
            Arc::clone(&authorization_service),
            workdir_root,
            InteractivityLevel::NoInteractive,
            true,
        )
        .await?;
        if matches!(
            registry.connection_state(&server_id).await,
            Some(McpConnectionState::Ready)
        ) {
            server_ids_to_inject.push(server_id);
        }
    }

    Ok(McpConfig {
        registry,
        server_ids_to_inject,
    })
}

pub(crate) async fn register_mcp_record_with_settings_runtime(
    record: &McpServerConfigRecord,
    settings_runtime: &DesktopSettingsRuntime,
    default_session_id: SessionId,
    state: &DesktopRuntimeState,
) -> Result<McpServerSummaryPayload, CommandErrorPayload> {
    let Some(config) = settings_runtime.mcp_config() else {
        return Ok(mcp_server_summary_from_record(record));
    };
    let server_id = register_mcp_record_with_registry(
        record,
        &config.registry,
        default_session_id,
        AgentId::new(),
        Arc::clone(&state.mcp_diagnostic_store),
        settings_runtime.authorization_service(),
        mcp_workdir_root_for_state(state),
        InteractivityLevel::FullyInteractive,
        false,
    )
    .await?;

    if matches!(
        config.registry.connection_state(&server_id).await,
        Some(McpConnectionState::Ready)
    ) {
        if let Err(error) = config
            .registry
            .inject_tools_into(settings_runtime.tool_registry(), &server_id)
            .await
        {
            config
                .registry
                .set_connection_state(
                    &server_id,
                    McpConnectionState::Failed {
                        last_error: error.to_string(),
                    },
                )
                .await
                .map_err(|error| runtime_operation_failed(error.to_string()))?;
        }
    }

    mcp_server_summary_from_registry(&config.registry, &server_id)
        .await
        .ok_or_else(|| {
            runtime_operation_failed("mcp server registry summary unavailable".to_owned())
        })
}

fn mcp_workdir_root_for_state(state: &DesktopRuntimeState) -> &Path {
    state
        .project_workspace_root()
        .unwrap_or_else(|| state.conversation_cwd())
}

pub(crate) async fn register_mcp_record_with_registry(
    record: &McpServerConfigRecord,
    registry: &McpRegistry,
    default_session_id: SessionId,
    default_agent_id: AgentId,
    diagnostic_store: Arc<dyn McpDiagnosticStore>,
    authorization_service: Arc<harness_execution::AuthorizationService>,
    workspace_root: &Path,
    interactivity: InteractivityLevel,
    allow_config_error_as_failed: bool,
) -> Result<McpServerId, CommandErrorPayload> {
    ensure_mcp_server_record_identity(record)?;

    let mut config_error = if allow_config_error_as_failed {
        ensure_mcp_server_record(record)
            .err()
            .map(|error| error.message)
    } else {
        ensure_mcp_server_record(record)?;
        None
    };
    let spec = if config_error.is_some() {
        mcp_server_spec_from_record_for_failed_registration(record, workspace_root)
    } else {
        match mcp_server_spec_from_record(record, workspace_root) {
            Ok(spec) => spec,
            Err(error) if allow_config_error_as_failed => {
                config_error = Some(error.message);
                mcp_server_spec_from_record_for_failed_registration(record, workspace_root)
            }
            Err(error) => return Err(error),
        }
    };
    let scope = match mcp_server_scope_from_record(record, default_session_id, default_agent_id) {
        Ok(scope) => scope,
        Err(_) if config_error.is_some() => McpServerScope::Global,
        Err(error) => return Err(error),
    };
    let server_id = spec.server_id.clone();

    if let Some(error) = config_error {
        registry
            .add_failed_server(spec, scope, error)
            .await
            .map_err(|error| runtime_operation_failed(error.to_string()))?;
        return Ok(server_id);
    }

    let transport = mcp_transport_for_config(&record.transport);
    let event_sink = Arc::new(DesktopMcpEventSink { diagnostic_store });
    let connect_context =
        McpConnectContext::default().with_authorization(mcp_authorization_context(
            Arc::clone(&authorization_service),
            &scope,
            default_session_id,
            workspace_root,
            interactivity,
        )?);
    match registry
        .add_managed_server_with_context(
            spec.clone(),
            scope.clone(),
            transport,
            event_sink,
            connect_context,
        )
        .await
    {
        Ok(()) => {}
        Err(error) => {
            registry
                .add_failed_server(spec, scope, error.to_string())
                .await
                .map_err(|error| runtime_operation_failed(error.to_string()))?;
        }
    }

    Ok(server_id)
}

fn mcp_server_spec_from_record_for_failed_registration(
    record: &McpServerConfigRecord,
    workspace_root: &Path,
) -> McpServerSpec {
    let transport = match &record.transport {
        McpServerTransportConfig::Stdio { command, args, .. } => {
            let mut policy = StdioPolicy::default();
            policy.working_dir = Some(workspace_root.to_path_buf());
            TransportChoice::Stdio {
                command: command.clone(),
                args: args.clone(),
                env: StdioEnv::Empty {
                    extra: BTreeMap::new(),
                },
                policy,
            }
        }
        McpServerTransportConfig::Http { url, .. } => TransportChoice::Http {
            url: url.clone(),
            headers: BTreeMap::new(),
        },
        McpServerTransportConfig::InProcess => TransportChoice::InProcess,
    };

    McpServerSpec::new(
        McpServerId(record.id.clone()),
        record.display_name.clone(),
        transport,
        McpServerSource::Workspace,
    )
}

fn mcp_authorization_context(
    authorization_service: Arc<harness_execution::AuthorizationService>,
    scope: &McpServerScope,
    default_session_id: SessionId,
    workspace_root: &Path,
    interactivity: InteractivityLevel,
) -> Result<McpAuthorizationContext, CommandErrorPayload> {
    let session_id = match scope {
        McpServerScope::Session(session_id) => *session_id,
        McpServerScope::Global | McpServerScope::Agent(_) => SessionId::new(),
        _ => {
            return Err(runtime_operation_failed(
                "unsupported mcp server scope".to_owned(),
            ));
        }
    };
    let _ = default_session_id;

    Ok(McpAuthorizationContext {
        authorization_service,
        tenant_id: TenantId::SINGLE,
        scope: scope.clone(),
        session_id,
        run_id: RunId::new(),
        permission_mode: PermissionMode::Default,
        interactivity,
        fallback_policy: FallbackPolicy::AskUser,
        workspace_root: workspace_root.to_path_buf(),
    })
}

pub(crate) async fn remove_mcp_server_from_settings_runtime(
    settings_runtime: &DesktopSettingsRuntime,
    id: &str,
) -> Result<(), CommandErrorPayload> {
    let Some(config) = settings_runtime.mcp_config() else {
        return Ok(());
    };
    let server_id = McpServerId(id.to_owned());
    if let Some(tool_names) = config.registry.injected_tool_names(&server_id).await {
        for tool_name in tool_names {
            if settings_runtime.tool_registry().get(&tool_name).is_some() {
                settings_runtime
                    .tool_registry()
                    .deregister(&tool_name)
                    .map_err(|error| runtime_operation_failed(error.to_string()))?;
            }
        }
    }
    match config.registry.remove_server(&server_id).await {
        Ok(()) | Err(jyowo_harness_sdk::ext::McpError::ServerNotFound(_)) => Ok(()),
        Err(_) => {
            // The registry removes the entry before attempting shutdown. A
            // bounded shutdown failure must not leave disable/restart stuck
            // after the runtime state has already been removed.
            log::warn!("MCP registry entry removed with an incomplete connection shutdown");
            Ok(())
        }
    }
}

pub(crate) fn mcp_server_spec_from_record(
    record: &McpServerConfigRecord,
    workspace_root: &Path,
) -> Result<McpServerSpec, CommandErrorPayload> {
    match &record.transport {
        McpServerTransportConfig::Stdio {
            command,
            args,
            env,
            inherit_env,
            working_dir,
        } => {
            let mut policy = StdioPolicy::default();
            policy.working_dir = Some(mcp_stdio_working_dir(
                working_dir.as_deref(),
                workspace_root,
            )?);
            Ok(McpServerSpec::new(
                McpServerId(record.id.clone()),
                record.display_name.clone(),
                TransportChoice::Stdio {
                    command: command.clone(),
                    args: args.clone(),
                    env: mcp_stdio_env(command, env, inherit_env),
                    policy,
                },
                McpServerSource::Workspace,
            ))
        }
        McpServerTransportConfig::Http {
            url,
            bearer_token_env_var,
            headers,
            headers_from_env,
        } => Ok(McpServerSpec::new(
            McpServerId(record.id.clone()),
            record.display_name.clone(),
            TransportChoice::Http {
                url: url.clone(),
                headers: mcp_http_headers(
                    headers,
                    headers_from_env,
                    bearer_token_env_var.as_deref(),
                )?,
            },
            McpServerSource::Workspace,
        )),
        McpServerTransportConfig::InProcess => Err(invalid_payload(
            "transport.kind must be stdio or http for workspace MCP servers".to_owned(),
        )),
    }
}

pub(crate) fn mcp_transport_for_config(
    transport: &McpServerTransportConfig,
) -> Arc<dyn jyowo_harness_sdk::ext::McpTransport> {
    match transport {
        McpServerTransportConfig::Http { .. } => Arc::new(HttpTransport::new()),
        McpServerTransportConfig::Stdio { .. } | McpServerTransportConfig::InProcess => {
            Arc::new(StdioTransport::new())
        }
    }
}

pub(crate) fn mcp_stdio_env(
    command: &str,
    env: &[McpNameValueRecord],
    inherit_env: &[String],
) -> StdioEnv {
    let extra = env
        .iter()
        .map(|record| (record.key.clone(), record.value.clone()))
        .collect::<BTreeMap<_, _>>();
    let inherit_env = mcp_effective_stdio_inherit_env(command, inherit_env);
    if inherit_env.is_empty() {
        StdioEnv::Empty { extra }
    } else {
        StdioEnv::Allowlist {
            inherit: inherit_env.into_iter().collect::<BTreeSet<_>>(),
            extra,
        }
    }
}

pub(crate) fn mcp_effective_stdio_inherit_env(
    command: &str,
    inherit_env: &[String],
) -> Vec<String> {
    if !inherit_env.is_empty() || !mcp_stdio_command_needs_execution_env(command) {
        return inherit_env.to_vec();
    }
    browser_mcp_preset_inherit_env()
}

fn mcp_stdio_command_needs_execution_env(command: &str) -> bool {
    let command_name = Path::new(command)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(command);
    matches!(command_name, "npx" | "npm" | "pnpm" | "yarn" | "bun")
}

pub(crate) fn mcp_stdio_working_dir(
    working_dir: Option<&str>,
    workspace_root: &Path,
) -> Result<PathBuf, CommandErrorPayload> {
    let Some(working_dir) = working_dir else {
        return Ok(workspace_root.to_path_buf());
    };
    ensure_non_empty("transport.workingDir", working_dir)?;
    let candidate = PathBuf::from(working_dir);
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        workspace_root.join(candidate)
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|error| invalid_payload(format!("transport.workingDir is invalid: {error}")))?;
    if !canonical.starts_with(workspace_root) {
        return Err(invalid_payload(
            "transport.workingDir must stay inside the workspace".to_owned(),
        ));
    }
    Ok(canonical)
}

pub(crate) fn mcp_http_headers(
    headers: &[McpNameValueRecord],
    headers_from_env: &[McpHeaderEnvRecord],
    bearer_token_env_var: Option<&str>,
) -> Result<BTreeMap<String, String>, CommandErrorPayload> {
    let mut resolved = BTreeMap::new();
    for header in headers {
        resolved.insert(header.key.trim().to_owned(), header.value.clone());
    }
    for header in headers_from_env {
        let value = std::env::var(&header.env_var).map_err(|_| {
            runtime_operation_failed(format!(
                "MCP header env var is unavailable: {}",
                header.env_var
            ))
        })?;
        resolved.insert(header.key.trim().to_owned(), value);
    }
    if let Some(env_var) = bearer_token_env_var {
        let token = std::env::var(env_var).map_err(|_| {
            runtime_operation_failed(format!(
                "MCP bearer token env var is unavailable: {env_var}"
            ))
        })?;
        resolved.insert("Authorization".to_owned(), format!("Bearer {token}"));
    }
    Ok(resolved)
}

pub(crate) fn mcp_server_scope_from_record(
    record: &McpServerConfigRecord,
    default_session_id: SessionId,
    default_agent_id: AgentId,
) -> Result<McpServerScope, CommandErrorPayload> {
    match record.scope.as_str() {
        "global" => Ok(McpServerScope::Global),
        "session" => Ok(McpServerScope::Session(default_session_id)),
        "agent" => Ok(McpServerScope::Agent(default_agent_id)),
        _ => Err(invalid_payload(
            "scope must be global, session, or agent".to_owned(),
        )),
    }
}

pub(crate) struct DesktopMcpEventSink {
    diagnostic_store: Arc<dyn McpDiagnosticStore>,
}

impl McpEventSink for DesktopMcpEventSink {
    fn emit(&self, event: Event) {
        if let Some(record) = mcp_diagnostic_record_from_event(event) {
            let _ = self.diagnostic_store.append_record(&record);
        }
    }
}

pub fn mcp_diagnostic_record_from_event(event: Event) -> Option<McpDiagnosticRecord> {
    let (server_id, event_type, severity, summary, timestamp) = match event {
        Event::McpToolInjected(event) => (
            event.server_id.0,
            "tool_injected",
            McpDiagnosticSeverity::Info,
            "MCP tool exposed.",
            event.at.to_rfc3339(),
        ),
        Event::McpConnectionLost(event) => (
            event.server_id.0,
            "connection_lost",
            if event.terminal {
                McpDiagnosticSeverity::Error
            } else {
                McpDiagnosticSeverity::Warning
            },
            if event.terminal {
                "MCP server connection lost."
            } else {
                "MCP server connection lost; reconnecting."
            },
            event.at.to_rfc3339(),
        ),
        Event::McpConnectionRecovered(event) => (
            event.server_id.0,
            "connection_recovered",
            McpDiagnosticSeverity::Info,
            "MCP server connection recovered.",
            event.at.to_rfc3339(),
        ),
        Event::McpOAuthRefresh(event) => (
            event.server_id.0,
            "oauth_refresh",
            match event.outcome {
                harness_contracts::McpOAuthRefreshOutcome::Error => McpDiagnosticSeverity::Error,
                _ => McpDiagnosticSeverity::Info,
            },
            match event.outcome {
                harness_contracts::McpOAuthRefreshOutcome::Started => "MCP OAuth refresh started.",
                harness_contracts::McpOAuthRefreshOutcome::Success => {
                    "MCP OAuth refresh completed."
                }
                harness_contracts::McpOAuthRefreshOutcome::Error => "MCP OAuth refresh failed.",
            },
            event.at.to_rfc3339(),
        ),
        Event::McpElicitationRequested(event) => (
            event.server_id.0,
            "elicitation_requested",
            McpDiagnosticSeverity::Info,
            "MCP elicitation requested.",
            event.at.to_rfc3339(),
        ),
        Event::McpElicitationResolved(event) => (
            event.server_id.0,
            "elicitation_resolved",
            match event.outcome {
                harness_contracts::ElicitationOutcome::Provided { .. } => {
                    McpDiagnosticSeverity::Info
                }
                _ => McpDiagnosticSeverity::Warning,
            },
            "MCP elicitation resolved.",
            event.at.to_rfc3339(),
        ),
        Event::McpToolsListChanged(event) => (
            event.server_id.0,
            "tools_changed",
            McpDiagnosticSeverity::Info,
            "MCP tools changed.",
            event.received_at.to_rfc3339(),
        ),
        Event::McpResourceUpdated(event) => (
            event.server_id.0,
            "resource_updated",
            McpDiagnosticSeverity::Info,
            match event.kind {
                harness_contracts::McpResourceUpdateKind::PromptsListChanged { .. } => {
                    "MCP prompts changed."
                }
                harness_contracts::McpResourceUpdateKind::ListChanged { .. } => {
                    "MCP resources changed."
                }
                harness_contracts::McpResourceUpdateKind::ResourceUpdated { .. } => {
                    "MCP resource updated."
                }
                _ => "MCP resource updated.",
            },
            event.at.to_rfc3339(),
        ),
        Event::McpSamplingRequested(event) => (
            event.server_id.0,
            "sampling",
            match event.outcome {
                harness_contracts::SamplingOutcome::Completed => McpDiagnosticSeverity::Info,
                harness_contracts::SamplingOutcome::UpstreamError { .. } => {
                    McpDiagnosticSeverity::Error
                }
                _ => McpDiagnosticSeverity::Warning,
            },
            "MCP sampling request handled.",
            event.at.to_rfc3339(),
        ),
        _ => return None,
    };

    Some(McpDiagnosticRecord {
        event_type: event_type.to_owned(),
        id: format!("mcp-diagnostic-{}", EventId::new()),
        server_id,
        severity,
        summary: summary.to_owned(),
        timestamp,
    })
}

pub(crate) async fn mcp_server_summary_from_registry(
    registry: &jyowo_harness_sdk::ext::McpRegistry,
    server_id: &McpServerId,
) -> Option<McpServerSummaryPayload> {
    let spec = registry.server_spec(server_id).await?;
    let scope = registry.server_scope(server_id).await?;
    let connection_state = registry.connection_state(server_id).await?;
    let exposed_tool_count = registry.injected_tool_count(server_id).await.unwrap_or(0);
    let (status, last_error) = mcp_connection_state_payload(&connection_state);

    Some(McpServerSummaryPayload {
        display_name: spec.display_name,
        enabled: true,
        exposed_tool_count: exposed_tool_count.try_into().unwrap_or(u32::MAX),
        id: server_id.0.clone(),
        last_diagnostic: None,
        last_diagnostic_at: None,
        last_diagnostic_severity: None,
        last_error,
        manageable: false,
        origin: mcp_server_origin_payload(&spec.source),
        scope: mcp_server_scope_payload(&scope),
        source_plugin_id: mcp_source_plugin_id(&spec.source),
        status,
        transport: mcp_transport_payload(&spec.transport),
    })
}

pub(crate) fn mcp_server_summary_from_record(
    record: &McpServerConfigRecord,
) -> McpServerSummaryPayload {
    McpServerSummaryPayload {
        display_name: record.display_name.clone(),
        enabled: record.enabled,
        exposed_tool_count: 0,
        id: record.id.clone(),
        last_diagnostic: None,
        last_diagnostic_at: None,
        last_diagnostic_severity: None,
        last_error: None,
        manageable: true,
        origin: "workspace",
        scope: record.scope.clone(),
        status: if record.enabled {
            "configured"
        } else {
            "disabled"
        },
        source_plugin_id: None,
        transport: mcp_transport_config_payload(&record.transport),
    }
}

pub(crate) fn browser_mcp_preset_ids() -> &'static [BrowserMcpPresetId; 2] {
    &[
        BrowserMcpPresetId::Playwright,
        BrowserMcpPresetId::ChromeDevtools,
    ]
}

pub(crate) fn browser_mcp_preset_summary(
    preset_id: BrowserMcpPresetId,
    records: &[McpServerConfigRecord],
) -> BrowserMcpPresetSummaryPayload {
    let enabled = records
        .iter()
        .find(|record| record.id == browser_mcp_preset_server_id(preset_id))
        .is_some_and(|record| record.enabled);
    browser_mcp_preset_summary_from_enabled(preset_id, enabled)
}

pub(crate) fn browser_mcp_preset_summary_from_enabled(
    preset_id: BrowserMcpPresetId,
    enabled: bool,
) -> BrowserMcpPresetSummaryPayload {
    BrowserMcpPresetSummaryPayload {
        description: browser_mcp_preset_description(preset_id),
        display_name: browser_mcp_preset_display_name(preset_id),
        enabled,
        id: preset_id,
        server_id: browser_mcp_preset_server_id(preset_id),
    }
}

fn mcp_server_config_payload_from_record(record: &McpServerConfigRecord) -> McpServerConfigPayload {
    McpServerConfigPayload {
        enabled: record.enabled,
        display_name: record.display_name.clone(),
        id: record.id.clone(),
        scope: record.scope.clone(),
        transport: mcp_server_config_transport_payload(&record.transport),
    }
}

fn mcp_server_config_transport_payload(
    transport: &McpServerTransportConfig,
) -> McpServerConfigTransportPayload {
    match transport {
        McpServerTransportConfig::Stdio {
            command,
            args,
            env,
            inherit_env,
            working_dir,
        } => McpServerConfigTransportPayload::Stdio {
            command: command.clone(),
            args: args.clone(),
            env: env.iter().map(mcp_name_value_config_payload).collect(),
            inherit_env: inherit_env.clone(),
            working_dir: working_dir.clone(),
        },
        McpServerTransportConfig::Http {
            url,
            bearer_token_env_var,
            headers,
            headers_from_env,
        } => McpServerConfigTransportPayload::Http {
            url: url.clone(),
            bearer_token_env_var: bearer_token_env_var.clone(),
            headers: headers.iter().map(mcp_name_value_config_payload).collect(),
            headers_from_env: headers_from_env.clone(),
        },
        McpServerTransportConfig::InProcess => McpServerConfigTransportPayload::InProcess,
    }
}

fn mcp_name_value_config_payload(record: &McpNameValueRecord) -> McpNameValueConfigPayload {
    McpNameValueConfigPayload {
        has_value: !record.value.is_empty(),
        key: record.key.clone(),
        value: None,
    }
}

pub(crate) fn browser_mcp_preset_record(
    preset_id: BrowserMcpPresetId,
    enabled: bool,
) -> McpServerConfigRecord {
    McpServerConfigRecord {
        enabled,
        display_name: browser_mcp_preset_display_name(preset_id).to_owned(),
        id: browser_mcp_preset_server_id(preset_id).to_owned(),
        scope: "global".to_owned(),
        transport: McpServerTransportConfig::Stdio {
            command: "npx".to_owned(),
            args: vec![
                "-y".to_owned(),
                browser_mcp_preset_package_arg(preset_id).to_owned(),
            ],
            env: Vec::new(),
            inherit_env: browser_mcp_preset_inherit_env(),
            working_dir: None,
        },
    }
}

pub(crate) fn browser_mcp_preset_inherit_env() -> Vec<String> {
    ["PATH", "HOME", "USER", "TMPDIR"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

pub(crate) fn browser_mcp_preset_server_id(preset_id: BrowserMcpPresetId) -> &'static str {
    match preset_id {
        BrowserMcpPresetId::Playwright => "browser-playwright",
        BrowserMcpPresetId::ChromeDevtools => "browser-chrome-devtools",
    }
}

pub(crate) fn browser_mcp_preset_display_name(preset_id: BrowserMcpPresetId) -> &'static str {
    match preset_id {
        BrowserMcpPresetId::Playwright => "Playwright Browser",
        BrowserMcpPresetId::ChromeDevtools => "Chrome DevTools Browser",
    }
}

pub(crate) fn browser_mcp_preset_description(preset_id: BrowserMcpPresetId) -> &'static str {
    match preset_id {
        BrowserMcpPresetId::Playwright => "Browser automation through Playwright MCP.",
        BrowserMcpPresetId::ChromeDevtools => "Browser inspection through Chrome DevTools MCP.",
    }
}

pub(crate) fn browser_mcp_preset_package_arg(preset_id: BrowserMcpPresetId) -> &'static str {
    match preset_id {
        BrowserMcpPresetId::Playwright => "@playwright/mcp@latest",
        BrowserMcpPresetId::ChromeDevtools => "chrome-devtools-mcp@latest",
    }
}

pub(crate) fn mcp_last_diagnostics_by_server(
    records: &[McpDiagnosticRecord],
) -> BTreeMap<String, McpDiagnosticRecord> {
    let mut last = BTreeMap::new();
    for record in records {
        last.insert(record.server_id.clone(), record.clone());
    }
    last
}

pub(crate) fn apply_mcp_last_diagnostic(
    summary: &mut McpServerSummaryPayload,
    diagnostic: Option<&McpDiagnosticRecord>,
) {
    if let Some(diagnostic) = diagnostic {
        summary.last_diagnostic = Some(diagnostic.summary.clone());
        summary.last_diagnostic_at = Some(diagnostic.timestamp.clone());
        summary.last_diagnostic_severity = Some(diagnostic.severity);
    }
}
