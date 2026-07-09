use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use chrono::Utc;
use futures::{future::join_all, StreamExt};
use harness_contracts::{
    BlobMeta, BlobRetention, BlobStore, BudgetMetric, Event, OverflowAction, OverflowMetadata,
    ToolCapability, ToolError, ToolResult, ToolResultOffloadedEvent, ToolResultPart,
    ToolUseHeartbeatEvent, ToolUseId,
};
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::{AuthorizedToolInput, ToolContext, ToolEvent, ToolJournalAuthority, ToolPool};

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizedToolCall {
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub input: AuthorizedToolInput,
}

#[derive(Clone)]
pub struct OrchestratorContext {
    pub pool: ToolPool,
    pub tool_context: ToolContext,
    pub blob_store: Option<Arc<dyn BlobStore>>,
    pub event_emitter: Arc<dyn ToolEventEmitter>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolResultEnvelope {
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub result: Result<ToolResult, ToolError>,
    pub overflow: Option<OverflowMetadata>,
    pub duration: Duration,
    pub progress_emitted: u32,
}

pub trait ToolEventEmitter: Send + Sync + 'static {
    fn emit(&self, event: Event);
}

#[derive(Debug, Default)]
pub struct NoopToolEventEmitter;

impl ToolEventEmitter for NoopToolEventEmitter {
    fn emit(&self, _event: Event) {}
}

#[derive(Clone)]
pub struct ToolOrchestrator {
    concurrency_limit: usize,
}

impl Default for ToolOrchestrator {
    fn default() -> Self {
        Self::new(10)
    }
}

impl ToolOrchestrator {
    pub fn new(concurrency_limit: usize) -> Self {
        Self {
            concurrency_limit: concurrency_limit.max(1),
        }
    }

    pub async fn dispatch(
        &self,
        calls: Vec<AuthorizedToolCall>,
        ctx: OrchestratorContext,
    ) -> Vec<ToolResultEnvelope> {
        let mut results = Vec::with_capacity(calls.len());
        let mut index = 0;

        while index < calls.len() {
            if self.is_concurrency_safe(&ctx.pool, &calls[index]) {
                let start = index;
                while index < calls.len() && self.is_concurrency_safe(&ctx.pool, &calls[index]) {
                    index += 1;
                }
                results.extend(
                    self.dispatch_parallel(calls[start..index].to_vec(), ctx.clone())
                        .await,
                );
            } else {
                results.push(Self::dispatch_one(calls[index].clone(), ctx.clone()).await);
                index += 1;
            }
        }

        results
    }

    fn is_concurrency_safe(&self, pool: &ToolPool, call: &AuthorizedToolCall) -> bool {
        pool.get(&call.tool_name)
            .map(|tool| tool.descriptor().properties.is_concurrency_safe)
            .unwrap_or(true)
    }

    async fn dispatch_parallel(
        &self,
        calls: Vec<AuthorizedToolCall>,
        ctx: OrchestratorContext,
    ) -> Vec<ToolResultEnvelope> {
        let semaphore = Arc::new(Semaphore::new(self.concurrency_limit));
        join_all(calls.into_iter().map(|call| {
            let semaphore = Arc::clone(&semaphore);
            let ctx = ctx.clone();
            async move {
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .expect("tool dispatch semaphore closed");
                Self::dispatch_one(call, ctx).await
            }
        }))
        .await
    }

    async fn dispatch_one(
        call: AuthorizedToolCall,
        ctx: OrchestratorContext,
    ) -> ToolResultEnvelope {
        let started = Instant::now();
        let tool_use_id = call.tool_use_id;
        let tool_name = call.tool_name.clone();
        let mut progress_emitted = 0;

        let mut overflow = None;
        let result = async {
            if ctx.tool_context.interrupt.is_interrupted() {
                return Err(ToolError::Interrupted);
            }

            let tool = ctx.pool.get(&call.tool_name).ok_or_else(|| {
                ToolError::Internal(format!("tool not found: {}", call.tool_name))
            })?;

            let mut tool_ctx = ctx.tool_context.clone();
            tool_ctx.tool_use_id = call.tool_use_id;

            validate_input_schema(tool.input_schema(), call.input.raw_input())?;

            tool.validate(call.input.raw_input(), &tool_ctx)
                .await
                .map_err(|error| ToolError::Validation(error.to_string()))?;

            let long_running_policy = tool.descriptor().properties.long_running.clone();
            let journal_authority = ctx.pool.journal_authority(&call.tool_name);
            let execute_and_collect = async {
                let stream = tool
                    .execute_authorized(call.input.clone(), tool_ctx.clone())
                    .await?;
                let result = if let Some(policy) = long_running_policy.as_ref() {
                    collect_stream_with_heartbeat(
                        stream,
                        &mut progress_emitted,
                        policy.stall_threshold,
                        &ctx,
                        call.tool_use_id,
                        journal_authority,
                        &tool.descriptor().budget,
                        &mut overflow,
                    )
                    .await?
                } else {
                    collect_stream(
                        stream,
                        &mut progress_emitted,
                        &ctx,
                        call.tool_use_id,
                        journal_authority,
                        &tool.descriptor().budget,
                        &mut overflow,
                    )
                    .await?
                };
                Ok(result)
            };

            match long_running_policy.as_ref() {
                Some(policy) => {
                    match tokio::time::timeout(policy.hard_timeout, execute_and_collect).await {
                        Ok(result) => result,
                        Err(_elapsed) => {
                            tool_ctx.interrupt.interrupt();
                            Err(ToolError::Timeout)
                        }
                    }
                }
                None => execute_and_collect.await,
            }
        }
        .await;

        ToolResultEnvelope {
            tool_use_id,
            tool_name,
            result,
            overflow,
            duration: started.elapsed(),
            progress_emitted,
        }
    }
}

