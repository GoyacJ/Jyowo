#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[tokio::test]
async fn save_provider_settings_payload_stores_official_quota_key_but_omits_raw_key_from_payload() {
    let raw_key = "provider-test-token";
    let official_quota_key = "official-quota-admin-token";
    let store = RecordingProviderSettingsStore::default();

    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some(raw_key.to_owned()),
            base_url: None,
            config_id: None,
            display_name: Some("OpenAI Mini".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: Some(official_quota_key.to_owned()),
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    let serialized = serde_json::to_string(&payload).unwrap();
    assert!(serialized.contains("\"hasOfficialQuotaApiKey\":true"));
    assert!(!serialized.contains(raw_key));
    assert!(!serialized.contains(official_quota_key));

    let record = store.record.lock().unwrap().clone().unwrap();
    assert_eq!(
        record.configs[0].official_quota_api_key.as_deref(),
        Some(official_quota_key)
    );

    let listed = list_provider_settings_with_store(&store).await.unwrap();
    let listed_serialized = serde_json::to_string(&listed).unwrap();
    assert!(listed_serialized.contains("\"hasOfficialQuotaApiKey\":true"));
    assert!(!listed_serialized.contains(raw_key));
    assert!(!listed_serialized.contains(official_quota_key));
}

#[tokio::test]
async fn save_provider_settings_payload_does_not_preserve_official_quota_key_when_base_url_changes()
{
    let store = RecordingProviderSettingsStore::default();
    *store.record.lock().unwrap() = Some(ProviderSettingsRecord {
        default_config_id: Some("openai-gateway".to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: "provider-test-token".to_owned(),
            protocol: ModelProtocol::Responses,
            base_url: Some("https://gateway.example.com".to_owned()),
            display_name: "OpenAI gateway".to_owned(),
            id: "openai-gateway".to_owned(),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: harness_contracts::ModelRequestOptions::default(),
            official_quota_api_key: Some("old-official-admin-key".to_owned()),
            provider_id: "openai".to_owned(),
            model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
        }],
    });

    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("new-provider-test-token".to_owned()),
            base_url: Some("https://new-gateway.example.com".to_owned()),
            config_id: Some("openai-gateway".to_owned()),
            display_name: Some("OpenAI gateway".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    assert!(!payload.config.has_official_quota_api_key);
    let record = store.record.lock().unwrap().clone().unwrap();
    assert_eq!(record.configs[0].official_quota_api_key, None);
}
