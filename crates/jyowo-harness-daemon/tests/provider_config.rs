use std::fs;

use harness_contracts::{
    ExecutionDefaultsRecord, ModelProtocol, ProviderProfileConversationCapability,
    ProviderProfileDefinition, ProviderProfileModelDescriptor, ProviderProfileModelLifecycle,
    ProviderRuntimeReasoningProtocolDescriptor, ProviderRuntimeSemanticsDescriptor,
    ProviderSecretEntry, ProviderSecretsRecord, ProviderSelectionRecord,
};
use harness_daemon::{ProviderConfigError, ProviderConfigResolver};
use tempfile::TempDir;

#[test]
fn missing_execution_defaults_use_the_disabled_contract_defaults() {
    let config = ConfigFixture::new();

    let defaults = ProviderConfigResolver::new(config.path())
        .resolve_execution_defaults()
        .expect("missing execution defaults should use contract defaults");

    assert_eq!(defaults, ExecutionDefaultsRecord::default());
    assert!(!defaults.subagents_enabled);
}

#[test]
fn execution_defaults_are_read_as_one_immutable_record() {
    let config = ConfigFixture::new();
    let expected = ExecutionDefaultsRecord {
        subagents_enabled: true,
        ..ExecutionDefaultsRecord::default()
    };
    config.write_json("execution-defaults.json", &expected);

    let defaults = ProviderConfigResolver::new(config.path())
        .resolve_execution_defaults()
        .expect("read execution defaults");

    assert_eq!(defaults, expected);
}

#[test]
fn malformed_execution_defaults_fail_closed() {
    let config = ConfigFixture::new();
    fs::write(
        config.path().join("execution-defaults.json"),
        br#"{"subagentsEnabled":"yes"}"#,
    )
    .unwrap();

    let error = ProviderConfigResolver::new(config.path())
        .resolve_execution_defaults()
        .expect_err("malformed execution defaults must not enable subagents");

    assert!(matches!(
        error,
        ProviderConfigError::Decode {
            kind: "execution defaults",
            ..
        }
    ));
}

#[test]
fn unspecified_config_uses_only_the_global_default() {
    let config = ConfigFixture::new();
    config.write_profiles(&[
        profile("requested", "anthropic", "claude-sonnet-4-20250514"),
        profile("default", "local-llama", "llama3.2"),
    ]);
    config.write_secrets(&[
        secret("requested", "requested-secret"),
        secret("default", "default-secret"),
    ]);
    config.write_selection(Some("default"));

    let resolved = ProviderConfigResolver::new(config.path())
        .resolve(None)
        .expect("resolve global default");

    assert_eq!(resolved.config_id, "default");
    assert_eq!(resolved.provider.provider_id(), "local-llama");
    assert_eq!(resolved.model_id, "llama3.2");
}

#[test]
fn explicit_config_is_selected_exactly_instead_of_using_the_default() {
    let config = ConfigFixture::new();
    config.write_profiles(&[
        profile("requested", "anthropic", "claude-sonnet-4-20250514"),
        profile("default", "local-llama", "llama3.2"),
    ]);
    config.write_secrets(&[
        secret("requested", "requested-secret"),
        secret("default", "default-secret"),
    ]);
    config.write_selection(Some("default"));

    let resolved = ProviderConfigResolver::new(config.path())
        .resolve(Some("requested"))
        .expect("resolve explicit config");

    assert_eq!(resolved.config_id, "requested");
    assert_eq!(resolved.provider.provider_id(), "anthropic");
    assert_eq!(resolved.model_id, "claude-sonnet-4-20250514");
}

#[test]
fn legacy_secret_array_written_by_the_current_desktop_store_is_supported() {
    let config = ConfigFixture::new();
    config.write_profiles(&[profile("selected", "anthropic", "claude-sonnet-4-20250514")]);
    config.write_json(
        "provider-secrets.json",
        &[secret("selected", "selected-secret")],
    );
    config.write_selection(Some("selected"));

    let resolved = ProviderConfigResolver::new(config.path())
        .resolve(None)
        .expect("resolve legacy desktop secret format");

    assert_eq!(resolved.config_id, "selected");
    assert_eq!(resolved.provider.provider_id(), "anthropic");
}