fn validate_input_schema(schema: &Value, input: &Value) -> Result<(), ToolError> {
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| ToolError::Validation(format!("input schema compile failed: {error}")))?;
    if validator.is_valid(input) {
        return Ok(());
    }
    let message = validator.iter_errors(input).next().map_or_else(
        || "input does not match tool input schema".to_owned(),
        |error| error.to_string(),
    );
    Err(ToolError::Validation(message))
}

async fn collect_stream(
    mut stream: crate::ToolStream,
    progress_emitted: &mut u32,
    ctx: &OrchestratorContext,
    tool_use_id: ToolUseId,
    journal_authority: ToolJournalAuthority,
    budget: &harness_contracts::ResultBudget,
    overflow: &mut Option<OverflowMetadata>,
) -> Result<ToolResult, ToolError> {
    let mut final_result = None;
    let mut text_partials = PartialTextBudget::default();

    while let Some(event) = stream.next().await {
        match event {
            ToolEvent::Progress(_) => {
                *progress_emitted += 1;
            }
            ToolEvent::Partial(part) => {
                let harness_contracts::MessagePart::Text(text) = part else {
                    return Err(ToolError::Message(
                        "non-text tool partials are not supported".to_owned(),
                    ));
                };
                if let Some(result) = apply_partial_budget(
                    &mut text_partials,
                    &text,
                    budget,
                    ctx,
                    tool_use_id,
                    overflow,
                )
                .await?
                {
                    return Ok(result);
                }
            }
            ToolEvent::Journal(event) => {
                ctx.event_emitter.emit(authorize_tool_journal_event(
                    event,
                    ctx,
                    tool_use_id,
                    journal_authority,
                )?);
            }
            ToolEvent::Final(result) => {
                final_result = Some(result);
                break;
            }
            ToolEvent::Error(error) => return Err(error),
        }
    }

    let result = final_result
        .ok_or_else(|| ToolError::Internal("tool stream ended without final result".to_owned()))?;
    let result = if text_partials.is_empty() {
        result
    } else {
        let text_partials = text_partials.into_text();
        match result {
            ToolResult::Text(text) => ToolResult::Text(format!("{text_partials}{text}")),
            ToolResult::Mixed(parts) => {
                let mut combined = Vec::with_capacity(parts.len() + 1);
                combined.push(ToolResultPart::Text {
                    text: text_partials,
                });
                combined.extend(parts);
                ToolResult::Mixed(combined)
            }
            other => {
                let mut parts = vec![ToolResultPart::Text {
                    text: text_partials,
                }];
                parts.extend(tool_result_to_parts(other));
                ToolResult::Mixed(parts)
            }
        }
    };
    apply_result_budget(result, budget, ctx, tool_use_id, overflow).await
}

