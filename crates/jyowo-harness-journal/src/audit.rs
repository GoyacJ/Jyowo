//! Audit query facade over journal envelopes.

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{
    DecidedBy, Decision, Event, EventId, EventKind, JournalError, PermissionSubject, RequestId,
    RunId, SessionId, Severity, TenantId, ToolUseId,
};
use serde::{Deserialize, Serialize};

use crate::EventStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditScope {
    Tenant,
    Session(SessionId),
    Run(RunId),
    ToolUse(ToolUseId),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AuditFilter {
    pub event_kinds: Vec<EventKind>,
    pub tool_use_ids: Vec<ToolUseId>,
    pub permission_subjects: Vec<PermissionSubject>,
    pub decisions: Vec<Decision>,
    pub decided_by: Vec<DecidedBy>,
    pub min_severity: Option<Severity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditOrder {
    EventIdAsc,
    TimeAsc,
    CausationDfs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditQuery {
    pub scope: AuditScope,
    pub filter: AuditFilter,
    pub order: AuditOrder,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub event_id: EventId,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub tool_use_id: Option<ToolUseId>,
    pub request_id: Option<RequestId>,
    pub event_kind: EventKind,
    pub recorded_at: DateTime<Utc>,
    pub severity: Option<Severity>,
    pub permission_subject: Option<PermissionSubject>,
    pub decision: Option<Decision>,
    pub decided_by: Option<DecidedBy>,
    pub event: Event,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditPage {
    pub records: Vec<AuditRecord>,
}

#[async_trait]
pub trait AuditStore: Send + Sync {
    async fn query(&self, tenant: TenantId, query: AuditQuery) -> Result<AuditPage, JournalError>;
}

pub struct EventStoreAudit<S: EventStore> {
    inner: S,
}

impl<S: EventStore> EventStoreAudit<S> {
    pub const fn new(inner: S) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<S: EventStore> AuditStore for EventStoreAudit<S> {
    async fn query(&self, tenant: TenantId, query: AuditQuery) -> Result<AuditPage, JournalError> {
        let envelopes = self.inner.query_after(tenant, None, usize::MAX).await?;
        let mut records: Vec<_> = envelopes.into_iter().map(AuditRecord::from).collect();
        stitch_permission_tool_use(&mut records);

        let scoped_request_ids = scoped_request_ids(&records, &query);
        records.retain(|record| {
            scope_matches(record, &query.scope, &scoped_request_ids)
                && filter_matches(record, &query.filter, &scoped_request_ids)
        });

        sort_records(&mut records, query.order);
        records.truncate(query.limit);
        Ok(AuditPage { records })
    }
}

impl From<crate::EventEnvelope> for AuditRecord {
    fn from(envelope: crate::EventEnvelope) -> Self {
        let event_kind = EventKind::from(&envelope.payload);
        let mut record = Self {
            event_id: envelope.event_id,
            session_id: envelope.session_id,
            run_id: envelope.run_id,
            tool_use_id: None,
            request_id: None,
            event_kind,
            recorded_at: envelope.recorded_at,
            severity: None,
            permission_subject: None,
            decision: None,
            decided_by: None,
            event: envelope.payload,
        };
        record.apply_payload_metadata();
        record
    }
}

impl AuditRecord {
    fn apply_payload_metadata(&mut self) {
        match &self.event {
            Event::SessionCreated(event) => {
                self.session_id = event.session_id;
            }
            Event::SessionForked(event) => {
                self.session_id = event.child_session_id;
            }
            Event::SessionEnded(event) => {
                self.session_id = event.session_id;
            }
            Event::SessionReloadRequested(event) => {
                self.session_id = event.session_id;
            }
            Event::SessionReloadApplied(event) => {
                self.session_id = event.session_id;
            }
            Event::RunStarted(event) => {
                self.session_id = event.session_id;
                self.run_id = Some(event.run_id);
            }
            Event::RunEnded(event) => {
                self.run_id = Some(event.run_id);
            }
            Event::ToolUseRequested(event) => {
                self.run_id = Some(event.run_id);
                self.tool_use_id = Some(event.tool_use_id);
            }
            Event::ToolUseApproved(event) => {
                self.tool_use_id = Some(event.tool_use_id);
            }
            Event::ToolUseDenied(event) => {
                self.tool_use_id = Some(event.tool_use_id);
            }
            Event::ToolUseCompleted(event) => {
                self.tool_use_id = Some(event.tool_use_id);
            }
            Event::ToolUseFailed(event) => {
                self.tool_use_id = Some(event.tool_use_id);
            }
            Event::ToolUseHeartbeat(event) => {
                self.run_id = Some(event.run_id);
                self.tool_use_id = Some(event.tool_use_id);
            }
            Event::ToolResultOffloaded(event) => {
                self.run_id = Some(event.run_id);
                self.tool_use_id = Some(event.tool_use_id);
            }
            Event::PermissionRequested(event) => {
                self.session_id = event.session_id;
                self.run_id = Some(event.run_id);
                self.tool_use_id = Some(event.tool_use_id);
                self.request_id = Some(event.request_id);
                self.severity = Some(event.severity);
                self.permission_subject = Some(event.subject.clone());
            }
            Event::PermissionResolved(event) => {
                self.request_id = Some(event.request_id);
                self.decision = Some(event.decision.clone());
                self.decided_by = Some(event.decided_by.clone());
            }
            Event::PermissionRequestSuppressed(event) => {
                self.session_id = event.session_id;
                self.run_id = Some(event.run_id);
                self.tool_use_id = Some(event.tool_use_id);
                self.request_id = Some(event.request_id);
                self.severity = Some(event.severity);
                self.permission_subject = Some(event.subject.clone());
                self.decision = event.reused_decision.clone();
            }
            _ => {}
        }
    }
}

fn stitch_permission_tool_use(records: &mut [AuditRecord]) {
    let request_to_tool: HashMap<_, _> = records
        .iter()
        .filter_map(|record| Some((record.request_id?, record.tool_use_id?)))
        .collect();
    for record in records {
        if record.tool_use_id.is_none() {
            if let Some(request_id) = record.request_id {
                record.tool_use_id = request_to_tool.get(&request_id).copied();
            }
        }
    }
}

fn scoped_request_ids(records: &[AuditRecord], query: &AuditQuery) -> HashSet<RequestId> {
    records
        .iter()
        .filter(|record| direct_scope_matches(record, &query.scope))
        .filter(|record| severity_matches(record, query.filter.min_severity))
        .filter_map(|record| record.request_id)
        .collect()
}

fn scope_matches(
    record: &AuditRecord,
    scope: &AuditScope,
    scoped_request_ids: &HashSet<RequestId>,
) -> bool {
    direct_scope_matches(record, scope)
        || record
            .request_id
            .is_some_and(|request_id| scoped_request_ids.contains(&request_id))
}

fn direct_scope_matches(record: &AuditRecord, scope: &AuditScope) -> bool {
    match scope {
        AuditScope::Tenant => true,
        AuditScope::Session(session_id) => record.session_id == *session_id,
        AuditScope::Run(run_id) => record.run_id == Some(*run_id),
        AuditScope::ToolUse(tool_use_id) => record.tool_use_id == Some(*tool_use_id),
    }
}

fn filter_matches(
    record: &AuditRecord,
    filter: &AuditFilter,
    scoped_request_ids: &HashSet<RequestId>,
) -> bool {
    if !filter.event_kinds.is_empty() && !filter.event_kinds.contains(&record.event_kind) {
        return false;
    }
    if !filter.tool_use_ids.is_empty()
        && !record
            .tool_use_id
            .is_some_and(|tool_use_id| filter.tool_use_ids.contains(&tool_use_id))
    {
        return false;
    }
    if !filter.permission_subjects.is_empty()
        && !record
            .permission_subject
            .as_ref()
            .is_some_and(|subject| filter.permission_subjects.contains(subject))
    {
        return false;
    }
    if !filter.decisions.is_empty()
        && !record
            .decision
            .as_ref()
            .is_some_and(|decision| filter.decisions.contains(decision))
    {
        return false;
    }
    if !filter.decided_by.is_empty()
        && !record
            .decided_by
            .as_ref()
            .is_some_and(|decided_by| filter.decided_by.contains(decided_by))
    {
        return false;
    }
    if !severity_matches(record, filter.min_severity)
        && !record
            .request_id
            .is_some_and(|request_id| scoped_request_ids.contains(&request_id))
    {
        return false;
    }
    true
}

fn severity_matches(record: &AuditRecord, min_severity: Option<Severity>) -> bool {
    match min_severity {
        Some(min) => record
            .severity
            .is_some_and(|severity| severity_rank(severity) >= severity_rank(min)),
        None => true,
    }
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Low => 1,
        Severity::Medium => 2,
        Severity::High => 3,
        Severity::Critical => 4,
        _ => 0,
    }
}

fn sort_records(records: &mut [AuditRecord], order: AuditOrder) {
    match order {
        AuditOrder::EventIdAsc => records.sort_by_key(|record| record.event_id.to_string()),
        AuditOrder::TimeAsc => records.sort_by_key(|record| record.recorded_at),
        AuditOrder::CausationDfs => records.sort_by_key(|record| {
            let causation = match &record.event {
                Event::ToolUseRequested(event) => Some(event.causation_id),
                Event::PermissionRequested(event) => Some(event.causation_id),
                Event::PermissionRequestSuppressed(event) => Some(event.causation_id),
                Event::ToolRegistrationShadowed(event) => event.causation_id,
                _ => None,
            };
            (
                causation.map(|id| id.to_string()),
                record.event_id.to_string(),
            )
        }),
    }
}
