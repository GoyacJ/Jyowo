#![cfg(feature = "code-runtime")]

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::{EmbeddedRefusedReason, Event, RunId, SandboxError, SessionId, ToolUseId};
use harness_sandbox::{
    CodeSandbox, CodeSandboxCapabilities, CodeSandboxRunContext, CompiledScript,
    EmbeddedStepSummary, EmbeddedToolCall, EmbeddedToolDispatcherCap, EmbeddedToolWhitelist,
    EventSink, LuaValue, MiniLuaCodeSandbox, ScriptLanguage, UsageMeter,
};
use parking_lot::Mutex;

#[derive(Default)]
struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: Event) -> Result<(), SandboxError> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<Event>>,
}

impl EventSink for RecordingSink {
    fn emit(&self, event: Event) -> Result<(), SandboxError> {
        self.events.lock().push(event);
        Ok(())
    }
}

#[derive(Default)]
struct RecordingUsage {
    instructions: Mutex<Vec<u64>>,
}

impl UsageMeter for RecordingUsage {
    fn record_instructions(&self, count: u64) {
        self.instructions.lock().push(count);
    }

    fn record_event(&self, _event: Event) {}
}

#[derive(Default)]
struct RecordingDispatcher {
    calls: Mutex<Vec<EmbeddedToolCall>>,
}

#[async_trait]
impl EmbeddedToolDispatcherCap for RecordingDispatcher {
    async fn dispatch(
        &self,
        request: EmbeddedToolCall,
    ) -> Result<EmbeddedStepSummary, SandboxError> {
        self.calls.lock().push(request.clone());
        Ok(EmbeddedStepSummary {
            tool_use_id: ToolUseId::new(),
            tool_name: request.name.clone(),
            output_json: format!("{{\"tool\":\"{}\"}}", request.name),
            duration_ms: 7,
            overflow: None,
        })
    }
}

fn script(source: &str) -> CompiledScript {
    CompiledScript {
        language: ScriptLanguage::MiniLua,
        source_hash: *blake3::hash(source.as_bytes()).as_bytes(),
        bytecode: source.as_bytes().to_vec(),
    }
}

fn context(
    dispatcher: Arc<RecordingDispatcher>,
    usage: Arc<RecordingUsage>,
) -> CodeSandboxRunContext {
    CodeSandboxRunContext {
        session_id: SessionId::new(),
        run_id: RunId::new(),
        parent_tool_use_id: ToolUseId::new(),
        embedded_dispatcher: dispatcher,
        usage_meter: usage,
        event_sink: Arc::new(NullSink),
    }
}

#[tokio::test]
async fn minilua_returns_basic_values_and_records_usage() {
    let sandbox = MiniLuaCodeSandbox::new();
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let usage = Arc::new(RecordingUsage::default());

    let result = sandbox
        .run(&script("return 1 + 2"), context(dispatcher, usage.clone()))
        .await
        .expect("script should run");

    assert_eq!(result.value, LuaValue::Number(3.0));
    assert!(result.stats.instructions > 0);
    assert_eq!(
        usage.instructions.lock().as_slice(),
        &[result.stats.instructions]
    );
}

#[tokio::test]
async fn minilua_enforces_instruction_call_depth_wall_clock_and_memory_limits() {
    let caps = CodeSandboxCapabilities {
        max_instructions: 4,
        ..CodeSandboxCapabilities::default()
    };
    let sandbox = MiniLuaCodeSandbox::with_capabilities(caps);
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let usage = Arc::new(RecordingUsage::default());
    let error = sandbox
        .run(&script("while true do end"), context(dispatcher, usage))
        .await
        .expect_err("unbounded loop should exceed instruction limit");
    assert!(matches!(
        error,
        SandboxError::CodeRuntime { ref detail } if detail.contains("InstructionLimit")
    ));

    let caps = CodeSandboxCapabilities {
        max_call_depth: 1,
        ..CodeSandboxCapabilities::default()
    };
    let sandbox = MiniLuaCodeSandbox::with_capabilities(caps);
    let error = sandbox
        .run(
            &script("function f() return f() end return f()"),
            context(
                Arc::new(RecordingDispatcher::default()),
                Arc::new(RecordingUsage::default()),
            ),
        )
        .await
        .expect_err("recursive call should exceed call depth");
    assert!(matches!(
        error,
        SandboxError::CodeRuntime { ref detail } if detail.contains("CallDepth")
    ));

    let caps = CodeSandboxCapabilities {
        wall_clock_budget: Duration::ZERO,
        ..CodeSandboxCapabilities::default()
    };
    let sandbox = MiniLuaCodeSandbox::with_capabilities(caps);
    let error = sandbox
        .run(
            &script("return 1"),
            context(
                Arc::new(RecordingDispatcher::default()),
                Arc::new(RecordingUsage::default()),
            ),
        )
        .await
        .expect_err("zero wall-clock budget should fail closed");
    assert!(matches!(
        error,
        SandboxError::CodeRuntime { ref detail } if detail.contains("WallClock")
    ));

    let caps = CodeSandboxCapabilities {
        max_string_bytes: 3,
        ..CodeSandboxCapabilities::default()
    };
    let sandbox = MiniLuaCodeSandbox::with_capabilities(caps);
    let error = sandbox
        .run(
            &script("return \"abcd\""),
            context(
                Arc::new(RecordingDispatcher::default()),
                Arc::new(RecordingUsage::default()),
            ),
        )
        .await
        .expect_err("oversized string should exceed memory quota");
    assert!(matches!(
        error,
        SandboxError::CodeRuntime { ref detail } if detail.contains("MemoryLimit")
    ));

    let caps = CodeSandboxCapabilities {
        max_table_entries: 2,
        ..CodeSandboxCapabilities::default()
    };
    let sandbox = MiniLuaCodeSandbox::with_capabilities(caps);
    let error = sandbox
        .run(
            &script("return {1, 2, 3}"),
            context(
                Arc::new(RecordingDispatcher::default()),
                Arc::new(RecordingUsage::default()),
            ),
        )
        .await
        .expect_err("oversized table literal should exceed memory quota");
    assert!(matches!(
        error,
        SandboxError::CodeRuntime { ref detail } if detail.contains("table entries")
    ));
}