async fn collect_stream_with_heartbeat(
    mut stream: crate::ToolStream,
    progress_emitted: &mut u32,
    stall_threshold: Duration,
    ctx: &OrchestratorContext,
    tool_use_id: ToolUseId,
    journal_authority: ToolJournalAuthority,
    budget: &harness_contracts::ResultBudget,
    overflow: &mut Option<OverflowMetadata>,
) -> Result<ToolResult, ToolError> {
    let mut final_result = None;
    let mut text_partials = PartialTextBudget::default();

    loop {
        match tokio::time::timeout(stall_threshold, stream.next()).await {
            Ok(Some(ToolEvent::Progress(_))) => {
                *progress_emitted += 1;
            }
            Ok(Some(ToolEvent::Partial(part))) => {
                let harness_contracts::MessagePart::Text(text) = part else {
                    return Err(ToolError::Message(
                        "non-text tool partials are not supported".to_owned(),
                    ));
                };
                if let Some(result) = apply_partial_budget(
                    &mut text_partials,
                    &text,
                    budget,
                    ctx,
                    tool_use_id,
                    overflow,
                )
                .await?
                {
                    return Ok(result);
                }
            }
            Ok(Some(ToolEvent::Journal(event))) => {
                ctx.event_emitter.emit(authorize_tool_journal_event(
                    event,
                    ctx,
                    tool_use_id,
                    journal_authority,
                )?);
            }
            Ok(Some(ToolEvent::Final(result))) => {
                final_result = Some(result);
                break;
            }
            Ok(Some(ToolEvent::Error(error))) => return Err(error),
            Ok(None) => break,
            Err(_elapsed) => {
                *progress_emitted += 1;
                ctx.event_emitter
                    .emit(Event::ToolUseHeartbeat(ToolUseHeartbeatEvent {
                        tool_use_id,
                        run_id: ctx.tool_context.run_id,
                        message: "still running".to_owned(),
                        fraction: None,
                        silent_for_ms: stall_threshold.as_millis().min(u128::from(u64::MAX)) as u64,
                        at: Utc::now(),
                    }));
            }
        }
    }

    let result = final_result
        .ok_or_else(|| ToolError::Internal("tool stream ended without final result".to_owned()))?;
    let result = if text_partials.is_empty() {
        result
    } else {
        let text_partials = text_partials.into_text();
        match result {
            ToolResult::Text(text) => ToolResult::Text(format!("{text_partials}{text}")),
            ToolResult::Mixed(parts) => {
                let mut combined = Vec::with_capacity(parts.len() + 1);
                combined.push(ToolResultPart::Text {
                    text: text_partials,
                });
                combined.extend(parts);
                ToolResult::Mixed(combined)
            }
            other => {
                let mut parts = vec![ToolResultPart::Text {
                    text: text_partials,
                }];
                parts.extend(tool_result_to_parts(other));
                ToolResult::Mixed(parts)
            }
        }
    };
    apply_result_budget(result, budget, ctx, tool_use_id, overflow).await
}

fn authorize_tool_journal_event(
    mut event: Event,
    ctx: &OrchestratorContext,
    tool_use_id: ToolUseId,
    journal_authority: ToolJournalAuthority,
) -> Result<Event, ToolError> {
    match &mut event {
        Event::AssistantClarificationRequested(requested) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Clarification)?;
            requested.run_id = ctx.tool_context.run_id;
        }
        Event::SandboxPreflightPassed(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
            event.run_id = ctx.tool_context.run_id;
            event.tool_use_id = Some(tool_use_id);
        }
        Event::SandboxPreflightFailed(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
            event.run_id = ctx.tool_context.run_id;
            event.tool_use_id = Some(tool_use_id);
        }
        Event::SandboxExecutionStarted(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
            event.run_id = ctx.tool_context.run_id;
            event.tool_use_id = Some(tool_use_id);
        }
        Event::SandboxExecutionCompleted(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
            event.run_id = ctx.tool_context.run_id;
            event.tool_use_id = Some(tool_use_id);
        }
        Event::SandboxActivityHeartbeat(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
            event.run_id = ctx.tool_context.run_id;
            event.tool_use_id = Some(tool_use_id);
        }
        Event::SandboxActivityTimeoutFired(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
            event.run_id = ctx.tool_context.run_id;
            event.tool_use_id = Some(tool_use_id);
        }
        Event::SandboxOutputSpilled(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
            event.run_id = ctx.tool_context.run_id;
            event.tool_use_id = Some(tool_use_id);
        }
        Event::SandboxBackpressureApplied(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
            event.run_id = ctx.tool_context.run_id;
            event.tool_use_id = Some(tool_use_id);
        }
        Event::SandboxSnapshotCreated(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
        }
        Event::SandboxContainerLifecycleTransition(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
        }
        Event::SandboxBackendFailed(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
            event.run_id = ctx.tool_context.run_id;
            event.tool_use_id = Some(tool_use_id);
        }
        Event::SandboxPostExecutionFailed(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::Sandbox)?;
            event.session_id = ctx.tool_context.session_id;
            event.run_id = ctx.tool_context.run_id;
            event.tool_use_id = Some(tool_use_id);
        }
        Event::ExecuteCodeStepInvoked(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::ExecuteCode)?;
            event.session_id = ctx.tool_context.session_id;
            event.run_id = ctx.tool_context.run_id;
            event.parent_tool_use_id = tool_use_id;
        }
        Event::ExecuteCodeWhitelistExtended(event) => {
            require_tool_journal_authority(journal_authority, ToolJournalAuthority::ExecuteCode)?;
            event.session_id = ctx.tool_context.session_id;
        }
        _ => {
            return Err(ToolError::PermissionDenied(
                "tool journal event type is not allowed".to_owned(),
            ));
        }
    }
    Ok(event)
}

