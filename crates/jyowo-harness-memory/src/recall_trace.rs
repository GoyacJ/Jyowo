//! Recall trace generation and collection.
//!
//! Traces store metadata about memory recall (IDs, scores, drop reasons,
//! provider latency, budget usage) without storing raw memory content.
//! Traces are linked to `MemoryRecalledEvent` via `trace_id`.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::Utc;
use harness_contracts::{
    ContentHash, MemoryCandidateTrace, MemoryDropReason, MemoryDroppedTrace, MemoryId,
    MemoryInjectedTrace, MemoryPolicyDecision, MemoryProviderTrace, MemoryRecallTrace,
    MemoryScoreBreakdown, MemoryTraceId, RunId, SessionId,
};

/// Builder for constructing a `MemoryRecallTrace` incrementally during recall.
#[derive(Debug)]
pub struct MemoryRecallTraceBuilder {
    trace_id: MemoryTraceId,
    session_id: SessionId,
    run_id: RunId,
    turn: u32,
    query_text_hash: ContentHash,
    provider_results: Vec<MemoryProviderTrace>,
    candidates: Vec<MemoryCandidateTrace>,
    injected: Vec<MemoryInjectedTrace>,
    dropped: Vec<MemoryDroppedTrace>,
    redacted_count: u32,
    injected_chars: u32,
    deadline_used_ms: u32,
}

impl MemoryRecallTraceBuilder {
    #[must_use]
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        turn: u32,
        query_text_hash: ContentHash,
    ) -> Self {
        Self {
            trace_id: MemoryTraceId::new(),
            session_id,
            run_id,
            turn,
            query_text_hash,
            provider_results: Vec::new(),
            candidates: Vec::new(),
            injected: Vec::new(),
            dropped: Vec::new(),
            redacted_count: 0,
            injected_chars: 0,
            deadline_used_ms: 0,
        }
    }

    pub fn trace_id(&self) -> MemoryTraceId {
        self.trace_id
    }

    pub fn add_provider_result(mut self, result: MemoryProviderTrace) -> Self {
        self.provider_results.push(result);
        self
    }

    pub fn add_candidate(mut self, candidate: MemoryCandidateTrace) -> Self {
        self.candidates.push(candidate);
        self
    }

    pub fn add_injected(
        mut self,
        memory_id: MemoryId,
        provider_id: &str,
        content_hash: ContentHash,
        injected_chars: u32,
        fence_id: &str,
    ) -> Self {
        self.injected.push(MemoryInjectedTrace {
            memory_id,
            provider_id: provider_id.to_owned(),
            content_hash,
            injected_chars,
            fence_id: fence_id.to_owned(),
        });
        self
    }

    pub fn add_dropped(
        mut self,
        reason: MemoryDropReason,
        memory_id: Option<MemoryId>,
        provider_id: Option<&str>,
    ) -> Self {
        self.dropped.push(MemoryDroppedTrace {
            memory_id,
            provider_id: provider_id.map(|s| s.to_owned()),
            content_hash: None,
            reason,
        });
        self
    }

    pub fn set_redacted(mut self, count: u32) -> Self {
        self.redacted_count = count;
        self
    }

    pub fn set_injected_chars(mut self, chars: u32) -> Self {
        self.injected_chars = chars;
        self
    }

    pub fn set_deadline_ms(mut self, ms: u32) -> Self {
        self.deadline_used_ms = ms;
        self
    }

    #[must_use]
    pub fn build(self) -> MemoryRecallTrace {
        MemoryRecallTrace {
            trace_id: self.trace_id,
            session_id: self.session_id,
            run_id: self.run_id,
            turn: self.turn,
            query_text_hash: self.query_text_hash,
            provider_results: self.provider_results,
            candidates: self.candidates,
            injected: self.injected,
            dropped: self.dropped,
            redacted_count: self.redacted_count,
            injected_chars: self.injected_chars,
            deadline_used_ms: self.deadline_used_ms,
            at: Utc::now(),
        }
    }
}

/// In-memory collector of recall traces for the session lifetime.
///
/// Traces can be queried by session ID and exported for IPC.
#[derive(Debug, Default)]
pub struct MemoryRecallTraceCollector {
    traces: Mutex<Vec<MemoryRecallTrace>>,
}

impl MemoryRecallTraceCollector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&self, trace: MemoryRecallTrace) {
        if let Ok(mut traces) = self.traces.lock() {
            traces.push(trace);
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.traces.lock().map(|t| t.len()).unwrap_or(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub fn for_session(&self, session_id: SessionId) -> Vec<MemoryRecallTrace> {
        self.traces
            .lock()
            .map(|traces| {
                traces
                    .iter()
                    .filter(|t| t.session_id == session_id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    #[must_use]
    pub fn for_run(&self, session_id: SessionId, run_id: RunId) -> Vec<MemoryRecallTrace> {
        self.traces
            .lock()
            .map(|traces| {
                traces
                    .iter()
                    .filter(|t| t.session_id == session_id && t.run_id == run_id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    #[must_use]
    pub fn get(&self, trace_id: MemoryTraceId) -> Option<MemoryRecallTrace> {
        self.traces
            .lock()
            .ok()
            .and_then(|traces| traces.iter().find(|t| t.trace_id == trace_id).cloned())
    }

    /// List trace summaries without full detail (for IPC listing).
    #[must_use]
    pub fn list_summaries(
        &self,
        session_id: Option<SessionId>,
        run_id: Option<RunId>,
    ) -> Vec<harness_contracts::MemoryRecallTraceSummary> {
        self.traces
            .lock()
            .map(|traces| {
                traces
                    .iter()
                    .filter(|t| session_id.map_or(true, |sid| t.session_id == sid))
                    .filter(|t| run_id.map_or(true, |rid| t.run_id == rid))
                    .map(|t| harness_contracts::MemoryRecallTraceSummary {
                        trace_id: t.trace_id,
                        session_id: t.session_id,
                        run_id: t.run_id,
                        injected_count: t.injected.len() as u32,
                        dropped_count: t.dropped.len() as u32,
                        redacted_count: t.redacted_count,
                        at: t.at,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