#[test]
fn missing_profile_is_diagnostic_without_exposing_other_secrets() {
    let config = ConfigFixture::new();
    config.write_profiles(&[profile(
        "configured",
        "anthropic",
        "claude-sonnet-4-20250514",
    )]);
    config.write_secrets(&[secret("configured", "do-not-leak-this-token")]);
    config.write_selection(Some("configured"));

    let error = ProviderConfigResolver::new(config.path())
        .resolve(Some("missing"))
        .expect_err("missing profile must fail");
    let message = error.to_string();

    assert!(matches!(
        error,
        ProviderConfigError::ProfileNotFound { ref config_id } if config_id == "missing"
    ));
    assert!(message.contains("missing"));
    assert!(!message.contains("do-not-leak-this-token"));
}

#[test]
fn missing_secret_is_diagnostic_and_redacted() {
    let config = ConfigFixture::new();
    config.write_profiles(&[
        profile("selected", "anthropic", "claude-sonnet-4-20250514"),
        profile("other", "local-llama", "llama3.2"),
    ]);
    config.write_secrets(&[secret("other", "do-not-leak-this-token")]);
    config.write_selection(Some("selected"));

    let error = ProviderConfigResolver::new(config.path())
        .resolve(None)
        .expect_err("missing secret must fail");
    let message = error.to_string();

    assert!(matches!(
        error,
        ProviderConfigError::SecretNotFound { ref config_id } if config_id == "selected"
    ));
    assert!(message.contains("selected"));
    assert!(!message.contains("do-not-leak-this-token"));
}

#[test]
fn empty_api_key_is_rejected_without_echoing_it() {
    let config = ConfigFixture::new();
    config.write_profiles(&[profile("selected", "anthropic", "claude-sonnet-4-20250514")]);
    config.write_secrets(&[secret("selected", "   ")]);
    config.write_selection(Some("selected"));

    let error = ProviderConfigResolver::new(config.path())
        .resolve(None)
        .expect_err("empty api key must fail");

    assert!(matches!(
        error,
        ProviderConfigError::EmptyApiKey { ref config_id } if config_id == "selected"
    ));
    assert!(!error.to_string().contains("   "));
}

#[test]
fn absent_default_never_falls_back_to_local_llama() {
    let config = ConfigFixture::new();
    config.write_profiles(&[profile("local", "local-llama", "llama3.2")]);
    config.write_secrets(&[secret("local", "local-secret")]);
    config.write_selection(None);

    let error = ProviderConfigResolver::new(config.path())
        .resolve(None)
        .expect_err("missing default must not use LocalLlama");

    assert!(matches!(error, ProviderConfigError::DefaultConfigNotSet));
}

#[test]
fn unsupported_provider_is_rejected_by_the_model_registry_without_secret_leakage() {
    let config = ConfigFixture::new();
    config.write_profiles(&[profile("selected", "not-a-provider", "model")]);
    config.write_secrets(&[secret("selected", "do-not-leak-this-token")]);
    config.write_selection(Some("selected"));

    let error = ProviderConfigResolver::new(config.path())
        .resolve(None)
        .expect_err("unsupported provider must fail");
    let message = error.to_string();

    assert!(matches!(
        error,
        ProviderConfigError::ProviderBuild { ref config_id, .. } if config_id == "selected"
    ));
    assert!(message.contains("selected"));
    assert!(!message.contains("do-not-leak-this-token"));
}

#[test]
fn duplicate_profile_ids_are_rejected() {
    let config = ConfigFixture::new();
    config.write_profiles(&[
        profile("selected", "anthropic", "claude-sonnet-4-20250514"),
        profile("selected", "anthropic", "claude-haiku-4-20250514"),
    ]);
    config.write_secrets(&[secret("selected", "secret")]);
    config.write_selection(Some("selected"));

    let error = ProviderConfigResolver::new(config.path())
        .resolve(None)
        .expect_err("duplicate profile ids must fail");

    assert!(matches!(
        error,
        ProviderConfigError::DuplicateProfileId { ref config_id } if config_id == "selected"
    ));
}

#[test]
fn duplicate_secret_ids_are_rejected() {
    let config = ConfigFixture::new();
    config.write_profiles(&[profile("selected", "anthropic", "claude-sonnet-4-20250514")]);
    config.write_secrets(&[
        secret("selected", "first-secret"),
        secret("selected", "second-secret"),
    ]);
    config.write_selection(Some("selected"));

    let error = ProviderConfigResolver::new(config.path())
        .resolve(None)
        .expect_err("duplicate secret ids must fail");

    assert!(matches!(
        error,
        ProviderConfigError::DuplicateSecretId { ref config_id } if config_id == "selected"
    ));
    assert!(!error.to_string().contains("first-secret"));
    assert!(!error.to_string().contains("second-secret"));
}

