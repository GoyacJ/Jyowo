use std::fs;

use harness_contracts::{
    ActionPlanId, ApproveMemoryCandidateRequest, ClientRequest, ContentHash,
    ExportMemoryItemsRequest, GetMemorySettingsRequest, GetModelRequestPreviewRequest,
    ListMemoryCandidatesRequest, ListMemoryRecallTracesRequest, MemoryEvidence,
    MemoryEvidenceOrigin, MemoryGlobalSettings, MemoryKind, MemoryModelRequestPreview,
    MemoryModelRequestPreviewSection, MemoryRecordDraft, MemorySource, MemoryVisibility,
    MergeMemoryCandidateRequest, RunId, ServerMessage, SessionId, TenantId,
    UpdateMemorySettingsRequest,
};
use harness_daemon::{MemoryService, MemoryServiceError, RuntimeConfigResolver};
use harness_memory::{
    local::LocalMemoryProvider, MemoryInbox, MemoryMetadata, MemoryRecallTraceBuilder,
    MemoryRecallTraceCollector, MemoryRecord, MemorySettingsStore, MemoryStore,
};
use tempfile::TempDir;

#[test]
fn memory_service_resolves_workspace_and_global_databases_through_runtime_config() {
    let root = TempDir::new().expect("temp root");
    let home = root.path().join("home");
    let config = home.join("config");
    let workspace = root.path().join("workspace");
    let other_workspace = root.path().join("other-workspace");
    fs::create_dir_all(&config).expect("config root");
    fs::create_dir_all(&workspace).expect("workspace root");
    fs::create_dir_all(&other_workspace).expect("other workspace root");

    let service = MemoryService::new(RuntimeConfigResolver::new(&config));
    let first = service
        .database_path(Some(&workspace))
        .expect("first workspace memory database");
    let same = service
        .database_path(Some(&workspace))
        .expect("same workspace memory database");
    let other = service
        .database_path(Some(&other_workspace))
        .expect("other workspace memory database");
    let global = service.database_path(None).expect("global memory database");

    assert_eq!(first, same);
    assert_ne!(first, other);
    assert_ne!(first, global);
    let canonical_home = home.canonicalize().expect("canonical home");
    assert!(first.starts_with(canonical_home.join("runtime/workspaces")));
    assert_eq!(global, canonical_home.join("runtime/memory/memory.sqlite3"));
}

