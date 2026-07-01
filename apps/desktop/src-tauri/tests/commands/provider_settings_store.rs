#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[test]
fn desktop_provider_settings_store_rejects_config_without_api_key() {
    let workspace = unique_workspace("conversation-model-no-key");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let error = DesktopProviderSettingsStore::new(workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: String::new(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI Work".to_owned(),
                id: "openai-work".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(error.message.contains("apiKey is required"));
}

#[cfg(unix)]
#[test]
fn desktop_provider_settings_store_writes_owner_only_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = unique_workspace("provider-settings-owner-only");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI Work".to_owned(),
                id: "openai-work".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();

    let settings_path = workspace
        .join(".jyowo")
        .join("runtime")
        .join("provider-settings.json");
    let mode = std::fs::metadata(settings_path)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn desktop_provider_settings_store_rejects_symlink_settings_file() {
    let workspace = unique_workspace("provider-settings-symlink-file");
    let external = unique_workspace("provider-settings-external-target");
    let settings_dir = workspace.join(".jyowo").join("runtime");
    let settings_path = settings_dir.join("provider-settings.json");
    std::fs::create_dir_all(&settings_dir).unwrap();
    std::fs::create_dir_all(&external).unwrap();
    std::os::unix::fs::symlink(external.join("provider-settings.json"), &settings_path).unwrap();
    let store = DesktopProviderSettingsStore::new(workspace);

    let error = store.load_record().unwrap_err();
    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");

    let error = store
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: String::new(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(!external.join("provider-settings.json").exists());
}
