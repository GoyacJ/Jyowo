//! Memory candidate inbox.
//!
//! Stores proposed memory candidates pending user review.
//! Candidates flow through states: Proposed → Approved/Rejected → Promoted/Merged/Expired.
//! No unapproved candidate enters model context.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::Utc;
use harness_contracts::{
    ContentHash, MemoryCandidate, MemoryCandidateId, MemoryCandidateState, MemoryEvidence,
    MemoryEvidenceOrigin, MemoryKind, MemoryMetadata, MemoryRecordDraft, MemorySource,
    MemoryVisibility, TenantId,
};

/// In-memory candidate inbox for a single tenant.
///
/// In production, this would be backed by SQLite alongside the local provider.
#[derive(Debug)]
pub struct MemoryInbox {
    tenant_id: TenantId,
    candidates: Mutex<HashMap<MemoryCandidateId, MemoryCandidate>>,
}

impl MemoryInbox {
    #[must_use]
    pub fn new(tenant_id: TenantId) -> Self {
        Self {
            tenant_id,
            candidates: Mutex::new(HashMap::new()),
        }
    }

    /// Propose a new memory candidate.
    pub fn propose(
        &self,
        draft: MemoryRecordDraft,
        evidence: MemoryEvidence,
    ) -> Result<MemoryCandidate, String> {
        let mut candidates = self
            .candidates
            .lock()
            .map_err(|e| format!("inbox lock: {e}"))?;

        let now = Utc::now();
        let candidate = MemoryCandidate {
            id: MemoryCandidateId::new(),
            tenant_id: self.tenant_id,
            state: MemoryCandidateState::Proposed,
            proposed_record: draft,
            evidence,
            created_at: now,
            updated_at: now,
            expires_at: None,
        };

        candidates.insert(candidate.id, candidate.clone());
        Ok(candidate)
    }

    /// Approve a candidate (move to Approved state).
    pub fn approve(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        let mut candidates = self
            .candidates
            .lock()
            .map_err(|e| format!("inbox lock: {e}"))?;

        let candidate = candidates
            .get_mut(&id)
            .ok_or_else(|| format!("candidate not found: {id}"))?;

        candidate.state = MemoryCandidateState::Approved;
        candidate.updated_at = Utc::now();
        Ok(candidate.clone())
    }

    /// Reject a candidate (move to Rejected state).
    pub fn reject(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        let mut candidates = self
            .candidates
            .lock()
            .map_err(|e| format!("inbox lock: {e}"))?;

        let candidate = candidates
            .get_mut(&id)
            .ok_or_else(|| format!("candidate not found: {id}"))?;

        candidate.state = MemoryCandidateState::Rejected;
        candidate.updated_at = Utc::now();
        Ok(candidate.clone())
    }

    /// Promote an approved candidate to Promoted state (ready for merge into long-term memory).
    pub fn promote(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        let mut candidates = self
            .candidates
            .lock()
            .map_err(|e| format!("inbox lock: {e}"))?;

        let candidate = candidates
            .get_mut(&id)
            .ok_or_else(|| format!("candidate not found: {id}"))?;

        candidate.state = MemoryCandidateState::Promoted;
        candidate.updated_at = Utc::now();
        Ok(candidate.clone())
    }

    /// List candidates, optionally filtered by state.
    pub fn list(
        &self,
        state: Option<MemoryCandidateState>,
    ) -> Result<Vec<MemoryCandidate>, String> {
        let candidates = self
            .candidates
            .lock()
            .map_err(|e| format!("inbox lock: {e}"))?;

        let results: Vec<_> = candidates
            .values()
            .filter(|c| state.map_or(true, |s| c.state == s))
            .cloned()
            .collect();

        Ok(results)
    }

    /// Import a candidate with a specific state (used for DREAMS.md migration).
    pub fn import(
        &self,
        draft: MemoryRecordDraft,
        evidence: MemoryEvidence,
    ) -> Result<MemoryCandidate, String> {
        let mut candidates = self
            .candidates
            .lock()
            .map_err(|e| format!("inbox lock: {e}"))?;

        let now = Utc::now();
        let candidate = MemoryCandidate {
            id: MemoryCandidateId::new(),
            tenant_id: self.tenant_id,
            state: MemoryCandidateState::Proposed,
            proposed_record: draft,
            evidence,
            created_at: now,
            updated_at: now,
            expires_at: None,
        };

        candidates.insert(candidate.id, candidate.clone());
        Ok(candidate)
    }
}

/// Marker trait for inbox storage backends.
pub trait InboxStore: Send + Sync + 'static {
    fn propose(
        &self,
        draft: MemoryRecordDraft,
        evidence: MemoryEvidence,
    ) -> Result<MemoryCandidate, String>;

    fn approve(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String>;

    fn reject(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String>;

    fn list(
        &self,
        state: Option<MemoryCandidateState>,
    ) -> Result<Vec<MemoryCandidate>, String>;
}

impl InboxStore for MemoryInbox {
    fn propose(
        &self,
        draft: MemoryRecordDraft,
        evidence: MemoryEvidence,
    ) -> Result<MemoryCandidate, String> {
        self.propose(draft, evidence)
    }

    fn approve(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        self.approve(id)
    }

    fn reject(&self, id: MemoryCandidateId) -> Result<MemoryCandidate, String> {
        self.reject(id)
    }

    fn list(
        &self,
        state: Option<MemoryCandidateState>,
    ) -> Result<Vec<MemoryCandidate>, String> {
        self.list(state)
    }
}

/// Migrate content from an old DREAMS.md file into inbox candidates.
///
/// Each paragraph becomes a separate candidate with source `Imported`.
pub fn migrate_dreams_to_inbox(
    inbox: &MemoryInbox,
    dreams_content: &str,
) -> Result<Vec<MemoryCandidate>, String> {
    let mut imported = Vec::new();
    // Split by double newlines (paragraph boundaries)
    for paragraph in dreams_content.split("\n\n") {
        let trimmed = paragraph.trim();
        if trimmed.is_empty() {
            continue;
        }
        let draft = MemoryRecordDraft {
            kind: MemoryKind::AgentSelfNote,
            visibility: MemoryVisibility::User {
                user_id: "imported".to_owned(),
            },
            content: trimmed.to_owned(),
            metadata: MemoryMetadata {
                ttl: None,
                tags: vec!["dreams-migration".to_owned()],
                source_trust: 0.3,
            },
            expires_at: None,
        };
        let evidence = MemoryEvidence {
            source: MemorySource::Imported,
            origin: MemoryEvidenceOrigin::Imported {
                importer: "dreams-migration".to_owned(),
                import_id: MemoryCandidateId::new().to_string(),
            },
            content_hash: ContentHash([0u8; 32]),
            session_id: None,
            run_id: None,
            message_id: None,
            tool_use_id: None,
        };
        let candidate = inbox.propose(draft, evidence)?;
        imported.push(candidate);
    }
    Ok(imported)
}
