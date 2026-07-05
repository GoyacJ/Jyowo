use harness_contracts::{
    AgentId, CorrelationId, MemoryId, Message, MessageId, MessagePart, MessageRole, NoopRedactor,
    RunId, SessionId, TeamId, TenantId, TurnInput,
};
use harness_journal::InMemoryEventStore;
use harness_memory::{MemoryMetadata, MemoryProvider, MemoryRecord, MemoryStore};
use harness_team::{SharedMemory, TeamMemberEngineConfig, TeamMemberRunRequest};
use std::sync::Arc;

#[tokio::test]
async fn shared_memory_denies_unaudited_provider_writes() {
    let team_id = TeamId::new();
    let memory = SharedMemory::new(team_id, "team-shared");

    let err = memory
        .upsert(record(team_id, TenantId::SINGLE))
        .await
        .unwrap_err();
    assert!(err
        .to_string()
        .contains("write_from_context for audited shared memory writes"));

    let err = memory.forget(MemoryId::new()).await.unwrap_err();
    assert!(err
        .to_string()
        .contains("write_from_context for audited shared memory writes"));
}

#[tokio::test]
async fn synthetic_team_member_request_cannot_mint_memory_write_context() {
    let request = TeamMemberRunRequest::synthetic(
        TenantId::SINGLE,
        TeamId::new(),
        AgentId::new(),
        "worker",
        SessionId::new(),
        RunId::new(),
        None,
        turn_input("remember"),
        "remember",
        CorrelationId::new(),
        TeamMemberEngineConfig::default(),
    );

    let err = request.memory_write_context().unwrap_err();
    assert!(err.to_string().contains("not runtime-issued"));
}

#[tokio::test]
async fn shared_memory_requires_runtime_context_for_public_writes() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let memory = SharedMemory::new(TeamId::new(), "team-shared").with_journal(
        harness_team::TeamJournalContext {
            tenant_id: TenantId::SINGLE,
            session_id: SessionId::new(),
        },
        store,
    );

    assert_eq!(memory.provider_id(), "team-shared");
}

#[test]
fn shared_memory_descriptor_uses_team_trust() {
    let memory = SharedMemory::new(TeamId::new(), "team-shared");
    let descriptor = memory.descriptor();

    assert_eq!(
        descriptor.provider_kind,
        harness_contracts::MemoryProviderKind::Team
    );
    assert_eq!(
        descriptor.trust_level,
        harness_contracts::MemoryProviderTrust::Team
    );
}

fn record(team_id: TeamId, tenant_id: TenantId) -> MemoryRecord {
    let now = chrono::Utc::now();
    MemoryRecord {
        id: MemoryId::new(),
        tenant_id,
        kind: harness_contracts::MemoryKind::ProjectFact,
        visibility: harness_contracts::MemoryVisibility::Team { team_id },
        content: "shared fact".to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: harness_contracts::MemorySource::AgentDerived,
            confidence: 1.0,
            evidence: None,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 1.0,
            recall_score_breakdown: None,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: now,
        updated_at: now,
    }
}

fn turn_input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: chrono::Utc::now(),
        },
        metadata: serde_json::Value::Null,
    }
}
