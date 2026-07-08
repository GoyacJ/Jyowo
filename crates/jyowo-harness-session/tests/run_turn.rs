use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{BlobId, BlobRef, Event, MessagePart, NoopRedactor, SessionError};
use harness_journal::InMemoryEventStore;
use harness_session::{Session, SessionOptions, SessionTurnContext, SessionTurnRunner};
use tokio::sync::Mutex;

#[tokio::test]
async fn run_turn_delegates_to_configured_runner_and_returns_run_id() {
    let root = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let session = Session::builder()
        .with_options(SessionOptions::new(root.path()))
        .with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_turn_runner(runner.clone())
        .build()
        .await
        .unwrap();

    let run_id = session.run_turn("hello").await.unwrap();

    let calls = runner.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].parts, vec![MessagePart::Text("hello".to_owned())]);
    assert_eq!(calls[0].ctx.run_id, run_id);
    assert_eq!(calls[0].ctx.turn_index, 0);
}

#[tokio::test]
async fn run_turn_delegates_multimodal_input_to_configured_runner() {
    let root = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let session = Session::builder()
        .with_options(SessionOptions::new(root.path()))
        .with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_turn_runner(runner.clone())
        .build()
        .await
        .unwrap();
    let image = MessagePart::Image {
        mime_type: "image/png".to_owned(),
        blob_ref: BlobRef {
            id: BlobId::new(),
            size: 0,
            content_hash: [0; 32],
            content_type: Some("image/png".to_owned()),
        },
    };

    session
        .run_turn_parts(vec![MessagePart::Text("hello".to_owned()), image.clone()])
        .await
        .unwrap();

    let calls = runner.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].parts,
        vec![MessagePart::Text("hello".to_owned()), image]
    );
}

#[derive(Default)]
struct RecordingRunner {
    calls: Mutex<Vec<RecordedCall>>,
}

struct RecordedCall {
    ctx: SessionTurnContext,
    parts: Vec<MessagePart>,
}

#[async_trait]
impl SessionTurnRunner for RecordingRunner {
    async fn run_turn(
        &self,
        ctx: SessionTurnContext,
        parts: Vec<MessagePart>,
    ) -> Result<Vec<Event>, SessionError> {
        self.calls.lock().await.push(RecordedCall { ctx, parts });
        Ok(Vec::new())
    }
}