#[cfg(unix)]
#[tokio::test]
async fn memory_service_rejects_symlinked_daemon_runtime_parent() {
    use std::os::unix::fs::symlink;

    let root = TempDir::new().expect("temp root");
    let config = root.path().join("home/config");
    let workspace = root.path().join("workspace");
    let external = root.path().join("external-runtime");
    fs::create_dir_all(&config).expect("config root");
    fs::create_dir_all(&workspace).expect("workspace root");
    fs::create_dir_all(&external).expect("external runtime root");
    symlink(&external, root.path().join("home/runtime")).expect("runtime symlink");
    let service = MemoryService::new(RuntimeConfigResolver::new(config));

    let result = service
        .handle(ClientRequest::ListMemoryItems {
            workspace_root: Some(workspace.to_string_lossy().into_owned()),
        })
        .await;

    assert!(result.is_err(), "daemon runtime symlink must fail closed");
    assert!(
        !external.join("workspaces").exists(),
        "memory open must not write through the symlink"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn memory_service_rejects_symlinked_sqlite_files_and_sidecars() {
    use std::os::unix::fs::symlink;

    for suffix in ["", "-wal", "-shm"] {
        let root = TempDir::new().expect("temp root");
        let config = root.path().join("home/config");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&config).expect("config root");
        fs::create_dir_all(&workspace).expect("workspace root");
        let service = MemoryService::new(RuntimeConfigResolver::new(config));
        let db_path = service
            .database_path(Some(&workspace))
            .expect("workspace database");
        let external = root.path().join(format!("external{suffix}"));
        fs::write(&external, []).expect("external file");
        symlink(&external, format!("{}{suffix}", db_path.to_string_lossy()))
            .expect("sqlite symlink");

        let result = service
            .handle(ClientRequest::ListMemoryItems {
                workspace_root: Some(workspace.to_string_lossy().into_owned()),
            })
            .await;

        assert!(
            matches!(result, Err(MemoryServiceError::RuntimeConfig(_))),
            "sqlite path suffix {suffix:?} must fail during secure path validation"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn memory_export_rejects_symlinked_export_directory() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let provider = LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
        .expect("local provider");
    provider
        .upsert(memory_record("must stay inside daemon storage"))
        .await
        .expect("insert memory");
    let external = fixture.workspace.join("external-exports");
    fs::create_dir_all(&external).expect("external exports");
    symlink(
        &external,
        db_path.parent().expect("memory parent").join("exports"),
    )
    .expect("exports symlink");

    let result = fixture
        .service
        .handle(ClientRequest::ExportMemoryItems {
            workspace_root: Some(fixture.workspace_string()),
            request: ExportMemoryItemsRequest {
                session_id: None,
                scope: "visible".to_owned(),
                format: "json".to_owned(),
                include_raw_content: true,
                include_metadata: true,
                include_hashes: true,
                explicit_user_action: true,
            },
        })
        .await;

    assert!(result.is_err());
    assert_eq!(fs::read_dir(external).expect("external exports").count(), 0);
}

#[tokio::test]
async fn task_runtime_local_provider_and_memory_service_share_authoritative_records() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let provider = LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
        .expect("task runtime local memory provider");
    let original = memory_record("original task memory");
    let memory_id = original.id;
    provider.upsert(original).await.expect("task writes memory");

    let listed = fixture
        .service
        .handle(ClientRequest::ListMemoryItems {
            workspace_root: Some(fixture.workspace_string()),
        })
        .await
        .expect("list memory");
    let ServerMessage::MemoryItems(listed) = listed else {
        panic!("expected memory items");
    };
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].id, memory_id);

    let updated = fixture
        .service
        .handle(ClientRequest::UpdateMemoryItem {
            workspace_root: Some(fixture.workspace_string()),
            memory_id,
            content: "updated by daemon".to_owned(),
            action_plan_id: None,
        })
        .await
        .expect("update memory");
    let ServerMessage::MemoryUpdated(updated) = updated else {
        panic!("expected updated memory");
    };
    assert_eq!(updated.item.content, "updated by daemon");
    assert_eq!(
        provider
            .get(memory_id)
            .await
            .expect("task reads update")
            .content,
        "updated by daemon"
    );

    fixture
        .service
        .handle(ClientRequest::DeleteMemoryItem {
            workspace_root: Some(fixture.workspace_string()),
            memory_id,
            action_plan_id: None,
        })
        .await
        .expect("delete memory");
    assert!(provider.get(memory_id).await.is_err());
}

