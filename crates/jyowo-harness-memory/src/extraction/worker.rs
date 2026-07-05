//! Extraction worker.
//!
//! Runs extraction after sessions end or are idle.
//! Respects policy: skips active sessions, short sessions, external-context threads.
//! Creates candidates via the inbox, not direct long-term records.

use std::sync::Arc;

use chrono::Utc;
use harness_contracts::{
    ContentHash, MemoryCandidateOperation, MemoryEvidence, MemoryEvidenceOrigin, MemoryMetadata,
    MemoryPermissionContext, MemoryPolicyDecision, MemoryRecordDraft, MemorySource,
    MemoryVisibility, MessageId, RunId, SessionId, TenantId,
};

use crate::extraction::job::{
    ExtractionJob, ExtractionJobConfig, ExtractionJobKind, ExtractionJobQueue, ExtractionJobState,
};
use crate::extraction::schema::{
    ExtractedCandidate, ExtractedConsolidation, ExtractedConsolidationAction, ExtractionOutput,
    ExtractionVisibility,
};
use crate::inbox::MemoryInbox;
use crate::policy::MemoryPolicyEngine;

/// Configuration for the extraction worker.
#[derive(Debug, Clone)]
pub struct ExtractionWorkerConfig {
    /// Minimum session duration before extraction is considered.
    pub min_session_duration_seconds: u64,
    /// Minimum idle time before extraction runs.
    pub min_idle_seconds: u64,
    /// Maximum candidates per extraction run.
    pub max_candidates_per_run: usize,
    /// Job queue configuration.
    pub job_config: ExtractionJobConfig,
}

impl Default for ExtractionWorkerConfig {
    fn default() -> Self {
        Self {
            min_session_duration_seconds: 60,
            min_idle_seconds: 300,
            max_candidates_per_run: 10,
            job_config: ExtractionJobConfig::default(),
        }
    }
}

/// Outcome of an extraction run.
#[derive(Debug, Clone)]
pub struct ExtractionRunOutcome {
    pub candidates_created: usize,
    pub skipped_reason: Option<String>,
}

/// Extracts typed memory candidates for a leased extraction job.
pub trait MemoryExtractor: Send + Sync {
    fn extract(&self, job: &ExtractionJob) -> Result<ExtractionOutput, String>;
}

#[derive(Debug, Default)]
pub struct HeuristicMemoryExtractor;

impl MemoryExtractor for HeuristicMemoryExtractor {
    fn extract(&self, job: &ExtractionJob) -> Result<ExtractionOutput, String> {
        let Some(excerpt) = job.source_excerpt.as_deref() else {
            return Ok(ExtractionOutput::default());
        };
        let candidates = excerpt
            .lines()
            .filter_map(heuristic_candidate_from_line)
            .take(5)
            .collect();
        Ok(ExtractionOutput {
            candidates,
            consolidations: Vec::new(),
            summary: None,
        })
    }
}

/// The extraction worker.
///
/// Polls the job queue for queued/retryable jobs, processes them using
/// a model provider, and writes results to the inbox.
pub struct ExtractionWorker {
    queue: ExtractionJobQueue,
    config: ExtractionWorkerConfig,
    policy_engine: MemoryPolicyEngine,
    inbox: MemoryInbox,
    extractor: Option<Arc<dyn MemoryExtractor>>,
}

impl ExtractionWorker {
    #[must_use]
    pub fn new(
        config: ExtractionWorkerConfig,
        policy_engine: MemoryPolicyEngine,
        inbox: MemoryInbox,
        extractor: Arc<dyn MemoryExtractor>,
    ) -> Self {
        let queue = ExtractionJobQueue::new(config.job_config.clone());
        Self::with_queue(config, policy_engine, inbox, queue, extractor)
    }

    pub fn open(
        db_path: &str,
        config: ExtractionWorkerConfig,
        policy_engine: MemoryPolicyEngine,
        inbox: MemoryInbox,
        extractor: Arc<dyn MemoryExtractor>,
    ) -> Result<Self, String> {
        let queue = ExtractionJobQueue::open(db_path, config.job_config.clone())?;
        Ok(Self::with_queue(
            config,
            policy_engine,
            inbox,
            queue,
            extractor,
        ))
    }

    #[must_use]
    pub fn with_queue(
        config: ExtractionWorkerConfig,
        policy_engine: MemoryPolicyEngine,
        inbox: MemoryInbox,
        queue: ExtractionJobQueue,
        extractor: Arc<dyn MemoryExtractor>,
    ) -> Self {
        Self {
            queue,
            config,
            policy_engine,
            inbox,
            extractor: Some(extractor),
        }
    }

