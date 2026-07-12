//! Tests for the memory candidate inbox.

use harness_contracts::*;
use harness_memory::{inbox::MemoryInbox, MemorySettingsStore};

fn make_draft(content: &str) -> MemoryRecordDraft {
    MemoryRecordDraft {
        kind: MemoryKind::ProjectFact,
        visibility: MemoryVisibility::Tenant,
        content: content.to_owned(),
        metadata: MemoryMetadata {
            ttl: None,
            tags: vec![],
            source_trust: 0.5,
        },
        expires_at: None,
    }
}

fn make_evidence() -> MemoryEvidence {
    let sid = SessionId::new();
    MemoryEvidence {
        source: MemorySource::AgentDerived,
        origin: MemoryEvidenceOrigin::AssistantMessage {
            session_id: sid,
            run_id: RunId::new(),
            message_id: MessageId::new(),
        },
        content_hash: ContentHash([1u8; 32]),
        session_id: None,
        run_id: None,
        message_id: None,
        tool_use_id: None,
    }
}

#[test]
fn inbox_starts_empty() {
    let inbox = MemoryInbox::new(TenantId::SINGLE);
    let candidates = inbox.list(None).unwrap();
    assert!(candidates.is_empty());
}

#[test]
fn sqlite_inbox_persists_candidates_after_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.sqlite3");

    let candidate_id = {
        let inbox = MemoryInbox::open(db_path.to_str().unwrap(), TenantId::SINGLE).unwrap();
        inbox
            .propose(make_draft("durable candidate"), make_evidence())
            .unwrap()
            .id
    };

    let reopened = MemoryInbox::open(db_path.to_str().unwrap(), TenantId::SINGLE).unwrap();
    let candidates = reopened.list(None).unwrap();

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, candidate_id);
    assert_eq!(candidates[0].proposed_record.content, "durable candidate");
}

#[test]
fn propose_adds_candidate_in_proposed_state() {
    let inbox = MemoryInbox::new(TenantId::SINGLE);
    let candidate = inbox
        .propose(make_draft("remember this"), make_evidence())
        .unwrap();
    assert_eq!(candidate.state, MemoryCandidateState::Proposed);
    assert_eq!(candidate.proposed_record.content, "remember this");
}

#[test]
fn approve_promotes_candidate_to_approved_state() {
    let inbox = MemoryInbox::new(TenantId::SINGLE);
    let candidate = inbox
        .propose(make_draft("approve me"), make_evidence())
        .unwrap();

    let approved = inbox.approve(candidate.id).unwrap();
    assert_eq!(approved.state, MemoryCandidateState::Approved);
}

#[test]
fn merge_marks_candidate_as_merged() {
    let inbox = MemoryInbox::new(TenantId::SINGLE);
    let candidate = inbox
        .propose(make_draft("merge me"), make_evidence())
        .unwrap();

    let merged = inbox.merge(candidate.id).unwrap();

    assert_eq!(merged.state, MemoryCandidateState::Merged);
    assert_eq!(
        inbox.list(Some(MemoryCandidateState::Merged)).unwrap()[0].id,
        candidate.id
    );
}

#[test]
fn reject_marks_candidate_as_rejected() {
    let inbox = MemoryInbox::new(TenantId::SINGLE);
    let candidate = inbox
        .propose(make_draft("bad idea"), make_evidence())
        .unwrap();

    let rejected = inbox.reject(candidate.id).unwrap();
    assert_eq!(rejected.state, MemoryCandidateState::Rejected);
}

#[test]
fn unapproved_candidates_not_returned_in_default_list() {
    let inbox = MemoryInbox::new(TenantId::SINGLE);

    // Add a proposed candidate
    inbox
        .propose(make_draft("unapproved"), make_evidence())
        .unwrap();

    // List with state filter Approved only
    let approved = inbox.list(Some(MemoryCandidateState::Approved)).unwrap();
    assert!(approved.is_empty());

    // List with state filter Proposed
    let proposed = inbox.list(Some(MemoryCandidateState::Proposed)).unwrap();
    assert_eq!(proposed.len(), 1);
}