#[tokio::test]
async fn memory_management_only_exposes_tenant_visible_records() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let provider = LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
        .expect("local provider");
    let tenant = memory_record("tenant-visible");
    let mut private = memory_record("private-secret");
    private.visibility = MemoryVisibility::Private {
        session_id: SessionId::new(),
    };
    let private_id = private.id;
    provider.upsert(tenant).await.expect("insert tenant memory");
    provider
        .upsert(private)
        .await
        .expect("insert private memory");

    let listed = fixture
        .service
        .handle(ClientRequest::ListMemoryItems {
            workspace_root: Some(fixture.workspace_string()),
        })
        .await
        .expect("list visible memories");
    let ServerMessage::MemoryItems(listed) = listed else {
        panic!("expected memory items");
    };
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].content_preview, "tenant-visible");

    assert!(fixture
        .service
        .handle(ClientRequest::GetMemoryItem {
            workspace_root: Some(fixture.workspace_string()),
            memory_id: private_id,
        })
        .await
        .is_err());
    assert!(fixture
        .service
        .handle(ClientRequest::UpdateMemoryItem {
            workspace_root: Some(fixture.workspace_string()),
            memory_id: private_id,
            content: "leaked update".to_owned(),
            action_plan_id: None,
        })
        .await
        .is_err());
    assert!(fixture
        .service
        .handle(ClientRequest::DeleteMemoryItem {
            workspace_root: Some(fixture.workspace_string()),
            memory_id: private_id,
            action_plan_id: None,
        })
        .await
        .is_err());
    assert_eq!(
        provider
            .get(private_id)
            .await
            .expect("private remains")
            .content,
        "private-secret"
    );

    let exported = fixture
        .service
        .handle(ClientRequest::ExportMemoryItems {
            workspace_root: Some(fixture.workspace_string()),
            request: ExportMemoryItemsRequest {
                session_id: None,
                scope: "visible".to_owned(),
                format: "json".to_owned(),
                include_raw_content: true,
                include_metadata: true,
                include_hashes: true,
                explicit_user_action: true,
            },
        })
        .await
        .expect("export visible memories");
    let ServerMessage::MemoryExported(exported) = exported else {
        panic!("expected memory export");
    };
    assert_eq!(exported.item_count, 1);
    let export = fs::read_to_string(exported.path).expect("read export");
    assert!(export.contains("tenant-visible"));
    assert!(!export.contains("private-secret"));
}

#[tokio::test]
async fn updating_memory_rejects_content_larger_than_64_kib() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let provider = LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
        .expect("local provider");
    let record = memory_record("original");
    let memory_id = record.id;
    provider.upsert(record).await.expect("insert memory");

    let result = fixture
        .service
        .handle(ClientRequest::UpdateMemoryItem {
            workspace_root: Some(fixture.workspace_string()),
            memory_id,
            content: "x".repeat(64 * 1024 + 1),
            action_plan_id: None,
        })
        .await;

    assert!(result.is_err());
    assert_eq!(
        provider
            .get(memory_id)
            .await
            .expect("memory remains")
            .content,
        "original"
    );
}

#[tokio::test]
async fn updating_memory_settings_rejects_zero_capacity_limits() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let store = MemorySettingsStore::open(&db_path.to_string_lossy()).expect("settings store");
    let original = MemoryGlobalSettings {
        use_memories: true,
        generate_memories: true,
        disable_generation_when_external_context_used: true,
        retention_days: None,
        max_memory_bytes: 1024,
        max_recall_records_per_turn: 4,
        max_recall_chars_per_turn: 2048,
    };
    store
        .update_global(TenantId::SINGLE, original.clone())
        .expect("write original settings");
    let mut invalid_settings = Vec::new();
    invalid_settings.push(MemoryGlobalSettings {
        max_memory_bytes: 0,
        ..original.clone()
    });
    invalid_settings.push(MemoryGlobalSettings {
        max_recall_records_per_turn: 0,
        ..original.clone()
    });
    invalid_settings.push(MemoryGlobalSettings {
        max_recall_chars_per_turn: 0,
        ..original.clone()
    });

    for settings in invalid_settings {
        let result = fixture
            .service
            .handle(ClientRequest::UpdateMemorySettings {
                workspace_root: Some(fixture.workspace_string()),
                request: UpdateMemorySettingsRequest {
                    tenant_id: TenantId::SINGLE,
                    settings,
                },
            })
            .await;
        assert!(result.is_err());
        assert_eq!(
            store
                .get_global(TenantId::SINGLE)
                .expect("settings remain readable"),
            original
        );
    }
}

