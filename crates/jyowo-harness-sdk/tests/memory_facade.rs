#![cfg(feature = "testing")]

use std::sync::Arc;

use futures::executor::block_on;
use harness_contracts::{
    ActionPlanId, ContentHash, MemoryCandidateOperation, MemoryCandidateState, MemoryEvidence,
    MemoryEvidenceOrigin, MemoryKind, MemoryMetadata, MemoryRecordDraft, MemorySource,
    MemoryThreadMode, MemoryThreadSettings, MemoryVisibility, MessageId, NoopRedactor, RunId,
    SessionId, TenantId,
};
use jyowo_harness_sdk::{prelude::*, testing::*};

#[test]
fn memory_facade_uses_default_local_provider_without_external_provider() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-default-local-memory");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("builder should install default local memory provider");

        let items = harness
            .list_memory_items(SessionOptions::new(&workspace))
            .await
            .expect("default local provider should support memory listing");

        assert!(items.is_empty());
        assert!(memory_db_path(&workspace).exists());
    });
}

#[test]
fn memory_facade_merge_marks_candidates_merged() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-merge-memory-candidates");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = memory_harness(&workspace).await;
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let db_path = memory_db_path(&workspace);
        let inbox = harness_memory::MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("open memory inbox");
        let first = inbox
            .propose(
                memory_draft("merge candidate one"),
                memory_evidence(session_id, run_id, 1),
            )
            .expect("propose first");
        let second = inbox
            .propose(
                memory_draft("merge candidate two"),
                memory_evidence(session_id, run_id, 2),
            )
            .expect("propose second");

        harness
            .merge_memory_candidate(
                options,
                harness_contracts::MergeMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_ids: vec![first.id, second.id],
                    merged_record: memory_draft("merged memory"),
                    evidence: memory_evidence(session_id, run_id, 3),
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect("merge should write memory and update candidates");

        let merged = inbox
            .list(Some(MemoryCandidateState::Merged))
            .expect("list merged candidates");
        let merged_ids = merged
            .into_iter()
            .map(|candidate| candidate.id.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            merged_ids,
            std::collections::BTreeSet::from([first.id.to_string(), second.id.to_string()])
        );
    });
}

#[test]
fn memory_facade_merge_derives_evidence_from_candidates_not_request_payload() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-merge-derives-evidence");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = memory_harness(&workspace).await;
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let db_path = memory_db_path(&workspace);
        let inbox = harness_memory::MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("open memory inbox");
        let candidate = inbox
            .propose(
                memory_draft("trusted candidate evidence"),
                memory_evidence(session_id, run_id, 1),
            )
            .expect("propose candidate");
        let mut forged_evidence = memory_evidence(session_id, run_id, 9);
        forged_evidence.source = MemorySource::WebRetrieval;
        forged_evidence.origin = MemoryEvidenceOrigin::WebRetrieval {
            url_hash: ContentHash([9; 32]),
            fetch_tool_use_id: None,
        };

        let merged = harness
            .merge_memory_candidate(
                options.clone(),
                harness_contracts::MergeMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_ids: vec![candidate.id],
                    merged_record: memory_draft("merged memory"),
                    evidence: forged_evidence,
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect("merge should ignore forged request evidence");

        let item = harness
            .get_memory_item(options, merged.memory_id)
            .await
            .expect("merged memory should exist");
        assert_eq!(item.metadata.source, MemorySource::UserInput);
    });
}

#[test]
fn memory_facade_merge_rejects_missing_candidate_before_write() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-merge-memory-missing-candidate");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = memory_harness(&workspace).await;
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let db_path = memory_db_path(&workspace);
        let inbox = harness_memory::MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("open memory inbox");
        let first = inbox
            .propose(
                memory_draft("merge candidate one"),
                memory_evidence(session_id, run_id, 1),
            )
            .expect("propose first");

        let error = harness
            .merge_memory_candidate(
                options.clone(),
                harness_contracts::MergeMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_ids: vec![first.id, harness_contracts::MemoryCandidateId::new()],
                    merged_record: memory_draft("merged memory should not be written"),
                    evidence: memory_evidence(session_id, run_id, 3),
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect_err("missing candidate should fail before memory write");

        assert!(error.to_string().contains("candidate not found"));
        let items = harness
            .list_memory_items(options)
            .await
            .expect("list memory items after failed merge");
        assert!(items.is_empty());
    });
}

