use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::{CacheImpact, HarnessError, RunId, SessionId};
use harness_tool_search::{MaterializationCoalescer, ReloadHandle};

#[tokio::test]
async fn coalesces_same_session_and_run_until_window_expires() {
    let coalescer = MaterializationCoalescer::new(Duration::from_millis(20), 32);
    let handle = Arc::new(RecordingReload::default());
    let session_id = SessionId::new();
    let run_id = RunId::new();

    let first = {
        let coalescer = coalescer.clone();
        let handle = handle.clone();
        tokio::spawn(async move {
            coalescer
                .submit(session_id, run_id, vec!["Read".to_owned()], handle)
                .await
                .unwrap()
        })
    };
    let second = {
        let coalescer = coalescer.clone();
        let handle = handle.clone();
        tokio::spawn(async move {
            coalescer
                .submit(session_id, run_id, vec!["Write".to_owned()], handle)
                .await
                .unwrap()
        })
    };

    let first_impact = first.await.unwrap();
    let second_impact = second.await.unwrap();

    assert_eq!(first_impact, second_impact);
    assert_eq!(
        handle.calls().await,
        vec![vec!["Read".to_owned(), "Write".to_owned()]]
    );
}

#[tokio::test]
async fn coalescer_isolates_different_runs_in_same_session() {
    let coalescer = MaterializationCoalescer::new(Duration::from_millis(20), 32);
    let handle = Arc::new(RecordingReload::default());
    let session_id = SessionId::new();

    let first = {
        let coalescer = coalescer.clone();
        let handle = handle.clone();
        tokio::spawn(async move {
            coalescer
                .submit(session_id, RunId::new(), vec!["Read".to_owned()], handle)
                .await
                .unwrap()
        })
    };
    let second = {
        let coalescer = coalescer.clone();
        let handle = handle.clone();
        tokio::spawn(async move {
            coalescer
                .submit(session_id, RunId::new(), vec!["Write".to_owned()], handle)
                .await
                .unwrap()
        })
    };

    first.await.unwrap();
    second.await.unwrap();

    let calls = handle.calls().await;
    assert_eq!(calls.len(), 2);
    assert!(calls.contains(&vec!["Read".to_owned()]));
    assert!(calls.contains(&vec!["Write".to_owned()]));
}

#[tokio::test]
async fn coalescer_isolates_different_sessions() {
    let coalescer = MaterializationCoalescer::new(Duration::ZERO, 32);
    let handle = Arc::new(RecordingReload::default());

    coalescer
        .submit(
            SessionId::new(),
            RunId::new(),
            vec!["Read".to_owned()],
            handle.clone(),
        )
        .await
        .unwrap();
    coalescer
        .submit(
            SessionId::new(),
            RunId::new(),
            vec!["Write".to_owned()],
            handle.clone(),
        )
        .await
        .unwrap();

    assert_eq!(handle.calls().await.len(), 2);
}

#[tokio::test]
async fn max_batch_flushes_immediately() {
    let coalescer = MaterializationCoalescer::new(Duration::from_secs(60), 2);
    let handle = Arc::new(RecordingReload::default());
    let session_id = SessionId::new();
    let run_id = RunId::new();

    let first = {
        let coalescer = coalescer.clone();
        let handle = handle.clone();
        tokio::spawn(async move {
            coalescer
                .submit(session_id, run_id, vec!["Read".to_owned()], handle)
                .await
                .unwrap()
        })
    };
    let second = {
        let coalescer = coalescer.clone();
        let handle = handle.clone();
        tokio::spawn(async move {
            coalescer
                .submit(session_id, run_id, vec!["Write".to_owned()], handle)
                .await
                .unwrap()
        })
    };

    first.await.unwrap();
    second.await.unwrap();

    assert_eq!(
        handle.calls().await,
        vec![vec!["Read".to_owned(), "Write".to_owned()]]
    );
}

#[derive(Default)]
struct RecordingReload {
    calls: tokio::sync::Mutex<Vec<Vec<String>>>,
}

impl RecordingReload {
    async fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().await.clone()
    }
}

#[async_trait]
impl ReloadHandle for RecordingReload {
    async fn reload_with_add_tools(&self, tools: Vec<String>) -> Result<CacheImpact, HarnessError> {
        self.calls.lock().await.push(tools);
        Ok(CacheImpact {
            prompt_cache_invalidated: true,
            reason: Some("coalesced".to_owned()),
        })
    }
}
