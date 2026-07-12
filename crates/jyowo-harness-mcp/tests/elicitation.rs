use std::sync::Arc;

use harness_contracts::{
    ElicitationOutcome, Event, McpServerId, PermissionMode, RequestId, RunId, SessionId,
};
use harness_mcp::{
    elicitation_from_jsonrpc_error, handle_jsonrpc_elicitation_error, summarize_elicitation_schema,
    DirectElicitationHandler, ElicitationError, ElicitationHandler, ElicitationRequest,
    RejectAllElicitationHandler, StreamElicitationHandler, MCP_ELICITATION_REQUIRED_CODE,
};
use parking_lot::Mutex;
use serde_json::json;

#[tokio::test]
async fn reject_all_elicitation_handler_declines() {
    let error = RejectAllElicitationHandler
        .handle(sample_request())
        .await
        .expect_err("rejects");
    assert_eq!(error, ElicitationError::UserDeclined);
}

#[tokio::test]
async fn direct_elicitation_handler_returns_closure_value() {
    let handler = DirectElicitationHandler::new(|request| async move {
        assert_eq!(request.subject, "credentials");
        Ok(json!({ "token": "secret" }))
    });

    let value = handler.handle(sample_request()).await.expect("value");
    assert_eq!(value, json!({ "token": "secret" }));
}

#[tokio::test]
async fn stream_elicitation_handler_emits_events_and_resolves() {
    let sink = Arc::new(CollectingSink::default());
    let handler = StreamElicitationHandler::new(
        SessionId::from_u128(1),
        Some(RunId::from_u128(2)),
        sink.clone(),
    );
    let request = sample_request();
    let request_id = request.request_id;

    let pending = {
        let handler = handler.clone();
        tokio::spawn(async move { handler.handle(request).await })
    };
    tokio::task::yield_now().await;

    handler
        .resolve_elicitation(request_id, json!({ "token": "secret" }))
        .await
        .expect("resolve");
    let value = pending.await.expect("join").expect("handled");
    assert_eq!(value, json!({ "token": "secret" }));

    let events = sink.events();
    assert!(matches!(events[0], Event::McpElicitationRequested(_)));
    assert!(matches!(
        events[1],
        Event::McpElicitationResolved(ref resolved)
            if matches!(resolved.outcome, ElicitationOutcome::Provided { .. })
    ));
}

#[tokio::test]
async fn stream_elicitation_handler_rejects_and_times_out() {
    let sink = Arc::new(CollectingSink::default());
    let handler = StreamElicitationHandler::new(SessionId::from_u128(1), None, sink.clone());
    let request_id = RequestId::from_u128(99);
    let mut request = sample_request();
    request.request_id = request_id;

    let pending = {
        let handler = handler.clone();
        tokio::spawn(async move { handler.handle(request).await })
    };
    tokio::task::yield_now().await;
    handler
        .reject_elicitation(request_id, "declined")
        .await
        .expect("reject");
    assert_eq!(
        pending.await.expect("join").expect_err("declined"),
        ElicitationError::UserDeclined
    );

    let mut timeout_request = sample_request();
    timeout_request.request_id = RequestId::from_u128(100);
    timeout_request.timeout = Some(std::time::Duration::from_millis(1));
    assert_eq!(
        handler.handle(timeout_request).await.expect_err("timeout"),
        ElicitationError::Timeout
    );

    let events = sink.events();
    assert!(events.iter().any(|event| matches!(
        event,
        Event::McpElicitationResolved(resolved)
            if resolved.outcome == ElicitationOutcome::UserDeclined
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::McpElicitationResolved(resolved)
            if resolved.outcome == ElicitationOutcome::Timeout
    )));
}

#[test]
fn schema_summary_counts_required_fields_and_secret_names() {
    let summary = summarize_elicitation_schema(&json!({
        "type": "object",
        "required": ["username", "api_key"],
        "properties": {
            "username": { "type": "string" },
            "api_key": { "type": "string" },
            "region": { "type": "string" }
        }
    }));

    assert_eq!(summary.field_count, 3);
    assert_eq!(summary.required_count, 2);
    assert!(summary.has_secret_field);
}

#[test]
fn jsonrpc_elicitation_error_parses_request() {
    let error = harness_mcp::JsonRpcError {
        code: MCP_ELICITATION_REQUIRED_CODE,
        message: "more input required".to_owned(),
        data: Some(json!({
            "server_id": "github",
            "request_id": RequestId::from_u128(42),
            "subject": "credentials",
            "detail": "token required",
            "schema": {
                "type": "object",
                "properties": { "token": { "type": "string" } }
            },
            "timeout_ms": 5000
        })),
        extra: Default::default(),
    };

    let request = elicitation_from_jsonrpc_error(&error).expect("parsed");
    assert_eq!(request.server_id, McpServerId("github".to_owned()));
    assert_eq!(request.subject, "credentials");
    assert_eq!(request.timeout, Some(std::time::Duration::from_secs(5)));
}

#[tokio::test]
async fn jsonrpc_elicitation_error_invokes_configured_handler() {
    let handler = Arc::new(DirectElicitationHandler::new(|request| async move {
        assert_eq!(request.subject, "credentials");
        Ok(json!({ "token": "secret" }))
    }));

    let value =
        handle_jsonrpc_elicitation_error(&sample_jsonrpc_error(), PermissionMode::Default, handler)
            .await
            .expect("handled")
            .expect("elicitation value");

    assert_eq!(value, json!({ "token": "secret" }));
}

#[tokio::test]
async fn bypass_and_dontask_force_reject_all_elicitation_handler() {
    let handler = Arc::new(DirectElicitationHandler::new(|_request| async move {
        Ok(json!({ "token": "should-not-be-used" }))
    }));

    let bypass = handle_jsonrpc_elicitation_error(
        &sample_jsonrpc_error(),
        PermissionMode::BypassPermissions,
        handler.clone(),
    )
    .await
    .expect_err("bypass rejects");
    let dont_ask =
        handle_jsonrpc_elicitation_error(&sample_jsonrpc_error(), PermissionMode::DontAsk, handler)
            .await
            .expect_err("dont ask rejects");

    assert_eq!(bypass, ElicitationError::UserDeclined);
    assert_eq!(dont_ask, ElicitationError::UserDeclined);
}

fn sample_request() -> ElicitationRequest {
    ElicitationRequest {
        request_id: RequestId::from_u128(42),
        server_id: McpServerId("github".to_owned()),
        schema: json!({
            "type": "object",
            "properties": {
                "token": { "type": "string" }
            }
        }),
        subject: "credentials".to_owned(),
        detail: Some("token required".to_owned()),
        timeout: None,
    }
}

fn sample_jsonrpc_error() -> harness_mcp::JsonRpcError {
    harness_mcp::JsonRpcError {
        code: MCP_ELICITATION_REQUIRED_CODE,
        message: "more input required".to_owned(),
        data: Some(json!({
            "server_id": "github",
            "request_id": RequestId::from_u128(42),
            "subject": "credentials",
            "schema": {
                "type": "object",
                "properties": { "token": { "type": "string" } }
            }
        })),
        extra: Default::default(),
    }
}

#[derive(Default)]
struct CollectingSink {
    events: Mutex<Vec<Event>>,
}

impl CollectingSink {
    fn events(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

impl harness_mcp::McpEventSink for CollectingSink {
    fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }
}
