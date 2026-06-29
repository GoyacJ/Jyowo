#![cfg(feature = "testing")]

use jyowo_harness_sdk::{ConfigSource, HarnessOptions, LastKnownGoodConfig, OptionsParseMode};

#[test]
fn strict_options_reject_unknown_fields() {
    let input = r#"{
        "workspace_root": "/tmp/jyowo-options",
        "model_id": "test",
        "unknown": true
    }"#;

    let error = HarnessOptions::parse(input, OptionsParseMode::Strict).unwrap_err();

    assert!(error.to_string().contains("unknown"));
}

#[test]
fn strict_options_reject_duplicate_fields() {
    let input = r#"{
        "workspace_root": "/tmp/jyowo-options",
        "model_id": "first",
        "model_id": "second"
    }"#;

    let error = HarnessOptions::parse(input, OptionsParseMode::Strict).unwrap_err();

    assert!(error.to_string().contains("duplicate"));
}

#[test]
fn load_with_fallback_uses_last_known_good() {
    let root = unique_options_dir("lkg");
    std::fs::create_dir_all(&root).unwrap();
    let primary = root.join("primary.json");
    let lkg = root.join("lkg.json");
    std::fs::write(
        &primary,
        r#"{
            "workspace_root": "/tmp/jyowo-primary",
            "model_id": "broken",
            "unknown": true
        }"#,
    )
    .unwrap();
    std::fs::write(
        &lkg,
        r#"{
            "workspace_root": "/tmp/jyowo-lkg",
            "model_id": "stable"
        }"#,
    )
    .unwrap();

    let loaded = HarnessOptions::load_with_fallback(
        &primary,
        LastKnownGoodConfig::new(lkg),
        OptionsParseMode::Strict,
    )
    .unwrap();

    assert_eq!(loaded.options.model_id, "stable");
    assert!(matches!(loaded.source, ConfigSource::LastKnownGood { .. }));
    assert!(loaded.primary_error.is_some());
}

#[test]
fn plaintext_secret_warning_is_observable() {
    let input = r#"{
        "workspace_root": "/tmp/jyowo-options",
        "model_id": "test",
        "default_session_options": {
            "workspace_root": "/tmp/jyowo-options",
            "model_extra": {
                "api_key": "plain-secret"
            }
        }
    }"#;

    let loaded = HarnessOptions::parse(input, OptionsParseMode::Strict).unwrap();

    assert!(loaded.warnings.iter().any(|warning| warning
        .path
        .ends_with("default_session_options.model_extra.api_key")));
}

fn unique_options_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-options-{name}-{}-{}",
        std::process::id(),
        harness_contracts::SessionId::new()
    ))
}