#[tokio::test]
async fn mutations_reject_unsupported_action_plans_before_writing() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let provider = LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
        .expect("local provider");
    let record = memory_record("original");
    let memory_id = record.id;
    provider.upsert(record).await.expect("insert memory");
    let inbox = MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE).expect("inbox");
    let approve = inbox
        .propose(candidate_draft("approve"), candidate_evidence())
        .expect("approve candidate");
    let merge_one = inbox
        .propose(candidate_draft("merge one"), candidate_evidence())
        .expect("first merge candidate");
    let merge_two = inbox
        .propose(candidate_draft("merge two"), candidate_evidence())
        .expect("second merge candidate");
    let action_plan_id = ActionPlanId::new();

    let requests = [
        ClientRequest::UpdateMemoryItem {
            workspace_root: Some(fixture.workspace_string()),
            memory_id,
            content: "changed".to_owned(),
            action_plan_id: Some(action_plan_id),
        },
        ClientRequest::DeleteMemoryItem {
            workspace_root: Some(fixture.workspace_string()),
            memory_id,
            action_plan_id: Some(action_plan_id),
        },
        ClientRequest::ApproveMemoryCandidate {
            workspace_root: Some(fixture.workspace_string()),
            request: ApproveMemoryCandidateRequest {
                tenant_id: TenantId::SINGLE,
                candidate_id: approve.id,
                action_plan_id: Some(action_plan_id),
            },
        },
        ClientRequest::MergeMemoryCandidate {
            workspace_root: Some(fixture.workspace_string()),
            request: MergeMemoryCandidateRequest {
                tenant_id: TenantId::SINGLE,
                candidate_ids: vec![merge_one.id, merge_two.id],
                merged_record: candidate_draft("merged"),
                evidence: candidate_evidence(),
                action_plan_id: Some(action_plan_id),
            },
        },
    ];

    for request in requests {
        assert!(fixture.service.handle(request).await.is_err());
    }
    assert_eq!(
        provider
            .get(memory_id)
            .await
            .expect("memory remains")
            .content,
        "original"
    );
    assert_eq!(
        provider
            .list(harness_memory::MemoryListScope::All)
            .await
            .expect("list memories")
            .len(),
        1
    );
    assert!(inbox
        .list(None)
        .expect("list candidates")
        .iter()
        .all(|candidate| candidate.state == harness_contracts::MemoryCandidateState::Proposed));
}

