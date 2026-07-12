#![cfg(feature = "stdio")]

use std::{
    collections::{BTreeMap, BTreeSet},
    process::Command,
    sync::Arc,
    time::Duration,
};

use futures::StreamExt;
use harness_contracts::{
    CapabilityRegistry, DeferPolicy, Event, McpServerId, McpServerSource, RedactRules, Redactor,
    RequestId, SessionId, TenantId, ToolActionPlan, ToolError,
};
use harness_mcp::{
    DirectElicitationHandler, ManagedMcpConnection, McpChange, McpClient, McpConnectContext,
    McpConnection, McpConnectionState, McpError, McpEventSink, McpServerScope, McpServerSpec,
    McpToolCallEvent, McpToolDescriptor, McpToolWrapper, NoopMcpMetricsSink, StdioEnv, StdioPolicy,
    StdioTransport, TransportChoice, MCP_ELICITATION_REQUIRED_CODE,
};
use harness_tool::{
    AuthorizedTicketSummary, AuthorizedToolInput, InterruptToken, Tool, ToolContext, ToolEvent,
};
use parking_lot::Mutex;
use serde_json::{json, Value};

mod support;

#[tokio::test]
async fn stdio_routes_interleaved_bidirectional_messages_without_blocking_response() {
    let marker =
        std::env::temp_dir().join(format!("jyowo-stdio-bidirectional-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        r#"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "{}"
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{{"tools":{{"listChanged":true}}}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
      ;;
    *'"method":"tools/list"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":77,"method":"ping","params":{{}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","id":"server-request","method":"fixture/unknown","params":{{}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}}'
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"tools":[]}}}}'
      ;;
  esac
done
"#,
        marker.display()
    );
    let spec = McpServerSpec::new(
        McpServerId("stdio-bidirectional".into()),
        "stdio bidirectional fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");
    let mut changes = connection.subscribe_changes().await.expect("changes");

    let tools = tokio::time::timeout(Duration::from_secs(1), connection.list_tools())
        .await
        .expect("interleaved server messages must not block tools/list")
        .expect("tools list");
    assert!(tools.is_empty());
    assert_eq!(changes.next().await, Some(McpChange::ToolsListChanged));

    let observed = wait_for_marker_text(&marker, |text| {
        text.contains("\"id\":77,\"result\":{}")
            && text.contains("\"id\":\"server-request\",\"error\":")
    })
    .await;
    let lines = observed
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("fixture input is JSON"))
        .collect::<Vec<_>>();
    let initialize = lines
        .iter()
        .find(|message| message.get("method") == Some(&json!("initialize")))
        .expect("initialize request");
    assert_eq!(initialize["params"]["protocolVersion"], json!("2025-11-25"));
    let initialized = lines
        .iter()
        .find(|message| message.get("method") == Some(&json!("notifications/initialized")))
        .expect("initialized notification");
    assert!(initialized.get("params").is_none());

    connection.shutdown().await.expect("shutdown");
    let _ = std::fs::remove_file(&marker);
}

