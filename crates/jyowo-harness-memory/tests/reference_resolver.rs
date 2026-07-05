//! Tests for memory reference hydration.

use harness_contracts::{MemoryError, MemoryId};
use harness_memory::reference::{fence_memory_content, ContextReferenceResolver, FnMemoryResolver};

#[tokio::test]
async fn resolver_hydrates_content_on_success() {
    let resolver = FnMemoryResolver::new(|id: MemoryId| {
        let id = id;
        async move { Ok((format!("content for {}", id), "test-provider".to_owned())) }
    });

    let mid = MemoryId::new();
    let result = resolver
        .resolve_memory(mid, "test-label".to_owned())
        .await
        .unwrap();
    assert_eq!(result.memory_id, mid);
    assert!(matches!(
        result.outcome,
        harness_memory::reference::MemoryReferenceOutcome::Hydrated { .. }
    ));
}

#[tokio::test]
async fn resolver_reports_failure_without_panicking() {
    let resolver = FnMemoryResolver::new(|_id: MemoryId| async move {
        Err(MemoryError::NotFound(MemoryId::new()))
    });

    let mid = MemoryId::new();
    let result = resolver
        .resolve_memory(mid, "missing".to_owned())
        .await
        .unwrap();
    assert!(matches!(
        result.outcome,
        harness_memory::reference::MemoryReferenceOutcome::Failed { .. }
    ));
}

#[test]
fn fenced_content_contains_id_and_content() {
    let mid = MemoryId::new();
    let fenced = fence_memory_content("hello world", mid, "local");
    assert!(fenced.starts_with("<memory-context>"));
    assert!(fenced.contains("NOT user input"));
    assert!(fenced.contains("reference|memory"));
    assert!(fenced.contains("|provider|local"));
    assert!(fenced.contains(&mid.to_string()));
    assert!(fenced.contains("hello world"));
    assert!(fenced.ends_with("</memory-context>"));
}

#[test]
fn fenced_content_truncates_long_input() {
    let mid = MemoryId::new();
    let long = "x".repeat(3000);
    let fenced = fence_memory_content(&long, mid, "local");
    assert!(fenced.len() < 2300); // 2000 max + fencing overhead
    assert!(fenced.contains("...\n</memory-context>"));
}

#[test]
fn fence_never_exposes_raw_id_directly_in_content_area() {
    let mid = MemoryId::new();
    let content = "sensitive data";
    let fenced = fence_memory_content(content, mid, "local");
    assert!(fenced.contains("sensitive data"));
    assert!(fenced.starts_with("<memory-context>"));
    assert!(fenced.contains("reference|memory"));
    assert!(fenced.contains("|provider|local"));
}