#[tokio::test]
async fn candidates_traces_and_settings_use_the_same_workspace_database() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let session_id = SessionId::new();
    let evidence = MemoryEvidence {
        source: MemorySource::AgentDerived,
        origin: MemoryEvidenceOrigin::Imported {
            importer: "test".to_owned(),
            import_id: "candidate".to_owned(),
        },
        content_hash: ContentHash([7; 32]),
        session_id: Some(session_id),
        run_id: None,
        message_id: None,
        tool_use_id: None,
    };
    MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE)
        .expect("inbox")
        .propose(
            MemoryRecordDraft {
                kind: MemoryKind::ProjectFact,
                visibility: MemoryVisibility::Tenant,
                content: "candidate".to_owned(),
                metadata: harness_contracts::MemoryMetadata {
                    ttl: None,
                    tags: vec!["test".to_owned()],
                    source_trust: 0.8,
                },
                expires_at: None,
            },
            evidence,
        )
        .expect("propose candidate");
    let trace = MemoryRecallTraceBuilder::new_for_tenant(
        TenantId::SINGLE,
        session_id,
        RunId::new(),
        1,
        ContentHash([9; 32]),
    )
    .build();
    MemoryRecallTraceCollector::open(&db_path.to_string_lossy())
        .expect("trace collector")
        .add(trace);
    let settings = MemoryGlobalSettings {
        use_memories: false,
        generate_memories: false,
        disable_generation_when_external_context_used: true,
        retention_days: Some(7),
        max_memory_bytes: 1234,
        max_recall_records_per_turn: 3,
        max_recall_chars_per_turn: 456,
    };
    MemorySettingsStore::open(&db_path.to_string_lossy())
        .expect("settings store")
        .update_global(TenantId::SINGLE, settings.clone())
        .expect("write settings");

    let candidates = fixture
        .service
        .handle(ClientRequest::ListMemoryCandidates {
            workspace_root: Some(fixture.workspace_string()),
            request: ListMemoryCandidatesRequest {
                tenant_id: TenantId::SINGLE,
                session_id: None,
                state: None,
                limit: 50,
                cursor: None,
            },
        })
        .await
        .expect("list candidates");
    let ServerMessage::MemoryCandidates(candidates) = candidates else {
        panic!("expected candidates");
    };
    assert_eq!(candidates.candidates.len(), 1);

    let traces = fixture
        .service
        .handle(ClientRequest::ListMemoryRecallTraces {
            workspace_root: Some(fixture.workspace_string()),
            request: ListMemoryRecallTracesRequest {
                tenant_id: TenantId::SINGLE,
                session_id: None,
                run_id: None,
                limit: 30,
                cursor: None,
            },
        })
        .await
        .expect("list traces");
    let ServerMessage::MemoryRecallTraces(traces) = traces else {
        panic!("expected traces");
    };
    assert_eq!(traces.traces.len(), 1);

    let loaded = fixture
        .service
        .handle(ClientRequest::GetMemorySettings {
            workspace_root: Some(fixture.workspace_string()),
            request: GetMemorySettingsRequest {
                tenant_id: TenantId::SINGLE,
            },
        })
        .await
        .expect("get settings");
    let ServerMessage::MemorySettings(loaded) = loaded else {
        panic!("expected settings");
    };
    assert_eq!(loaded.settings, settings);

    let changed = MemoryGlobalSettings {
        use_memories: true,
        ..settings
    };
    fixture
        .service
        .handle(ClientRequest::UpdateMemorySettings {
            workspace_root: Some(fixture.workspace_string()),
            request: UpdateMemorySettingsRequest {
                tenant_id: TenantId::SINGLE,
                settings: changed.clone(),
            },
        })
        .await
        .expect("update settings");
    assert_eq!(
        MemorySettingsStore::open(&db_path.to_string_lossy())
            .expect("reopen settings")
            .get_global(TenantId::SINGLE)
            .expect("read changed settings"),
        changed
    );
}

#[tokio::test]
async fn approving_a_promoted_candidate_does_not_create_an_orphan_memory() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let inbox = MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE).expect("inbox");
    let candidate = inbox
        .propose(candidate_draft("approve once"), candidate_evidence())
        .expect("propose candidate");
    let request = ClientRequest::ApproveMemoryCandidate {
        workspace_root: Some(fixture.workspace_string()),
        request: ApproveMemoryCandidateRequest {
            tenant_id: TenantId::SINGLE,
            candidate_id: candidate.id,
            action_plan_id: None,
        },
    };

    fixture
        .service
        .handle(request.clone())
        .await
        .expect("first approval");
    let second = fixture.service.handle(request).await;

    assert!(second.is_err());
    let provider = LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
        .expect("local provider");
    assert_eq!(
        provider
            .list(harness_memory::MemoryListScope::All)
            .await
            .expect("list memories")
            .len(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_candidate_approval_creates_exactly_one_memory() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let inbox = MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE).expect("inbox");
    let candidate = inbox
        .propose(
            candidate_draft("approve concurrently"),
            candidate_evidence(),
        )
        .expect("propose candidate");
    let request = ClientRequest::ApproveMemoryCandidate {
        workspace_root: Some(fixture.workspace_string()),
        request: ApproveMemoryCandidateRequest {
            tenant_id: TenantId::SINGLE,
            candidate_id: candidate.id,
            action_plan_id: None,
        },
    };
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
    let first_service = fixture.service.clone();
    let first_request = request.clone();
    let first_barrier = barrier.clone();
    let first = tokio::spawn(async move {
        first_barrier.wait().await;
        first_service.handle(first_request).await
    });
    let second_service = fixture.service.clone();
    let second_barrier = barrier.clone();
    let second = tokio::spawn(async move {
        second_barrier.wait().await;
        second_service.handle(request).await
    });

    barrier.wait().await;
    let results = [
        first.await.expect("first task"),
        second.await.expect("second task"),
    ];

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let provider = LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
        .expect("local provider");
    assert_eq!(
        provider
            .list(harness_memory::MemoryListScope::All)
            .await
            .expect("list memories")
            .len(),
        1
    );
}