#[tokio::test]
async fn stdio_rejects_missing_expected_capability_without_sending_initialized() {
    let marker = std::env::temp_dir().join(format!(
        "jyowo-stdio-failed-handshake-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        r#"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "{}"
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{{}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
      ;;
  esac
done
"#,
        marker.display()
    );
    let spec = McpServerSpec::new(
        McpServerId("stdio-failed-handshake".into()),
        "stdio failed handshake fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );

    let error = match McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
    {
        Ok(_) => panic!("missing tools capability must reject handshake"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("missing required capabilities"));
    let observed = std::fs::read_to_string(&marker).expect("fixture input marker");
    assert!(!observed.contains("notifications/initialized"));
    let _ = std::fs::remove_file(&marker);
}

#[tokio::test]
async fn stdio_connect_fails_when_initialized_cannot_be_written() {
    let script = r#"
IFS= read -r line
exec 0<&-
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
sleep 1
"#;
    let mut spec = McpServerSpec::new(
        McpServerId("stdio-initialized-write-failure".into()),
        "stdio initialized write failure fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );
    spec.timeouts.handshake = Duration::from_millis(500);

    let result = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await;

    assert!(
        result.is_err(),
        "connect must not return a connection before initialized reaches stdin"
    );
}

#[tokio::test]
async fn stdio_connect_rejects_or_observes_eof_after_initialize_response() {
    let script = r#"
IFS= read -r line
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"0.1.0"}}}'
exec 1>&-
sleep 1
"#;
    let mut spec = McpServerSpec::new(
        McpServerId("stdio-handshake-eof".into()),
        "stdio handshake EOF fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );
    spec.timeouts.handshake = Duration::from_millis(500);

    let result = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await;

    if let Ok(connection) = result {
        let call_result = tokio::time::timeout(Duration::from_millis(200), connection.list_tools())
            .await
            .expect("handshake EOF must promptly close a returned peer");
        assert!(call_result.is_err(), "handshake EOF must close the peer");
        let _ = connection.shutdown().await;
    }
}

#[tokio::test]
async fn stdio_handshake_uses_handshake_timeout() {
    let script = r#"
IFS= read -r line
sleep 60
"#;
    let mut spec = McpServerSpec::new(
        McpServerId("stdio-handshake-timeout".into()),
        "stdio handshake timeout fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );
    spec.timeouts.handshake = Duration::from_millis(50);
    spec.timeouts.call_default = Duration::from_secs(5);

    let result = tokio::time::timeout(
        Duration::from_millis(500),
        McpClient::new(Arc::new(StdioTransport::new()))
            .connect_with_context(spec, support::authorized_connect_context()),
    )
    .await
    .expect("handshake must honor its timeout");
    assert!(result.is_err());
}

#[tokio::test]
async fn stdio_transport_initializes_lists_and_calls_tool() {
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"0.1.0"}}}'
      ;;
    *'"method":"tools/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"Echo input","inputSchema":{"type":"object"}}]}}'
      ;;
    *'"method":"tools/call"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"echo:hi"}],"isError":false}}'
      ;;
  esac
done
"#;
    let spec = McpServerSpec::new(
        McpServerId("stdio".into()),
        "stdio fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(std::sync::Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");

    let tools = connection.list_tools().await.expect("tools list");
    assert_eq!(tools[0].name, "echo");

    let result = connection
        .call_tool("echo", json!({ "text": "hi" }))
        .await
        .expect("tool call");
    assert_eq!(result, harness_mcp::McpToolResult::text("echo:hi"));

    connection.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn stdio_transport_continues_tool_call_after_elicitation_resolution() {
    let script = format!(
        r#"
call_count=0
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-03-26","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
      ;;
    *'"method":"tools/call"'*)
      call_count=$((call_count + 1))
      if [ "$call_count" -eq 1 ]; then
        printf '%s\n' '{{"jsonrpc":"2.0","id":2,"error":{{"code":{code},"message":"more input required","data":{{"server_id":"stdio","request_id":"{request_id}","subject":"credentials","schema":{{"type":"object"}}}}}}}}'
      else
        case "$line" in
          *'"token":"resolved"'*)
            printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"content":[{{"type":"text","text":"stdio-found"}}],"isError":false}}}}'
            ;;
        esac
      fi
      ;;
  esac
done
"#,
        code = MCP_ELICITATION_REQUIRED_CODE,
        request_id = RequestId::from_u128(42)
    );
    let spec = McpServerSpec::new(
        McpServerId("stdio".into()),
        "stdio fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );
    let handler =
        DirectElicitationHandler::new(|_request| async { Ok(json!({ "token": "resolved" })) });

    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(
            spec,
            support::with_transport_authorization(
                McpConnectContext::default().with_elicitation_handler(Arc::new(handler)),
            ),
        )
        .await
        .expect("stdio connects");

    let result = connection
        .call_tool("search", json!({ "q": "mcp" }))
        .await
        .expect("tool call continues");
    assert_eq!(result, harness_mcp::McpToolResult::text("stdio-found"));

    connection.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn stdio_transport_maps_resource_updated_notifications_to_changes() {
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{},"resources":{"subscribe":true}},"serverInfo":{"name":"fixture","version":"0.1.0"}}}'
      ;;
    *'"method":"resources/subscribe"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{}}'
      printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/resources/updated","params":{"uri":"jyowo://sessions/1"}}'
      ;;
    *'"method":"resources/unsubscribe"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{}}'
      ;;
  esac