#[tokio::test]
async fn minilua_embedded_tools_are_whitelisted_and_dispatched_through_host_callback() {
    let sandbox = MiniLuaCodeSandbox::new();
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let usage = Arc::new(RecordingUsage::default());

    let result = sandbox
        .run(
            &script("return emb.tool(\"Grep\", \"{\\\"pattern\\\":\\\"x\\\"}\")"),
            context(dispatcher.clone(), usage),
        )
        .await
        .expect("whitelisted embedded tool should dispatch");

    assert_eq!(result.embedded_steps.len(), 1);
    assert_eq!(result.embedded_steps[0].tool_name, "Grep");
    assert_eq!(result.embedded_steps[0].duration_ms, 7);
    assert_eq!(
        result.value,
        LuaValue::String("{\"tool\":\"Grep\"}".to_owned())
    );
    assert_eq!(dispatcher.calls.lock()[0].name, "Grep");
    assert_eq!(dispatcher.calls.lock()[0].input_json, "{\"pattern\":\"x\"}");
}

#[tokio::test]
async fn minilua_refused_embedded_tools_emit_structured_reason() {
    let sandbox = MiniLuaCodeSandbox::new();
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let usage = Arc::new(RecordingUsage::default());
    let sink = Arc::new(RecordingSink::default());
    let ctx = CodeSandboxRunContext {
        session_id: SessionId::new(),
        run_id: RunId::new(),
        parent_tool_use_id: ToolUseId::new(),
        embedded_dispatcher: dispatcher.clone(),
        usage_meter: usage,
        event_sink: sink.clone(),
    };

    let error = sandbox
        .run(&script("return emb.tool(\"bash\", \"{}\")"), ctx)
        .await
        .expect_err("non-whitelisted tool should be denied before dispatch");
    assert!(matches!(
        error,
        SandboxError::CodeRuntime { ref detail } if detail.contains("EmbeddedDenied")
    ));
    assert!(dispatcher.calls.lock().is_empty());
    assert!(matches!(
        sink.events.lock().as_slice(),
        [Event::ExecuteCodeStepInvoked(event)]
            if event.embedded_tool == "bash"
                && event.step_seq == 1
                && event.refused_reason == Some(EmbeddedRefusedReason::NotWhitelisted)
    ));
}

#[tokio::test]
async fn minilua_self_reentrant_execute_code_is_refused_before_whitelist() {
    let sandbox = MiniLuaCodeSandbox::new();
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let usage = Arc::new(RecordingUsage::default());
    let sink = Arc::new(RecordingSink::default());
    let ctx = CodeSandboxRunContext {
        session_id: SessionId::new(),
        run_id: RunId::new(),
        parent_tool_use_id: ToolUseId::new(),
        embedded_dispatcher: dispatcher.clone(),
        usage_meter: usage,
        event_sink: sink.clone(),
    };

    let error = sandbox
        .run(&script("return emb.tool(\"execute_code\", \"{}\")"), ctx)
        .await
        .expect_err("execute_code cannot call itself");

    assert!(matches!(
        error,
        SandboxError::CodeRuntime { ref detail } if detail.contains("SelfReentrant")
    ));
    assert!(dispatcher.calls.lock().is_empty());
    assert!(matches!(
        sink.events.lock().as_slice(),
        [Event::ExecuteCodeStepInvoked(event)]
            if event.embedded_tool == "execute_code"
                && event.step_seq == 1
                && event.refused_reason == Some(EmbeddedRefusedReason::SelfReentrant)
    ));
}

#[test]
fn minilua_default_whitelist_excludes_write_network_and_shell_tools() {
    let whitelist = EmbeddedToolWhitelist::default();

    assert!(whitelist.names.contains("Grep"));
    assert!(whitelist.names.contains("Glob"));
    assert!(whitelist.names.contains("FileRead"));
    assert!(whitelist.names.contains("ListDir"));
    assert!(whitelist.names.contains("WebSearch"));
    assert!(whitelist.names.contains("ReadBlob"));
    assert!(whitelist.names.contains("tool_search"));
    assert!(!whitelist.names.contains("Bash"));
    assert!(!whitelist.names.contains("FileWrite"));
    assert!(!whitelist.names.contains("FileEdit"));
    assert!(!whitelist.names.contains("WebFetch"));
}
