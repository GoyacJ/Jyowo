use std::time::{Duration, Instant};

use futures::{stream, StreamExt};
use harness_contracts::{ModelError, UsageSnapshot};
use harness_model::{
    wrap_stream_with_cancel_deadline, ErrorClass, InferContext, ModelStream, ModelStreamEvent,
};

#[tokio::test]
async fn cancel_during_stream_poll_emits_transient_stream_error() {
    let ctx = InferContext::for_test();
    let cancel = ctx.cancel.clone();
    let mut stream = wrap_stream_with_cancel_deadline(pending_stream(), &ctx);

    cancel.cancel();
    let event = stream.next().await.expect("cancel should emit an event");

    assert!(matches!(
        event,
        ModelStreamEvent::StreamError {
            error: ModelError::Cancelled,
            class: ErrorClass::Transient,
            ..
        }
    ));
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn deadline_during_stream_poll_emits_deadline_stream_error() {
    let mut ctx = InferContext::for_test();
    ctx.deadline = Some(Instant::now() + Duration::from_millis(5));
    let mut stream = wrap_stream_with_cancel_deadline(pending_stream(), &ctx);

    let event = stream.next().await.expect("deadline should emit an event");

    assert!(matches!(
        event,
        ModelStreamEvent::StreamError {
            error: ModelError::DeadlineExceeded(_),
            class: ErrorClass::Transient,
            ..
        }
    ));
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn cancel_deadline_wrapper_passes_through_normal_events() {
    let ctx = InferContext::for_test();
    let mut stream = wrap_stream_with_cancel_deadline(
        Box::pin(stream::iter([ModelStreamEvent::MessageStart {
            message_id: "assistant-1".to_owned(),
            usage: UsageSnapshot::default(),
        }])),
        &ctx,
    );

    assert!(matches!(
        stream.next().await,
        Some(ModelStreamEvent::MessageStart { .. })
    ));
    assert!(stream.next().await.is_none());
}

fn pending_stream() -> ModelStream {
    Box::pin(stream::pending())
}