#[test]
fn zero_context_or_output_limits_are_rejected() {
    for field in ["context_window", "max_output_tokens"] {
        let config = ConfigFixture::new();
        let mut selected = profile("selected", "anthropic", "claude-sonnet-4-20250514");
        if field == "context_window" {
            selected.model_descriptor.context_window = 0;
            selected
                .model_descriptor
                .conversation_capability
                .context_window = 0;
        } else {
            selected.model_descriptor.max_output_tokens = 0;
            selected
                .model_descriptor
                .conversation_capability
                .max_output_tokens = 0;
        }
        config.write_profiles(&[selected]);
        config.write_secrets(&[secret("selected", "secret")]);
        config.write_selection(Some("selected"));

        let error = ProviderConfigResolver::new(config.path())
            .resolve(None)
            .expect_err("zero token limits must fail");
        assert!(matches!(error, ProviderConfigError::InvalidProfile { .. }));
    }
}

#[test]
fn modalities_must_be_nonempty_and_unique() {
    for modalities in [vec![], vec!["text".to_owned(), "text".to_owned()]] {
        let config = ConfigFixture::new();
        let mut selected = profile("selected", "anthropic", "claude-sonnet-4-20250514");
        selected
            .model_descriptor
            .conversation_capability
            .input_modalities = modalities;
        config.write_profiles(&[selected]);
        config.write_secrets(&[secret("selected", "secret")]);
        config.write_selection(Some("selected"));

        let error = ProviderConfigResolver::new(config.path())
            .resolve(None)
            .expect_err("invalid modalities must fail");
        assert!(matches!(error, ProviderConfigError::InvalidProfile { .. }));
    }
}

#[test]
fn descriptor_limits_must_match_nested_capability_limits() {
    for field in ["context_window", "max_output_tokens"] {
        let config = ConfigFixture::new();
        let mut selected = profile("selected", "anthropic", "claude-sonnet-4-20250514");
        if field == "context_window" {
            selected
                .model_descriptor
                .conversation_capability
                .context_window += 1;
        } else {
            selected
                .model_descriptor
                .conversation_capability
                .max_output_tokens += 1;
        }
        config.write_profiles(&[selected]);
        config.write_secrets(&[secret("selected", "secret")]);
        config.write_selection(Some("selected"));

        let error = ProviderConfigResolver::new(config.path())
            .resolve(None)
            .expect_err("inconsistent capability limits must fail");
        assert!(matches!(error, ProviderConfigError::InvalidProfile { .. }));
    }
}

#[test]
fn runtime_semantics_protocol_must_match_the_profile_protocol() {
    let mut selected = profile("selected", "local-llama", "llama3.2");
    let mut semantics = chat_runtime_semantics();
    semantics.protocol = ModelProtocol::Messages;
    selected.model_descriptor.runtime_semantics = Some(semantics);

    assert_invalid_profile(selected, "runtime semantics protocol");
}

#[test]
fn tool_calling_capability_must_match_runtime_tool_semantics() {
    let mut selected = profile("selected", "local-llama", "llama3.2");
    selected
        .model_descriptor
        .conversation_capability
        .tool_calling = false;
    selected.model_descriptor.runtime_semantics = Some(chat_runtime_semantics());

    assert_invalid_profile(selected, "tool calling semantics");
}

#[test]
fn streaming_capability_must_match_runtime_streaming_semantics() {
    let mut selected = profile("selected", "local-llama", "llama3.2");
    selected.model_descriptor.conversation_capability.streaming = false;
    selected.model_descriptor.runtime_semantics = Some(chat_runtime_semantics());

    assert_invalid_profile(selected, "streaming semantics");
}

#[test]
fn prompt_cache_capability_must_match_runtime_cache_semantics() {
    let mut selected = profile("selected", "local-llama", "llama3.2");
    let mut semantics = chat_runtime_semantics();
    semantics.cache_protocol = "openai_auto".to_owned();
    selected.model_descriptor.runtime_semantics = Some(semantics);

    assert_invalid_profile(selected, "cache semantics");
}

