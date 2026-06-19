use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use futures::StreamExt;
use harness_contracts::{
    canonical_mcp_tool_name, now, DeferPolicy, DeferredToolHint, Event, McpResourceUpdateKind,
    McpResourceUpdatedEvent, McpServerId, McpServerSource, McpToolsListChangedEvent, PluginId,
    ToolDeferredPoolChangedEvent, ToolPoolChangeSource, ToolsListChangedDisposition, TrustLevel,
    UnexpectedErrorEvent,
};
use harness_tool::ToolRegistry;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{
    trust_level_for_source, FilterDecision, ManagedMcpConnection, McpChange, McpConnection,
    McpConnectionState, McpError, McpEventSink, McpMetric, McpMetricsSink, McpServerPattern,
    McpServerRef, McpServerScope, McpServerSpec, McpToolDescriptor, McpToolResult, McpToolWrapper,
    McpTransport, NoopMcpMetricsSink, RequiredEvaluation,
};

#[derive(Clone)]
pub struct McpRegistry {
    inner: Arc<RwLock<BTreeMap<McpServerId, ManagedMcpServer>>>,
    metrics_sink: Arc<dyn McpMetricsSink>,
}

impl Default for McpRegistry {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(BTreeMap::new())),
            metrics_sink: Arc::new(NoopMcpMetricsSink),
        }
    }
}

#[derive(Clone)]
pub struct ManagedMcpServer {
    pub spec: McpServerSpec,
    pub scope: McpServerScope,
    pub connection: Arc<dyn McpConnection>,
    pub connection_state: McpConnectionState,
    pub injected_tools: BTreeMap<String, DeferPolicy>,
    pub known_resources: BTreeSet<String>,
    pub resource_observers: BTreeMap<String, ResourceObservationState>,
    pub known_prompts: BTreeSet<String>,
    pub pending_list_changed: bool,
    pub last_list_changed: Option<DateTime<Utc>>,
    pub schema_fingerprint: Option<McpSchemaFingerprint>,
}

pub type McpSchemaFingerprint = [u8; 32];

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceObservationState {
    pub subscribed_at: DateTime<Utc>,
    pub last_update: DateTime<Utc>,
    pub window_started_at: DateTime<Utc>,
    pub updates_in_window: u32,
    pub downgraded: bool,
}

