use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::{ModelError, StopReason, ToolUseId, UsageSnapshot};
use harness_provider_state::ProviderContinuationKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ContentDelta, ContentType, ErrorClass, ErrorHints, HealthStatus, InferContext, ModelDescriptor,
    ModelProvider, ModelRequest, ModelStream, ModelStreamEvent, PromptCacheStyle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CassetteMode {
    Record,
    Replay,
    Passthrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderContinuationRecording {
    OmitPayload,
    RecordPayloadForLocalTests,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CassetteProviderOptions {
    pub continuation_recording: ProviderContinuationRecording,
}

impl Default for CassetteProviderOptions {
    fn default() -> Self {
        Self {
            continuation_recording: ProviderContinuationRecording::OmitPayload,
        }
    }
}

pub struct CassetteProvider {
    inner: Arc<dyn ModelProvider>,
    cassette: PathBuf,
    mode: CassetteMode,
    options: CassetteProviderOptions,
}

impl CassetteProvider {
    #[must_use]
    pub fn new(
        inner: Arc<dyn ModelProvider>,
        cassette: impl Into<PathBuf>,
        mode: CassetteMode,
    ) -> Self {
        Self {
            inner,
            cassette: cassette.into(),
            mode,
            options: CassetteProviderOptions::default(),
        }
    }

    #[must_use]
    pub fn new_with_options(
        inner: Arc<dyn ModelProvider>,
        cassette: impl Into<PathBuf>,
        mode: CassetteMode,
        options: CassetteProviderOptions,
    ) -> Self {
        Self {
            inner,
            cassette: cassette.into(),
            mode,
            options,
        }
    }

    async fn replay(&self, req: &ModelRequest) -> Result<ModelStream, ModelError> {
        let key = request_key(req);
        let cassette = read_cassette(&self.cassette).await?;
        let entry = cassette
            .entries
            .into_iter()
            .find(|entry| entry.request_key == key)
            .ok_or_else(|| ModelError::ProviderUnavailable("cassette miss".to_owned()))?;
        let events = entry
            .events
            .into_iter()
            .map(ModelStreamEvent::from)
            .collect::<Vec<_>>();
        Ok(Box::pin(stream::iter(events)))
    }

    async fn record(
        &self,
        req: ModelRequest,
        ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        let key = request_key(&req);
        let events = self.inner.infer(req, ctx).await?.collect::<Vec<_>>().await;
        let recorded = events
            .iter()
            .cloned()
            .map(|event| RecordedModelStreamEvent::from_event(event, self.options))
            .collect::<Vec<_>>();
        let mut cassette = read_cassette(&self.cassette).await.unwrap_or_default();
        cassette.entries.retain(|entry| entry.request_key != key);
        cassette.entries.push(CassetteEntry {
            request_key: key,
            events: recorded,
        });
        cassette.metadata =
            CassetteMetadata::from_entries_and_options(&cassette.entries, self.options);
        write_cassette(&self.cassette, &cassette).await?;
        Ok(Box::pin(stream::iter(events)))
    }

    fn validate_mode(&self) -> Result<(), ModelError> {
        if std::env::var_os("CI").is_some() && self.mode != CassetteMode::Replay {
            return Err(ModelError::InvalidRequest(
                "cassette record and passthrough modes are disabled in CI".to_owned(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl ModelProvider for CassetteProvider {
    fn provider_id(&self) -> &str {
        self.inner.provider_id()
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        self.inner.supported_models()
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.validate_mode()?;
        match self.mode {
            CassetteMode::Record => self.record(req, ctx).await,
            CassetteMode::Replay => self.replay(&req).await,
            CassetteMode::Passthrough => self.inner.infer(req, ctx).await,
        }
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        self.inner.prompt_cache_style()
    }
    async fn health(&self) -> HealthStatus {
        self.inner.health().await
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CassetteFile {
    #[serde(default)]
    metadata: CassetteMetadata,
    entries: Vec<CassetteEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct CassetteMetadata {
    #[serde(default)]
    contains_private_provider_state: bool,
    #[serde(default)]
    provider_continuation_payloads: ProviderContinuationPayloadPolicy,
}

impl CassetteMetadata {
    fn from_entries_and_options(
        entries: &[CassetteEntry],
        options: CassetteProviderOptions,
    ) -> Self {
        let contains_recorded_payload = entries.iter().any(CassetteEntry::contains_private_payload);
        if contains_recorded_payload
            || options.continuation_recording
                == ProviderContinuationRecording::RecordPayloadForLocalTests
        {
            Self {
                contains_private_provider_state: true,
                provider_continuation_payloads:
                    ProviderContinuationPayloadPolicy::RecordedLocalTestOnly,
            }
        } else {
            Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderContinuationPayloadPolicy {
    #[default]
    Omitted,
    RecordedLocalTestOnly,
}

#[derive(Debug, Serialize, Deserialize)]
struct CassetteEntry {
    request_key: String,
    events: Vec<RecordedModelStreamEvent>,
}

impl CassetteEntry {
    fn contains_private_payload(&self) -> bool {
        self.events.iter().any(|event| {
            matches!(
                event,
                RecordedModelStreamEvent::ProviderContinuationDelta {
                    payload: Some(_),
                    ..
                }
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RecordedModelStreamEvent {
    MessageStart {
        message_id: String,
        usage: UsageSnapshot,
    },
    ContentBlockStart {
        index: u32,
        content_type: RecordedContentType,
    },
    ContentBlockDelta {
        index: u32,
        delta: RecordedContentDelta,
    },
    ContentBlockStop {
        index: u32,
    },
    MessageDelta {
        stop_reason: Option<StopReason>,
        usage_delta: UsageSnapshot,
    },
    MessageStop,
    ProviderContinuationDelta {
        kind: ProviderContinuationKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
    StreamError {
        error: ModelError,
        class: RecordedErrorClass,
        provider_error_code: Option<String>,
        request_id: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RecordedContentType {
    Text,
    Thinking,
    ToolUse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RecordedContentDelta {
    Text {
        text: String,
    },
    Thinking {
        text: Option<String>,
        provider_native: Option<Value>,
        signature: Option<String>,
    },
    ReasoningSummary {
        text: String,
        provider_native: Option<Value>,
    },
    ToolUseStart {
        id: String,
        name: String,
    },
    ToolUseInputJson {
        json: String,
    },
    ToolUseComplete {
        id: ToolUseId,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RecordedErrorClass {
    Transient,
    RateLimited { retry_after_millis: Option<u64> },
    ContextOverflow,
    AuthExpired,
    Fatal,
}

impl RecordedModelStreamEvent {
    fn from_event(value: ModelStreamEvent, options: CassetteProviderOptions) -> Self {
        match value {
            ModelStreamEvent::MessageStart { message_id, usage } => {
                Self::MessageStart { message_id, usage }
            }
            ModelStreamEvent::ContentBlockStart {
                index,
                content_type,
            } => Self::ContentBlockStart {
                index,
                content_type: content_type.into(),
            },
            ModelStreamEvent::ContentBlockDelta { index, delta } => Self::ContentBlockDelta {
                index,
                delta: delta.into(),
            },
            ModelStreamEvent::ContentBlockStop { index } => Self::ContentBlockStop { index },
            ModelStreamEvent::MessageDelta {
                stop_reason,
                usage_delta,
            } => Self::MessageDelta {
                stop_reason,
                usage_delta,
            },
            ModelStreamEvent::MessageStop => Self::MessageStop,
            ModelStreamEvent::ProviderContinuationDelta { kind, payload } => {
                let payload = match options.continuation_recording {
                    ProviderContinuationRecording::OmitPayload => None,
                    ProviderContinuationRecording::RecordPayloadForLocalTests => Some(payload),
                };
                Self::ProviderContinuationDelta { kind, payload }
            }
            ModelStreamEvent::StreamError {
                error,
                class,
                hints,
            } => Self::StreamError {
                error,
                class: class.into(),
                provider_error_code: hints.provider_error_code,
                request_id: hints.request_id,
            },
        }
    }
}

impl From<RecordedModelStreamEvent> for ModelStreamEvent {
    fn from(value: RecordedModelStreamEvent) -> Self {
        match value {
            RecordedModelStreamEvent::MessageStart { message_id, usage } => {
                Self::MessageStart { message_id, usage }
            }
            RecordedModelStreamEvent::ContentBlockStart {
                index,
                content_type,
            } => Self::ContentBlockStart {
                index,
                content_type: content_type.into(),
            },
            RecordedModelStreamEvent::ContentBlockDelta { index, delta } => {
                Self::ContentBlockDelta {
                    index,
                    delta: delta.into(),
                }
            }
            RecordedModelStreamEvent::ContentBlockStop { index } => {
                Self::ContentBlockStop { index }
            }
            RecordedModelStreamEvent::MessageDelta {
                stop_reason,
                usage_delta,
            } => Self::MessageDelta {
                stop_reason,
                usage_delta,
            },
            RecordedModelStreamEvent::MessageStop => Self::MessageStop,
            RecordedModelStreamEvent::ProviderContinuationDelta { kind, payload } => {
                Self::ProviderContinuationDelta {
                    kind,
                    payload: payload.unwrap_or(Value::Null),
                }
            }
            RecordedModelStreamEvent::StreamError {
                error,
                class,
                provider_error_code,
                request_id,
            } => Self::StreamError {
                error,
                class: class.into(),
                hints: ErrorHints {
                    raw_headers: None,
                    provider_error_code,
                    request_id,
                },
            },
        }
    }
}

impl From<ContentType> for RecordedContentType {
    fn from(value: ContentType) -> Self {
        match value {
            ContentType::Text => Self::Text,
            ContentType::Thinking => Self::Thinking,
            ContentType::ToolUse => Self::ToolUse,
        }
    }
}

impl From<RecordedContentType> for ContentType {
    fn from(value: RecordedContentType) -> Self {
        match value {
            RecordedContentType::Text => Self::Text,
            RecordedContentType::Thinking => Self::Thinking,
            RecordedContentType::ToolUse => Self::ToolUse,
        }
    }
}

impl From<ContentDelta> for RecordedContentDelta {
    fn from(value: ContentDelta) -> Self {
        match value {
            ContentDelta::Text(text) => Self::Text { text },
            ContentDelta::Thinking(thinking) => Self::Thinking {
                text: thinking.text,
                provider_native: thinking.provider_native,
                signature: thinking.signature,
            },
            ContentDelta::ReasoningSummary(summary) => Self::ReasoningSummary {
                text: summary.text,
                provider_native: summary.provider_native,
            },
            ContentDelta::ToolUseStart { id, name } => Self::ToolUseStart { id, name },
            ContentDelta::ToolUseInputJson(json) => Self::ToolUseInputJson { json },
            ContentDelta::ToolUseComplete { id, name, input } => {
                Self::ToolUseComplete { id, name, input }
            }
        }
    }
}

impl From<RecordedContentDelta> for ContentDelta {
    fn from(value: RecordedContentDelta) -> Self {
        match value {
            RecordedContentDelta::Text { text } => Self::Text(text),
            RecordedContentDelta::Thinking {
                text,
                provider_native,
                signature,
            } => Self::Thinking(crate::ThinkingDelta {
                text,
                provider_native,
                signature,
            }),
            RecordedContentDelta::ReasoningSummary {
                text,
                provider_native,
            } => Self::ReasoningSummary(crate::ReasoningSummaryDelta {
                text,
                provider_native,
            }),
            RecordedContentDelta::ToolUseStart { id, name } => Self::ToolUseStart { id, name },
            RecordedContentDelta::ToolUseInputJson { json } => Self::ToolUseInputJson(json),
            RecordedContentDelta::ToolUseComplete { id, name, input } => {
                Self::ToolUseComplete { id, name, input }
            }
        }
    }
}

impl From<ErrorClass> for RecordedErrorClass {
    fn from(value: ErrorClass) -> Self {
        match value {
            ErrorClass::Transient => Self::Transient,
            ErrorClass::RateLimited { retry_after } => Self::RateLimited {
                retry_after_millis: retry_after.map(duration_millis),
            },
            ErrorClass::ContextOverflow => Self::ContextOverflow,
            ErrorClass::AuthExpired => Self::AuthExpired,
            ErrorClass::Fatal => Self::Fatal,
        }
    }
}

impl From<RecordedErrorClass> for ErrorClass {
    fn from(value: RecordedErrorClass) -> Self {
        match value {
            RecordedErrorClass::Transient => Self::Transient,
            RecordedErrorClass::RateLimited { retry_after_millis } => Self::RateLimited {
                retry_after: retry_after_millis.map(Duration::from_millis),
            },
            RecordedErrorClass::ContextOverflow => Self::ContextOverflow,
            RecordedErrorClass::AuthExpired => Self::AuthExpired,
            RecordedErrorClass::Fatal => Self::Fatal,
        }
    }
}

fn request_key(req: &ModelRequest) -> String {
    format!("{req:?}")
}

async fn read_cassette(path: &Path) -> Result<CassetteFile, ModelError> {
    let path = path.to_path_buf();
    let result = tokio::task::spawn_blocking(move || std::fs::read_to_string(path))
        .await
        .map_err(|error| ModelError::Io(error.to_string()))?;
    match result {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|error| ModelError::UnexpectedResponse(error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(CassetteFile::default()),
        Err(error) => Err(ModelError::Io(error.to_string())),
    }
}

async fn write_cassette(path: &Path, cassette: &CassetteFile) -> Result<(), ModelError> {
    let path = path.to_path_buf();
    let raw = serde_json::to_string_pretty(cassette)
        .map_err(|error| ModelError::UnexpectedResponse(error.to_string()))?;
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, raw)
    })
    .await
    .map_err(|error| ModelError::Io(error.to_string()))?
    .map_err(|error| ModelError::Io(error.to_string()))
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