done
"#;
    let spec = McpServerSpec::new(
        McpServerId("stdio-resources".into()),
        "stdio resources fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");
    let mut changes = connection.subscribe_changes().await.expect("changes");

    connection
        .subscribe_resource("jyowo://sessions/1")
        .await
        .expect("subscribe");
    let change = tokio::time::timeout(Duration::from_secs(1), changes.next())
        .await
        .expect("resource update notification");
    assert_eq!(
        change,
        Some(McpChange::ResourceUpdated {
            uri: "jyowo://sessions/1".into()
        })
    );
    connection
        .unsubscribe_resource("jyowo://sessions/1")
        .await
        .expect("unsubscribe");
    connection.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn stdio_tool_call_stream_surfaces_cancelled_notifications() {
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"0.1.0"}}}'
      ;;
    *'"method":"tools/call"'*)
      case "$line" in
        *'"progressToken":2'*) ;;
        *) exit 2 ;;
      esac
      printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2,"reason":"client interrupted"}}'
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"late"}],"isError":false}}'
      ;;
  esac
done
"#;
    let spec = McpServerSpec::new(
        McpServerId("stdio-cancel".into()),
        "stdio cancel fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");

    let mut events = connection
        .call_tool_events("slow", json!({}))
        .await
        .expect("tool call stream");
    let event = tokio::time::timeout(Duration::from_secs(1), events.next())
        .await
        .expect("cancel notification");

    assert_eq!(
        event,
        Some(McpToolCallEvent::Cancelled {
            request_id: Some("2".into()),
            reason: Some("client interrupted".into())
        })
    );

    connection.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn stdio_tool_call_stream_preserves_legacy_elicitation_error_mapping() {
    let script = format!(
        r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-03-26","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
      ;;
    *'"method":"tools/call"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"error":{{"code":{code},"message":"more input required","data":{{"server_id":"stdio","request_id":"{request_id}","subject":"credentials","schema":{{"type":"object"}}}}}}}}'
      ;;
  esac
done
"#,
        code = MCP_ELICITATION_REQUIRED_CODE,
        request_id = RequestId::from_u128(84)
    );
    let spec = McpServerSpec::new(
        McpServerId("stdio-stream-elicitation".into()),
        "stdio stream elicitation fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );
    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");
    let mut events = connection
        .call_tool_events("search", json!({}))
        .await
        .expect("tool stream");

    let Some(McpToolCallEvent::Error(error)) = events.next().await else {
        panic!("expected elicitation error event")
    };
    assert!(matches!(error, harness_mcp::McpError::Elicitation(_)));
    connection.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn concurrent_stdio_tool_call_events_do_not_cross_request_ids() {
    let script = r#"
call_count=0
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"0.1.0"}}}'
      ;;
    *'"method":"tools/call"'*)
      call_count=$((call_count + 1))
      if [ "$call_count" -eq 1 ]; then
        case "$line" in
          *'"progressToken":2'*) ;;
          *) exit 2 ;;
        esac
      else
        case "$line" in
          *'"progressToken":3'*) ;;
          *) exit 3 ;;
        esac
        printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"2","progress":99,"message":"same text, string token"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":2,"progress":1,"message":"first"}}'
        printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":3,"progress":2,"message":"second"}}'
        printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"second-result"}],"isError":false}}'
        printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"first-result"}],"isError":false}}'
      fi
      ;;
  esac
