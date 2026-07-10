use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use harness_context::{
    AutocompactProvider, AuxFailureBudget, CompactHint, CompactSummaryLimits, ContextBuffer,
    ContextEngine, ContextOutcome, ContextProvider, MicrocompactProvider,
};
use harness_contracts::{
    ContextError, ContextStageId, Event, Message, MessageId, MessagePart, MessageRole, ModelError,
    SessionId, TenantId,
};
#[cfg(feature = "recall-memory")]
use harness_contracts::{MemoryError, MemoryId};
#[cfg(feature = "recall-memory")]
use harness_memory::{
    MemoryLifecycle, MemoryListScope, MemoryManager, MemoryQuery, MemoryRecord, MemoryStore,
    MemorySummary,
};
use harness_model::{
    AuxModelProvider, AuxOptions, AuxTask, ConversationModelCapability, HealthStatus, InferContext,
    ModelDescriptor, ModelMetricsSink, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use parking_lot::Mutex as ParkingMutex;
use tokio::sync::Mutex;

#[tokio::test]
async fn microcompact_calls_aux_and_replaces_oldest_batch() {
    let aux = Arc::new(RecordingAuxProvider::with_responses(vec![Ok(
        "summary with enough detail to pass the lower bound".repeat(2),
    )]));
    let mut buffer = buffer_with_messages(&[
        "old one with enough text",
        "old two with enough text",
        "recent one",
        "recent two",
        "recent three",
    ]);
    let recent_ids = ids(&buffer.active.history[2..]);
    let engine = ContextEngine::builder()
        .with_provider(
            MicrocompactProvider::new(aux.clone())
                .with_batch_size(2)
                .with_limits(short_test_limits()),
        )
        .build()
        .unwrap();

    let outcome = engine
        .compact(
            &mut buffer,
            CompactHint {
                estimated_tokens: 1_000,
                target_tokens: Some(10),
            },
        )
        .await
        .unwrap();

    assert!(matches!(outcome, ContextOutcome::Modified { .. }));
    assert_eq!(aux.tasks().await, vec![AuxTask::Compact]);
    assert!(text_parts(&buffer).any(|text| text.contains("[MICROCOMPACT_SUMMARY]")));
    assert_eq!(ids(&buffer.active.history[1..]), recent_ids);
}

#[tokio::test]
async fn microcompact_preserves_completed_tool_pairs_as_one_boundary() {
    let aux = Arc::new(RecordingAuxProvider::with_responses(vec![Ok(
        "tool pair summary with enough detail".repeat(2),
    )]));
    let tool_use_id = harness_contracts::ToolUseId::new();
    let mut buffer = ContextBuffer::default();
    buffer.active.history = vec![
        tool_use_message(tool_use_id, "grep"),
        tool_result_message(tool_use_id, "tool result text"),
        text_message("recent one"),
        text_message("recent two"),
        text_message("recent three"),
    ];
    let engine = ContextEngine::builder()
        .with_provider(
            MicrocompactProvider::new(aux)
                .with_batch_size(1)
                .with_limits(short_test_limits()),
        )
        .build()
        .unwrap();

    engine
        .compact(
            &mut buffer,
            CompactHint {
                estimated_tokens: 1_000,
                target_tokens: Some(10),
            },
        )
        .await
        .unwrap();

    assert_eq!(buffer.active.history.len(), 4);
    assert!(buffer.active.tool_use_pairs.is_empty());
}

#[tokio::test]
async fn microcompact_degrades_without_aux_or_after_aux_error_cooldown() {
    let mut no_aux_buffer = buffer_with_messages(&["old", "older", "r1", "r2", "r3"]);
    let no_aux_engine = ContextEngine::builder()
        .with_provider(MicrocompactProvider::without_aux())
        .build()
        .unwrap();

    let no_aux = no_aux_engine
        .compact(
            &mut no_aux_buffer,
            CompactHint {
                estimated_tokens: 1_000,
                target_tokens: Some(10),
            },
        )
        .await
        .unwrap();

    assert_eq!(no_aux, ContextOutcome::NoChange);

    let aux = Arc::new(RecordingAuxProvider::with_responses(vec![Err(
        ModelError::ProviderUnavailable("down".to_owned()),
    )]));
    let mut error_buffer = buffer_with_messages(&["old", "older", "r1", "r2", "r3"]);
    let engine = ContextEngine::builder()
        .with_provider(
            MicrocompactProvider::new(aux.clone())
                .with_failure_budget(AuxFailureBudget {
                    failure_max_per_turn: 1,
                    cooldown_turns: 3,
                    failure_window: std::time::Duration::from_secs(60),
                })
                .with_limits(short_test_limits()),
        )
        .build()
        .unwrap();

    let first = engine
        .compact(
            &mut error_buffer,
            CompactHint {
                estimated_tokens: 1_000,
                target_tokens: Some(10),
            },
        )
        .await
        .unwrap();
    let second = engine
        .compact(
            &mut error_buffer,
            CompactHint {
                estimated_tokens: 1_000,
                target_tokens: Some(10),
            },
        )
        .await
        .unwrap();

    assert_eq!(first, ContextOutcome::NoChange);
    assert_eq!(second, ContextOutcome::NoChange);
    assert_eq!(aux.call_count().await, 1);
}

#[tokio::test]
async fn microcompact_uses_aux_timeout_and_fails_open() {
    let aux = Arc::new(
        RecordingAuxProvider::with_responses(vec![Ok("late summary".to_owned())]).with_options(
            AuxOptions {
                per_task_timeout: std::time::Duration::from_millis(10),
                fail_open: true,
                ..AuxOptions::default()
            },
            std::time::Duration::from_millis(100),
        ),
    );
    let mut buffer = buffer_with_messages(&["old", "older", "r1", "r2", "r3"]);
    let engine = ContextEngine::builder()
        .with_provider(MicrocompactProvider::new(aux.clone()).with_limits(short_test_limits()))
        .build()
        .unwrap();

    let outcome = engine
        .compact(
            &mut buffer,
            CompactHint {
                estimated_tokens: 1_000,
                target_tokens: Some(10),
            },
        )
        .await
        .unwrap();

    assert_eq!(outcome, ContextOutcome::NoChange);
    assert_eq!(aux.call_count().await, 1);
}

#[tokio::test]
async fn microcompact_rejects_too_short_summary_and_truncates_too_long_summary() {
    let short_aux = Arc::new(RecordingAuxProvider::with_responses(vec![Ok(
        "short".to_owned()
    )]));
    let mut short_buffer = buffer_with_messages(&["old", "older", "r1", "r2", "r3"]);
    let short_engine = ContextEngine::builder()
        .with_provider(
            MicrocompactProvider::new(short_aux).with_limits(CompactSummaryLimits {
                max_input_chars: 1_024,
                min_output_tokens: 4,
                max_output_tokens: 64,
            }),
        )
        .build()
        .unwrap();

    let short = short_engine
        .compact(
            &mut short_buffer,
            CompactHint {
                estimated_tokens: 1_000,
                target_tokens: Some(10),
            },
        )
        .await
        .unwrap();

    assert_eq!(short, ContextOutcome::NoChange);

    let long_aux = Arc::new(RecordingAuxProvider::with_responses(vec![Ok(
        "word ".repeat(80)
    )]));
    let mut long_buffer = buffer_with_messages(&["old", "older", "r1", "r2", "r3"]);
    let long_engine = ContextEngine::builder()
        .with_provider(
            MicrocompactProvider::new(long_aux).with_limits(CompactSummaryLimits {
                max_input_chars: 1_024,
                min_output_tokens: 1,
                max_output_tokens: 8,
            }),
        )
        .build()
        .unwrap();

    long_engine
        .compact(
            &mut long_buffer,
            CompactHint {
                estimated_tokens: 1_000,
                target_tokens: Some(10),
            },
        )
        .await
        .unwrap();

    let summary = text_parts(&long_buffer)
        .find(|text| text.contains("[MICROCOMPACT_SUMMARY]"))
        .unwrap();
    assert!(summary.contains("[truncated]"));
}

#[tokio::test]
async fn autocompact_forks_when_hard_budget_is_exceeded() {
    let aux = Arc::new(RecordingAuxProvider::with_responses(vec![Ok(
        "autocompact summary with enough detail".repeat(2),
    )]));
    let mut buffer = buffer_with_messages(&["old", "older", "r1", "r2", "r3"]);
    let engine = ContextEngine::builder()
        .with_provider(AutocompactProvider::new(Some(aux)).with_limits(short_test_limits()))
        .build()
        .unwrap();

    let outcome = engine
        .compact(
            &mut buffer,
            CompactHint {
                estimated_tokens: 960,
                target_tokens: Some(100),
            },
        )
        .await
        .unwrap();

    assert!(matches!(outcome, ContextOutcome::Forked { .. }));
    assert!(text_parts(&buffer).any(|text| text.contains("[AUTOCOMPACT_HANDOFF]")));
}

#[tokio::test]
async fn context_builder_injects_aux_compaction_providers() {
    let aux = Arc::new(RecordingAuxProvider::with_responses(vec![
        Ok("summary with enough detail to pass the default compact output lower bound ".repeat(16)),
        Ok("autocompact handoff with enough detail to pass the default lower bound ".repeat(16)),
    ]));
    let mut buffer = buffer_with_messages(&["old", "older", "r1", "r2", "r3"]);
    let engine = ContextEngine::builder()
        .with_aux_provider(aux.clone())
        .build()
        .unwrap();

    engine
        .compact(
            &mut buffer,
            CompactHint {
                estimated_tokens: 1_000,
                target_tokens: Some(10),
            },
        )
        .await
        .unwrap();

    assert!(aux.call_count().await >= 1);
}

#[tokio::test]
async fn context_builder_wires_aux_queue_wait_metrics_sink() {
    let aux = Arc::new(RecordingAuxProvider::with_responses(vec![
        Ok("summary with enough detail to pass the default compact output lower bound ".repeat(16)),
        Ok("autocompact handoff with enough detail to pass the default lower bound ".repeat(16)),
    ]));
    let metrics = Arc::new(RecordingModelMetricsSink::default());
    let mut buffer = buffer_with_messages(&["old", "older", "r1", "r2", "r3"]);
    let engine = ContextEngine::builder()
        .with_aux_provider(aux)
        .with_model_metrics_sink(metrics.clone())
        .build()
        .unwrap();

    engine
        .compact(
            &mut buffer,
            CompactHint {
                estimated_tokens: 1_000,
                target_tokens: Some(10),
            },
        )
        .await
        .unwrap();

    assert!(metrics
        .aux_waits
        .lock()
        .iter()
        .any(|(model, _)| model == "test-aux"));
}

#[tokio::test]
async fn default_pipeline_runs_all_stages() {
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let engine = ContextEngine::builder()
        .with_default_compaction(None)
        .build()
        .unwrap();

    let compacted = engine
        .emergency_compact_prompt(
            tenant_id,
            session_id,
            None,
            Vec::new(),
            vec![
                text_message(&"old tool result ".repeat(64)),
                text_message(&"middle context ".repeat(64)),
                text_message("recent one"),
                text_message("recent two"),
                text_message("recent three"),
            ],
            2_000,
            100,
        )
        .await
        .unwrap();

    let stages = compacted
        .prompt
        .events
        .iter()
        .filter_map(|event| match event {
            Event::ContextStageTransitioned(stage) => Some(stage.stage.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        stages,
        vec![
            ContextStageId::ToolResultBudget,
            ContextStageId::Snip,
            ContextStageId::Microcompact,
            ContextStageId::Collapse,
            ContextStageId::Autocompact,
        ]
    );
}

#[tokio::test]
async fn emergency_compact_prompt_emits_budget_and_stage_events() {
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let engine = ContextEngine::builder()
        .with_provider(EmergencyTestProvider)
        .build()
        .unwrap();

    let compacted = engine
        .emergency_compact_prompt(
            tenant_id,
            session_id,
            None,
            Vec::new(),
            vec![text_message("old text"), text_message("new text")],
            1_000,
            100,
        )
        .await
        .unwrap();

    assert!(matches!(
        compacted.outcome,
        ContextOutcome::Modified { bytes_saved: 8 }
    ));
    assert!(compacted
        .prompt
        .messages
        .iter()
        .any(|message| message_text(message).contains("[EMERGENCY_COMPACTED]")));
    assert!(compacted
        .prompt
        .events
        .iter()
        .any(|event| matches!(event, Event::ContextBudgetExceeded(_))));
    assert!(compacted.prompt.events.iter().any(|event| matches!(
        event,
        Event::ContextStageTransitioned(stage)
            if stage.stage == ContextStageId::Snip && stage.provider_id == "emergency-test"
    )));
}

#[cfg(feature = "recall-memory")]
#[tokio::test]
async fn emergency_compact_prompt_adds_pre_compress_memory_facts_to_compact_input() {
    let aux = Arc::new(RecordingAuxProvider::with_responses(vec![Ok(
        "summary with enough detail to pass the lower bound".repeat(2),
    )]));
    let manager = MemoryManager::new();
    manager
        .register_provider(Arc::new(PreCompressProvider {
            facts: "provider fact before compact".to_owned(),
        }))
        .unwrap();
    let engine = ContextEngine::builder()
        .with_memory_manager(Arc::new(manager))
        .with_provider(
            MicrocompactProvider::new(aux.clone())
                .with_batch_size(2)
                .with_limits(short_test_limits()),
        )
        .build()
        .unwrap();

    engine
        .emergency_compact_prompt(
            TenantId::SINGLE,
            SessionId::new(),
            None,
            Vec::new(),
            vec![
                text_message("old one with enough text"),
                text_message("old two with enough text"),
                text_message("recent one"),
                text_message("recent two"),
                text_message("recent three"),
            ],
            2_000,
            100,
        )
        .await
        .unwrap();

    let requests = aux.calls.lock().await;
    let compact_request = &requests
        .first()
        .expect("aux compact request should be made")
        .1;
    let request_text = compact_request
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(request_text.contains("provider fact before compact"));
}

#[test]
fn context_crate_keeps_compact_dependency_boundary() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();

    assert!(!manifest.contains("jyowo-harness-tool"));
    assert!(!manifest.contains("jyowo-harness-session"));
    assert!(!manifest.contains("jyowo-harness-engine"));
    assert!(!manifest.contains("jyowo-harness-hook"));
    assert!(!manifest.contains("jyowo-harness-observability"));
}

struct EmergencyTestProvider;

#[async_trait]
impl ContextProvider for EmergencyTestProvider {
    fn provider_id(&self) -> &'static str {
        "emergency-test"
    }

    fn stage(&self) -> ContextStageId {
        ContextStageId::Snip
    }

    async fn apply(
        &self,
        ctx: &mut ContextBuffer,
        _hint: &CompactHint,
    ) -> Result<ContextOutcome, ContextError> {
        if let Some(first) = ctx.active.history.first_mut() {
            first.parts = vec![MessagePart::Text("[EMERGENCY_COMPACTED]".to_owned())];
        }
        Ok(ContextOutcome::Modified { bytes_saved: 8 })
    }
}

struct RecordingAuxProvider {
    inner: Arc<TestModelProvider>,
    options: AuxOptions,
    delay: std::time::Duration,
    responses: Mutex<Vec<Result<String, ModelError>>>,
    calls: Mutex<Vec<(AuxTask, ModelRequest)>>,
}

#[derive(Default)]
struct RecordingModelMetricsSink {
    aux_waits: ParkingMutex<Vec<(String, std::time::Duration)>>,
}

impl ModelMetricsSink for RecordingModelMetricsSink {
    fn record_credential_pool_cooldown(&self, _model_id: &str) {}

    fn record_aux_queue_wait(&self, model_id: &str, duration: std::time::Duration) {
        self.aux_waits.lock().push((model_id.to_owned(), duration));
    }
}

#[cfg(feature = "recall-memory")]
struct PreCompressProvider {
    facts: String,
}

#[cfg(feature = "recall-memory")]
#[async_trait]
impl MemoryStore for PreCompressProvider {
    fn provider_id(&self) -> &'static str {
        "pre-compress"
    }

    async fn recall(&self, _query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(Vec::new())
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(&self, _scope: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

#[cfg(feature = "recall-memory")]
#[async_trait]
impl MemoryLifecycle for PreCompressProvider {
    async fn on_pre_compress(
        &self,
        _messages: &[harness_contracts::MessageView<'_>],
    ) -> Result<Option<String>, MemoryError> {
        Ok(Some(self.facts.clone()))
    }
}

#[cfg(feature = "recall-memory")]
impl harness_memory::MemoryProvider for PreCompressProvider {}

impl RecordingAuxProvider {
    fn with_responses(responses: Vec<Result<String, ModelError>>) -> Self {
        Self {
            inner: Arc::new(TestModelProvider),
            options: AuxOptions::default(),
            delay: std::time::Duration::ZERO,
            responses: Mutex::new(responses),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn with_options(mut self, options: AuxOptions, delay: std::time::Duration) -> Self {
        self.options = options;
        self.delay = delay;
        self
    }

    async fn tasks(&self) -> Vec<AuxTask> {
        self.calls
            .lock()
            .await
            .iter()
            .map(|(task, _)| *task)
            .collect()
    }

    async fn call_count(&self) -> usize {
        self.calls.lock().await.len()
    }
}

#[async_trait]
impl AuxModelProvider for RecordingAuxProvider {
    fn inner(&self) -> Arc<dyn ModelProvider> {
        self.inner.clone()
    }

    fn aux_options(&self) -> AuxOptions {
        self.options.clone()
    }

    async fn call_aux(&self, task: AuxTask, req: ModelRequest) -> Result<String, ModelError> {
        self.calls.lock().await.push((task, req));
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.responses.lock().await.remove(0)
    }
}

struct TestModelProvider;

#[async_trait]
impl ModelProvider for TestModelProvider {
    fn provider_id(&self) -> &'static str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            supported_parameters: Vec::new(),
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "test".to_owned(),
            model_id: "test-aux".to_owned(),
            display_name: "Test Aux".to_owned(),
            context_window: 8_192,
            max_output_tokens: 1_024,
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
                harness_model::ModelProtocol::Messages,
            ),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(stream::iter([ModelStreamEvent::MessageStop])))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

fn buffer_with_messages(texts: &[&str]) -> ContextBuffer {
    let mut buffer = ContextBuffer::default();
    buffer.active.history = texts.iter().map(|text| text_message(text)).collect();
    buffer
}

fn text_message(text: &str) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::User,
        parts: vec![MessagePart::Text(text.to_owned())],
        created_at: chrono::Utc::now(),
    }
}

fn message_text(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_use_message(tool_use_id: harness_contracts::ToolUseId, name: &str) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::Assistant,
        parts: vec![MessagePart::ToolUse {
            id: tool_use_id,
            name: name.to_owned(),
            input: serde_json::json!({}),
        }],
        created_at: chrono::Utc::now(),
    }
}

fn tool_result_message(tool_use_id: harness_contracts::ToolUseId, text: &str) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::Tool,
        parts: vec![MessagePart::ToolResult {
            tool_use_id,
            content: harness_contracts::ToolResult::Text(text.to_owned()),
        }],
        created_at: chrono::Utc::now(),
    }
}

fn ids(messages: &[Message]) -> Vec<MessageId> {
    messages.iter().map(|message| message.id).collect()
}

fn text_parts(buffer: &ContextBuffer) -> impl Iterator<Item = &str> {
    buffer.active.history.iter().flat_map(|message| {
        message.parts.iter().filter_map(|part| match part {
            MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
    })
}

fn short_test_limits() -> CompactSummaryLimits {
    CompactSummaryLimits {
        max_input_chars: 1_024,
        min_output_tokens: 4,
        max_output_tokens: 128,
    }
}
