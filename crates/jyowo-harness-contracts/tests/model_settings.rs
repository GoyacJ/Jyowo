use chrono::{TimeZone, Utc};
use harness_contracts::{
    export_all_schemas, CapabilityRouteHealth, CapabilityRouteKind, ModelUsageBucket, ModelUsagePeriod,
    ModelUsageSummary, ModelUsageWindow, OfficialQuotaSnapshot,
    OfficialQuotaStatus, ProviderProbeErrorKind, ProviderProbeSnapshot, ProviderProbeStatus,
    UsageSnapshot,
};
use serde_json::{json, Value};

fn sample_usage_snapshot() -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        cost_micros: 1_000,
        tool_calls: 0,
    }
}

fn sample_usage_window(period: ModelUsagePeriod) -> ModelUsageWindow {
    ModelUsageWindow {
        period,
        period_start: Some(Utc.with_ymd_and_hms(2026, 6, 30, 0, 0, 0).unwrap()),
        period_end: Some(Utc.with_ymd_and_hms(2026, 6, 30, 23, 59, 59).unwrap()),
        total: sample_usage_snapshot(),
        by_model: vec![ModelUsageBucket {
            key: "openai/gpt-4.1".to_owned(),
            provider_id: "openai".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            usage: sample_usage_snapshot(),
            last_used_at: Some(Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap()),
        }],
    }
}

#[test]
fn provider_probe_snapshot_serializes_snake_case_wire_shape() {
    let snapshot = ProviderProbeSnapshot {
        config_id: "cfg-openai".to_owned(),
        provider_id: "openai".to_owned(),
        model_id: "gpt-4.1".to_owned(),
        status: ProviderProbeStatus::Online,
        timeout_ms: 10_000,
        latency_ms: Some(812),
        checked_at: Utc.with_ymd_and_hms(2026, 6, 30, 0, 0, 0).unwrap(),
        error_kind: None,
        safe_message: None,
    };

    assert_eq!(
        serde_json::to_value(&snapshot).unwrap(),
        json!({
            "config_id": "cfg-openai",
            "provider_id": "openai",
            "model_id": "gpt-4.1",
            "status": "online",
            "timeout_ms": 10000,
            "latency_ms": 812,
            "checked_at": "2026-06-30T00:00:00Z",
            "error_kind": null,
            "safe_message": null
        })
    );
}

#[test]
fn provider_probe_snapshot_deserializes_round_trip() {
    let value = json!({
        "config_id": "cfg-openai",
        "provider_id": "openai",
        "model_id": "gpt-4.1",
        "status": "timeout",
        "timeout_ms": 5000,
        "latency_ms": null,
        "checked_at": "2026-06-30T00:00:00Z",
        "error_kind": "timeout",
        "safe_message": "Probe timed out."
    });

    let snapshot: ProviderProbeSnapshot = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(snapshot.status, ProviderProbeStatus::Timeout);
    assert_eq!(snapshot.error_kind, Some(ProviderProbeErrorKind::Timeout));
    assert_eq!(serde_json::to_value(snapshot).unwrap(), value);
}

#[test]
fn provider_probe_snapshot_requires_checked_at() {
    let value = json!({
        "config_id": "cfg-openai",
        "provider_id": "openai",
        "model_id": "gpt-4.1",
        "status": "online",
        "timeout_ms": 10000,
        "latency_ms": 812
    });

    let error = serde_json::from_value::<ProviderProbeSnapshot>(value).unwrap_err();
    assert!(error.to_string().contains("checked_at"));
}

#[test]
fn provider_probe_status_rejects_never_checked_variant() {
    let value = json!({
        "config_id": "cfg-openai",
        "provider_id": "openai",
        "model_id": "gpt-4.1",
        "status": "never_checked",
        "timeout_ms": 10000,
        "latency_ms": null,
        "checked_at": "2026-06-30T00:00:00Z",
        "error_kind": null,
        "safe_message": null
    });

    assert!(serde_json::from_value::<ProviderProbeSnapshot>(value).is_err());
}

#[test]
fn model_usage_summary_serializes_timezone_fields() {
    let summary = ModelUsageSummary {
        timezone_id: Some("America/New_York".to_owned()),
        timezone_offset_minutes: -240,
        today: sample_usage_window(ModelUsagePeriod::Today),
        month_to_date: sample_usage_window(ModelUsagePeriod::MonthToDate),
        all_time: sample_usage_window(ModelUsagePeriod::AllTime),
        generated_at: Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap(),
    };

    let value = serde_json::to_value(&summary).unwrap();
    assert_eq!(value["timezone_id"], json!("America/New_York"));
    assert_eq!(value["timezone_offset_minutes"], json!(-240));
    assert_eq!(value["today"]["period"], json!("today"));
    assert_eq!(value["month_to_date"]["period"], json!("month_to_date"));
    assert_eq!(value["all_time"]["period"], json!("all_time"));
    assert!(value.get("generated_at").is_some());

    let round_trip: ModelUsageSummary = serde_json::from_value(value).unwrap();
    assert_eq!(round_trip.timezone_id, Some("America/New_York".to_owned()));
    assert_eq!(round_trip.timezone_offset_minutes, -240);
}

