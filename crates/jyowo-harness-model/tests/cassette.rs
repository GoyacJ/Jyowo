#![cfg(feature = "cassette")]

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};

use async_trait::async_trait;
use chrono::Utc;
use futures::{stream, StreamExt};
use harness_contracts::{Message, MessageId, MessagePart, MessageRole, UsageSnapshot};
use harness_model::*;

fn request() -> ModelRequest {
    ModelRequest {
        model_id: "mock-model".to_owned(),
        messages: vec![Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text("hello".to_owned())],
            created_at: Utc::now(),
        }],
        tools: None,
        system: None,
        temperature: None,
        max_tokens: Some(16),
        stream: false,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::ChatCompletions,
        extra: serde_json::Value::Null,
    }
}

fn cassette_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-harness-model-cassette-{}.json",
        harness_contracts::RequestId::new()
    ))
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct CiEnvGuard {
    original: Option<OsString>,
}

impl CiEnvGuard {
    fn set() -> Self {
        let original = std::env::var_os("CI");
        std::env::set_var("CI", "true");
        Self { original }
    }
}

impl Drop for CiEnvGuard {
    fn drop(&mut self) {
        if let Some(value) = self.original.take() {
            std::env::set_var("CI", value);
        } else {
            std::env::remove_var("CI");
        }
    }
}

#[derive(Clone)]
struct CountingProvider {
    hits: Arc<AtomicUsize>,
    events: Vec<ModelStreamEvent>,
}

#[async_trait]
impl ModelProvider for CountingProvider {
    fn provider_id(&self) -> &str {
        "counting"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        Vec::new()
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, harness_contracts::ModelError> {
        self.hits.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(stream::iter(self.events.clone())))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn cassette_records_then_replays_without_calling_inner_provider() {
    let _guard = env_lock().lock().unwrap();
    if std::env::var_os("CI").is_some() {
        return;
    }

    let path = cassette_path();
    let req = request();
    let recorded_events = vec![
        ModelStreamEvent::MessageStart {
            message_id: "msg_1".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockStart {
            index: 0,
            content_type: ContentType::Text,
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("world".to_owned()),
        },
        ModelStreamEvent::ContentBlockStop { index: 0 },
        ModelStreamEvent::MessageStop,
    ];
    let record_hits = Arc::new(AtomicUsize::new(0));
    let replay_hits = Arc::new(AtomicUsize::new(0));

    let record_provider = CassetteProvider::new(
        Arc::new(CountingProvider {
            hits: record_hits.clone(),
            events: recorded_events.clone(),
        }),
        path.clone(),
        CassetteMode::Record,
    );
    let record_events = record_provider
        .infer(req.clone(), InferContext::for_test())
        .await
        .expect("record should call inner provider")
        .collect::<Vec<_>>()
        .await;

    let replay_provider = CassetteProvider::new(
        Arc::new(CountingProvider {
            hits: replay_hits.clone(),
            events: Vec::new(),
        }),
        path.clone(),
        CassetteMode::Replay,
    );
    let replay_events = replay_provider
        .infer(req, InferContext::for_test())
        .await
        .expect("replay should read cassette")
        .collect::<Vec<_>>()
        .await;

    assert_eq!(record_events, recorded_events);
    assert_eq!(replay_events, recorded_events);
    assert_eq!(record_hits.load(Ordering::SeqCst), 1);
    assert_eq!(replay_hits.load(Ordering::SeqCst), 0);

    let _ = std::fs::remove_file(path);
}

#[tokio::test(flavor = "current_thread")]
async fn cassette_rejects_record_and_passthrough_modes_when_ci_is_set() {
    let _guard = env_lock().lock().unwrap();
    let _ci = CiEnvGuard::set();
    let hits = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(CountingProvider {
        hits: hits.clone(),
        events: Vec::new(),
    });

    for mode in [CassetteMode::Record, CassetteMode::Passthrough] {
        let cassette = CassetteProvider::new(provider.clone(), cassette_path(), mode);
        match cassette.infer(request(), InferContext::for_test()).await {
            Err(harness_contracts::ModelError::InvalidRequest(message)) => {
                assert!(message.contains("disabled in CI"));
            }
            Err(error) => panic!("expected invalid request, got {error}"),
            Ok(_) => panic!("expected cassette mode to be rejected in CI"),
        }
    }

    assert_eq!(hits.load(Ordering::SeqCst), 0);
}