#[test]
fn reasoning_capability_must_match_runtime_reasoning_semantics() {
    let mut selected = profile("selected", "local-llama", "llama3.2");
    let mut semantics = chat_runtime_semantics();
    semantics.reasoning_protocol = ProviderRuntimeReasoningProtocolDescriptor::PublicThinking;
    selected.model_descriptor.runtime_semantics = Some(semantics);

    assert_invalid_profile(selected, "reasoning semantics");
}

#[test]
fn structured_output_capability_must_match_runtime_output_semantics() {
    let mut selected = profile("selected", "local-llama", "llama3.2");
    let mut semantics = chat_runtime_semantics();
    semantics.output_protocol = "structured_json".to_owned();
    selected.model_descriptor.runtime_semantics = Some(semantics);

    assert_invalid_profile(selected, "output semantics");
}

struct ConfigFixture {
    root: TempDir,
}

impl ConfigFixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("temp config root"),
        }
    }

    fn path(&self) -> &std::path::Path {
        self.root.path()
    }

    fn write_profiles(&self, profiles: &[ProviderProfileDefinition]) {
        self.write_json("provider-profiles.json", profiles);
    }

    fn write_secrets(&self, entries: &[ProviderSecretEntry]) {
        self.write_json(
            "provider-secrets.json",
            &ProviderSecretsRecord {
                entries: entries.to_vec(),
            },
        );
    }

    fn write_selection(&self, default_config_id: Option<&str>) {
        self.write_json(
            "provider-selection.json",
            &ProviderSelectionRecord {
                default_config_id: default_config_id.map(ToOwned::to_owned),
            },
        );
    }

    fn write_json(&self, file_name: &str, value: &(impl serde::Serialize + ?Sized)) {
        let bytes = serde_json::to_vec_pretty(value).expect("serialize fixture");
        fs::write(self.path().join(file_name), bytes).expect("write fixture");
    }
}

fn secret(config_id: &str, api_key: &str) -> ProviderSecretEntry {
    ProviderSecretEntry {
        config_id: config_id.to_owned(),
        api_key: api_key.to_owned(),
        official_quota_api_key: None,
    }
}

fn assert_invalid_profile(profile: ProviderProfileDefinition, expected_reason: &str) {
    let config = ConfigFixture::new();
    config.write_profiles(&[profile]);
    config.write_secrets(&[secret("selected", "secret")]);
    config.write_selection(Some("selected"));

    let error = ProviderConfigResolver::new(config.path())
        .resolve(None)
        .expect_err("inconsistent runtime semantics must fail");

    assert!(matches!(
        error,
        ProviderConfigError::InvalidProfile { ref reason, .. }
            if reason.contains(expected_reason)
    ));
}

fn chat_runtime_semantics() -> ProviderRuntimeSemanticsDescriptor {
    ProviderRuntimeSemanticsDescriptor {
        protocol: ModelProtocol::ChatCompletions,
        tool_protocol: "openai_chat_tools".to_owned(),
        reasoning_protocol: ProviderRuntimeReasoningProtocolDescriptor::None,
        streaming_protocol: "sse".to_owned(),
        cache_protocol: "none".to_owned(),
        media_protocol: "openai_content_parts".to_owned(),
        output_protocol: "text_and_tool_use".to_owned(),
        provider_continuation_dialect: Some("openai_chat.plain".to_owned()),
    }
}

fn profile(config_id: &str, provider_id: &str, model_id: &str) -> ProviderProfileDefinition {
    let protocol = if provider_id == "anthropic" {
        ModelProtocol::Messages
    } else {
        ModelProtocol::ChatCompletions
    };
    ProviderProfileDefinition {
        id: config_id.to_owned(),
        display_name: config_id.to_owned(),
        provider_id: provider_id.to_owned(),
        model_id: model_id.to_owned(),
        protocol,
        model_options: Default::default(),
        base_url: None,
        provider_defaults: None,
        model_descriptor: ProviderProfileModelDescriptor {
            protocol,
            context_window: 32_000,
            display_name: model_id.to_owned(),
            lifecycle: ProviderProfileModelLifecycle::Stable,
            max_output_tokens: 4_096,
            model_id: model_id.to_owned(),
            provider_id: provider_id.to_owned(),
            conversation_capability: ProviderProfileConversationCapability {
                input_modalities: vec!["text".to_owned()],
                output_modalities: vec!["text".to_owned()],
                context_window: 32_000,
                max_output_tokens: 4_096,
                streaming: true,
                tool_calling: true,
                reasoning: false,
                prompt_cache: false,
                structured_output: false,
            },
            runtime_semantics: None,
        },
    }
}