#[test]
fn memory_facade_merge_rejects_duplicate_candidates_before_write() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-merge-duplicate-candidates");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = memory_harness(&workspace).await;
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let db_path = memory_db_path(&workspace);
        let inbox = harness_memory::MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("open memory inbox");
        let candidate = inbox
            .propose(
                memory_draft("duplicate candidate should not merge"),
                memory_evidence(session_id, run_id, 1),
            )
            .expect("propose candidate");

        let error = harness
            .merge_memory_candidate(
                options.clone(),
                harness_contracts::MergeMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_ids: vec![candidate.id, candidate.id],
                    merged_record: memory_draft("merged memory should not be written"),
                    evidence: memory_evidence(session_id, run_id, 2),
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect_err("duplicate candidates should fail before memory write");

        assert!(error.to_string().contains("candidate ids must be distinct"));
        let items = harness
            .list_memory_items(options)
            .await
            .expect("list memory items after failed merge");
        assert!(items.is_empty());
    });
}

#[test]
fn memory_facade_approve_candidate_obeys_policy_before_write() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-approve-memory-policy-deny");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = memory_harness(&workspace).await;
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let db_path = memory_db_path(&workspace);
        let inbox = harness_memory::MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("open memory inbox");
        let candidate = inbox
            .propose(
                memory_draft("policy denied approval should not write"),
                memory_evidence(session_id, run_id, 1),
            )
            .expect("propose candidate");
        set_read_only(&db_path, session_id);

        let error = harness
            .approve_memory_candidate(
                options.clone(),
                harness_contracts::ApproveMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_id: candidate.id,
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect_err("policy deny should reject candidate approval");

        assert!(error.to_string().contains("memory write denied by policy"));
        let items = harness
            .list_memory_items(options)
            .await
            .expect("list memory items after failed approval");
        assert!(items.is_empty());
    });
}

#[test]
fn memory_facade_merge_rejects_cross_session_candidate_before_write() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-merge-cross-session-candidate");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = memory_harness(&workspace).await;
        let session_id = SessionId::new();
        let other_session_id = SessionId::new();
        let run_id = RunId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let db_path = memory_db_path(&workspace);
        let inbox = harness_memory::MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("open memory inbox");
        let first = inbox
            .propose(
                memory_draft("merge candidate one"),
                memory_evidence(session_id, run_id, 1),
            )
            .expect("propose first");
        let second = inbox
            .propose(
                memory_draft("merge candidate two from another session"),
                memory_evidence(other_session_id, run_id, 2),
            )
            .expect("propose second");

        let error = harness
            .merge_memory_candidate(
                options.clone(),
                harness_contracts::MergeMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_ids: vec![first.id, second.id],
                    merged_record: memory_draft("merged memory should not be written"),
                    evidence: memory_evidence(session_id, run_id, 3),
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect_err("cross-session candidate should fail before memory write");

        assert!(error
            .to_string()
            .contains("candidate does not belong to session"));
        let items = harness
            .list_memory_items(options)
            .await
            .expect("list memory items after failed merge");
        assert!(items.is_empty());
    });
}

