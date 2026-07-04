#![cfg(feature = "stdio")]

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use futures::StreamExt;
use harness_contracts::{Event, McpServerId, McpServerSource, RedactRules, Redactor, RequestId};
use harness_mcp::{
    DirectElicitationHandler, McpChange, McpClient, McpConnectContext, McpEventSink, McpServerSpec,
    McpToolCallEvent, StdioEnv, StdioPolicy, StdioTransport, TransportChoice,
    MCP_ELICITATION_REQUIRED_CODE,
};
use parking_lot::Mutex;
use serde_json::json;

mod support;

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
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"resources":{"subscribe":true}},"serverInfo":{"name":"fixture","version":"0.1.0"}}}'
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
      printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"2","reason":"client interrupted"}}'
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
trap 'printf done > "{}"; exit 0' TERM
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-03-26","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fixture","version":"0.1.0"}}}}}}'
      ;;
    *'"method":"shutdown"'*)
      printf done > "{}"
      exit 0
      ;;
  esac
done
"#,
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
        "done"
    );
    let _ = std::fs::remove_file(&marker);
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
