use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::NaiveDate;
use harness_contracts::{
    ExecutionDefaultsRecord, ModelModality, ModelProtocol, ModelRequestOptions,
    ProviderProfileDefinition, ProviderProfileModelLifecycle,
    ProviderRuntimeReasoningProtocolDescriptor, ProviderSecretEntry, ProviderSecretsRecord,
    ProviderSelectionRecord,
};
use harness_model::{
    build_provider, provider_requires_api_key, CacheProtocolSemantics, ConversationModelCapability,
    MediaProtocolSemantics, ModelDescriptor, ModelLifecycle, ModelProvider, ModelRuntimeSemantics,
    OutputProtocolSemantics, ProviderBuildConfig, ProviderRegistryError, ProviderRequestDefaults,
    ReasoningProtocolSemantics, StreamingProtocolSemantics, ToolProtocolSemantics,
};
use harness_provider_state::ProviderContinuationKind;
use serde::de::DeserializeOwned;
use thiserror::Error;

const PROFILES_FILE: &str = "provider-profiles.json";
const SECRETS_FILE: &str = "provider-secrets.json";
const SELECTION_FILE: &str = "provider-selection.json";
const EXECUTION_DEFAULTS_FILE: &str = "execution-defaults.json";

/// Resolves the immutable provider configuration for one daemon run.
#[derive(Debug, Clone)]
pub struct ProviderConfigResolver {
    config_root: PathBuf,
}

impl ProviderConfigResolver {
    #[must_use]
    pub fn new(config_root: impl Into<PathBuf>) -> Self {
        Self {
            config_root: config_root.into(),
        }
    }

    /// Resolves an explicit config ID, or the global default when no ID was persisted.
    pub fn resolve(
        &self,
        model_config_id: Option<&str>,
    ) -> Result<ResolvedProviderConfig, ProviderConfigError> {
        let profiles = read_json::<Vec<ProviderProfileDefinition>>(
            &self.config_root.join(PROFILES_FILE),
            "provider profiles",
        )?;
        reject_duplicate_profile_ids(&profiles)?;
        let config_id = match model_config_id {
            Some(config_id) => config_id.to_owned(),
            None => read_json::<ProviderSelectionRecord>(
                &self.config_root.join(SELECTION_FILE),
                "provider selection",
            )?
            .default_config_id
            .filter(|config_id| !config_id.is_empty())
            .ok_or(ProviderConfigError::DefaultConfigNotSet)?,
        };
        let profile = profiles
            .into_iter()
            .find(|profile| profile.id == config_id)
            .ok_or_else(|| ProviderConfigError::ProfileNotFound {
                config_id: config_id.clone(),
            })?;
        let api_key = if provider_requires_api_key(&profile.provider_id) {
            let secrets = read_secrets(&self.config_root.join(SECRETS_FILE))?;
            reject_duplicate_secret_ids(&secrets.entries)?;
            let secret = secrets
                .entries
                .into_iter()
                .find(|secret| secret.config_id == config_id)
                .ok_or_else(|| ProviderConfigError::SecretNotFound {
                    config_id: config_id.clone(),
                })?;
            let api_key = secret.api_key.trim();
            if api_key.is_empty() {
                return Err(ProviderConfigError::EmptyApiKey { config_id });
            }
            api_key.to_owned()
        } else {
            String::new()
        };

        let descriptor = descriptor_from_profile(&profile)?;
        let provider_id = profile.provider_id.clone();
        let model_id = profile.model_id.clone();
        let protocol = profile.protocol;
        let model_options = profile.model_options.clone();
        let provider_defaults = profile
            .provider_defaults
            .map(|defaults| ProviderRequestDefaults {
                body: defaults
                    .body
                    .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new())),
                headers: defaults.headers,
            });
        let provider = build_provider(ProviderBuildConfig {
            provider_id,
            api_key,
            base_url: profile
                .base_url
                .filter(|base_url| !base_url.trim().is_empty()),
            model_descriptor: Some(descriptor),
            provider_defaults,
        })
        .map_err(|source| ProviderConfigError::ProviderBuild {
            config_id: config_id.clone(),
            source,
        })?;

        Ok(ResolvedProviderConfig {
            config_id,
            provider: Arc::from(provider),
            model_id,
            protocol,
            model_options,
        })
    }

    pub fn resolve_execution_defaults(
        &self,
    ) -> Result<ExecutionDefaultsRecord, ProviderConfigError> {
        let path = self.config_root.join(EXECUTION_DEFAULTS_FILE);
        match fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|source| ProviderConfigError::Decode {
                    kind: "execution defaults",
                    path,
                    source,
                })
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(ExecutionDefaultsRecord::default())
            }
            Err(source) => Err(ProviderConfigError::Read {
                kind: "execution defaults",
                path,
                source,
            }),
        }
    }
}