done
"#;
    let spec = McpServerSpec::new(
        McpServerId("stdio-concurrent-events".into()),
        "stdio concurrent events fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );
    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");

    let mut first = connection
        .call_tool_events("first", json!({}))
        .await
        .expect("first stream");
    let mut second = connection
        .call_tool_events("second", json!({}))
        .await
        .expect("second stream");

    assert_eq!(
        first.next().await,
        Some(McpToolCallEvent::Progress {
            progress_token: Some("2".into()),
            progress: Some(1.0),
            total: None,
            message: Some("first".into()),
        })
    );
    assert_eq!(
        second.next().await,
        Some(McpToolCallEvent::Progress {
            progress_token: Some("3".into()),
            progress: Some(2.0),
            total: None,
            message: Some("second".into()),
        })
    );
    assert_eq!(
        first.next().await,
        Some(McpToolCallEvent::Final(harness_mcp::McpToolResult::text(
            "first-result"
        )))
    );
    assert_eq!(
        second.next().await,
        Some(McpToolCallEvent::Final(harness_mcp::McpToolResult::text(
            "second-result"
        )))
    );

    connection.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn stdio_cancel_uses_the_active_tool_calls_numeric_peer_request_id() {
    let marker =
        std::env::temp_dir().join(format!("jyowo-stdio-numeric-cancel-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
      ;;
    *'"method":"notifications/cancelled"'*)
      printf '%s\n' "$line" >> "{}"
      ;;
    *'"method":"tools/call"'*) ;;
  esac
done
"#,
        marker.display()
    );
    let spec = McpServerSpec::new(
        McpServerId("stdio-numeric-cancel".into()),
        "stdio numeric cancel fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );
    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");

    let events = connection
        .call_tool_events_for_request("tool-use-uuid", "slow", json!({}))
        .await
        .expect("tool call stream");
    connection
        .cancel_tool_call("tool-use-uuid", Some("client interrupted".into()))
        .await
        .expect("cancel notification");
    let observed = wait_for_marker_text(&marker, |text| text.contains("client interrupted")).await;
    let notification: Value = observed
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("cancel JSON"))
        .find(|notification| notification["params"]["reason"] == "client interrupted")
        .expect("client cancellation");
    assert_eq!(notification["params"]["requestId"], json!(2));

    drop(events);
    connection
        .cancel_tool_call("tool-use-uuid", Some("after drop".into()))
        .await
        .expect("cancel after stream drop");
    let observed = wait_for_marker_text(&marker, |text| text.contains("after drop")).await;
    let notification: Value = observed
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("cancel JSON"))
        .find(|notification| notification["params"]["reason"] == "after drop")
        .expect("post-drop cancellation");
    assert_eq!(notification["params"]["requestId"], json!("tool-use-uuid"));

    connection.shutdown().await.expect("shutdown");
    let _ = std::fs::remove_file(&marker);
}

#[tokio::test]
async fn stdio_notification_reports_a_writer_failure() {
    let marker =
        std::env::temp_dir().join(format!("jyowo-stdio-writer-closed-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        r#"
IFS= read -r line
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
IFS= read -r line
exec 0<&-
printf '%s' closed > "{}"
sleep 1
"#,
        marker.display()
    );
    let spec = McpServerSpec::new(
        McpServerId("stdio-notification-write-failure".into()),
        "stdio notification write failure fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );
    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");
    wait_for_marker_text(&marker, |text| text == "closed").await;

    let result = tokio::time::timeout(Duration::from_millis(200), async {
        let mut request_id = 2_u64;
        loop {
            match connection
                .cancel_tool_call(&request_id.to_string(), None)
                .await
            {
                Ok(()) => {
                    request_id += 1;
                    tokio::task::yield_now().await;
                }
                Err(error) => break Err::<(), _>(error),
            }
        }
    })
    .await
    .expect("closed stdio transport must reject a notification");

    let error = result.expect_err("notification caller must observe the stdin write failure");
    assert!(
        matches!(&error, McpError::Transport(message) if message.contains("stdio writer failed")),
        "notification caller must observe the typed stdin writer failure: {error}"
    );
    let _ = std::fs::remove_file(&marker);
}

#[tokio::test]
async fn malformed_stdio_message_closes_peer_and_wakes_pending_request() {
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"0.1.0"}}}'
      ;;
    *'"method":"tools/list"'*)
      printf '%s\n' 'not-json'
      sleep 60
      ;;
  esac
done
"#;
    let mut spec = McpServerSpec::new(
        McpServerId("stdio-malformed".into()),
        "stdio malformed fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );
    spec.timeouts.call_default = Duration::from_secs(30);
    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");

    let result = tokio::time::timeout(Duration::from_millis(500), connection.list_tools())
        .await
        .expect("malformed input must wake pending request");
    assert!(result.is_err());
    connection.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn stdio_eof_closes_peer_and_wakes_pending_request() {
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"0.1.0"}}}'
      ;;
    *'"method":"tools/list"'*)
      exit 0
      ;;
  esac