#[test]
fn memory_facade_update_and_delete_obey_policy() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-update-delete-memory-policy-deny");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = memory_harness(&workspace).await;
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let db_path = memory_db_path(&workspace);
        let inbox = harness_memory::MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("open memory inbox");
        let candidate = inbox
            .propose(
                memory_draft("editable memory"),
                memory_evidence(session_id, run_id, 1),
            )
            .expect("propose candidate");
        let approved = harness
            .approve_memory_candidate(
                options.clone(),
                harness_contracts::ApproveMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_id: candidate.id,
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect("approval should seed durable memory");
        set_read_only(&db_path, session_id);

        let update_error = harness
            .update_memory_item_content(
                options.clone(),
                approved.memory_id,
                "updated content",
                Some(ActionPlanId::new()),
            )
            .await
            .expect_err("policy deny should reject memory update");
        assert!(update_error
            .to_string()
            .contains("memory write denied by policy"));

        let delete_error = harness
            .delete_memory_item(
                options.clone(),
                approved.memory_id,
                Some(ActionPlanId::new()),
            )
            .await
            .expect_err("policy deny should reject memory delete");
        assert!(delete_error
            .to_string()
            .contains("memory write denied by policy"));

        let item = harness
            .get_memory_item(options, approved.memory_id)
            .await
            .expect("memory item should still exist");
        assert_eq!(item.content, "editable memory");
    });
}

#[test]
fn memory_facade_reject_rejects_cross_session_candidate() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-reject-cross-session-candidate");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = memory_harness(&workspace).await;
        let session_id = SessionId::new();
        let other_session_id = SessionId::new();
        let run_id = RunId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let db_path = memory_db_path(&workspace);
        let inbox = harness_memory::MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("open memory inbox");
        let candidate = inbox
            .propose(
                memory_draft("reject candidate from another session"),
                memory_evidence(other_session_id, run_id, 1),
            )
            .expect("propose candidate");

        let error = harness
            .reject_memory_candidate(
                options,
                harness_contracts::RejectMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_id: candidate.id,
                    reason: "not for this session".to_owned(),
                },
            )
            .await
            .expect_err("cross-session candidate should not be rejected");

        assert!(error
            .to_string()
            .contains("candidate does not belong to session"));
        let still_proposed = inbox
            .list(Some(MemoryCandidateState::Proposed))
            .expect("list proposed candidates");
        assert_eq!(still_proposed.len(), 1);
        assert_eq!(still_proposed[0].id, candidate.id);
    });
}

#[test]
fn memory_facade_reject_rejects_non_proposed_candidate() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-reject-non-proposed-candidate");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = memory_harness(&workspace).await;
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let db_path = memory_db_path(&workspace);
        let inbox = harness_memory::MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("open memory inbox");
        let candidate = inbox
            .propose(
                memory_draft("already promoted candidate"),
                memory_evidence(session_id, run_id, 1),
            )
            .expect("propose candidate");
        let approved = harness
            .approve_memory_candidate(
                options.clone(),
                harness_contracts::ApproveMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_id: candidate.id,
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect("approval should promote candidate");

        let error = harness
            .reject_memory_candidate(
                options,
                harness_contracts::RejectMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_id: approved.candidate.id,
                    reason: "too late".to_owned(),
                },
            )
            .await
            .expect_err("promoted candidate should not be rejected");

        assert!(error.to_string().contains("candidate is not proposed"));
        let promoted = inbox
            .list(Some(MemoryCandidateState::Promoted))
            .expect("list promoted candidates");
        assert_eq!(promoted.len(), 1);
        assert_eq!(promoted[0].id, candidate.id);
    });
}

#[test]
fn memory_facade_approve_update_candidate_updates_existing_memory() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-approve-update-candidate");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = memory_harness(&workspace).await;
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let db_path = memory_db_path(&workspace);
        let inbox = harness_memory::MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("open memory inbox");
        let seed = inbox
            .propose(
                memory_draft("original memory"),
                memory_evidence(session_id, run_id, 1),
            )
            .expect("propose seed");
        let approved = harness
            .approve_memory_candidate(
                options.clone(),
                harness_contracts::ApproveMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_id: seed.id,
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect("approval should create seed memory");
        let update = inbox
            .propose_with_operation(
                MemoryCandidateOperation::Update {
                    memory_id: approved.memory_id,
                },
                memory_draft("updated memory"),
                memory_evidence(session_id, run_id, 2),
            )
            .expect("propose update");

        let response = harness
            .approve_memory_candidate(
                options.clone(),
                harness_contracts::ApproveMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_id: update.id,
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect("approval should update existing memory");

        assert_eq!(response.memory_id, approved.memory_id);
        let item = harness
            .get_memory_item(options.clone(), approved.memory_id)
            .await
            .expect("updated memory should exist");
        assert_eq!(item.content, "updated memory");
        let items = harness
            .list_memory_items(options)
            .await
            .expect("list memory items");
        assert_eq!(items.len(), 1);
    });
}