    #[must_use]
    pub fn new_unconfigured(
        config: ExtractionWorkerConfig,
        policy_engine: MemoryPolicyEngine,
        inbox: MemoryInbox,
    ) -> Self {
        let queue = ExtractionJobQueue::new(config.job_config.clone());
        Self::with_unconfigured_queue(config, policy_engine, inbox, queue)
    }

    pub fn open_unconfigured(
        db_path: &str,
        config: ExtractionWorkerConfig,
        policy_engine: MemoryPolicyEngine,
        inbox: MemoryInbox,
    ) -> Result<Self, String> {
        let queue = ExtractionJobQueue::open(db_path, config.job_config.clone())?;
        Ok(Self::with_unconfigured_queue(
            config,
            policy_engine,
            inbox,
            queue,
        ))
    }

    #[must_use]
    pub fn with_unconfigured_queue(
        config: ExtractionWorkerConfig,
        policy_engine: MemoryPolicyEngine,
        inbox: MemoryInbox,
        queue: ExtractionJobQueue,
    ) -> Self {
        Self {
            queue,
            config,
            policy_engine,
            inbox,
            extractor: None,
        }
    }

    /// Access the job queue.
    pub fn queue(&self) -> &ExtractionJobQueue {
        &self.queue
    }

    /// Access the candidate inbox.
    pub fn inbox(&self) -> &MemoryInbox {
        &self.inbox
    }