pub type ListChangedDisposition = ToolsListChangedDisposition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListChangedOutcome {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub disposition: ListChangedDisposition,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_metrics_sink(metrics_sink: Arc<dyn McpMetricsSink>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(BTreeMap::new())),
            metrics_sink,
        }
    }

    pub fn clone_with_metrics_sink(&self, metrics_sink: Arc<dyn McpMetricsSink>) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            metrics_sink,
        }
    }

    pub async fn add_ready_server(
        &self,
        spec: McpServerSpec,
        scope: McpServerScope,
        connection: Arc<dyn McpConnection>,
    ) -> Result<(), McpError> {
        let derived = trust_level_for_source(&spec.source);
        if spec.trust != derived {
            return Err(McpError::Protocol(format!(
                "trust mismatch for {}: expected {:?}, got {:?}",
                spec.server_id.0, derived, spec.trust
            )));
        }

        self.inner.write().await.insert(
            spec.server_id.clone(),
            ManagedMcpServer {
                spec,
                scope,
                connection,
                connection_state: McpConnectionState::Ready,
                injected_tools: BTreeMap::new(),
                known_resources: BTreeSet::new(),
                resource_observers: BTreeMap::new(),
                known_prompts: BTreeSet::new(),
                pending_list_changed: false,
                last_list_changed: None,
                schema_fingerprint: None,
            },
        );
        Ok(())
    }

    pub async fn add_managed_server(
        &self,
        spec: McpServerSpec,
        scope: McpServerScope,
        transport: Arc<dyn McpTransport>,
        event_sink: Arc<dyn McpEventSink>,
    ) -> Result<(), McpError> {
        let derived = trust_level_for_source(&spec.source);
        if spec.trust != derived {
            return Err(McpError::Protocol(format!(
                "trust mismatch for {}: expected {:?}, got {:?}",
                spec.server_id.0, derived, spec.trust
            )));
        }

        let connection = Arc::new(
            ManagedMcpConnection::connect_with_metrics(
                transport,
                spec.clone(),
                scope.clone(),
                event_sink,
                Arc::clone(&self.metrics_sink),
            )
            .await?,
        );
        self.inner.write().await.insert(
            spec.server_id.clone(),
            ManagedMcpServer {
                spec,
                scope,
                connection,
                connection_state: McpConnectionState::Ready,
                injected_tools: BTreeMap::new(),
                known_resources: BTreeSet::new(),
                resource_observers: BTreeMap::new(),
                known_prompts: BTreeSet::new(),
                pending_list_changed: false,
                last_list_changed: None,
                schema_fingerprint: None,
            },
        );
        Ok(())
    }

    pub async fn add_failed_server(
        &self,
        spec: McpServerSpec,
        scope: McpServerScope,
        last_error: String,
    ) -> Result<(), McpError> {
        let derived = trust_level_for_source(&spec.source);
        if spec.trust != derived {
            return Err(McpError::Protocol(format!(
                "trust mismatch for {}: expected {:?}, got {:?}",
                spec.server_id.0, derived, spec.trust
            )));
        }

        self.inner.write().await.insert(
            spec.server_id.clone(),
            ManagedMcpServer {
                spec,
                scope,
                connection: Arc::new(FailedMcpConnection),
                connection_state: McpConnectionState::Failed { last_error },
                injected_tools: BTreeMap::new(),
                known_resources: BTreeSet::new(),
                resource_observers: BTreeMap::new(),
                known_prompts: BTreeSet::new(),
                pending_list_changed: false,
                last_list_changed: None,
                schema_fingerprint: None,
            },
        );
        Ok(())
    }

    pub async fn add_plugin_server(
        &self,
        plugin_id: PluginId,
        plugin_trust: TrustLevel,
        mut spec: McpServerSpec,
    ) -> Result<(), McpError> {
        spec.source = McpServerSource::Plugin(plugin_id);
        spec.trust = plugin_trust;
        self.inner.write().await.insert(
            spec.server_id.clone(),
            ManagedMcpServer {
                spec,
                scope: McpServerScope::Global,
                connection: Arc::new(RegisteredPluginMcpConnection),
                connection_state: McpConnectionState::Ready,
                injected_tools: BTreeMap::new(),
                known_resources: BTreeSet::new(),
                resource_observers: BTreeMap::new(),
                known_prompts: BTreeSet::new(),
                pending_list_changed: false,
                last_list_changed: None,
                schema_fingerprint: None,
            },
        );
        Ok(())
    }

    pub async fn subscribe_list_changed(
        &self,
        tool_registry: ToolRegistry,
        server_id: McpServerId,
        event_sink: Arc<dyn McpEventSink>,
    ) -> Result<(), McpError> {
        let connection = self
            .inner
            .read()
            .await
            .get(&server_id)
            .map(|managed| Arc::clone(&managed.connection))
            .ok_or_else(|| McpError::ServerNotFound(server_id.0.clone()))?;
        let mut changes = connection.subscribe_changes().await?;
        let registry = self.clone();
        tokio::spawn(async move {
            while let Some(change) = changes.next().await {
                let result = match change {
                    McpChange::ToolsListChanged => registry
                        .handle_list_changed(&tool_registry, &server_id, event_sink.clone())
                        .await
                        .map(|_| ()),
                    McpChange::ResourcesListChanged => {
                        registry
                            .handle_resources_list_changed(&server_id, event_sink.clone())
                            .await
                    }
                    McpChange::ResourceUpdated { uri } => {
                        registry
                            .handle_resource_updated(&server_id, uri, event_sink.clone())
                            .await
                    }
                    McpChange::PromptsListChanged => {
                        registry
                            .handle_prompts_list_changed(&server_id, event_sink.clone())
                            .await
                    }
                    McpChange::Cancelled { .. } | McpChange::Progress { .. } => Ok(()),
                };
                if let Err(error) = result {
                    event_sink.emit(Event::UnexpectedError(UnexpectedErrorEvent {
                        session_id: None,
                        run_id: None,
                        error: format!(
                            "mcp list_changed handling failed for {}: {error}",
                            server_id.0
                        ),
                        at: now(),
                    }));
                }
            }
        });
        Ok(())
    }

    pub async fn server_spec(&self, server_id: &McpServerId) -> Option<McpServerSpec> {
        self.inner
            .read()
            .await
            .get(server_id)
            .map(|managed| managed.spec.clone())
    }

    pub async fn server_ids(&self) -> Vec<McpServerId> {
        self.inner.read().await.keys().cloned().collect()
    }

    pub async fn connection_state(&self, server_id: &McpServerId) -> Option<McpConnectionState> {
        self.inner
            .read()
            .await
            .get(server_id)
            .map(|managed| managed.connection_state.clone())
    }

    pub async fn server_scope(&self, server_id: &McpServerId) -> Option<McpServerScope> {
        self.inner
            .read()
            .await
            .get(server_id)
            .map(|managed| managed.scope.clone())
    }

    pub async fn injected_tool_count(&self, server_id: &McpServerId) -> Option<usize> {
        self.inner
            .read()
            .await
            .get(server_id)
            .map(|managed| managed.injected_tools.len())
    }

    pub async fn injected_tool_names(&self, server_id: &McpServerId) -> Option<Vec<String>> {
        self.inner
            .read()
            .await
            .get(server_id)
            .map(|managed| managed.injected_tools.keys().cloned().collect())
    }

    pub async fn last_list_changed(&self, server_id: &McpServerId) -> Option<DateTime<Utc>> {
        self.inner
            .read()
            .await
            .get(server_id)
            .and_then(|managed| managed.last_list_changed)
    }

    pub async fn schema_fingerprint(
        &self,
        server_id: &McpServerId,
    ) -> Option<McpSchemaFingerprint> {
        self.inner
            .read()
            .await
            .get(server_id)
            .and_then(|managed| managed.schema_fingerprint)
    }

    pub async fn set_connection_state(
        &self,
        server_id: &McpServerId,
        state: McpConnectionState,
    ) -> Result<(), McpError> {
        self.inner
            .write()
            .await
            .get_mut(server_id)
            .ok_or_else(|| McpError::ServerNotFound(server_id.0.clone()))?
            .connection_state = state;
        Ok(())
    }

    pub async fn evaluate_required(
        &self,
        refs: &[McpServerRef],
        required: &[McpServerPattern],
    ) -> Vec<RequiredEvaluation> {
        let managed = self.inner.read().await.clone();
        required
            .iter()
            .map(|pattern| evaluate_required_pattern(&managed, refs, pattern))
            .collect()
    }

    pub async fn remove_server(&self, server_id: &McpServerId) -> Result<(), McpError> {
        self.inner
            .write()
            .await
            .remove(server_id)
            .ok_or_else(|| McpError::ServerNotFound(server_id.0.clone()))?;
        Ok(())
    }

    pub async fn inject_tools_into(
        &self,
        tool_registry: &ToolRegistry,
        server_id: &McpServerId,
    ) -> Result<Vec<String>, McpError> {
        let managed = self
            .inner
            .read()
            .await
            .get(server_id)
            .cloned()
            .ok_or_else(|| McpError::ServerNotFound(server_id.0.clone()))?;

        let latest = self.snapshot_for_latest_tools(
            &managed,
            server_id,
            managed.connection.list_tools().await?,
        )?;
        let mut registered = Vec::new();
        let injected_snapshot = latest
            .iter()
            .map(|(name, (_, policy))| (name.clone(), *policy))
            .collect::<BTreeMap<_, _>>();
        let schema_fingerprint = schema_fingerprint_from_snapshot(&latest)?;

        for (canonical, (mcp_tool, defer_policy)) in latest {
            let tool = McpToolWrapper::new_with_metrics_and_cancel_ack_timeout(
                server_id.clone(),
                managed.spec.source.clone(),
                managed.spec.trust,
                mcp_tool,
                Arc::clone(&managed.connection),
                defer_policy,
                canonical.clone(),
                Arc::clone(&self.metrics_sink),
                managed.spec.timeouts.cancel_ack,
            );
            tool_registry.register(Box::new(tool))?;
            registered.push(canonical);
        }

        if let Some(managed) = self.inner.write().await.get_mut(server_id) {
            managed.injected_tools = injected_snapshot;
            managed.schema_fingerprint = Some(schema_fingerprint);
        }

        Ok(registered)
    }

    pub async fn handle_list_changed(
        &self,
        tool_registry: &ToolRegistry,
        server_id: &McpServerId,
        event_sink: Arc<dyn McpEventSink>,
    ) -> Result<ListChangedOutcome, McpError> {
        let managed = self
            .inner
            .read()
            .await
            .get(server_id)
            .cloned()
            .ok_or_else(|| McpError::ServerNotFound(server_id.0.clone()))?;
        let latest = self.snapshot_for_latest_tools(
            &managed,
            server_id,
            managed.connection.list_tools().await?,
        )?;
        let latest_policies = latest
            .iter()
            .map(|(name, (_, policy))| (name.clone(), *policy))
            .collect::<BTreeMap<_, _>>();
        let latest_fingerprint = schema_fingerprint_from_snapshot(&latest)?;
        let schema_changed = managed
            .schema_fingerprint
            .is_some_and(|fingerprint| fingerprint != latest_fingerprint);
        let added = latest
            .keys()
            .filter(|name| !managed.injected_tools.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();
        let removed = managed
            .injected_tools
            .keys()
            .filter(|name| !latest.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();

        if added.is_empty() && removed.is_empty() && !schema_changed {
            if let Some(managed) = self.inner.write().await.get_mut(server_id) {
                managed.last_list_changed = Some(now());
            }
            let outcome = ListChangedOutcome {
                added,
                removed,
                disposition: ToolsListChangedDisposition::NoChange,
            };
            emit_tools_list_changed(&managed, server_id, &outcome, &event_sink);
            self.record_list_changed(server_id, &outcome);
            return Ok(outcome);
        }

        let has_always_load_delta = added.iter().chain(removed.iter()).any(|name| {
            latest_policies
                .get(name)
                .or_else(|| managed.injected_tools.get(name))
                == Some(&DeferPolicy::AlwaysLoad)
        }) || (schema_changed
            && latest_policies
                .values()
                .chain(managed.injected_tools.values())
                .any(|policy| *policy == DeferPolicy::AlwaysLoad));

        if has_always_load_delta {
            if let Some(managed) = self.inner.write().await.get_mut(server_id) {
                managed.pending_list_changed = true;
                managed.last_list_changed = Some(now());
                managed.schema_fingerprint = Some(latest_fingerprint);
            }
            let outcome = ListChangedOutcome {
                added,
                removed,
                disposition: ToolsListChangedDisposition::PendingForReload,
            };
            emit_tools_list_changed(&managed, server_id, &outcome, &event_sink);
            self.record_list_changed(server_id, &outcome);
            return Ok(outcome);
        }

        for name in &removed {
            if tool_registry.get(name).is_some() {
                tool_registry.deregister(name)?;
            }
        }
        let names_to_register = if schema_changed && added.is_empty() && removed.is_empty() {
            latest.keys().cloned().collect::<Vec<_>>()
        } else {
            added.clone()
        };
        if schema_changed && added.is_empty() && removed.is_empty() {
            for name in &names_to_register {
                if tool_registry.get(name).is_some() {
                    tool_registry.deregister(name)?;
                }
            }
        }
        for name in &names_to_register {
            let (mcp_tool, defer_policy) = latest
                .get(name)
                .cloned()
                .ok_or_else(|| McpError::Protocol(format!("missing added tool: {name}")))?;
            let tool = McpToolWrapper::new_with_metrics_and_cancel_ack_timeout(
                server_id.clone(),
                managed.spec.source.clone(),
                managed.spec.trust,
                mcp_tool,
                Arc::clone(&managed.connection),
                defer_policy,
                name.clone(),
                Arc::clone(&self.metrics_sink),
                managed.spec.timeouts.cancel_ack,
            );
            tool_registry.register(Box::new(tool))?;
        }
        if let Some(managed) = self.inner.write().await.get_mut(server_id) {
            managed.injected_tools = latest_policies;
            managed.pending_list_changed = false;
            managed.last_list_changed = Some(now());
            managed.schema_fingerprint = Some(latest_fingerprint);
        }

        let outcome = ListChangedOutcome {
            added,
            removed,
            disposition: ToolsListChangedDisposition::DeferredApplied,
        };
        emit_tools_list_changed(&managed, server_id, &outcome, &event_sink);
        self.record_list_changed(server_id, &outcome);
        emit_deferred_pool_changed(&managed, server_id, &outcome, tool_registry, &event_sink);
        Ok(outcome)
    }

    pub async fn pending_list_changed_servers(&self) -> Vec<McpServerId> {
        self.inner
            .read()
            .await
            .iter()
            .filter_map(|(server_id, managed)| {
                managed.pending_list_changed.then_some(server_id.clone())
            })
            .collect()
    }

    pub async fn pending_mcp_servers_for_tool_search(
        &self,
        server_ids: &[McpServerId],
    ) -> Vec<McpServerId> {
        let managed = self.inner.read().await.clone();
        let mut pending = Vec::new();
        for server_id in server_ids {
            let Some(server) = managed.get(server_id) else {
                pending.push(server_id.clone());
                continue;
            };
            let connection_state = server.connection.connection_state().await;
            let lifecycle_pending = matches!(
                (&server.connection_state, &connection_state),
                (McpConnectionState::Connecting, _)
                    | (McpConnectionState::Reconnecting { .. }, _)
                    | (_, McpConnectionState::Connecting)
                    | (_, McpConnectionState::Reconnecting { .. })
            ) && server.spec.reconnect.keep_deferred_during_reconnect;
            if server.pending_list_changed || lifecycle_pending {
                pending.push(server_id.clone());
            }
        }
        pending
    }

    pub async fn handle_resources_list_changed(
        &self,
        server_id: &McpServerId,
        event_sink: Arc<dyn McpEventSink>,
    ) -> Result<(), McpError> {
        let managed = self
            .inner
            .read()
            .await
            .get(server_id)
            .cloned()
            .ok_or_else(|| McpError::ServerNotFound(server_id.0.clone()))?;
        let latest = managed
            .connection
            .list_resources()
            .await?
            .into_iter()
            .map(|resource| resource.uri)
            .collect::<BTreeSet<_>>();
        let added = latest
            .difference(&managed.known_resources)
            .count()
            .try_into()
            .unwrap_or(u32::MAX);
        let removed = managed
            .known_resources
            .difference(&latest)
            .count()
            .try_into()
            .unwrap_or(u32::MAX);

        if let Some(managed) = self.inner.write().await.get_mut(server_id) {
            managed.known_resources = latest;
        }
        let kind = McpResourceUpdateKind::ListChanged { added, removed };
        emit_resource_updated(&managed, server_id, kind.clone(), &event_sink);
        self.record_resource_updated(server_id, kind);
        Ok(())
    }

    pub async fn handle_resource_updated(
        &self,
        server_id: &McpServerId,
        uri: String,
        event_sink: Arc<dyn McpEventSink>,
    ) -> Result<(), McpError> {
        self.record_resource_update_observation(server_id, &uri, now())
            .await?;
        let managed = self
            .inner
            .read()
            .await
            .get(server_id)
            .cloned()
            .ok_or_else(|| McpError::ServerNotFound(server_id.0.clone()))?;
        let kind = McpResourceUpdateKind::ResourceUpdated { uri: uri.clone() };
        emit_resource_updated(&managed, server_id, kind.clone(), &event_sink);
        self.record_resource_updated(server_id, kind);
        self.enforce_noisy_resource_policy(server_id, &uri).await?;
        Ok(())
    }

    pub async fn subscribe_resource(
        &self,
        server_id: &McpServerId,
        uri: &str,
    ) -> Result<(), McpError> {
        self.connection_for(server_id)
            .await?
            .subscribe_resource(uri)
            .await?;
        let at = now();
        if let Some(managed) = self.inner.write().await.get_mut(server_id) {
            managed.resource_observers.insert(
                uri.to_owned(),
                ResourceObservationState {
                    subscribed_at: at,
                    last_update: at,
                    window_started_at: at,
                    updates_in_window: 0,
                    downgraded: false,
                },
            );
        }
        Ok(())
    }

    pub async fn unsubscribe_resource(
        &self,
        server_id: &McpServerId,
        uri: &str,
    ) -> Result<(), McpError> {
        self.connection_for(server_id)
            .await?
            .unsubscribe_resource(uri)
            .await?;
        if let Some(managed) = self.inner.write().await.get_mut(server_id) {
            managed.resource_observers.remove(uri);
        }
        Ok(())
    }

    async fn record_resource_update_observation(
        &self,
        server_id: &McpServerId,
        uri: &str,
        at: DateTime<Utc>,
    ) -> Result<(), McpError> {
        let mut guard = self.inner.write().await;
        let Some(managed) = guard.get_mut(server_id) else {
            return Err(McpError::ServerNotFound(server_id.0.clone()));
        };
        let Some(subscription) = managed.resource_observers.get_mut(uri) else {
            return Ok(());
        };
        let window = chrono::Duration::from_std(managed.spec.resource_update_policy.window)
            .unwrap_or_else(|_| chrono::Duration::seconds(60));
        if at.signed_duration_since(subscription.window_started_at) > window {
            subscription.window_started_at = at;
            subscription.updates_in_window = 0;
        }
        subscription.last_update = at;
        subscription.updates_in_window = subscription.updates_in_window.saturating_add(1);
        Ok(())
    }

    async fn enforce_noisy_resource_policy(
        &self,
        server_id: &McpServerId,
        uri: &str,
    ) -> Result<(), McpError> {
        let connection = {
            let mut guard = self.inner.write().await;
            let Some(managed) = guard.get_mut(server_id) else {
                return Err(McpError::ServerNotFound(server_id.0.clone()));
            };
            let threshold = managed.spec.resource_update_policy.max_updates_per_window;
            let Some(subscription) = managed.resource_observers.get_mut(uri) else {
                return Ok(());
            };
            if subscription.downgraded || subscription.updates_in_window <= threshold {
                return Ok(());
            }
            subscription.downgraded = true;
            Arc::clone(&managed.connection)
        };
        connection.unsubscribe_resource(uri).await
    }

    pub async fn enforce_resource_update_idle_at(
        &self,
        server_id: &McpServerId,
        at: DateTime<Utc>,
    ) -> Result<(), McpError> {
        let mut guard = self.inner.write().await;
        let Some(managed) = guard.get_mut(server_id) else {
            return Err(McpError::ServerNotFound(server_id.0.clone()));
        };
        let idle = chrono::Duration::from_std(managed.spec.timeouts.idle)
            .unwrap_or_else(|_| chrono::Duration::seconds(300));
        let idle_uri = managed
            .resource_observers
            .iter()
            .find(|(_, subscription)| {
                !subscription.downgraded
                    && at.signed_duration_since(subscription.last_update) > idle
            })
            .map(|(uri, _)| uri.clone());
        if let Some(uri) = idle_uri {
            managed.connection_state = McpConnectionState::Reconnecting {
                attempt: 1,
                last_error: format!("resource update stream idle for {uri}"),
            };
        }
        Ok(())
    }

    pub async fn handle_prompts_list_changed(
        &self,
        server_id: &McpServerId,
        event_sink: Arc<dyn McpEventSink>,
    ) -> Result<(), McpError> {
        let managed = self
            .inner
            .read()
            .await
            .get(server_id)
            .cloned()
            .ok_or_else(|| McpError::ServerNotFound(server_id.0.clone()))?;
        let latest = managed
            .connection
            .list_prompts()
            .await?
            .into_iter()
            .map(|prompt| prompt.name)
            .collect::<BTreeSet<_>>();
        let added = latest
            .difference(&managed.known_prompts)
            .count()
            .try_into()
            .unwrap_or(u32::MAX);
        let removed = managed
            .known_prompts
            .difference(&latest)
            .count()
            .try_into()
            .unwrap_or(u32::MAX);

        if let Some(managed) = self.inner.write().await.get_mut(server_id) {
            managed.known_prompts = latest;
        }
        let kind = McpResourceUpdateKind::PromptsListChanged { added, removed };
        emit_resource_updated(&managed, server_id, kind.clone(), &event_sink);
        self.record_resource_updated(server_id, kind);
        Ok(())
    }

    async fn connection_for(
        &self,
        server_id: &McpServerId,
    ) -> Result<Arc<dyn McpConnection>, McpError> {
        self.inner
            .read()
            .await
            .get(server_id)
            .map(|managed| Arc::clone(&managed.connection))
            .ok_or_else(|| McpError::ServerNotFound(server_id.0.clone()))
    }

    fn record_list_changed(&self, server_id: &McpServerId, outcome: &ListChangedOutcome) {
        self.metrics_sink.record(McpMetric::ListChanged {
            server_id: server_id.clone(),
            disposition: outcome.disposition.clone(),
        });
    }

    fn record_resource_updated(&self, server_id: &McpServerId, kind: McpResourceUpdateKind) {
        self.metrics_sink.record(McpMetric::ResourceUpdated {
            server_id: server_id.clone(),
            kind,
        });
    }

    fn record_tool_filter_skipped(&self, server_id: &McpServerId, reason: &'static str) {
        self.metrics_sink.record(McpMetric::ToolFilterSkipped {
            server_id: server_id.clone(),
            reason,
        });
    }

    fn snapshot_for_latest_tools(
        &self,
        managed: &ManagedMcpServer,
        server_id: &McpServerId,
        latest_tools: Vec<McpToolDescriptor>,
    ) -> Result<BTreeMap<String, (McpToolDescriptor, DeferPolicy)>, McpError> {
        let mut latest = BTreeMap::new();
        for mcp_tool in latest_tools {
            let canonical = canonical_tool_name(server_id, &mcp_tool.name)?;
            match managed.spec.tool_filter.evaluate(&canonical) {
                FilterDecision::Inject => {
                    let defer_policy = resolve_defer_policy(&mcp_tool);
                    latest.insert(canonical, (mcp_tool, defer_policy));
                }
                FilterDecision::Skip { reason } => {
                    self.record_tool_filter_skipped(server_id, filter_skip_reason_bucket(&reason));
                }
                FilterDecision::Reject { reason } => return Err(McpError::FilterConflict(reason)),
            }
        }
        Ok(latest)
    }
}

struct RegisteredPluginMcpConnection;

#[async_trait::async_trait]
impl McpConnection for RegisteredPluginMcpConnection {
    fn connection_id(&self) -> &str {
        "registered-plugin-mcp"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Ok(Vec::new())
    }

    async fn call_tool(&self, name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Err(McpError::Protocol(format!(
            "plugin MCP server {name} has no active connection"
        )))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

struct FailedMcpConnection;

#[async_trait::async_trait]
impl McpConnection for FailedMcpConnection {
    fn connection_id(&self) -> &str {
        "failed-mcp"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Err(McpError::Connection(
            "MCP server is not connected".to_owned(),
        ))
    }

    async fn call_tool(&self, name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Err(McpError::Connection(format!(
            "MCP server is not connected for tool {name}"
        )))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

pub(crate) fn effective_tool_schema_fingerprint(
    spec: &McpServerSpec,
    latest_tools: Vec<McpToolDescriptor>,
) -> Result<McpSchemaFingerprint, McpError> {
    let mut latest = BTreeMap::new();
    for mcp_tool in latest_tools {
        let canonical = canonical_tool_name(&spec.server_id, &mcp_tool.name)?;
        match spec.tool_filter.evaluate(&canonical) {
            FilterDecision::Inject => {
                let defer_policy = resolve_defer_policy(&mcp_tool);
                latest.insert(canonical, (mcp_tool, defer_policy));
            }
            FilterDecision::Skip { .. } => {}
            FilterDecision::Reject { reason } => return Err(McpError::FilterConflict(reason)),
        }
    }
    schema_fingerprint_from_snapshot(&latest)
}

fn schema_fingerprint_from_snapshot(
    snapshot: &BTreeMap<String, (McpToolDescriptor, DeferPolicy)>,
) -> Result<McpSchemaFingerprint, McpError> {
    let entries = snapshot
        .iter()
        .map(|(name, (tool, defer_policy))| SchemaFingerprintEntry {
            name,
            description: &tool.description,
            input_schema: &tool.input_schema,
            output_schema: &tool.output_schema,
            annotations: &tool.annotations,
            meta: &tool.meta,
            defer_policy: match defer_policy {
                DeferPolicy::AlwaysLoad => "always_load",
                DeferPolicy::AutoDefer => "auto_defer",
                DeferPolicy::ForceDefer => "force_defer",
                _ => "unknown",
            },
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&entries)
        .map_err(|error| McpError::Protocol(format!("failed to hash MCP schema: {error}")))?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

fn filter_skip_reason_bucket(reason: &str) -> &'static str {
    if reason.contains("allow and deny matched") && reason.contains("deny wins") {
        "conflict_deny"
    } else if reason.contains("deny glob matched") {
        "deny_matched"
    } else if reason.contains("no allow glob matched") {
        "allow_miss"
    } else {
        "other"
    }
}

#[derive(Serialize)]
struct SchemaFingerprintEntry<'a> {
    name: &'a str,
    description: &'a Option<String>,
    input_schema: &'a Value,
    output_schema: &'a Option<Value>,
    annotations: &'a Option<crate::McpToolAnnotations>,
    meta: &'a BTreeMap<String, Value>,
    defer_policy: &'static str,
}

fn emit_resource_updated(
    managed: &ManagedMcpServer,
    server_id: &McpServerId,
    kind: McpResourceUpdateKind,
    event_sink: &Arc<dyn McpEventSink>,
) {
    event_sink.emit(Event::McpResourceUpdated(McpResourceUpdatedEvent {
        session_id: session_id_for_scope(&managed.scope),
        server_id: server_id.clone(),
        kind,
        at: now(),
    }));
}

fn emit_tools_list_changed(
    managed: &ManagedMcpServer,
    server_id: &McpServerId,
    outcome: &ListChangedOutcome,
    event_sink: &Arc<dyn McpEventSink>,
) {
    event_sink.emit(Event::McpToolsListChanged(McpToolsListChangedEvent {
        session_id: session_id_for_scope(&managed.scope),
        server_id: server_id.clone(),
        received_at: now(),
        pending_since: (outcome.disposition == ToolsListChangedDisposition::PendingForReload)
            .then(now),
        added_count: outcome.added.len().try_into().unwrap_or(u32::MAX),
        removed_count: outcome.removed.len().try_into().unwrap_or(u32::MAX),
        disposition: outcome.disposition.clone(),
    }));
}

fn emit_deferred_pool_changed(
    managed: &ManagedMcpServer,
    server_id: &McpServerId,
    outcome: &ListChangedOutcome,
    tool_registry: &ToolRegistry,
    event_sink: &Arc<dyn McpEventSink>,
) {
    let McpServerScope::Session(session_id) = managed.scope else {
        return;
    };
    event_sink.emit(Event::ToolDeferredPoolChanged(
        ToolDeferredPoolChangedEvent {
            session_id,
            added: outcome
                .added
                .iter()
                .cloned()
                .map(|name| DeferredToolHint { name, hint: None })
                .collect(),
            removed: outcome.removed.clone(),
            source: ToolPoolChangeSource::McpListChanged {
                server_id: server_id.clone(),
            },
            deferred_total: tool_registry
                .snapshot()
                .as_descriptors()
                .into_iter()
                .filter(|descriptor| descriptor.properties.defer_policy == DeferPolicy::AutoDefer)
                .count()
                .try_into()
                .unwrap_or(u32::MAX),
            at: now(),
        },
    ));
}

fn session_id_for_scope(scope: &McpServerScope) -> Option<harness_contracts::SessionId> {
    match scope {
        McpServerScope::Session(session_id) => Some(*session_id),
        McpServerScope::Global | McpServerScope::Agent(_) => None,
    }
}

pub fn collapse_reserved_separator(
    server_id: &McpServerId,
    upstream: &str,
) -> Result<String, McpError> {
    let collapsed = upstream.replace("__", "_");
    canonical_mcp_tool_name(&server_id.0, &collapsed)
        .map_err(|error| McpError::ToolNamingViolation(error.to_string()))
}

fn canonical_tool_name(server_id: &McpServerId, upstream: &str) -> Result<String, McpError> {
    match canonical_mcp_tool_name(&server_id.0, upstream) {
        Ok(name) => Ok(name),
        Err(harness_contracts::ToolNameError::ReservedSeparator(_)) => {
            collapse_reserved_separator(server_id, upstream)
        }
        Err(error) => Err(McpError::ToolNamingViolation(error.to_string())),
    }
}

fn resolve_defer_policy(mcp_tool: &McpToolDescriptor) -> DeferPolicy {
    match mcp_tool.meta.get("anthropic/alwaysLoad") {
        Some(serde_json::Value::Bool(true)) => DeferPolicy::AlwaysLoad,
        _ => DeferPolicy::AutoDefer,
    }
}

fn evaluate_required_pattern(
    managed: &BTreeMap<McpServerId, ManagedMcpServer>,
    refs: &[McpServerRef],
    pattern: &McpServerPattern,
) -> RequiredEvaluation {
    for reference in refs {
        if let McpServerRef::Inline(spec) = reference {
            if pattern_matches_server(pattern, &spec.server_id) {
                if !pattern.allow_inline {
                    return RequiredEvaluation::InlineDisallowed {
                        pattern: pattern.pattern.clone(),
                        server_id: spec.server_id.clone(),
                    };
                }
                return RequiredEvaluation::Satisfied;
            }
        }
    }

    let matching = managed
        .iter()
        .filter(|(server_id, server)| pattern_matches_managed(pattern, server_id, server))
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return RequiredEvaluation::Missing {
            pattern: pattern.pattern.clone(),
        };
    }

    if !pattern.require_ready {
        return RequiredEvaluation::Satisfied;
    }

    if matching
        .iter()
        .any(|(_, server)| server.connection_state == McpConnectionState::Ready)
    {
        return RequiredEvaluation::Satisfied;
    }

    let (server_id, server) = matching[0];
    RequiredEvaluation::NotReady {
        server_id: server_id.clone(),
        state: server.connection_state.clone(),
    }
}

fn pattern_matches_managed(
    pattern: &McpServerPattern,
    server_id: &McpServerId,
    server: &ManagedMcpServer,
) -> bool {
    pattern_matches_server(pattern, server_id)
        || server
            .injected_tools
            .keys()
            .any(|tool_name| glob_matches(&pattern.pattern, tool_name))
}

fn pattern_matches_server(pattern: &McpServerPattern, server_id: &McpServerId) -> bool {
    pattern.pattern == server_id.0 || glob_matches(&pattern.pattern, &server_id.0)
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == value;
    }

    let mut remaining = value;
    let starts_with_wildcard = pattern.starts_with('*');
    let ends_with_wildcard = pattern.ends_with('*');
    let parts = pattern
        .split('*')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return true;
    }
    if !starts_with_wildcard {
        let Some(first) = parts.first() else {
            return true;
        };
        if !remaining.starts_with(first) {
            return false;
        }
        remaining = &remaining[first.len()..];
    }

    let first_index = usize::from(!starts_with_wildcard);
    let last_exclusive = if ends_with_wildcard {
        parts.len()
    } else {
        parts.len().saturating_sub(1)
    };
    for part in &parts[first_index..last_exclusive] {
        let Some(index) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[index + part.len()..];
    }

    if !ends_with_wildcard {
        let Some(last) = parts.last() else {
            return true;
        };
        return remaining.ends_with(last);
    }

    true
}