done
"#;
    let mut spec = McpServerSpec::new(
        McpServerId("stdio-eof".into()),
        "stdio EOF fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );
    spec.timeouts.call_default = Duration::from_secs(30);
    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");

    let result = tokio::time::timeout(Duration::from_millis(500), connection.list_tools())
        .await
        .expect("EOF must wake pending request");
    assert!(result.is_err());
    connection.shutdown().await.expect("shutdown");
}

#[test]
fn stdio_env_resolver_denies_credentials_before_spawning() {
    let parent = BTreeMap::from([
        ("OPENAI_API_KEY".to_owned(), "secret".to_owned()),
        ("PATH".to_owned(), "/bin".to_owned()),
    ]);
    let env = StdioEnv::InheritWithDeny {
        deny: BTreeSet::from(["OPENAI_API_KEY".to_owned()]),
        extra: BTreeMap::from([("EXTRA".to_owned(), "1".to_owned())]),
    };

    let resolved = StdioTransport::resolve_env(&env, &parent);

    assert!(!resolved.contains_key("OPENAI_API_KEY"));
    assert_eq!(resolved.get("PATH").map(String::as_str), Some("/bin"));
    assert_eq!(resolved.get("EXTRA").map(String::as_str), Some("1"));
}

#[tokio::test]
async fn stdio_transport_journals_redacted_capped_stderr_lines() {
    let script = r#"
printf '%s\n' 'token=secret-1234567890 extra-tail' >&2
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"0.1.0"}}}'
      ;;
  esac
done
"#;
    let sink = Arc::new(CollectingSink::default());
    let spec = McpServerSpec::new(
        McpServerId("stderr".into()),
        "stderr fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: StdioEnv::default(),
            policy: StdioPolicy {
                stderr_line_max_bytes: 24,
                redact_stderr: true,
                graceful_kill_after: Duration::from_millis(50),
                working_dir: None,
            },
        },
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(Arc::new(
        StdioTransport::with_event_sink(sink.clone()).with_redactor(Arc::new(TokenRedactor)),
    ))
    .connect_with_context(spec, support::authorized_connect_context())
    .await
    .expect("stdio connects");

    tokio::time::sleep(Duration::from_millis(50)).await;
    connection.shutdown().await.expect("shutdown");

    let events = sink.events();
    assert!(events.iter().any(|event| matches!(
        event,
        Event::UnexpectedError(event)
            if event.error.contains("mcp stdio stderr stderr: token=[redacted]")
                && !event.error.contains("secret-1234567890")
                && !event.error.contains("extra-tail")
    )));
}

#[tokio::test]
async fn stdio_shutdown_waits_for_graceful_child_exit() {
    let marker = std::env::temp_dir().join(format!("jyowo-stdio-shutdown-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        r#"
    trap 'printf term > "{}"; exit 0' TERM
    while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-03-26","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
      ;;
        *'"method":"shutdown"'*)
          printf shutdown > "{}"
          exit 0
          ;;
      esac
    done
    printf eof > "{}"
"#,
        marker.display(),
        marker.display(),
        marker.display()
    );
    let spec = McpServerSpec::new(
        McpServerId("shutdown".into()),
        "shutdown fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: StdioEnv::default(),
            policy: StdioPolicy {
                stderr_line_max_bytes: 4096,
                redact_stderr: true,
                graceful_kill_after: Duration::from_secs(1),
                working_dir: None,
            },
        },
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");
    connection.shutdown().await.expect("shutdown");

    assert_eq!(
        std::fs::read_to_string(&marker).expect("marker exists"),
        "eof"
    );
    let _ = std::fs::remove_file(&marker);
}