#[tokio::test]
async fn candidate_approval_rolls_back_memory_when_state_transition_fails() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let inbox = MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE).expect("inbox");
    let candidate = inbox
        .propose(candidate_draft("atomic approval"), candidate_evidence())
        .expect("propose candidate");
    rusqlite::Connection::open(&db_path)
        .expect("sqlite connection")
        .execute_batch(
            "CREATE TRIGGER reject_candidate_promotion
             BEFORE UPDATE ON memory_candidates
             BEGIN
               SELECT RAISE(ABORT, 'forced candidate transition failure');
             END;",
        )
        .expect("install failure trigger");

    let result = fixture
        .service
        .handle(ClientRequest::ApproveMemoryCandidate {
            workspace_root: Some(fixture.workspace_string()),
            request: ApproveMemoryCandidateRequest {
                tenant_id: TenantId::SINGLE,
                candidate_id: candidate.id,
                action_plan_id: None,
            },
        })
        .await;

    assert!(result.is_err());
    let provider = LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
        .expect("local provider");
    assert!(provider
        .list(harness_memory::MemoryListScope::All)
        .await
        .expect("list memories")
        .is_empty());
}

#[tokio::test]
async fn merging_duplicate_candidate_ids_is_rejected_without_partial_writes() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let inbox = MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE).expect("inbox");
    let candidate = inbox
        .propose(candidate_draft("duplicate merge"), candidate_evidence())
        .expect("propose candidate");

    let result = fixture
        .service
        .handle(ClientRequest::MergeMemoryCandidate {
            workspace_root: Some(fixture.workspace_string()),
            request: MergeMemoryCandidateRequest {
                tenant_id: TenantId::SINGLE,
                candidate_ids: vec![candidate.id, candidate.id],
                merged_record: candidate_draft("merged"),
                evidence: candidate_evidence(),
                action_plan_id: None,
            },
        })
        .await;

    assert!(result.is_err());
    let provider = LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
        .expect("local provider");
    assert!(provider
        .list(harness_memory::MemoryListScope::All)
        .await
        .expect("list memories")
        .is_empty());
    assert_eq!(
        inbox.list(None).expect("list candidates")[0].state,
        harness_contracts::MemoryCandidateState::Proposed
    );
}

#[tokio::test]
async fn candidate_merge_rolls_back_all_writes_when_one_transition_fails() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let inbox = MemoryInbox::open(&db_path.to_string_lossy(), TenantId::SINGLE).expect("inbox");
    let first = inbox
        .propose(candidate_draft("merge first"), candidate_evidence())
        .expect("first candidate");
    let second = inbox
        .propose(candidate_draft("merge second"), candidate_evidence())
        .expect("second candidate");
    rusqlite::Connection::open(&db_path)
        .expect("sqlite connection")
        .execute_batch(&format!(
            "CREATE TRIGGER reject_second_candidate_merge
             BEFORE UPDATE ON memory_candidates
             WHEN OLD.id = '{}'
             BEGIN
               SELECT RAISE(ABORT, 'forced second transition failure');
             END;",
            second.id
        ))
        .expect("install failure trigger");

    let result = fixture
        .service
        .handle(ClientRequest::MergeMemoryCandidate {
            workspace_root: Some(fixture.workspace_string()),
            request: MergeMemoryCandidateRequest {
                tenant_id: TenantId::SINGLE,
                candidate_ids: vec![first.id, second.id],
                merged_record: candidate_draft("atomic merge"),
                evidence: candidate_evidence(),
                action_plan_id: None,
            },
        })
        .await;

    assert!(result.is_err());
    let provider = LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
        .expect("local provider");
    assert!(provider
        .list(harness_memory::MemoryListScope::All)
        .await
        .expect("list memories")
        .is_empty());
    assert!(inbox
        .list(None)
        .expect("list candidates")
        .iter()
        .all(|candidate| candidate.state == harness_contracts::MemoryCandidateState::Proposed));
}

