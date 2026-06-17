#![cfg(feature = "in-memory")]

use std::path::PathBuf;
use std::sync::Arc;

use harness_contracts::*;
use harness_journal::*;

fn permission_requested(
    session_id: SessionId,
    run_id: RunId,
    tool_use_id: ToolUseId,
    request_id: RequestId,
) -> Event {
    Event::PermissionRequested(PermissionRequestedEvent {
        request_id,
        run_id,
        session_id,
        tenant_id: TenantId::SINGLE,
        tool_use_id,
        tool_name: "shell".to_owned(),
        subject: PermissionSubject::CommandExec {
            command: "rm".to_owned(),
            argv: vec!["rm".to_owned(), "-rf".to_owned(), "tmp".to_owned()],
            cwd: Some(PathBuf::from("/tmp")),
            fingerprint: None,
        },
        severity: Severity::High,
        scope_hint: DecisionScope::Any,
        fingerprint: None,
        presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
        interactivity: InteractivityLevel::FullyInteractive,
        causation_id: EventId::new(),
        at: harness_contracts::now(),
    })
}

fn permission_resolved(request_id: RequestId) -> Event {
    Event::PermissionResolved(PermissionResolvedEvent {
        request_id,
        decision: Decision::AllowOnce,
        decided_by: DecidedBy::User,
        scope: DecisionScope::Any,
        fingerprint: None,
        rationale: None,
        at: harness_contracts::now(),
    })
}

fn unexpected() -> Event {
    Event::UnexpectedError(UnexpectedErrorEvent {
        session_id: None,
        run_id: None,
        error: "noise".to_owned(),
        at: harness_contracts::now(),
    })
}

#[tokio::test]
async fn audit_store_stitches_permission_resolution_to_tool_use_scope() {
    let store = InMemoryEventStore::new(Arc::new(NoopRedactor));
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    store
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                permission_requested(session_id, run_id, tool_use_id, request_id),
                permission_resolved(request_id),
                unexpected(),
            ],
        )
        .await
        .expect("events append");

    let audit = EventStoreAudit::new(store);
    let page = audit
        .query(
            TenantId::SINGLE,
            AuditQuery {
                scope: AuditScope::ToolUse(tool_use_id),
                filter: AuditFilter {
                    min_severity: Some(Severity::High),
                    ..AuditFilter::default()
                },
                order: AuditOrder::EventIdAsc,
                limit: 10,
            },
        )
        .await
        .expect("audit query succeeds");
    assert_eq!(page.records.len(), 2);
    assert!(page
        .records
        .iter()
        .any(|record| matches!(record.event, Event::PermissionRequested(_))));
    assert!(page
        .records
        .iter()
        .any(|record| matches!(record.event, Event::PermissionResolved(_))));

    let decisions = audit
        .query(
            TenantId::SINGLE,
            AuditQuery {
                scope: AuditScope::ToolUse(tool_use_id),
                filter: AuditFilter {
                    decisions: vec![Decision::AllowOnce],
                    ..AuditFilter::default()
                },
                order: AuditOrder::EventIdAsc,
                limit: 10,
            },
        )
        .await
        .expect("decision audit query succeeds");
    assert_eq!(decisions.records.len(), 1);
    assert!(matches!(
        decisions.records[0].event,
        Event::PermissionResolved(_)
    ));
}