#[test]
fn memory_facade_approve_delete_candidate_deletes_existing_memory() {
    block_on(async {
        let workspace = unique_workspace("sdk-facade-approve-delete-candidate");
        std::fs::create_dir_all(&workspace).unwrap();

        let harness = memory_harness(&workspace).await;
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let options = SessionOptions::new(&workspace).with_session_id(session_id);
        let db_path = memory_db_path(&workspace);
        let inbox = harness_memory::MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("open memory inbox");
        let seed = inbox
            .propose(
                memory_draft("delete me"),
                memory_evidence(session_id, run_id, 1),
            )
            .expect("propose seed");
        let approved = harness
            .approve_memory_candidate(
                options.clone(),
                harness_contracts::ApproveMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_id: seed.id,
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect("approval should create seed memory");
        let delete = inbox
            .propose_with_operation(
                MemoryCandidateOperation::Delete {
                    memory_id: approved.memory_id,
                },
                memory_draft("delete me"),
                memory_evidence(session_id, run_id, 2),
            )
            .expect("propose delete");

        let response = harness
            .approve_memory_candidate(
                options.clone(),
                harness_contracts::ApproveMemoryCandidateRequest {
                    tenant_id: TenantId::SINGLE,
                    candidate_id: delete.id,
                    action_plan_id: Some(ActionPlanId::new()),
                },
            )
            .await
            .expect("approval should delete existing memory");

        assert_eq!(response.memory_id, approved.memory_id);
        let items = harness
            .list_memory_items(options)
            .await
            .expect("list memory items");
        assert!(items.is_empty());
    });
}

async fn memory_harness(workspace: &std::path::Path) -> Harness {
    Harness::builder()
        .with_workspace_root(workspace)
        .with_model(TestModelProvider::default())
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .expect("builder should create memory facade")
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-{name}-{}-{}",
        std::process::id(),
        SessionId::new()
    ))
}

fn memory_db_path(workspace: &std::path::Path) -> std::path::PathBuf {
    workspace
        .join(".jyowo")
        .join("runtime")
        .join("memory")
        .join("memory.sqlite3")
}

fn memory_draft(content: &str) -> MemoryRecordDraft {
    MemoryRecordDraft {
        kind: MemoryKind::ProjectFact,
        visibility: MemoryVisibility::Tenant,
        content: content.to_owned(),
        metadata: MemoryMetadata {
            ttl: None,
            tags: vec![],
            source_trust: 0.8,
        },
        expires_at: None,
    }
}

fn memory_evidence(session_id: SessionId, run_id: RunId, seed: u8) -> MemoryEvidence {
    MemoryEvidence {
        source: MemorySource::UserInput,
        origin: MemoryEvidenceOrigin::UserMessage {
            session_id,
            run_id,
            message_id: MessageId::new(),
        },
        content_hash: ContentHash([seed; 32]),
        session_id: Some(session_id),
        run_id: Some(run_id),
        message_id: None,
        tool_use_id: None,
    }
}

fn set_read_only(db_path: &std::path::Path, session_id: SessionId) {
    let settings = harness_memory::settings::MemorySettingsStore::open(&db_path.to_string_lossy())
        .expect("open memory settings");
    settings
        .update_thread(
            TenantId::SINGLE,
            MemoryThreadSettings {
                session_id,
                use_memories: None,
                generate_memories: None,
                memory_mode: MemoryThreadMode::ReadOnly,
            },
        )
        .expect("set read-only thread memory mode");
}