#[tokio::test]
async fn model_request_preview_is_loaded_from_the_authoritative_workspace_database() {
    let fixture = Fixture::new();
    let db_path = fixture
        .service
        .database_path(Some(&fixture.workspace))
        .expect("workspace database");
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let preview = MemoryModelRequestPreview {
        session_id,
        run_id,
        trace_id: None,
        sections: vec![MemoryModelRequestPreviewSection {
            source: MemorySource::AgentDerived,
            provider_id: Some("local".to_owned()),
            memory_ids: Vec::new(),
            redacted_content: "[memory redacted]".to_owned(),
        }],
        redacted_count: 1,
        token_estimate: 4,
        tool_names: vec!["memory".to_owned()],
        policy_decisions: vec!["allow".to_owned()],
        content_hash: ContentHash([3; 32]),
    };
    MemoryRecallTraceCollector::open(&db_path.to_string_lossy())
        .expect("trace collector")
        .add_model_request_preview(TenantId::SINGLE, preview.clone());

    let response = fixture
        .service
        .handle(ClientRequest::GetModelRequestPreview {
            workspace_root: Some(fixture.workspace_string()),
            request: GetModelRequestPreviewRequest {
                tenant_id: TenantId::SINGLE,
                session_id,
                run_id,
                trace_id: None,
            },
        })
        .await
        .expect("get model request preview");

    let ServerMessage::ModelRequestPreview(response) = response else {
        panic!("expected model request preview");
    };
    assert_eq!(response.preview, preview);
}

struct Fixture {
    _root: TempDir,
    workspace: std::path::PathBuf,
    service: MemoryService,
}

impl Fixture {
    fn new() -> Self {
        let root = TempDir::new().expect("temp root");
        let config = root.path().join("home/config");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&config).expect("config root");
        fs::create_dir_all(&workspace).expect("workspace root");
        let service = MemoryService::new(RuntimeConfigResolver::new(config));
        Self {
            _root: root,
            workspace,
            service,
        }
    }

    fn workspace_string(&self) -> String {
        self.workspace.to_string_lossy().into_owned()
    }
}

fn memory_record(content: &str) -> MemoryRecord {
    let now = chrono::Utc::now();
    MemoryRecord {
        id: harness_contracts::MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::ProjectFact,
        visibility: MemoryVisibility::Tenant,
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: vec!["task".to_owned()],
            source: MemorySource::AgentDerived,
            evidence: None,
            confidence: 0.9,
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

fn candidate_draft(content: &str) -> MemoryRecordDraft {
    MemoryRecordDraft {
        kind: MemoryKind::ProjectFact,
        visibility: MemoryVisibility::Tenant,
        content: content.to_owned(),
        metadata: harness_contracts::MemoryMetadata {
            ttl: None,
            tags: vec!["test".to_owned()],
            source_trust: 0.8,
        },
        expires_at: None,
    }
}

fn candidate_evidence() -> MemoryEvidence {
    MemoryEvidence {
        source: MemorySource::AgentDerived,
        origin: MemoryEvidenceOrigin::Imported {
            importer: "test".to_owned(),
            import_id: "candidate".to_owned(),
        },
        content_hash: ContentHash([7; 32]),
        session_id: None,
        run_id: None,
        message_id: None,
        tool_use_id: None,
    }
}