    /// Enqueue an extraction job for a session.
    pub fn enqueue_session(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        evidence_hash: [u8; 32],
    ) -> Result<String, String> {
        let job = ExtractionJob {
            job_id: format!("job-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)),
            tenant_id,
            session_id,
            run_id,
            source_message_id: None,
            source_user_id: None,
            source_excerpt: None,
            evidence_hash,
            job_kind: ExtractionJobKind::MemoryExtraction,
            state: ExtractionJobState::Queued,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
            next_attempt_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.queue.enqueue(job)
    }

    /// Enqueue an extraction job with the source message/user used for evidence.
    pub fn enqueue_session_from_message(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        source_message_id: MessageId,
        source_user_id: Option<String>,
        source_excerpt: Option<String>,
        evidence_hash: [u8; 32],
    ) -> Result<String, String> {
        let job = ExtractionJob {
            job_id: format!("job-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)),
            tenant_id,
            session_id,
            run_id,
            source_message_id: Some(source_message_id),
            source_user_id,
            source_excerpt,
            evidence_hash,
            job_kind: ExtractionJobKind::MemoryExtraction,
            state: ExtractionJobState::Queued,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
            next_attempt_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.queue.enqueue(job)
    }

    /// Process one job (poll and execute).
    ///
    /// Returns None if no jobs are available.
    pub fn poll_and_process(
        &self,
        owner: &str,
        session_ended: bool,
        session_idle_seconds: u64,
        has_external_context: bool,
    ) -> Result<Option<ExtractionRunOutcome>, String> {
        // Policy gate: don't process if session is still active (unless policy allows)
        if !session_ended && session_idle_seconds < self.config.min_idle_seconds {
            return Ok(Some(ExtractionRunOutcome {
                candidates_created: 0,
                skipped_reason: Some("session still active".to_owned()),
            }));
        }

        // Lease a job
        let Some(job) = self.queue.lease_next(owner)? else {
            return Ok(None);
        };

        // External context gate
        if has_external_context {
            // Check policy
            let thread = harness_contracts::MemoryThreadSettings {
                session_id: job.session_id,
                use_memories: None,
                generate_memories: None,
                memory_mode: harness_contracts::MemoryThreadMode::ReadWrite,
            };
            let decision = self.policy_engine.evaluate_generation(
                &thread,
                has_external_context,
                &no_generation_permission(),
            );
            if matches!(decision, MemoryPolicyDecision::Deny { .. }) {
                self.queue.skip(&job.job_id)?;
                return Ok(Some(ExtractionRunOutcome {
                    candidates_created: 0,
                    skipped_reason: Some("external context blocked by policy".to_owned()),
                }));
            }
        }

        let Some(extractor) = &self.extractor else {
            self.queue.block(&job.job_id, "extractor unavailable")?;
            return Ok(Some(ExtractionRunOutcome {
                candidates_created: 0,
                skipped_reason: Some("extractor unavailable".to_owned()),
            }));
        };

        let output = match extractor.extract(&job) {
            Ok(output) => output,
            Err(_) => {
                self.queue.fail(&job.job_id)?;
                return Ok(Some(ExtractionRunOutcome {
                    candidates_created: 0,
                    skipped_reason: Some("extractor failed".to_owned()),
                }));
            }
        };

        let candidates = output
            .candidates
            .iter()
            .take(self.config.max_candidates_per_run)
            .collect::<Vec<_>>();
        let remaining = self
            .config
            .max_candidates_per_run
            .saturating_sub(candidates.len());
        let consolidations = output
            .consolidations
            .iter()
            .take(remaining)
            .collect::<Vec<_>>();
        if !candidates
            .iter()
            .all(|candidate| valid_candidate(candidate, &job))
            || !consolidations
                .iter()
                .all(|consolidation| valid_consolidation(consolidation, &job))
        {
            self.queue.fail(&job.job_id)?;
            return Ok(Some(ExtractionRunOutcome {
                candidates_created: 0,
                skipped_reason: Some("extractor output invalid".to_owned()),
            }));
        }

        if candidates.is_empty() && consolidations.is_empty() {
            self.queue.complete(&job.job_id)?;

            return Ok(Some(ExtractionRunOutcome {
                candidates_created: 0,
                skipped_reason: None,
            }));
        }

        let Some(source_message_id) = job.source_message_id else {
            self.queue.fail(&job.job_id)?;
            return Ok(Some(ExtractionRunOutcome {
                candidates_created: 0,
                skipped_reason: Some("extractor evidence missing".to_owned()),
            }));
        };

        let mut staged_proposals = Vec::with_capacity(candidates.len() + consolidations.len());
        for candidate in candidates {
            let content = match sanitize_extracted_content(candidate.content.trim()) {
                Ok(content) => content,
                Err(reason) => {
                    self.queue.fail(&job.job_id)?;
                    return Ok(Some(ExtractionRunOutcome {
                        candidates_created: 0,
                        skipped_reason: Some(reason.to_owned()),
                    }));
                }
            };
            staged_proposals.push(StagedInboxProposal {
                operation: MemoryCandidateOperation::Create,
                draft: MemoryRecordDraft {
                    kind: candidate.kind.clone().into(),
                    visibility: extraction_visibility(candidate.visibility.clone(), &job)?,
                    content,
                    metadata: MemoryMetadata {
                        ttl: None,
                        tags: Default::default(),
                        source_trust: f64::from(candidate.confidence),
                    },
                    expires_at: None,
                },
                evidence: MemoryEvidence {
                    source: MemorySource::AgentDerived,
                    origin: MemoryEvidenceOrigin::AssistantMessage {
                        session_id: job.session_id,
                        run_id: job.run_id,
                        message_id: source_message_id,
                    },
                    content_hash: ContentHash(job.evidence_hash),
                    session_id: Some(job.session_id),
                    run_id: Some(job.run_id),
                    message_id: Some(source_message_id),
                    tool_use_id: None,
                },
            });
        }
        for consolidation in consolidations {
            let content = match sanitize_extracted_content(consolidation.content.trim()) {
                Ok(content) => content,
                Err(reason) => {
                    self.queue.fail(&job.job_id)?;
                    return Ok(Some(ExtractionRunOutcome {
                        candidates_created: 0,
                        skipped_reason: Some(reason.to_owned()),
                    }));
                }
            };
            let tags = match consolidation_tags(consolidation) {
                Ok(tags) => tags,
                Err(reason) => {
                    self.queue.fail(&job.job_id)?;
                    return Ok(Some(ExtractionRunOutcome {
                        candidates_created: 0,
                        skipped_reason: Some(reason.to_owned()),
                    }));
                }
            };
            staged_proposals.push(StagedInboxProposal {
                operation: consolidation_operation(consolidation),
                draft: MemoryRecordDraft {
                    kind: harness_contracts::MemoryKind::ProjectFact,
                    visibility: MemoryVisibility::Tenant,
                    content,
                    metadata: MemoryMetadata {
                        ttl: None,
                        tags,
                        source_trust: 0.8,
                    },
                    expires_at: None,
                },
                evidence: MemoryEvidence {
                    source: MemorySource::Consolidated {
                        from: vec![consolidation.memory_id],
                    },
                    origin: MemoryEvidenceOrigin::AssistantMessage {
                        session_id: job.session_id,
                        run_id: job.run_id,
                        message_id: source_message_id,
                    },
                    content_hash: ContentHash(job.evidence_hash),
                    session_id: Some(job.session_id),
                    run_id: Some(job.run_id),
                    message_id: Some(source_message_id),
                    tool_use_id: None,
                },
            });
        }

        let candidates_created = staged_proposals.len();
        for proposal in staged_proposals {
            self.inbox.propose_with_operation(
                proposal.operation,
                proposal.draft,
                proposal.evidence,
            )?;
        }

        self.queue.complete(&job.job_id)?;

        Ok(Some(ExtractionRunOutcome {
            candidates_created,
            skipped_reason: None,
        }))
    }
}

struct StagedInboxProposal {
    operation: MemoryCandidateOperation,
    draft: MemoryRecordDraft,
    evidence: MemoryEvidence,
}

fn no_generation_permission() -> MemoryPermissionContext {
    MemoryPermissionContext {
        explicit_user_instruction: false,
        include_raw_content: false,
        action_plan_id: None,
        authorization_ticket_id: None,
        non_interactive_policy_grant: false,
    }
}

fn consolidation_operation(consolidation: &ExtractedConsolidation) -> MemoryCandidateOperation {
    match consolidation.action {
        ExtractedConsolidationAction::Merge | ExtractedConsolidationAction::Demote => {
            MemoryCandidateOperation::Update {
                memory_id: consolidation.memory_id,
            }
        }
        ExtractedConsolidationAction::Expire => MemoryCandidateOperation::Delete {
            memory_id: consolidation.memory_id,
        },
    }
}

fn consolidation_tags(consolidation: &ExtractedConsolidation) -> Result<Vec<String>, &'static str> {
    let action = match consolidation.action {
        ExtractedConsolidationAction::Merge => "merge",
        ExtractedConsolidationAction::Demote => "demote",
        ExtractedConsolidationAction::Expire => "expire",
    };
    let mut tags = vec![format!("consolidation:{action}")];
    let reason = consolidation.reason.trim();
    if !reason.is_empty() {
        let reason = sanitize_extracted_content(reason)?;
        if !reason.trim().is_empty() {
            tags.push(format!("consolidation_reason:{}", reason.trim()));
        }
    }
    Ok(tags)
}

fn sanitize_extracted_content(content: &str) -> Result<String, &'static str> {
    #[cfg(feature = "threat-scanner")]
    {
        let scanner = crate::MemoryThreatScanner::default();
        let report = scanner.scan(content);
        if report.action == harness_contracts::ThreatAction::Block {
            return Err("extractor output blocked by threat scanner");
        }
        if let Some(redacted_content) = report.redacted_content {
            return Ok(redacted_content);
        }
    }

    Ok(content.to_owned())
}

fn heuristic_candidate_from_line(line: &str) -> Option<ExtractedCandidate> {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    let content = lower
        .strip_prefix("remember:")
        .or_else(|| lower.strip_prefix("remember "))
        .map(|_| {
            trimmed
                .split_once(':')
                .map(|(_, value)| value)
                .unwrap_or_else(|| trimmed.trim_start_matches("remember"))
                .trim()
        })?;
    (!content.is_empty()).then(|| ExtractedCandidate {
        kind: crate::extraction::schema::ExtractionMemoryKind::ProjectFact,
        visibility: ExtractionVisibility::Tenant,
        content: content.to_owned(),
        confidence: 0.7,
    })
}

fn valid_consolidation(consolidation: &ExtractedConsolidation, job: &ExtractionJob) -> bool {
    !consolidation.content.trim().is_empty()
        && !consolidation.reason.trim().is_empty()
        && job.source_message_id.is_some()
}

fn valid_candidate(candidate: &ExtractedCandidate, job: &ExtractionJob) -> bool {
    !candidate.content.trim().is_empty()
        && candidate.confidence.is_finite()
        && (0.0..=1.0).contains(&candidate.confidence)
        && job.source_message_id.is_some()
        && match &candidate.visibility {
            ExtractionVisibility::Tenant => true,
            ExtractionVisibility::User => job.source_user_id.is_some(),
        }
}

fn extraction_visibility(
    visibility: ExtractionVisibility,
    job: &ExtractionJob,
) -> Result<MemoryVisibility, String> {
    match visibility {
        ExtractionVisibility::Tenant => Ok(MemoryVisibility::Tenant),
        ExtractionVisibility::User => job
            .source_user_id
            .clone()
            .map(|user_id| MemoryVisibility::User { user_id })
            .ok_or_else(|| "extractor user visibility missing user scope".to_owned()),
    }
}
