//! Tests for recall trace generation.

use chrono::Utc;
use harness_contracts::*;
use harness_memory::recall_trace::{MemoryRecallTraceBuilder, MemoryRecallTraceCollector};

fn make_score() -> MemoryScoreBreakdown {
    MemoryScoreBreakdown {
        lexical_score: 0.85,
        vector_score: Some(0.7),
        confidence_score: 0.9,
        recency_score: 0.5,
        access_score: 0.3,
        source_trust_score: 0.8,
        explicit_selection_boost: 0.0,
        final_score: 0.75,
    }
}

#[test]
fn trace_contains_candidate_ids_and_scores_but_not_raw_content() {
    let sid = SessionId::new();
    let rid = RunId::new();
    let mid = MemoryId::new();
    let trace_id = MemoryTraceId::new();
    let content_hash = ContentHash([1u8; 32]);

    let trace = MemoryRecallTrace {
        trace_id,
        session_id: sid,
        run_id: rid,
        turn: 1,
        query_text_hash: ContentHash([2u8; 32]),
        provider_results: vec![MemoryProviderTrace {
            provider_id: "local".to_owned(),
            trust_level: MemoryProviderTrust::BuiltIn,
            readable: true,
            writable: true,
            requested_count: 10,
            returned_count: 5,
            timed_out: false,
            error_kind: None,
            latency_ms: 42,
        }],
        candidates: vec![MemoryCandidateTrace {
            memory_id: mid,
            provider_id: "local".to_owned(),
            content_hash: content_hash.clone(),
            score: make_score(),
            policy_decision: MemoryPolicyDecision::Allow,
        }],
        injected: vec![MemoryInjectedTrace {
            memory_id: mid,
            provider_id: "local".to_owned(),
            content_hash: content_hash.clone(),
            injected_chars: 100,
            fence_id: "mem_turn_1".to_owned(),
        }],
        dropped: vec![MemoryDroppedTrace {
            memory_id: Some(mid),
            provider_id: Some("local".to_owned()),
            content_hash: Some(content_hash),
            reason: MemoryDropReason::BudgetExceeded,
        }],
        redacted_count: 0,
        injected_chars: 100,
        deadline_used_ms: 500,
        at: Utc::now(),
    };

    // Verify JSON serialization contains no raw content
    let json = serde_json::to_string(&trace).unwrap();
    assert!(!json.contains("\"content\""));
    assert!(!json.contains("\"raw_content\""));
    assert!(!json.contains("\"prompt\""));
    assert!(!json.contains("\"message_text\""));
    assert!(json.contains("memory_id"));
    assert!(json.contains("content_hash"));
    assert!(json.contains("lexical_score"));
}

#[test]
fn budget_dropped_records_appear_with_drop_reason() {
    let trace = MemoryRecallTrace {
        trace_id: MemoryTraceId::new(),
        session_id: SessionId::new(),
        run_id: RunId::new(),
        turn: 1,
        query_text_hash: ContentHash([3u8; 32]),
        provider_results: vec![],
        candidates: vec![],
        injected: vec![],
        dropped: vec![
            MemoryDroppedTrace {
                memory_id: Some(MemoryId::new()),
                provider_id: None,
                content_hash: None,
                reason: MemoryDropReason::BudgetExceeded,
            },
            MemoryDroppedTrace {
                memory_id: Some(MemoryId::new()),
                provider_id: None,
                content_hash: None,
                reason: MemoryDropReason::ThreatBlocked,
            },
            MemoryDroppedTrace {
                memory_id: None,
                provider_id: Some("timeout-p".to_owned()),
                content_hash: None,
                reason: MemoryDropReason::ProviderTimeout,
            },
        ],
        redacted_count: 2,
        injected_chars: 0,
        deadline_used_ms: 250,
        at: Utc::now(),
    };

    assert_eq!(trace.dropped.len(), 3);
    assert!(trace
        .dropped
        .iter()
        .any(|d| matches!(d.reason, MemoryDropReason::BudgetExceeded)));
    assert!(trace
        .dropped
        .iter()
        .any(|d| matches!(d.reason, MemoryDropReason::ThreatBlocked)));
    assert!(trace
        .dropped
        .iter()
        .any(|d| matches!(d.reason, MemoryDropReason::ProviderTimeout)));
}

#[test]
fn trace_builder_produces_valid_trace() {
    let sid = SessionId::new();
    let rid = RunId::new();
    let trace = MemoryRecallTraceBuilder::new(sid, rid, 2, ContentHash([4u8; 32]))
        .add_provider_result(MemoryProviderTrace {
            provider_id: "local".to_owned(),
            trust_level: MemoryProviderTrust::BuiltIn,
            readable: true,
            writable: true,
            requested_count: 5,
            returned_count: 3,
            timed_out: false,
            error_kind: None,
            latency_ms: 15,
        })
        .add_candidate(MemoryCandidateTrace {
            memory_id: MemoryId::new(),
            provider_id: "local".to_owned(),
            content_hash: ContentHash([5u8; 32]),
            score: make_score(),
            policy_decision: MemoryPolicyDecision::Allow,
        })
        .add_dropped(MemoryDropReason::ScoreBelowThreshold, Some(MemoryId::new()), Some("local"))
        .set_redacted(1)
        .set_injected_chars(200)
        .set_deadline_ms(100)
        .build();
    assert_eq!(trace.turn, 2);
    assert_eq!(trace.provider_results.len(), 1);
    assert_eq!(trace.candidates.len(), 1);
    assert_eq!(trace.dropped.len(), 1);
    assert_eq!(trace.redacted_count, 1);
    assert_eq!(trace.injected_chars, 200);
    assert_eq!(trace.deadline_used_ms, 100);
}

#[test]
fn trace_collector_accumulates_traces() {
    let collector = MemoryRecallTraceCollector::new();
    let sid = SessionId::new();
    let rid = RunId::new();

    let builder1 = MemoryRecallTraceBuilder::new(sid, rid, 1, ContentHash([6u8; 32]));
    collector.add(builder1.build());

    let builder2 = MemoryRecallTraceBuilder::new(sid, rid, 2, ContentHash([7u8; 32]));
    collector.add(builder2.build());

    assert_eq!(collector.len(), 2);
    assert_eq!(collector.for_session(sid).len(), 2);
    assert_eq!(
        collector.for_session(SessionId::new()).len(),
        0
    );
}