fn require_tool_journal_authority(
    actual: ToolJournalAuthority,
    expected: ToolJournalAuthority,
) -> Result<(), ToolError> {
    if actual == expected {
        return Ok(());
    }

    Err(ToolError::PermissionDenied(
        "tool journal event producer is not allowed".to_owned(),
    ))
}

async fn apply_partial_budget(
    partials: &mut PartialTextBudget,
    text: &str,
    budget: &harness_contracts::ResultBudget,
    ctx: &OrchestratorContext,
    tool_use_id: ToolUseId,
    overflow: &mut Option<OverflowMetadata>,
) -> Result<Option<ToolResult>, ToolError> {
    let next_size = partials.measure_after(text, budget.metric);
    if next_size <= budget.limit {
        partials.push_full(text, next_size);
        return Ok(None);
    }

    match budget.on_overflow {
        OverflowAction::Reject => Err(ToolError::ResultTooLarge {
            original: next_size,
            limit: budget.limit,
            metric: budget.metric,
        }),
        OverflowAction::Truncate => {
            partials.push_truncated(text, budget);
            Ok(Some(ToolResult::Text(partials.text.clone())))
        }
        OverflowAction::Offload => {
            let received_text = partials.text_with(text);
            offload_text_with_original(
                &received_text,
                next_size,
                budget,
                ctx,
                tool_use_id,
                overflow,
            )
            .await
            .map(Some)
        }
        _ => Ok(None),
    }
}

async fn apply_result_budget(
    result: ToolResult,
    budget: &harness_contracts::ResultBudget,
    ctx: &OrchestratorContext,
    tool_use_id: ToolUseId,
    overflow: &mut Option<OverflowMetadata>,
) -> Result<ToolResult, ToolError> {
    let Some(text) = budgeted_text(&result) else {
        return Ok(result);
    };
    let original_size = measure(&text, budget.metric);
    if original_size <= budget.limit {
        return Ok(result);
    }

    match budget.on_overflow {
        OverflowAction::Truncate => Ok(ToolResult::Text(truncate_by_metric(
            &text,
            budget.metric,
            budget.limit,
        ))),
        OverflowAction::Offload => offload_text(&text, budget, ctx, tool_use_id, overflow).await,
        _ => Err(ToolError::ResultTooLarge {
            original: original_size,
            limit: budget.limit,
            metric: budget.metric,
        }),
    }
}

async fn offload_text(
    text: &str,
    budget: &harness_contracts::ResultBudget,
    ctx: &OrchestratorContext,
    tool_use_id: ToolUseId,
    overflow: &mut Option<OverflowMetadata>,
) -> Result<ToolResult, ToolError> {
    offload_text_with_original(
        text,
        measure(text, budget.metric),
        budget,
        ctx,
        tool_use_id,
        overflow,
    )
    .await
}

async fn offload_text_with_original(
    text: &str,
    original_size: u64,
    budget: &harness_contracts::ResultBudget,
    ctx: &OrchestratorContext,
    tool_use_id: ToolUseId,
    overflow: &mut Option<OverflowMetadata>,
) -> Result<ToolResult, ToolError> {
    let blob_store = ctx
        .blob_store
        .as_ref()
        .ok_or(ToolError::CapabilityMissing(ToolCapability::BlobReader))?;
    let bytes = Bytes::from(text.to_owned());
    let content_hash = *blake3::hash(&bytes).as_bytes();
    let meta = BlobMeta {
        content_type: Some("text/plain; charset=utf-8".to_owned()),
        size: bytes.len() as u64,
        content_hash,
        created_at: Utc::now(),
        retention: BlobRetention::SessionScoped(ctx.tool_context.session_id),
    };
    let blob_ref = blob_store
        .put(ctx.tool_context.tenant_id, bytes, meta)
        .await
        .map_err(|error| ToolError::OffloadFailed(error.to_string()))?;
    let head = take_chars(text, budget.preview_head_chars as usize);
    let tail = take_tail_chars(text, budget.preview_tail_chars as usize);
    let metadata = OverflowMetadata {
        blob_ref: blob_ref.clone(),
        head_chars: head.chars().count() as u32,
        tail_chars: tail.chars().count() as u32,
        original_size,
        original_metric: budget.metric,
        effective_limit: budget.limit,
    };
    ctx.event_emitter
        .emit(Event::ToolResultOffloaded(ToolResultOffloadedEvent {
            tool_use_id,
            run_id: ctx.tool_context.run_id,
            blob_ref: blob_ref.clone(),
            original_metric: budget.metric,
            original_size,
            effective_limit: budget.limit,
            head_chars: metadata.head_chars,
            tail_chars: metadata.tail_chars,
            at: Utc::now(),
        }));
    *overflow = Some(metadata);
    Ok(ToolResult::Mixed(vec![
        ToolResultPart::Text { text: head },
        ToolResultPart::Blob {
            content_type: "text/plain; charset=utf-8".to_owned(),
            blob_ref,
            summary: Some("tool result exceeded budget; content was offloaded".to_owned()),
        },
        ToolResultPart::Text { text: tail },
    ]))
}

