use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{
    AgentId, ContextVisibility, Event, Message, ModelRef, SessionId, SubagentId, SubagentStatus,
    TeamId, TeamTerminationReason, TenantId, ToolUseId, UsageSnapshot,
};
use harness_journal::{
    EventStore, EventStream, Projection, ReplayCursor,
    SessionProjection as JournalSessionProjection,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::{ObservabilityError, PricingTableEntry, UsageAccumulator, UsageReport, UsageScope};

#[derive(Clone)]
pub struct ReplayEngine {
    store: Arc<dyn EventStore>,
}

impl ReplayEngine {
    #[must_use]
    pub fn new(store: Arc<dyn EventStore>) -> Self {
        Self { store }
    }

    pub async fn replay(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<EventStream, ObservabilityError> {
        Ok(self.store.read(tenant, session_id, cursor).await?)
    }

    pub async fn reconstruct_projection(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<JournalSessionProjection, ObservabilityError> {
        let envelopes = self
            .store
            .read_envelopes(tenant, session_id, cursor)
            .await?
            .collect::<Vec<_>>()
            .await;
        let last_offset = envelopes.last().map(|envelope| envelope.offset);
        let events = envelopes
            .iter()
            .map(|envelope| &envelope.payload)
            .collect::<Vec<_>>();
        let mut projection = JournalSessionProjection::replay(events)?;
        if let Some(last_offset) = last_offset {
            projection.last_offset = last_offset;
        }
        Ok(projection)
    }

    pub async fn reconstruct_usage_report_with_pricing(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
        pricing: Vec<PricingTableEntry>,
    ) -> Result<UsageReport, ObservabilityError> {
        let usage = UsageAccumulator::default();
        for entry in pricing {
            usage.register_pricing(entry);
        }

        let events = self
            .store
            .read(tenant, session_id, cursor)
            .await?
            .collect::<Vec<_>>()
            .await;
        for event in events {
            let Event::UsageAccumulated(event) = event else {
                continue;
            };

            let mut scopes = Vec::with_capacity(4);
            scopes.push(UsageScope::Tenant(tenant));
            scopes.push(UsageScope::Session(event.session_id));
            if let Some(run_id) = event.run_id {
                scopes.push(UsageScope::Run(run_id));
            }
            if let Some(model_ref) = &event.model_ref {
                scopes.push(UsageScope::Model(model_usage_key(model_ref)));
            }

            usage.record_scopes_with_pricing(
                scopes,
                event.model_ref,
                event.pricing_snapshot_id,
                event.delta,
            );
        }

        Ok(usage.report())
    }

    pub async fn reconstruct_team_projection(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<TeamProjection, ObservabilityError> {
        let events = self
            .store
            .read(tenant, session_id, cursor)
            .await?
            .collect::<Vec<_>>()
            .await;
        Ok(TeamProjection::replay(events.iter()))
    }

    pub async fn reconstruct_subagent_projection(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<SubagentProjection, ObservabilityError> {
        let events = self
            .store
            .read(tenant, session_id, cursor)
            .await?
            .collect::<Vec<_>>()
            .await;
        Ok(SubagentProjection::replay(events.iter()))
    }

    pub async fn diff(
        &self,
        tenant: TenantId,
        session_a: SessionId,
        session_b: SessionId,
    ) -> Result<SessionDiff, ObservabilityError> {
        let a = self
            .reconstruct_projection(tenant, session_a, ReplayCursor::FromStart)
            .await?;
        let b = self
            .reconstruct_projection(tenant, session_b, ReplayCursor::FromStart)
            .await?;

        let events_a = self
            .store
            .read(tenant, session_a, ReplayCursor::FromStart)
            .await?
            .collect::<Vec<_>>()
            .await;
        let events_b = self
            .store
            .read(tenant, session_b, ReplayCursor::FromStart)
            .await?
            .collect::<Vec<_>>()
            .await;

        Ok(SessionDiff::between(&a, &b, &events_a, &events_b))
    }

    pub async fn export_session<W>(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        format: ExportFormat,
        mut out: W,
    ) -> Result<(), ObservabilityError>
    where
        W: AsyncWrite + Unpin,
    {
        match format {
            ExportFormat::Json => {
                let projection = self
                    .reconstruct_projection(tenant, session_id, ReplayCursor::FromStart)
                    .await?;
                write_json(&mut out, &SessionExport::from_projection(&projection)).await?;
            }
            ExportFormat::JsonLines => {
                let mut events = self
                    .store
                    .read(tenant, session_id, ReplayCursor::FromStart)
                    .await?;
                while let Some(event) = events.next().await {
                    write_json_line(&mut out, &event).await?;
                }
            }
            ExportFormat::Markdown => {
                let projection = self
                    .reconstruct_projection(tenant, session_id, ReplayCursor::FromStart)
                    .await?;
                write_markdown(&mut out, &projection.messages).await?;
            }
            ExportFormat::Har => {
                let envelopes = self
                    .store
                    .read_envelopes(tenant, session_id, ReplayCursor::FromStart)
                    .await?
                    .collect::<Vec<_>>()
                    .await;
                write_json(&mut out, &HarExport::from_envelopes(&envelopes)).await?;
            }
        }
        out.flush()
            .await
            .map_err(|error| ObservabilityError::Exporter(error.to_string()))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TeamProjection {
    pub teams: HashMap<TeamId, TeamReplayState>,
}

impl TeamProjection {
    fn replay<'a>(events: impl IntoIterator<Item = &'a Event>) -> Self {
        let mut projection = Self::default();
        for event in events {
            match event {
                Event::TeamCreated(event) => {
                    projection.teams.insert(
                        event.team_id,
                        TeamReplayState {
                            name: event.name.clone(),
                            terminated: None,
                            ..TeamReplayState::default()
                        },
                    );
                }
                Event::TeamMemberJoined(event) => {
                    let team = projection.teams.entry(event.team_id).or_default();
                    team.members.insert(
                        event.agent_id,
                        TeamMemberReplayState {
                            role: event.role.clone(),
                            session_id: event.session_id,
                            visibility: event.visibility.clone(),
                            active: true,
                        },
                    );
                }
                Event::TeamMemberLeft(event) => {
                    if let Some(team) = projection.teams.get_mut(&event.team_id) {
                        if let Some(member) = team.members.get_mut(&event.agent_id) {
                            member.active = false;
                        }
                    }
                }
                Event::AgentMessageSent(event) => {
                    projection
                        .teams
                        .entry(event.team_id)
                        .or_default()
                        .sent_messages += 1;
                }
                Event::AgentMessageRouted(event) => {
                    projection
                        .teams
                        .entry(event.team_id)
                        .or_default()
                        .routed_messages += event.resolved_recipients.len() as u64;
                }
                Event::TeamTurnCompleted(event) => {
                    let team = projection.teams.entry(event.team_id).or_default();
                    team.turns_completed += 1;
                    add_usage(&mut team.usage, &event.usage);
                }
                Event::TeamTerminated(event) => {
                    projection
                        .teams
                        .entry(event.team_id)
                        .or_default()
                        .terminated = Some(event.reason.clone());
                }
                _ => {}
            }
        }
        projection
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TeamReplayState {
    pub name: String,
    pub members: HashMap<AgentId, TeamMemberReplayState>,
    pub sent_messages: u64,
    pub routed_messages: u64,
    pub turns_completed: u64,
    pub usage: UsageSnapshot,
    pub terminated: Option<TeamTerminationReason>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamMemberReplayState {
    pub role: String,
    pub session_id: SessionId,
    pub visibility: ContextVisibility,
    pub active: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubagentProjection {
    pub subagents: HashMap<SubagentId, SubagentReplayState>,
    pub spawn_paused: bool,
}

impl SubagentProjection {
    fn replay<'a>(events: impl IntoIterator<Item = &'a Event>) -> Self {
        let mut projection = Self::default();
        for event in events {
            match event {
                Event::SubagentSpawned(event) => {
                    projection.subagents.insert(
                        event.subagent_id,
                        SubagentReplayState {
                            agent_name: event.agent_ref.name.clone(),
                            parent_session_id: event.parent_session_id,
                            depth: event.depth,
                            trigger_tool_use_id: event.trigger_tool_use_id,
                            status: None,
                            ..SubagentReplayState::default()
                        },
                    );
                }
                Event::SubagentAnnounced(event) => {
                    let state = projection.subagents.entry(event.subagent_id).or_default();
                    state.status = Some(event.status.clone());
                    state.summary = Some(event.summary.clone());
                    add_usage(&mut state.usage, &event.usage);
                }
                Event::SubagentTerminated(event) => {
                    let state = projection.subagents.entry(event.subagent_id).or_default();
                    state.terminated = true;
                    add_usage(&mut state.usage, &event.final_usage);
                }
                Event::SubagentSpawnPaused(event) => {
                    projection.spawn_paused = event.paused;
                }
                Event::SubagentPermissionForwarded(event) => {
                    projection
                        .subagents
                        .entry(event.subagent_id)
                        .or_default()
                        .forwarded_permissions += 1;
                }
                Event::SubagentPermissionResolved(event) => {
                    projection
                        .subagents
                        .entry(event.subagent_id)
                        .or_default()
                        .resolved_permissions += 1;
                }
                _ => {}
            }
        }
        projection
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubagentReplayState {
    pub agent_name: String,
    pub parent_session_id: SessionId,
    pub depth: u8,
    pub trigger_tool_use_id: Option<ToolUseId>,
    pub status: Option<SubagentStatus>,
    pub summary: Option<String>,
    pub usage: UsageSnapshot,
    pub terminated: bool,
    pub forwarded_permissions: u64,
    pub resolved_permissions: u64,
}

#[derive(Serialize)]
struct HarExport {
    log: HarLog,
}

#[derive(Serialize)]
struct HarLog {
    version: &'static str,
    creator: HarCreator,
    entries: Vec<HarEntry>,
}

#[derive(Serialize)]
struct HarCreator {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct HarEntry {
    #[serde(rename = "startedDateTime")]
    started_date_time: String,
    time: u64,
    request: HarRequest,
    response: HarResponse,
    cache: serde_json::Value,
    timings: HarTimings,
}

#[derive(Serialize)]
struct HarRequest {
    method: &'static str,
    url: String,
    #[serde(rename = "httpVersion")]
    http_version: &'static str,
    headers: Vec<serde_json::Value>,
    #[serde(rename = "queryString")]
    query_string: Vec<serde_json::Value>,
    cookies: Vec<serde_json::Value>,
    #[serde(rename = "headersSize")]
    headers_size: i64,
    #[serde(rename = "bodySize")]
    body_size: i64,
}

#[derive(Serialize)]
struct HarResponse {
    status: u16,
    #[serde(rename = "statusText")]
    status_text: &'static str,
    #[serde(rename = "httpVersion")]
    http_version: &'static str,
    headers: Vec<serde_json::Value>,
    cookies: Vec<serde_json::Value>,
    content: HarContent,
    #[serde(rename = "redirectURL")]
    redirect_url: &'static str,
    #[serde(rename = "headersSize")]
    headers_size: i64,
    #[serde(rename = "bodySize")]
    body_size: i64,
}

#[derive(Serialize)]
struct HarContent {
    size: usize,
    #[serde(rename = "mimeType")]
    mime_type: &'static str,
    text: String,
}

#[derive(Serialize)]
struct HarTimings {
    send: i64,
    wait: i64,
    receive: i64,
}

impl HarExport {
    fn from_envelopes(envelopes: &[harness_journal::EventEnvelope]) -> Self {
        let entries = envelopes
            .iter()
            .map(|envelope| {
                let text = serde_json::to_string(&envelope.payload).unwrap_or_default();
                HarEntry {
                    started_date_time: envelope.recorded_at.to_rfc3339(),
                    time: 0,
                    request: HarRequest {
                        method: "EVENT",
                        url: format!(
                            "harness://session/{}/events/{}",
                            envelope.session_id, envelope.offset.0
                        ),
                        http_version: "HARNESS/1.0",
                        headers: Vec::new(),
                        query_string: Vec::new(),
                        cookies: Vec::new(),
                        headers_size: -1,
                        body_size: 0,
                    },
                    response: HarResponse {
                        status: 200,
                        status_text: "OK",
                        http_version: "HARNESS/1.0",
                        headers: Vec::new(),
                        cookies: Vec::new(),
                        content: HarContent {
                            size: text.len(),
                            mime_type: "application/json",
                            text,
                        },
                        redirect_url: "",
                        headers_size: -1,
                        body_size: -1,
                    },
                    cache: serde_json::json!({}),
                    timings: HarTimings {
                        send: 0,
                        wait: 0,
                        receive: 0,
                    },
                }
            })
            .collect();
        Self {
            log: HarLog {
                version: "1.2",
                creator: HarCreator {
                    name: "jyowo-harness-observability",
                    version: env!("CARGO_PKG_VERSION"),
                },
                entries,
            },
        }
    }
}

#[derive(Serialize)]
struct SessionExport<'a> {
    messages: &'a [Message],
    usage: &'a UsageSnapshot,
    end_reason: &'a Option<harness_contracts::EndReason>,
    last_offset: harness_contracts::JournalOffset,
}

impl<'a> SessionExport<'a> {
    fn from_projection(projection: &'a JournalSessionProjection) -> Self {
        Self {
            messages: &projection.messages,
            usage: &projection.usage,
            end_reason: &projection.end_reason,
            last_offset: projection.last_offset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionDiff {
    pub added_messages: Vec<Message>,
    pub removed_messages: Vec<Message>,
    pub tool_divergence: Vec<ToolDivergence>,
    pub usage_delta: UsageSnapshot,
}

impl SessionDiff {
    fn between(
        a: &JournalSessionProjection,
        b: &JournalSessionProjection,
        events_a: &[Event],
        events_b: &[Event],
    ) -> Self {
        let a_ids = a
            .messages
            .iter()
            .map(|message| message.id)
            .collect::<HashSet<_>>();
        let b_ids = b
            .messages
            .iter()
            .map(|message| message.id)
            .collect::<HashSet<_>>();
        let added_messages = b
            .messages
            .iter()
            .filter(|message| !a_ids.contains(&message.id))
            .cloned()
            .collect();
        let removed_messages = a
            .messages
            .iter()
            .filter(|message| !b_ids.contains(&message.id))
            .cloned()
            .collect();

        Self {
            added_messages,
            removed_messages,
            tool_divergence: tool_divergence(events_a, events_b),
            usage_delta: usage_delta(&a.usage, &b.usage),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolDivergence {
    pub tool_use_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Json,
    JsonLines,
    Markdown,
    Har,
}

fn usage_delta(a: &UsageSnapshot, b: &UsageSnapshot) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: b.input_tokens.saturating_sub(a.input_tokens),
        output_tokens: b.output_tokens.saturating_sub(a.output_tokens),
        cache_read_tokens: b.cache_read_tokens.saturating_sub(a.cache_read_tokens),
        cache_write_tokens: b.cache_write_tokens.saturating_sub(a.cache_write_tokens),
        cost_micros: b.cost_micros.saturating_sub(a.cost_micros),
        tool_calls: b.tool_calls.saturating_sub(a.tool_calls),
    }
}

fn add_usage(total: &mut UsageSnapshot, delta: &UsageSnapshot) {
    total.input_tokens = total.input_tokens.saturating_add(delta.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(delta.output_tokens);
    total.cache_read_tokens = total
        .cache_read_tokens
        .saturating_add(delta.cache_read_tokens);
    total.cache_write_tokens = total
        .cache_write_tokens
        .saturating_add(delta.cache_write_tokens);
    total.cost_micros = total.cost_micros.saturating_add(delta.cost_micros);
    total.tool_calls = total.tool_calls.saturating_add(delta.tool_calls);
}

fn tool_divergence(a: &[Event], b: &[Event]) -> Vec<ToolDivergence> {
    let a_tools = tool_observations(a);
    let b_tools = tool_observations(b);
    let mut ids = a_tools
        .keys()
        .chain(b_tools.keys())
        .copied()
        .collect::<Vec<_>>();
    ids.sort_by_key(ToString::to_string);
    ids.dedup();

    ids.into_iter()
        .filter_map(
            |tool_use_id| match (a_tools.get(&tool_use_id), b_tools.get(&tool_use_id)) {
                (None, Some(observed)) => Some(ToolDivergence {
                    tool_use_id: tool_use_id.to_string(),
                    reason: format!(
                        "added tool {} with status {}",
                        observed.name, observed.status
                    ),
                }),
                (Some(observed), None) => Some(ToolDivergence {
                    tool_use_id: tool_use_id.to_string(),
                    reason: format!(
                        "removed tool {} with status {}",
                        observed.name, observed.status
                    ),
                }),
                (Some(left), Some(right)) if left != right => Some(ToolDivergence {
                    tool_use_id: tool_use_id.to_string(),
                    reason: format!(
                        "changed from {}:{} to {}:{}",
                        left.name, left.status, right.name, right.status
                    ),
                }),
                _ => None,
            },
        )
        .collect()
}

fn model_usage_key(model_ref: &ModelRef) -> String {
    format!("{}/{}", model_ref.provider_id, model_ref.model_id)
}

fn tool_observations(events: &[Event]) -> HashMap<ToolUseId, ToolObservation> {
    let mut tools = HashMap::new();
    for event in events {
        match event {
            Event::AssistantMessageCompleted(event) => {
                for tool in &event.tool_uses {
                    tools.entry(tool.tool_use_id).or_insert(ToolObservation {
                        name: tool.tool_name.clone(),
                        status: "mentioned".to_owned(),
                    });
                }
            }
            Event::ToolUseRequested(event) => {
                tools.insert(
                    event.tool_use_id,
                    ToolObservation {
                        name: event.tool_name.clone(),
                        status: "requested".to_owned(),
                    },
                );
            }
            Event::ToolUseCompleted(event) => {
                tools
                    .entry(event.tool_use_id)
                    .and_modify(|tool| tool.status = "completed".to_owned())
                    .or_insert(ToolObservation {
                        name: "unknown".to_owned(),
                        status: "completed".to_owned(),
                    });
            }
            Event::ToolUseDenied(event) => {
                tools
                    .entry(event.tool_use_id)
                    .and_modify(|tool| tool.status = "denied".to_owned())
                    .or_insert(ToolObservation {
                        name: "unknown".to_owned(),
                        status: "denied".to_owned(),
                    });
            }
            Event::ToolUseFailed(event) => {
                tools
                    .entry(event.tool_use_id)
                    .and_modify(|tool| tool.status = "failed".to_owned())
                    .or_insert(ToolObservation {
                        name: "unknown".to_owned(),
                        status: "failed".to_owned(),
                    });
            }
            _ => {}
        }
    }
    tools
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolObservation {
    name: String,
    status: String,
}

async fn write_json<W, T>(out: &mut W, value: &T) -> Result<(), ObservabilityError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| ObservabilityError::Replay(error.to_string()))?;
    out.write_all(&bytes)
        .await
        .map_err(|error| ObservabilityError::Exporter(error.to_string()))
}

async fn write_json_line<W, T>(out: &mut W, value: &T) -> Result<(), ObservabilityError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let bytes =
        serde_json::to_vec(value).map_err(|error| ObservabilityError::Replay(error.to_string()))?;
    out.write_all(&bytes)
        .await
        .map_err(|error| ObservabilityError::Exporter(error.to_string()))?;
    out.write_all(b"\n")
        .await
        .map_err(|error| ObservabilityError::Exporter(error.to_string()))
}

async fn write_markdown<W>(out: &mut W, messages: &[Message]) -> Result<(), ObservabilityError>
where
    W: AsyncWrite + Unpin,
{
    for message in messages {
        out.write_all(format!("## {:?}\n\n", message.role).as_bytes())
            .await
            .map_err(|error| ObservabilityError::Exporter(error.to_string()))?;
        for part in &message.parts {
            out.write_all(format!("{part:?}\n\n").as_bytes())
                .await
                .map_err(|error| ObservabilityError::Exporter(error.to_string()))?;
        }
    }
    Ok(())
}
