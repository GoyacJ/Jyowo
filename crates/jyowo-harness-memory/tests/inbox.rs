//! Tests for the memory candidate inbox.

use harness_contracts::*;
use harness_memory::inbox::MemoryInbox;

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