#[tokio::test]
async fn stdio_shutdown_times_out_blocked_notification_and_kills_child() {
    let marker =
        std::env::temp_dir().join(format!("jyowo-stdio-term-child-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        r#"
trap 'printf term > "{}"; exit 0' TERM
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-03-26","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
      break
      ;;
  esac
done
while :; do :; done
"#,
        marker.display()
    );
    let spec = McpServerSpec::new(
        McpServerId("blocked-shutdown".into()),
        "blocked shutdown fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: StdioEnv::default(),
            policy: StdioPolicy {
                stderr_line_max_bytes: 4096,
                redact_stderr: true,
                graceful_kill_after: Duration::from_millis(100),
                working_dir: None,
            },
        },
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");
    let mut blocked_writes = Vec::new();
    for request_id in 0..70 {
        let blocked_connection = Arc::clone(&connection);
        blocked_writes.push(tokio::spawn(async move {
            blocked_connection
                .cancel_tool_call(&request_id.to_string(), Some("x".repeat(1024 * 1024)))
                .await
        }));
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        blocked_writes.iter().any(|write| !write.is_finished()),
        "fixture must saturate the bounded writer"
    );

    let shutdown_result = tokio::time::timeout(Duration::from_secs(1), connection.shutdown()).await;
    for blocked_write in blocked_writes {
        blocked_write.abort();
        let _ = blocked_write.await;
    }
    drop(connection);

    assert!(
        shutdown_result.is_ok(),
        "shutdown notification must not block child termination"
    );
    assert_eq!(
        std::fs::read_to_string(&marker).expect("TERM marker exists"),
        "term"
    );
    let _ = std::fs::remove_file(&marker);
}

#[tokio::test]
async fn wrapper_cancel_deadline_bounds_blocked_stdio_and_marks_connection_unhealthy() {
    let marker =
        std::env::temp_dir().join(format!("jyowo-stdio-blocked-cancel-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
      ;;
    *'"method":"tools/call"'*)
      printf '%s' blocked > "{}"
      break
      ;;
  esac
done
while :; do :; done
"#,
        marker.display()
    );
    let mut spec = McpServerSpec::new(
        McpServerId("blocked-wrapper-cancel".into()),
        "blocked wrapper cancel fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: StdioEnv::default(),
            policy: StdioPolicy {
                stderr_line_max_bytes: 4096,
                redact_stderr: true,
                graceful_kill_after: Duration::from_millis(100),
                working_dir: None,
            },
        },
        McpServerSource::Workspace,
    );
    spec.timeouts.cancel_ack = Duration::from_millis(50);
    let managed = Arc::new(
        ManagedMcpConnection::connect_with_context_and_metrics(
            Arc::new(StdioTransport::new()),
            spec.clone(),
            McpServerScope::Session(SessionId::new()),
            support::authorized_connect_context(),
        )
        .await
        .expect("managed stdio connects"),
    );
    let connection: Arc<dyn McpConnection> = managed.clone();
    let tool: Arc<dyn Tool> = Arc::new(McpToolWrapper::new_with_metrics_and_cancel_ack_timeout(
        spec.server_id.clone(),
        spec.source.clone(),
        spec.manifest_origin.clone(),
        spec.trust,
        McpToolDescriptor {
            name: "slow".into(),
            title: None,
            icons: None,
            execution: None,
            description: Some("slow fixture".into()),
            input_schema: json!({ "type": "object" }),
            output_schema: None,
            annotations: None,
            meta: BTreeMap::new(),
        },
        connection,
        DeferPolicy::AutoDefer,
        "mcp__blocked__slow".into(),
        Arc::new(NoopMcpMetricsSink),
        spec.timeouts.cancel_ack,
    ));
    let interrupt = InterruptToken::new();
    let mut context = wrapper_tool_context();
    context.interrupt = interrupt.clone();
    let mut stream = run_wrapper_authorized(&tool, json!({}), context)
        .await
        .expect("wrapper starts tool call");
    wait_for_marker_text(&marker, |text| text == "blocked").await;

    let mut blocked_writes = Vec::new();
    for request_id in 0..70 {
        let managed = Arc::clone(&managed);
        blocked_writes.push(tokio::spawn(async move {
            managed
                .cancel_tool_call(&request_id.to_string(), Some("x".repeat(1024 * 1024)))
                .await
        }));
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        blocked_writes.iter().any(|write| !write.is_finished()),
        "fixture must block stdio cancellation writes"
    );

    let started = std::time::Instant::now();
    interrupt.interrupt();
    let event = tokio::time::timeout(Duration::from_millis(300), stream.next())
        .await
        .expect("wrapper cancellation must be bounded");
    assert_eq!(event, Some(ToolEvent::Error(ToolError::Interrupted)));
    assert!(started.elapsed() < Duration::from_millis(300));
    assert!(matches!(
        managed.state().await,
        McpConnectionState::Reconnecting { ref last_error, .. }
            if last_error.contains("acknowledgement timed out")
    ));

    for blocked_write in blocked_writes {
        blocked_write.abort();
        let _ = blocked_write.await;
    }
    drop(stream);
    managed.shutdown().await.expect("managed shutdown");
    let _ = std::fs::remove_file(&marker);
}

