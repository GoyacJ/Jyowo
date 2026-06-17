//! In-process code sandbox contracts.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use harness_contracts::{
    EmbeddedRefusedReason, Event, ExecuteCodeStepInvokedEvent, OverflowMetadata, RunId,
    SandboxError, SessionId, ToolUseId,
};

use crate::EventSink;

#[async_trait]
pub trait CodeSandbox: Send + Sync + 'static {
    fn capabilities(&self) -> CodeSandboxCapabilities;

    async fn run(
        &self,
        script: &CompiledScript,
        ctx: CodeSandboxRunContext,
    ) -> Result<CodeSandboxResult, SandboxError>;
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct CodeSandboxCapabilities {
    pub language: ScriptLanguage,
    pub max_instructions: u64,
    pub max_call_depth: u32,
    pub max_string_bytes: u64,
    pub max_table_entries: u64,
    pub wall_clock_budget: Duration,
    pub deterministic: bool,
}

impl Default for CodeSandboxCapabilities {
    fn default() -> Self {
        Self {
            language: ScriptLanguage::MiniLua,
            max_instructions: 1_000_000,
            max_call_depth: 32,
            max_string_bytes: 4 * 1_024 * 1_024,
            max_table_entries: 65_536,
            wall_clock_budget: Duration::from_secs(30),
            deterministic: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ScriptLanguage {
    MiniLua,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct CompiledScript {
    pub language: ScriptLanguage,
    pub source_hash: [u8; 32],
    pub bytecode: Vec<u8>,
}

impl Default for CompiledScript {
    fn default() -> Self {
        Self {
            language: ScriptLanguage::MiniLua,
            source_hash: [0; 32],
            bytecode: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct CodeSandboxRunContext {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub parent_tool_use_id: ToolUseId,
    pub embedded_dispatcher: Arc<dyn EmbeddedToolDispatcherCap>,
    pub usage_meter: Arc<dyn UsageMeter>,
    pub event_sink: Arc<dyn EventSink>,
}

#[async_trait]
pub trait EmbeddedToolDispatcherCap: Send + Sync + 'static {
    async fn dispatch(
        &self,
        request: EmbeddedToolCall,
    ) -> Result<EmbeddedStepSummary, SandboxError>;
}

pub trait UsageMeter: Send + Sync + 'static {
    fn record_instructions(&self, count: u64);

    fn record_event(&self, event: Event);
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct EmbeddedToolCall {
    pub name: String,
    pub input_json: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedStepSummary {
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub output_json: String,
    pub duration_ms: u64,
    pub overflow: Option<OverflowMetadata>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodeSandboxResult {
    pub value: LuaValue,
    pub stats: SandboxRunStats,
    pub embedded_steps: Vec<EmbeddedStepSummary>,
}

impl Default for CodeSandboxResult {
    fn default() -> Self {
        Self {
            value: LuaValue::Nil,
            stats: SandboxRunStats::default(),
            embedded_steps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LuaValue {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash)]
pub struct SandboxRunStats {
    pub instructions: u64,
    pub wall_clock: Duration,
    pub max_call_depth: u32,
    pub embedded_call_count: u32,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct EmbeddedToolWhitelist {
    pub names: BTreeSet<String>,
}

impl Default for EmbeddedToolWhitelist {
    fn default() -> Self {
        Self {
            names: [
                "Grep",
                "Glob",
                "FileRead",
                "ListDir",
                "WebSearch",
                "ReadBlob",
                "tool_search",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MiniLuaCodeSandbox {
    capabilities: CodeSandboxCapabilities,
    embedded_tool_whitelist: EmbeddedToolWhitelist,
}

impl MiniLuaCodeSandbox {
    pub fn new() -> Self {
        Self {
            capabilities: CodeSandboxCapabilities::default(),
            embedded_tool_whitelist: EmbeddedToolWhitelist::default(),
        }
    }

    pub fn with_capabilities(capabilities: CodeSandboxCapabilities) -> Self {
        Self {
            capabilities,
            embedded_tool_whitelist: EmbeddedToolWhitelist::default(),
        }
    }
}

impl Default for MiniLuaCodeSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CodeSandbox for MiniLuaCodeSandbox {
    fn capabilities(&self) -> CodeSandboxCapabilities {
        self.capabilities.clone()
    }

    async fn run(
        &self,
        script: &CompiledScript,
        ctx: CodeSandboxRunContext,
    ) -> Result<CodeSandboxResult, SandboxError> {
        if script.language != ScriptLanguage::MiniLua {
            return Err(code_runtime_error("ScriptError: unsupported language"));
        }
        if self.capabilities.wall_clock_budget.is_zero() {
            return Err(code_runtime_error("WallClock: budget exhausted"));
        }

        let started = Instant::now();
        let source = std::str::from_utf8(&script.bytecode)
            .map_err(|error| code_runtime_error(format!("ScriptError: {error}")))?;
        reject_forbidden_libraries(source)?;
        enforce_memory_limit(source, self.capabilities.max_string_bytes)?;
        enforce_table_limit(source, self.capabilities.max_table_entries)?;
        let instructions = estimate_instructions(source, self.capabilities.max_instructions)?;
        let max_call_depth = estimate_call_depth(source, self.capabilities.max_call_depth)?;

        let (value, embedded_steps) = self.evaluate_return_value(source, &ctx).await?;
        if started.elapsed() > self.capabilities.wall_clock_budget {
            return Err(code_runtime_error("WallClock: budget exhausted"));
        }

        ctx.usage_meter.record_instructions(instructions);
        Ok(CodeSandboxResult {
            value,
            stats: SandboxRunStats {
                instructions,
                wall_clock: started.elapsed(),
                max_call_depth,
                embedded_call_count: embedded_steps.len().try_into().unwrap_or(u32::MAX),
            },
            embedded_steps,
        })
    }
}

impl MiniLuaCodeSandbox {
    async fn evaluate_return_value(
        &self,
        source: &str,
        ctx: &CodeSandboxRunContext,
    ) -> Result<(LuaValue, Vec<EmbeddedStepSummary>), SandboxError> {
        let Some(expression) = return_expression(source) else {
            return Ok((LuaValue::Nil, Vec::new()));
        };
        if expression.starts_with("emb.tool(") {
            let call = parse_embedded_tool_call(expression)?;
            if call.name == "execute_code" {
                emit_refused_embedded_step(
                    ctx,
                    &call,
                    EmbeddedRefusedReason::SelfReentrant,
                    Duration::ZERO,
                    None,
                )?;
                return Err(code_runtime_error(
                    "SelfReentrant: execute_code cannot call itself",
                ));
            }
            if !self.embedded_tool_whitelist.names.contains(&call.name) {
                emit_refused_embedded_step(
                    ctx,
                    &call,
                    EmbeddedRefusedReason::NotWhitelisted,
                    Duration::ZERO,
                    None,
                )?;
                return Err(code_runtime_error(format!(
                    "EmbeddedDenied: {} is not allowed",
                    call.name
                )));
            }
            let step = ctx.embedded_dispatcher.dispatch(call).await?;
            return Ok((LuaValue::String(step.output_json.clone()), vec![step]));
        }
        evaluate_literal_or_arithmetic(expression).map(|value| (value, Vec::new()))
    }
}

fn emit_refused_embedded_step(
    ctx: &CodeSandboxRunContext,
    call: &EmbeddedToolCall,
    refused_reason: EmbeddedRefusedReason,
    duration: Duration,
    overflow: Option<OverflowMetadata>,
) -> Result<(), SandboxError> {
    let args_hash = blake3::hash(call.input_json.as_bytes());
    ctx.event_sink
        .emit(Event::ExecuteCodeStepInvoked(ExecuteCodeStepInvokedEvent {
            parent_tool_use_id: ctx.parent_tool_use_id,
            run_id: ctx.run_id,
            session_id: ctx.session_id,
            embedded_tool: call.name.clone(),
            args_hash: *args_hash.as_bytes(),
            step_seq: 1,
            duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
            overflow,
            refused_reason: Some(refused_reason),
            at: harness_contracts::now(),
        }))
}

fn return_expression(source: &str) -> Option<&str> {
    source
        .rsplit_once("return")
        .map(|(_, expression)| expression.trim())
}

fn reject_forbidden_libraries(source: &str) -> Result<(), SandboxError> {
    for forbidden in [
        "io.",
        "os.",
        "package.",
        "require",
        "debug.",
        "getmetatable",
        "setmetatable",
        "dofile",
        "loadfile",
    ] {
        if source.contains(forbidden) {
            return Err(code_runtime_error(format!(
                "ScriptError: forbidden symbol {forbidden}"
            )));
        }
    }
    Ok(())
}

fn enforce_memory_limit(source: &str, max_string_bytes: u64) -> Result<(), SandboxError> {
    let bytes = string_literal_bytes(source);
    if bytes > max_string_bytes {
        return Err(code_runtime_error(format!(
            "MemoryLimit: string bytes {bytes} exceed {max_string_bytes}"
        )));
    }
    Ok(())
}

fn enforce_table_limit(source: &str, max_table_entries: u64) -> Result<(), SandboxError> {
    let entries = table_literal_entries(source);
    if entries > max_table_entries {
        return Err(code_runtime_error(format!(
            "MemoryLimit: table entries {entries} exceed {max_table_entries}"
        )));
    }
    Ok(())
}

fn table_literal_entries(source: &str) -> u64 {
    let mut entries = 0_u64;
    let mut depth = 0_u32;
    let mut current_has_value = false;
    let mut in_string = None;
    let mut escaped = false;

    for ch in source.chars() {
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => in_string = Some(ch),
            '{' => {
                depth = depth.saturating_add(1);
                current_has_value = false;
            }
            '}' if depth > 0 => {
                if current_has_value {
                    entries += 1;
                }
                depth -= 1;
                current_has_value = depth > 0;
            }
            ',' if depth > 0 => {
                if current_has_value {
                    entries += 1;
                }
                current_has_value = false;
            }
            ch if depth > 0 && !ch.is_whitespace() => current_has_value = true,
            _ => {}
        }
    }

    entries
}

fn estimate_instructions(source: &str, max_instructions: u64) -> Result<u64, SandboxError> {
    let estimated = if source.contains("while true") || source.contains("while(true)") {
        max_instructions.saturating_add(1)
    } else {
        source
            .split(|ch: char| ch.is_whitespace() || matches!(ch, '(' | ')' | ',' | ';'))
            .filter(|part| !part.is_empty())
            .count()
            .max(1) as u64
    };
    if estimated > max_instructions {
        return Err(code_runtime_error(format!(
            "InstructionLimit: {estimated} exceed {max_instructions}"
        )));
    }
    Ok(estimated)
}

fn estimate_call_depth(source: &str, max_call_depth: u32) -> Result<u32, SandboxError> {
    let recursive = source.contains("function f()") && source.contains("return f()");
    let depth = if recursive {
        max_call_depth.saturating_add(1)
    } else if source.contains("function ") {
        1
    } else {
        0
    };
    if depth > max_call_depth {
        return Err(code_runtime_error(format!(
            "CallDepth: {depth} exceed {max_call_depth}"
        )));
    }
    Ok(depth)
}

fn parse_embedded_tool_call(expression: &str) -> Result<EmbeddedToolCall, SandboxError> {
    let inner = expression
        .strip_prefix("emb.tool(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| code_runtime_error("ScriptError: malformed emb.tool call"))?;
    let (name, rest) = parse_quoted(inner.trim())?;
    let rest = rest.trim_start();
    let rest = rest
        .strip_prefix(',')
        .ok_or_else(|| code_runtime_error("ScriptError: emb.tool missing input"))?
        .trim_start();
    let (input_json, trailing) = parse_quoted(rest)?;
    if !trailing.trim().is_empty() {
        return Err(code_runtime_error("ScriptError: trailing emb.tool input"));
    }
    Ok(EmbeddedToolCall { name, input_json })
}

fn evaluate_literal_or_arithmetic(expression: &str) -> Result<LuaValue, SandboxError> {
    let expression = expression.trim();
    if expression == "nil" {
        return Ok(LuaValue::Nil);
    }
    if expression == "true" {
        return Ok(LuaValue::Bool(true));
    }
    if expression == "false" {
        return Ok(LuaValue::Bool(false));
    }
    if let Ok((value, trailing)) = parse_quoted(expression) {
        if trailing.trim().is_empty() {
            return Ok(LuaValue::String(value));
        }
    }
    if expression.contains('+') {
        let mut sum = 0.0;
        for part in expression.split('+') {
            sum += part
                .trim()
                .parse::<f64>()
                .map_err(|error| code_runtime_error(format!("ScriptError: {error}")))?;
        }
        return Ok(LuaValue::Number(sum));
    }
    expression
        .parse::<f64>()
        .map(LuaValue::Number)
        .map_err(|error| code_runtime_error(format!("ScriptError: {error}")))
}

fn parse_quoted(input: &str) -> Result<(String, &str), SandboxError> {
    let mut chars = input.char_indices();
    let Some((_, quote @ ('"' | '\''))) = chars.next() else {
        return Err(code_runtime_error("ScriptError: expected quoted string"));
    };
    let mut escaped = false;
    let mut output = String::new();
    for (index, ch) in chars {
        if escaped {
            output.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\'' => '\'',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return Ok((output, &input[index + ch.len_utf8()..]));
        }
        output.push(ch);
    }
    Err(code_runtime_error("ScriptError: unterminated string"))
}

fn string_literal_bytes(source: &str) -> u64 {
    let mut bytes = 0_u64;
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        if !matches!(ch, '"' | '\'') {
            continue;
        }
        let quote = ch;
        let mut escaped = false;
        for ch in chars.by_ref() {
            if escaped {
                bytes += ch.len_utf8() as u64;
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == quote {
                break;
            }
            bytes += ch.len_utf8() as u64;
        }
    }
    bytes
}

fn code_runtime_error(details: impl Into<String>) -> SandboxError {
    SandboxError::CodeRuntime {
        detail: details.into(),
    }
}
