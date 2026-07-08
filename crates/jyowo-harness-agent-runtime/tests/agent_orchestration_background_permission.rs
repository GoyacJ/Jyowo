use std::sync::Arc;

use harness_agent_runtime::{
    AgentRuntimeStore, BackgroundAgentManager, BackgroundAgentStartRequest,
    BackgroundAgentTransitionError,
};
use harness_contracts::{
    BackgroundAgentState, Decision, Event, NoopRedactor, RedactRules, Redactor, RequestId, RunId,
    SessionId, TenantId,
};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use tempfile::tempdir;

#[derive(Debug)]
struct TokenRedactor;

impl Redactor for TokenRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("sk-test-secret", "[REDACTED]")
    }
}

fn manager(
    store: Arc<AgentRuntimeStore>,
    event_store: Arc<InMemoryEventStore>,
    session_id: SessionId,
) -> BackgroundAgentManager {
    let event_store: Arc<dyn EventStore> = event_store;
    BackgroundAgentManager::new(
        store,
        event_store,
        TenantId::SINGLE,
        session_id,
        Arc::new(TokenRedactor),
    )
}

async fn events(event_store: &InMemoryEventStore, session_id: SessionId) -> Vec<Event> {
    let mut stream = event_store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("read events");
    let mut events = Vec::new();
    while let Some(event) = futures::StreamExt::next(&mut stream).await {
        events.push(event);
    }
    events
}

#[tokio::test]
async fn agent_orchestration_background_permission_recovery_allows_pending_decision_after_restart()
{
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = manager(
        Arc::clone(&store),
        Arc::clone(&event_store),
        conversation_id,
    );
    let request_id = RequestId::new();
    let recoverable = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "permission".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start");
    manager
        .wait_for_permission(
            recoverable.background_agent_id.as_str(),
            request_id,
            "permission required",
        )
        .await
        .expect("wait for permission");
    let recoverable_after_restart = manager
        .recover_on_startup("process restart")
        .await
        .expect("recover")
        .into_iter()
        .find(|record| record.background_agent_id == recoverable.background_agent_id)
        .expect("recovered permission record");
    assert_eq!(
        recoverable_after_restart.state,
        BackgroundAgentState::Recoverable
    );

    assert!(matches!(
        manager
            .send_input(
                recoverable.background_agent_id.as_str(),
                request_id,
                "not input",
            )
            .await,
        Err(BackgroundAgentTransitionError::InvalidTransition { .. })
    ));

    let approved = manager
        .resolve_permission(
            recoverable.background_agent_id.as_str(),
            request_id,
            true,
            "approved",
        )
        .await
        .expect("recoverable permission resolves after restart");
    assert_eq!(approved.state, BackgroundAgentState::Running);
}

#[tokio::test]
async fn agent_orchestration_background_live_permission_resolves_and_denies() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = workspace.path().canonicalize().expect("canonical");
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
            .expect("store opens"),
    );
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = manager(
        Arc::clone(&store),
        Arc::clone(&event_store),
        conversation_id,
    );
    let request_id = RequestId::new();
    let live_permission = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "permission".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start");
    manager
        .wait_for_permission(
            live_permission.background_agent_id.as_str(),
            request_id,
            "permission required",
        )
        .await
        .expect("wait for permission");

    let approved = manager
        .resolve_permission(
            live_permission.background_agent_id.as_str(),
            request_id,
            true,
            "approved",
        )
        .await
        .expect("resolve permission");
    assert_eq!(approved.state, BackgroundAgentState::Running);
    let approved_attempt_id = live_permission
        .run_id
        .as_deref()
        .map(RunId::parse)
        .transpose()
        .expect("run id parses");
    let approved_events = events(&event_store, conversation_id).await;
    assert!(approved_events.iter().any(|event| {
        matches!(
            event,
            Event::BackgroundAgentPermissionRequested(requested)
                if requested.background_agent_id.to_string() == live_permission.background_agent_id
                    && requested.tenant_id == TenantId::SINGLE
                    && requested.conversation_id == conversation_id
                    && requested.request_id == request_id
                    && requested.attempt_id == approved_attempt_id
                    && requested.reason.as_str() == "permission required"
        )
    }));
    assert!(approved_events.iter().any(|event| {
        matches!(
            event,
            Event::BackgroundAgentPermissionResolved(resolved)
                if resolved.background_agent_id.to_string() == live_permission.background_agent_id
                    && resolved.tenant_id == TenantId::SINGLE
                    && resolved.conversation_id == conversation_id
                    && resolved.request_id == request_id
                    && resolved.attempt_id == approved_attempt_id
                    && resolved.decision == Decision::AllowOnce
        )
    }));

    let denied = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "permission denied".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start denied");
    let denied_request_id = RequestId::new();
    manager
        .wait_for_permission(
            denied.background_agent_id.as_str(),
            denied_request_id,
            "permission required",
        )
        .await
        .expect("wait denied");
    let failed = manager
        .resolve_permission(
            denied.background_agent_id.as_str(),
            denied_request_id,
            false,
            "denied",
        )
        .await
        .expect("deny permission");
    assert_eq!(failed.state, BackgroundAgentState::Failed);
    assert!(events(&event_store, conversation_id)
        .await
        .iter()
        .any(|event| matches!(event, Event::BackgroundAgentFailed(_))));
}