#[cfg(unix)]
#[tokio::test]
async fn stdio_connection_drop_kills_child() {
    let marker =
        std::env::temp_dir().join(format!("jyowo-stdio-drop-child-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        r#"
printf '%s' "$$" > "{}"
IFS= read -r line
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-03-26","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
exec sleep 60
"#,
        marker.display()
    );
    let spec = McpServerSpec::new(
        McpServerId("drop-child".into()),
        "drop child fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("stdio connects");
    let pid = read_fixture_pid(&marker).await;

    drop(connection);

    let exited = wait_for_process_exit(pid).await;
    if !exited {
        kill_fixture_process(pid);
    }
    let _ = std::fs::remove_file(&marker);
    assert!(
        exited,
        "stdio child must exit when its connection is dropped"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn stdio_connect_failure_kills_spawned_child() {
    let marker = std::env::temp_dir().join(format!(
        "jyowo-stdio-connect-failure-child-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let script = format!(
        r#"
printf '%s' "$$" > "{}"
IFS= read -r line
printf '%s\n' 'not-json'
exec sleep 60
"#,
        marker.display()
    );
    let spec = McpServerSpec::new(
        McpServerId("connect-failure-child".into()),
        "connect failure child fixture",
        TransportChoice::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: StdioEnv::default(),
            policy: StdioPolicy::default(),
        },
        McpServerSource::Workspace,
    );

    let connection_result = McpClient::new(Arc::new(StdioTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await;
    assert!(
        connection_result.is_err(),
        "invalid initialize response must fail the connection"
    );
    let pid = read_fixture_pid(&marker).await;

    let exited = wait_for_process_exit(pid).await;
    if !exited {
        kill_fixture_process(pid);
    }
    let _ = std::fs::remove_file(&marker);
    assert!(
        exited,
        "spawned stdio child must exit when connection setup fails"
    );
}

#[cfg(unix)]
async fn read_fixture_pid(marker: &std::path::Path) -> u32 {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let Ok(value) = std::fs::read_to_string(marker) {
                break value.parse().expect("fixture pid");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture pid marker")
}

async fn wait_for_marker_text(
    marker: &std::path::Path,
    predicate: impl Fn(&str) -> bool,
) -> String {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let Ok(value) = std::fs::read_to_string(marker) {
                if predicate(&value) {
                    break value;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture marker predicate")
}

fn wrapper_tool_context() -> ToolContext {
    ToolContext {
        tool_use_id: harness_contracts::ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::path::PathBuf::from("."),
        project_workspace_root: None,
        sandbox: None,
        cap_registry: Arc::new(CapabilityRegistry::default()),
        redactor: Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::new(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

async fn run_wrapper_authorized(
    tool: &Arc<dyn Tool>,
    input: Value,
    context: ToolContext,
) -> Result<harness_tool::ToolStream, ToolError> {
    tool.validate(&input, &context)
        .await
        .expect("test input validates");
    let plan = tool.plan(&input, &context).await?;
    let authorized = AuthorizedToolInput::new(input, plan.clone(), wrapper_ticket_for(&plan))?;
    tool.execute_authorized(authorized, context).await
}

fn wrapper_ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    let ledger = harness_tool::TicketLedger::default();
    let claims = harness_tool::AuthorizationTicketClaims {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: harness_contracts::RunId::new(),
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
    };
    let ticket = ledger
        .mint(claims.clone(), chrono::Utc::now())
        .expect("test ticket should mint");
    ledger
        .consume(ticket.id, &claims, chrono::Utc::now())
        .expect("test ticket should consume")
}

#[cfg(unix)]
async fn wait_for_process_exit(pid: u32) -> bool {
    for _ in 0..50 {
        if !fixture_process_is_running(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    !fixture_process_is_running(pid)
}

#[cfg(unix)]
fn fixture_process_is_running(pid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn kill_fixture_process(pid: u32) {
    let _ = Command::new("/bin/kill")
        .args(["-KILL", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status();
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

impl McpEventSink for CollectingSink {
    fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }
}

struct TokenRedactor;

impl Redactor for TokenRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("secret-1234567890", "[redacted]")
    }
}