fn supported_quota_value() -> Value {
    json!({
        "config_id": "cfg-openai",
        "provider_id": "openai",
        "model_id": "gpt-4.1",
        "scope": "account",
        "status": "supported",
        "period_start": null,
        "period_end": null,
        "quota_used": 100,
        "quota_total": 1000,
        "quota_remaining": 900,
        "unit": "tokens",
        "billing_label": "Pay as you go",
        "source_url": "https://platform.openai.com/usage",
        "fetched_at": "2026-06-30T00:00:00Z",
        "expires_at": "2026-06-30T01:00:00Z",
        "is_stale": false,
        "safe_message": null
    })
}

#[test]
fn official_quota_snapshot_serializes_freshness_fields() {
    let snapshot: OfficialQuotaSnapshot = serde_json::from_value(supported_quota_value()).unwrap();
    let value = serde_json::to_value(&snapshot).unwrap();

    assert_eq!(
        value["source_url"],
        json!("https://platform.openai.com/usage")
    );
    assert_eq!(value["fetched_at"], json!("2026-06-30T00:00:00Z"));
    assert_eq!(value["expires_at"], json!("2026-06-30T01:00:00Z"));
    assert_eq!(value["is_stale"], json!(false));
}

#[test]
fn official_quota_snapshot_requires_fetched_at_expires_at_and_is_stale() {
    for missing in ["fetched_at", "expires_at", "is_stale"] {
        let mut value = supported_quota_value();
        if let Value::Object(ref mut map) = value {
            map.remove(missing);
        }
        assert!(
            serde_json::from_value::<OfficialQuotaSnapshot>(value).is_err(),
            "expected missing {missing} to fail"
        );
    }
}

#[test]
fn official_quota_snapshot_rejects_empty_source_url_for_non_not_configured_status() {
    for status in ["supported", "unsupported", "auth_required", "failed"] {
        let mut value = supported_quota_value();
        value["status"] = json!(status);
        value["source_url"] = json!("");
        assert!(
            serde_json::from_value::<OfficialQuotaSnapshot>(value).is_err(),
            "expected empty source_url to fail for status {status}"
        );
    }
}

#[test]
fn official_quota_snapshot_allows_empty_source_url_for_not_configured() {
    let value = json!({
        "config_id": "cfg-openai",
        "provider_id": "openai",
        "model_id": null,
        "scope": "account",
        "status": "not_configured",
        "period_start": null,
        "period_end": null,
        "quota_used": null,
        "quota_total": null,
        "quota_remaining": null,
        "unit": null,
        "billing_label": null,
        "source_url": "",
        "fetched_at": "2026-06-30T00:00:00Z",
        "expires_at": "2026-06-30T01:00:00Z",
        "is_stale": false,
        "safe_message": null
    });

    let snapshot: OfficialQuotaSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(snapshot.status, OfficialQuotaStatus::NotConfigured);
}

#[test]
fn official_quota_snapshot_requires_safe_message_for_error_statuses() {
    for status in ["unsupported", "auth_required", "failed"] {
        let mut value = supported_quota_value();
        value["status"] = json!(status);
        value["safe_message"] = Value::Null;
        assert!(
            serde_json::from_value::<OfficialQuotaSnapshot>(value.clone()).is_err(),
            "expected missing safe_message to fail for status {status}"
        );

        value["safe_message"] = json!("");
        assert!(
            serde_json::from_value::<OfficialQuotaSnapshot>(value).is_err(),
            "expected empty safe_message to fail for status {status}"
        );
    }
}

#[test]
fn capability_route_health_serializes_round_trip() {
    let health = CapabilityRouteHealth {
        kind: CapabilityRouteKind::ImageGeneration,
        config_id: Some("cfg-openai".to_owned()),
        provider_id: Some("openai".to_owned()),
        model_id: Some("gpt-4.1".to_owned()),
        probe: Some(ProviderProbeSnapshot {
            config_id: "cfg-openai".to_owned(),
            provider_id: "openai".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            status: ProviderProbeStatus::Online,
            timeout_ms: 10_000,
            latency_ms: Some(812),
            checked_at: Utc.with_ymd_and_hms(2026, 6, 30, 0, 0, 0).unwrap(),
            error_kind: None,
            safe_message: None,
        }),
    };

    let value = serde_json::to_value(&health).unwrap();
    assert_eq!(value["kind"], json!("image_generation"));
    assert_eq!(value["config_id"], json!("cfg-openai"));

    let round_trip: CapabilityRouteHealth = serde_json::from_value(value).unwrap();
    assert_eq!(round_trip.kind, CapabilityRouteKind::ImageGeneration);
    assert!(round_trip.probe.is_some());
}

#[test]
fn model_settings_schema_exports_are_registered() {
    let schemas = export_all_schemas();
    for key in [
        "provider_probe_status",
        "provider_probe_error_kind",
        "provider_probe_snapshot",
        "model_usage_bucket",
        "model_usage_period",
        "model_usage_window",
        "model_usage_summary",
        "official_quota_scope",
        "official_quota_status",
        "official_quota_snapshot",
        "capability_route_health",
    ] {
        assert!(schemas.contains_key(key), "missing model settings schema: {key}");
    }
}