#[test]
fn list_by_state_filters_correctly() {
    let inbox = MemoryInbox::new(TenantId::SINGLE);

    let c1 = inbox
        .propose(make_draft("proposed"), make_evidence())
        .unwrap();
    let _ = inbox.reject(c1.id).unwrap();

    let c2 = inbox
        .propose(make_draft("approved"), make_evidence())
        .unwrap();
    let _ = inbox.approve(c2.id).unwrap();

    // All
    assert_eq!(inbox.list(None).unwrap().len(), 2);

    // By state
    assert_eq!(
        inbox
            .list(Some(MemoryCandidateState::Proposed))
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        inbox
            .list(Some(MemoryCandidateState::Rejected))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        inbox
            .list(Some(MemoryCandidateState::Approved))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn candidate_promotion_rechecks_policy_in_its_write_transaction() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.sqlite3");
    let inbox = MemoryInbox::open(db_path.to_str().unwrap(), TenantId::SINGLE).unwrap();
    let candidate = inbox
        .propose(make_draft("policy protected"), make_evidence())
        .unwrap();
    let settings = MemorySettingsStore::open(db_path.to_str().unwrap()).unwrap();
    let mut global = settings.get_global(TenantId::SINGLE).unwrap();
    global.generate_memories = false;
    settings.update_global(TenantId::SINGLE, global).unwrap();

    let result = inbox.promote_into_memory_for_actor(
        candidate.id,
        &MemoryActorContext {
            tenant_id: TenantId::SINGLE,
            user_id: None,
            team_id: None,
            session_id: None,
        },
        &MemoryActor::User { user_label: None },
        &MemoryPermissionContext {
            explicit_user_instruction: true,
            include_raw_content: false,
            action_plan_id: None,
            authorization_ticket_id: None,
            non_interactive_policy_grant: false,
        },
    );

    assert!(matches!(
        result,
        Err(harness_memory::MemoryCandidateMutationError::PolicyDenied(
            _
        ))
    ));
    assert_eq!(
        inbox.list(None).unwrap()[0].state,
        MemoryCandidateState::Proposed
    );
}

#[test]
fn candidate_merge_rechecks_policy_in_its_write_transaction() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.sqlite3");
    let inbox = MemoryInbox::open(db_path.to_str().unwrap(), TenantId::SINGLE).unwrap();
    let evidence = make_evidence();
    let first = inbox
        .propose(make_draft("first"), evidence.clone())
        .unwrap();
    let second = inbox
        .propose(make_draft("second"), evidence.clone())
        .unwrap();
    let settings = MemorySettingsStore::open(db_path.to_str().unwrap()).unwrap();
    let mut global = settings.get_global(TenantId::SINGLE).unwrap();
    global.generate_memories = false;
    settings.update_global(TenantId::SINGLE, global).unwrap();
    let merged_evidence = harness_memory::derive_merged_candidate_evidence(
        &[first.clone(), second.clone()],
        "merged",
    )
    .unwrap();

    let result = inbox.merge_into_memory(
        &[first.id, second.id],
        &make_record("merged", merged_evidence),
        &MemoryActor::User { user_label: None },
        &manual_permission(),
    );

    assert!(matches!(
        result,
        Err(harness_memory::MemoryCandidateMutationError::PolicyDenied(
            _
        ))
    ));
    assert!(inbox
        .list(None)
        .unwrap()
        .iter()
        .all(|candidate| candidate.state == MemoryCandidateState::Proposed));
}

#[test]
fn candidate_merge_uses_transaction_candidates_as_evidence_authority() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.sqlite3");
    let inbox = MemoryInbox::open(db_path.to_str().unwrap(), TenantId::SINGLE).unwrap();
    let evidence = make_evidence();
    let first = inbox
        .propose(make_draft("first"), evidence.clone())
        .unwrap();
    let second = inbox.propose(make_draft("second"), evidence).unwrap();

    let result = inbox.merge_into_memory(
        &[first.id, second.id],
        &make_record("forged", make_evidence()),
        &MemoryActor::User { user_label: None },
        &manual_permission(),
    );

    assert!(result.is_err());
    assert!(inbox
        .list(None)
        .unwrap()
        .iter()
        .all(|candidate| candidate.state == MemoryCandidateState::Proposed));
}

fn make_record(content: &str, evidence: MemoryEvidence) -> harness_memory::MemoryRecord {
    let now = chrono::Utc::now();
    harness_memory::MemoryRecord {
        id: MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::ProjectFact,
        visibility: MemoryVisibility::Tenant,
        content: content.to_owned(),
        metadata: harness_memory::MemoryMetadata {
            tags: Vec::new(),
            source: evidence.source.clone(),
            evidence: Some(evidence),
            confidence: 0.5,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 0.0,
            recall_score_breakdown: None,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: now,
        updated_at: now,
    }
}

fn manual_permission() -> MemoryPermissionContext {
    MemoryPermissionContext {
        explicit_user_instruction: true,
        include_raw_content: false,
        action_plan_id: None,
        authorization_ticket_id: None,
        non_interactive_policy_grant: false,
    }
}
