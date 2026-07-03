use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    Event, Message, MessagePart, SandboxError, SessionId, StopReason, TenantId, ToolUseId,
    UsageSnapshot,
};
use harness_execution::{
    AuthorizationEventSink, AuthorizationService, ExecutionError, TicketLedger,
};
use harness_journal::{EventStore, InMemoryEventStore};
use harness_model::{ContentDelta, ModelStreamEvent};
use harness_permission::{NoopDecisionPersistence, PermissionAuthority, PermissionBroker};
use harness_sandbox::{
    ExecContext, ExecSpec, ProcessHandle, SandboxBackend, SandboxCapabilities, SessionSnapshotFile,
    SnapshotSpec,
};
use serde_json::Value;

pub fn tool_call_events(name: &str, input: Value) -> Vec<ModelStreamEvent> {
    let delta = ContentDelta::ToolUseComplete {
        id: ToolUseId::new(),
        name: name.to_owned(),
        input,
    };
    assistant_events([(0, delta)], StopReason::ToolUse)
}

pub fn text_events(text: &str) -> Vec<ModelStreamEvent> {
    assistant_events(
        [(0, ContentDelta::Text(text.to_owned()))],
        StopReason::EndTurn,
    )
}

pub fn thinking_then_text_events(thinking: &str, text: &str) -> Vec<ModelStreamEvent> {
    let thinking = ContentDelta::Thinking(harness_model::ThinkingDelta {
        provider_native: None,
        signature: None,
        text: Some(thinking.to_owned()),
    });
    assistant_events(
        [(0, thinking), (1, ContentDelta::Text(text.to_owned()))],
        StopReason::EndTurn,
    )
}

fn assistant_events<const N: usize>(
    deltas: [(usize, ContentDelta); N],
    stop_reason: StopReason,
) -> Vec<ModelStreamEvent> {
    let mut events = vec![ModelStreamEvent::MessageStart {
        message_id: "assistant-1".to_owned(),
        usage: UsageSnapshot::default(),
    }];
    events.extend(
        deltas
            .into_iter()
            .map(|(index, delta)| ModelStreamEvent::ContentBlockDelta {
                index: index.try_into().unwrap(),
                delta,
            }),
    );
    events.extend([
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(stop_reason),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]);
    events
}

pub fn message_text(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn test_authorization_service(
    policy_broker: Arc<dyn PermissionBroker>,
    event_store: Arc<InMemoryEventStore>,
) -> Arc<AuthorizationService> {
    let authority = PermissionAuthority::builder()
        .with_policy_broker(policy_broker)
        .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
        .build()
        .unwrap();
    Arc::new(AuthorizationService::new(
        Arc::new(authority),
        Arc::new(AllowPreflightSandbox),
        Arc::new(JournalAuthorizationEventSink { event_store }),
        Arc::new(TicketLedger::default()),
    ))
}

struct JournalAuthorizationEventSink {
    event_store: Arc<InMemoryEventStore>,
}

#[async_trait]
impl AuthorizationEventSink for JournalAuthorizationEventSink {
    async fn emit_batch(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        events: Vec<Event>,
    ) -> Result<(), ExecutionError> {
        self.event_store
            .append(tenant_id, session_id, &events)
            .await
            .map(|_| ())
            .map_err(|error| ExecutionError::EventSinkFailed {
                reason: error.to_string(),
            })
    }
}

struct AllowPreflightSandbox;

#[async_trait]
impl SandboxBackend for AllowPreflightSandbox {
    fn backend_id(&self) -> &str {
        "test"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            supports_network: true,
            supports_filesystem_write: true,
            resource_limit_support: harness_sandbox::ResourceLimitSupport {
                memory: true,
                cpu: true,
                pids: true,
                wall_clock: true,
                open_files: true,
            },
            ..SandboxCapabilities::default()
        }
    }

    async fn execute(
        &self,
        _spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        Err(SandboxError::Unavailable {
            backend: "test".to_owned(),
            detail: "test sandbox does not execute processes".to_owned(),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}