#[derive(Default)]
struct PartialTextBudget {
    text: String,
    measured: u64,
}

impl PartialTextBudget {
    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn into_text(self) -> String {
        self.text
    }

    fn measure_after(&self, text: &str, metric: BudgetMetric) -> u64 {
        measure_after_append(&self.text, text, metric)
    }

    fn push_full(&mut self, text: &str, measured: u64) {
        self.text.push_str(text);
        self.measured = measured;
    }

    fn push_truncated(&mut self, text: &str, budget: &harness_contracts::ResultBudget) {
        let remaining = budget.limit.saturating_sub(self.measured);
        if remaining > 0 {
            self.text
                .push_str(&truncate_by_metric(text, budget.metric, remaining));
            self.measured = measure(&self.text, budget.metric);
        }
    }

    fn text_with(&self, text: &str) -> String {
        let mut received = String::with_capacity(self.text.len() + text.len());
        received.push_str(&self.text);
        received.push_str(text);
        received
    }
}

fn budgeted_text(result: &ToolResult) -> Option<String> {
    match result {
        ToolResult::Text(text) => Some(text.clone()),
        ToolResult::Structured(value) => serde_json::to_string(value).ok(),
        ToolResult::Mixed(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    ToolResultPart::Text { text: part_text } => text.push_str(part_text),
                    ToolResultPart::Structured { value, .. } => {
                        text.push_str(&serde_json::to_string(value).ok()?);
                    }
                    ToolResultPart::Code { text: code, .. } => text.push_str(code),
                    ToolResultPart::Artifact { .. } => {}
                    _ => {}
                }
            }
            Some(text)
        }
        _ => None,
    }
}

fn measure(text: &str, metric: BudgetMetric) -> u64 {
    match metric {
        BudgetMetric::Bytes => text.len() as u64,
        BudgetMetric::Lines => text.lines().count() as u64,
        _ => text.chars().count() as u64,
    }
}

fn measure_after_append(prefix: &str, suffix: &str, metric: BudgetMetric) -> u64 {
    match metric {
        BudgetMetric::Bytes => prefix.len() as u64 + suffix.len() as u64,
        BudgetMetric::Lines => {
            let mut combined = String::with_capacity(prefix.len() + suffix.len());
            combined.push_str(prefix);
            combined.push_str(suffix);
            combined.lines().count() as u64
        }
        _ => prefix.chars().count() as u64 + suffix.chars().count() as u64,
    }
}

fn take_chars(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn take_tail_chars(text: &str, count: usize) -> String {
    let mut chars = text.chars().rev().take(count).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}

fn truncate_by_metric(text: &str, metric: BudgetMetric, limit: u64) -> String {
    match metric {
        BudgetMetric::Bytes => take_bytes(text, limit as usize),
        BudgetMetric::Lines => text
            .lines()
            .take(limit as usize)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => take_chars(text, limit as usize),
    }
}

fn take_bytes(text: &str, count: usize) -> String {
    if text.len() <= count {
        return text.to_owned();
    }
    let mut end = count;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn tool_result_to_parts(result: ToolResult) -> Vec<ToolResultPart> {
    match result {
        ToolResult::Text(text) => vec![ToolResultPart::Text { text }],
        ToolResult::Structured(value) => vec![ToolResultPart::Structured {
            value,
            schema_ref: None,
        }],
        ToolResult::Blob {
            content_type,
            blob_ref,
        } => vec![ToolResultPart::Blob {
            content_type,
            blob_ref,
            summary: None,
        }],
        ToolResult::Mixed(parts) => parts,
        _ => vec![ToolResultPart::Text {
            text: String::new(),
        }],
    }
}