/// Provider and model metadata selected for one immutable run input.
#[derive(Clone)]
pub struct ResolvedProviderConfig {
    pub config_id: String,
    pub provider: Arc<dyn ModelProvider>,
    pub model_id: String,
    pub protocol: ModelProtocol,
    pub model_options: ModelRequestOptions,
}

impl fmt::Debug for ResolvedProviderConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedProviderConfig")
            .field("config_id", &self.config_id)
            .field("provider_id", &self.provider.provider_id())
            .field("model_id", &self.model_id)
            .field("protocol", &self.protocol)
            .field("model_options", &self.model_options)
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum ProviderConfigError {
    #[error("failed to read {kind} from {path}")]
    Read {
        kind: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {kind} from {path}")]
    Decode {
        kind: &'static str,
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("global default provider config is not set")]
    DefaultConfigNotSet,
    #[error("provider profile was not found for config {config_id}")]
    ProfileNotFound { config_id: String },
    #[error("provider profile id is duplicated for config {config_id}")]
    DuplicateProfileId { config_id: String },
    #[error("provider secret was not found for config {config_id}")]
    SecretNotFound { config_id: String },
    #[error("provider secret id is duplicated for config {config_id}")]
    DuplicateSecretId { config_id: String },
    #[error("provider api key is empty for config {config_id}")]
    EmptyApiKey { config_id: String },
    #[error("provider profile is invalid for config {config_id}: {reason}")]
    InvalidProfile { config_id: String, reason: String },
    #[error("failed to build provider for config {config_id}")]
    ProviderBuild {
        config_id: String,
        #[source]
        source: ProviderRegistryError,
    },
}

fn read_json<T: DeserializeOwned>(
    path: &Path,
    kind: &'static str,
) -> Result<T, ProviderConfigError> {
    let bytes = fs::read(path).map_err(|source| ProviderConfigError::Read {
        kind,
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| ProviderConfigError::Decode {
        kind,
        path: path.to_owned(),
        source,
    })
}

fn read_secrets(path: &Path) -> Result<ProviderSecretsRecord, ProviderConfigError> {
    let secrets = read_json::<ProviderSecretsFile>(path, "provider secrets")?;
    Ok(match secrets {
        ProviderSecretsFile::Record(record) => record,
        ProviderSecretsFile::LegacyEntries(entries) => ProviderSecretsRecord { entries },
    })
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum ProviderSecretsFile {
    Record(ProviderSecretsRecord),
    LegacyEntries(Vec<ProviderSecretEntry>),
}

fn reject_duplicate_profile_ids(
    profiles: &[ProviderProfileDefinition],
) -> Result<(), ProviderConfigError> {
    let mut ids = HashSet::new();
    for profile in profiles {
        if !ids.insert(profile.id.as_str()) {
            return Err(ProviderConfigError::DuplicateProfileId {
                config_id: profile.id.clone(),
            });
        }
    }
    Ok(())
}

fn reject_duplicate_secret_ids(secrets: &[ProviderSecretEntry]) -> Result<(), ProviderConfigError> {
    let mut ids = HashSet::new();
    for secret in secrets {
        if !ids.insert(secret.config_id.as_str()) {
            return Err(ProviderConfigError::DuplicateSecretId {
                config_id: secret.config_id.clone(),
            });
        }
    }
    Ok(())
}

fn descriptor_from_profile(
    profile: &ProviderProfileDefinition,
) -> Result<ModelDescriptor, ProviderConfigError> {
    let descriptor = &profile.model_descriptor;
    if descriptor.provider_id != profile.provider_id
        || descriptor.model_id != profile.model_id
        || descriptor.protocol != profile.protocol
    {
        return Err(invalid_profile(
            profile,
            "embedded model descriptor does not match the provider profile",
        ));
    }
    if descriptor.context_window == 0 || descriptor.max_output_tokens == 0 {
        return Err(invalid_profile(
            profile,
            "context and output token limits must be greater than zero",
        ));
    }
    if descriptor.context_window != descriptor.conversation_capability.context_window
        || descriptor.max_output_tokens != descriptor.conversation_capability.max_output_tokens
    {
        return Err(invalid_profile(
            profile,
            "model descriptor limits do not match conversation capability limits",
        ));
    }
    validate_modalities(
        profile,
        "input",
        &descriptor.conversation_capability.input_modalities,
    )?;
    validate_modalities(
        profile,
        "output",
        &descriptor.conversation_capability.output_modalities,
    )?;
    let capability = ConversationModelCapability {
        input_modalities: descriptor
            .conversation_capability
            .input_modalities
            .iter()
            .map(|value| model_modality(profile, value))
            .collect::<Result<Vec<_>, _>>()?,
        output_modalities: descriptor
            .conversation_capability
            .output_modalities
            .iter()
            .map(|value| model_modality(profile, value))
            .collect::<Result<Vec<_>, _>>()?,
        context_window: descriptor.conversation_capability.context_window,
        max_output_tokens: descriptor.conversation_capability.max_output_tokens,
        streaming: descriptor.conversation_capability.streaming,
        tool_calling: descriptor.conversation_capability.tool_calling,
        reasoning: descriptor.conversation_capability.reasoning,
        prompt_cache: descriptor.conversation_capability.prompt_cache,
        structured_output: descriptor.conversation_capability.structured_output,
    };
    let lifecycle = match &descriptor.lifecycle {
        ProviderProfileModelLifecycle::Stable => ModelLifecycle::Stable,
        ProviderProfileModelLifecycle::Preview => ModelLifecycle::Preview,
        ProviderProfileModelLifecycle::Retiring { retirement_date } => ModelLifecycle::Retiring {
            retirement_date: NaiveDate::parse_from_str(retirement_date, "%Y-%m-%d")
                .map_err(|_| invalid_profile(profile, "retirement date must use YYYY-MM-DD"))?,
        },
    };
    let runtime_semantics = match &descriptor.runtime_semantics {
        Some(semantics) => {
            let runtime_semantics = ModelRuntimeSemantics {
                protocol: semantics.protocol,
                tool_protocol: tool_protocol(profile, &semantics.tool_protocol)?,
                reasoning_protocol: reasoning_protocol(profile, &semantics.reasoning_protocol)?,
                streaming_protocol: streaming_protocol(profile, &semantics.streaming_protocol)?,
                cache_protocol: cache_protocol(profile, &semantics.cache_protocol)?,
                media_protocol: media_protocol(profile, &semantics.media_protocol)?,
                output_protocol: output_protocol(profile, &semantics.output_protocol)?,
                provider_continuation_dialect: semantics.provider_continuation_dialect.clone(),
            };
            validate_runtime_semantics(profile, &capability, &runtime_semantics)?;
            runtime_semantics
        }
        None => ModelRuntimeSemantics::messages_default(profile.protocol),
    };

    Ok(ModelDescriptor {
        provider_id: descriptor.provider_id.clone(),
        model_id: descriptor.model_id.clone(),
        display_name: descriptor.display_name.clone(),
        protocol: descriptor.protocol,
        supported_parameters: Vec::new(),
        context_window: descriptor.context_window,
        max_output_tokens: descriptor.max_output_tokens,
        provider_declared_capability: capability.clone(),
        conversation_capability: capability,
        runtime_semantics,
        lifecycle,
        pricing: None,
    })
}

fn validate_runtime_semantics(
    profile: &ProviderProfileDefinition,
    capability: &ConversationModelCapability,
    semantics: &ModelRuntimeSemantics,
) -> Result<(), ProviderConfigError> {
    if semantics.protocol != profile.protocol {
        return Err(invalid_profile(
            profile,
            "runtime semantics protocol does not match the provider profile",
        ));
    }
    let has_tool_protocol = !matches!(semantics.tool_protocol, ToolProtocolSemantics::None);
    if capability.tool_calling != has_tool_protocol {
        return Err(invalid_profile(
            profile,
            "tool calling semantics do not match conversation capability",
        ));
    }
    let has_streaming_protocol = !matches!(
        semantics.streaming_protocol,
        StreamingProtocolSemantics::None
    );
    if capability.streaming != has_streaming_protocol {
        return Err(invalid_profile(
            profile,
            "streaming semantics do not match conversation capability",
        ));
    }
    let has_cache_protocol = !matches!(semantics.cache_protocol, CacheProtocolSemantics::None);
    if capability.prompt_cache != has_cache_protocol {
        return Err(invalid_profile(
            profile,
            "cache semantics do not match conversation capability",
        ));
    }
    let has_reasoning_protocol = !matches!(
        semantics.reasoning_protocol,
        ReasoningProtocolSemantics::None
    );
    if capability.reasoning != has_reasoning_protocol {
        return Err(invalid_profile(
            profile,
            "reasoning semantics do not match conversation capability",
        ));
    }
    if matches!(
        semantics.output_protocol,
        OutputProtocolSemantics::StructuredJson
    ) && !capability.structured_output
    {
        return Err(invalid_profile(
            profile,
            "output semantics require structured output capability",
        ));
    }
    if matches!(
        semantics.output_protocol,
        OutputProtocolSemantics::TextAndToolUse
    ) && !capability.tool_calling
    {
        return Err(invalid_profile(
            profile,
            "output semantics require tool calling capability",
        ));
    }
    Ok(())
}

fn validate_modalities(
    profile: &ProviderProfileDefinition,
    direction: &str,
    modalities: &[String],
) -> Result<(), ProviderConfigError> {
    let unique = modalities.iter().collect::<HashSet<_>>();
    if modalities.is_empty() || unique.len() != modalities.len() {
        return Err(invalid_profile(
            profile,
            &format!("{direction} modalities must be nonempty and unique"),
        ));
    }
    Ok(())
}

fn model_modality(
    profile: &ProviderProfileDefinition,
    value: &str,
) -> Result<ModelModality, ProviderConfigError> {
    match value {
        "text" => Ok(ModelModality::Text),
        "image" => Ok(ModelModality::Image),
        "audio" => Ok(ModelModality::Audio),
        "video" => Ok(ModelModality::Video),
        "file" => Ok(ModelModality::File),
        "embedding" => Ok(ModelModality::Embedding),
        _ => Err(invalid_profile(profile, "unknown model modality")),
    }
}

fn tool_protocol(
    profile: &ProviderProfileDefinition,
    value: &str,
) -> Result<ToolProtocolSemantics, ProviderConfigError> {
    match value {
        "none" => Ok(ToolProtocolSemantics::None),
        "openai_chat_tools" => Ok(ToolProtocolSemantics::OpenAiChatTools),
        "openai_responses_tools" => Ok(ToolProtocolSemantics::OpenAiResponsesTools),
        "anthropic_tools" => Ok(ToolProtocolSemantics::AnthropicTools),
        "gemini_tools" => Ok(ToolProtocolSemantics::GeminiTools),
        "bedrock_converse_tools" => Ok(ToolProtocolSemantics::BedrockConverseTools),
        _ => Err(invalid_profile(profile, "unknown tool protocol")),
    }
}

fn reasoning_protocol(
    profile: &ProviderProfileDefinition,
    value: &ProviderRuntimeReasoningProtocolDescriptor,
) -> Result<ReasoningProtocolSemantics, ProviderConfigError> {
    match value {
        ProviderRuntimeReasoningProtocolDescriptor::None => Ok(ReasoningProtocolSemantics::None),
        ProviderRuntimeReasoningProtocolDescriptor::PublicThinking => {
            Ok(ReasoningProtocolSemantics::PublicThinking)
        }
        ProviderRuntimeReasoningProtocolDescriptor::PublicSummary => {
            Ok(ReasoningProtocolSemantics::PublicSummary)
        }
        ProviderRuntimeReasoningProtocolDescriptor::ProviderPrivateReplay {
            continuation_kind,
            required_for_assistant_tool_replay,
        } => Ok(ReasoningProtocolSemantics::ProviderPrivateReplay {
            continuation_kind: continuation_kind_from_profile(profile, continuation_kind)?,
            required_for_assistant_tool_replay: *required_for_assistant_tool_replay,
        }),
    }
}

fn continuation_kind_from_profile(
    profile: &ProviderProfileDefinition,
    value: &str,
) -> Result<ProviderContinuationKind, ProviderConfigError> {
    match value {
        "reasoning_replay" => Ok(ProviderContinuationKind::ReasoningReplay),
        "tool_replay" => Ok(ProviderContinuationKind::ToolReplay),
        "cache_replay" => Ok(ProviderContinuationKind::CacheReplay),
        _ => value
            .strip_prefix("provider_native:")
            .filter(|value| !value.is_empty())
            .map(|value| ProviderContinuationKind::ProviderNative(value.to_owned()))
            .ok_or_else(|| invalid_profile(profile, "unknown provider continuation kind")),
    }
}

fn streaming_protocol(
    profile: &ProviderProfileDefinition,
    value: &str,
) -> Result<StreamingProtocolSemantics, ProviderConfigError> {
    match value {
        "none" => Ok(StreamingProtocolSemantics::None),
        "sse" => Ok(StreamingProtocolSemantics::Sse),
        "json_lines" => Ok(StreamingProtocolSemantics::JsonLines),
        "provider_native" => Ok(StreamingProtocolSemantics::ProviderNative),
        _ => Err(invalid_profile(profile, "unknown streaming protocol")),
    }
}

fn cache_protocol(
    profile: &ProviderProfileDefinition,
    value: &str,
) -> Result<CacheProtocolSemantics, ProviderConfigError> {
    match value {
        "none" => Ok(CacheProtocolSemantics::None),
        "openai_auto" => Ok(CacheProtocolSemantics::OpenAiAuto),
        "anthropic_ephemeral" => Ok(CacheProtocolSemantics::AnthropicEphemeral),
        "gemini_context_cache" => Ok(CacheProtocolSemantics::GeminiContextCache),
        _ => Err(invalid_profile(profile, "unknown cache protocol")),
    }
}

fn media_protocol(
    profile: &ProviderProfileDefinition,
    value: &str,
) -> Result<MediaProtocolSemantics, ProviderConfigError> {
    match value {
        "text_only" => Ok(MediaProtocolSemantics::TextOnly),
        "openai_content_parts" => Ok(MediaProtocolSemantics::OpenAiContentParts),
        "provider_native" => Ok(MediaProtocolSemantics::ProviderNative),
        _ => Err(invalid_profile(profile, "unknown media protocol")),
    }
}

fn output_protocol(
    profile: &ProviderProfileDefinition,
    value: &str,
) -> Result<OutputProtocolSemantics, ProviderConfigError> {
    match value {
        "text" => Ok(OutputProtocolSemantics::Text),
        "text_and_tool_use" => Ok(OutputProtocolSemantics::TextAndToolUse),
        "structured_json" => Ok(OutputProtocolSemantics::StructuredJson),
        _ => Err(invalid_profile(profile, "unknown output protocol")),
    }
}

fn invalid_profile(profile: &ProviderProfileDefinition, reason: &str) -> ProviderConfigError {
    ProviderConfigError::InvalidProfile {
        config_id: profile.id.clone(),
        reason: reason.to_owned(),
    }
}
