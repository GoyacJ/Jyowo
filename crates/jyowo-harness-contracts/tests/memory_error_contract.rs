use harness_contracts::{
    MemoryActorContext, MemoryError, MemoryId, MemoryVisibility, SessionId, TenantId, ThreatAction,
    ThreatCategory,
};
use serde_json::Value;

#[test]
fn memory_error_typed_variants_have_stable_serde_tags() {
    let session_id = SessionId::new();
    let actor = MemoryActorContext {
        tenant_id: TenantId::SINGLE,
        user_id: Some("user-1".to_owned()),
        team_id: None,
        session_id: Some(session_id),
    };

    assert_unit_variant(MemoryError::ExternalSlotOccupied, "external_slot_occupied");
    assert_unit_variant(MemoryError::ExternalSlotLockBusy, "external_slot_lock_busy");
    assert_unit_variant(
        MemoryError::ExternalProviderNotConfigured,
        "external_provider_not_configured",
    );

    assert_struct_variant(
        MemoryError::ThreatDetected {
            pattern_id: "credential_api_key".to_owned(),
            category: ThreatCategory::Credential,
            action: ThreatAction::Redact,
        },
        "threat_detected",
    );
    assert_struct_variant(
        MemoryError::TooLarge {
            bytes: 2048,
            max: 1024,
        },
        "too_large",
    );
    assert_struct_variant(
        MemoryError::MemdirOverflow {
            chars: 2048,
            threshold: 1024,
        },
        "memdir_overflow",
    );
    assert_struct_variant(
        MemoryError::RecallDeadlineExceeded {
            provider: "external".to_owned(),
        },
        "recall_deadline_exceeded",
    );
    assert_struct_variant(
        MemoryError::ConcurrentWriteLockFailed { retries: 3 },
        "concurrent_write_lock_failed",
    );
    assert_struct_variant(
        MemoryError::VisibilityViolation {
            actor,
            visibility: MemoryVisibility::Private { session_id },
        },
        "visibility_violation",
    );
    assert_newtype_variant(MemoryError::NotFound(MemoryId::new()), "not_found");
}

fn assert_unit_variant(error: MemoryError, expected: &str) {
    let value = serde_json::to_value(error).unwrap();
    assert_eq!(value, Value::String(expected.to_owned()));
}

fn assert_newtype_variant(error: MemoryError, expected: &str) {
    let value = serde_json::to_value(error).unwrap();
    assert!(value.get(expected).is_some(), "{value}");
}

fn assert_struct_variant(error: MemoryError, expected: &str) {
    let value = serde_json::to_value(error).unwrap();
    assert!(value.get(expected).is_some(), "{value}");
}
