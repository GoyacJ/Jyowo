use std::sync::Arc;

use harness_contracts::{
    ElicitationOutcome, Event, McpServerId, PermissionMode, RequestId, RunId, SessionId,
};
use harness_mcp::{
    summarize_elicitation_schema, url_elicitations_from_jsonrpc_error, DirectElicitationHandler,
    ElicitationCompletion, ElicitationError, ElicitationHandler, ElicitationJsonRpcHandler,
    ElicitationMode, ElicitationRequest, JsonRpcError, JsonRpcNotification, JsonRpcRequest,
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
async fn form_elicitation_uses_standard_request_and_result_models() {
    let handler = ElicitationJsonRpcHandler::new(
        McpServerId("github".to_owned()),
        PermissionMode::Default,
        Arc::new(DirectElicitationHandler::new(|request| async move {
            assert_eq!(request.subject, "Provide credentials");
            assert_eq!(request.mode, ElicitationMode::Form);
            assert_eq!(request.schema["required"], json!(["token"]));
            Ok(json!({ "token": "secret" }))
        })),
    );

    let result = handler
        .route_elicitation_request(JsonRpcRequest::new(
            json!(1),
            "elicitation/create",
            Some(json!({
                "mode": "form",
                "message": "Provide credentials",
                "requestedSchema": {
                    "type": "object",
                    "properties": { "token": { "type": "string" } },
                    "required": ["token"]
                }
            })),
        ))
        .await
        .expect("form result");

    assert_eq!(
        result,
        json!({ "action": "accept", "content": { "token": "secret" } })
    );
}

#[tokio::test]
async fn url_elicitation_has_no_form_content_and_error_code_only_carries_urls() {
    let handler = ElicitationJsonRpcHandler::new(
        McpServerId("github".to_owned()),
        PermissionMode::Default,
        Arc::new(DirectElicitationHandler::new(|request| async move {
            assert_eq!(
                request.mode,
                ElicitationMode::Url {
                    elicitation_id: "oauth-1".to_owned(),
                    url: "https://example.com/authorize".to_owned(),
                }
            );
            Ok(json!(null))
        })),
    );
    let result = handler
        .route_elicitation_request(JsonRpcRequest::new(
            json!(2),
            "elicitation/create",
            Some(json!({
                "mode": "url",
                "message": "Authorize access",
                "elicitationId": "oauth-1",
                "url": "https://example.com/authorize"
            })),
        ))
        .await
        .expect("url result");
    assert_eq!(result, json!({ "action": "accept" }));

    let error = JsonRpcError {
        code: MCP_ELICITATION_REQUIRED_CODE,
        message: "URL interaction required".to_owned(),
        data: Some(json!({
            "elicitations": [{
                "mode": "url",
                "message": "Authorize access",
                "elicitationId": "oauth-1",
                "url": "https://example.com/authorize"
            }]
        })),
        extra: Default::default(),
    };
    let elicitations = url_elicitations_from_jsonrpc_error(&error).expect("URL elicitations");
    assert_eq!(elicitations.len(), 1);

    let legacy_form_error = JsonRpcError {
        code: MCP_ELICITATION_REQUIRED_CODE,
        message: "legacy form".to_owned(),
        data: Some(json!({ "schema": { "type": "object" } })),
        extra: Default::default(),
    };
    assert!(url_elicitations_from_jsonrpc_error(&legacy_form_error).is_none());

    for invalid in [
        json!({
            "mode": "url",
            "message": "Authorize access",
            "elicitationId": "   ",
            "url": "https://example.com/authorize"
        }),
        json!({
            "mode": "url",
            "message": "\t",
            "elicitationId": "oauth-1",
            "url": "https://example.com/authorize"
        }),
    ] {
        let error = JsonRpcError {
            code: MCP_ELICITATION_REQUIRED_CODE,
            message: "URL interaction required".to_owned(),
            data: Some(json!({ "elicitations": [invalid] })),
            extra: Default::default(),
        };
        assert!(url_elicitations_from_jsonrpc_error(&error).is_none());
    }
}

#[tokio::test]
async fn url_completion_is_correlated_with_the_original_url_request() {
    #[derive(Default)]
    struct CompletionHandler {
        completion: Mutex<Option<ElicitationCompletion>>,
    }

    #[async_trait::async_trait]
    impl ElicitationHandler for CompletionHandler {
        fn handler_id(&self) -> &'static str {
            "completion"
        }

        async fn handle(
            &self,
            _request: ElicitationRequest,
        ) -> Result<serde_json::Value, ElicitationError> {
            Ok(json!(null))
        }

        async fn handle_url_completion(
            &self,
            completion: ElicitationCompletion,
        ) -> Result<(), ElicitationError> {
            *self.completion.lock() = Some(completion);
            Ok(())
        }
    }

    let completion_handler = Arc::new(CompletionHandler::default());
    let handler = ElicitationJsonRpcHandler::new(
        McpServerId("github".to_owned()),
        PermissionMode::Default,
        completion_handler.clone(),
    );
    handler
        .route_elicitation_request(JsonRpcRequest::new(
            json!(2),
            "elicitation/create",
            Some(json!({
                "mode": "url",
                "message": "Authorize access",
                "elicitationId": "oauth-1",
                "url": "https://example.com/authorize"
            })),
        ))
        .await
        .expect("url result");

    handler
        .route_elicitation_completion(JsonRpcNotification::new(
            "notifications/elicitation/complete",
            Some(json!({ "elicitationId": "oauth-1" })),
        ))
        .await
        .expect("completion notification");

    let completion = completion_handler
        .completion
        .lock()
        .clone()
        .expect("completion delivered");
    assert_eq!(completion.server_id, McpServerId("github".to_owned()));
    assert_eq!(completion.elicitation.elicitation_id, "oauth-1");
    assert_eq!(completion.elicitation.url, "https://example.com/authorize");
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
        mode: ElicitationMode::Form,
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
