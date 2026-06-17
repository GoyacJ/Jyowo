#![cfg(feature = "integrity")]

use bytes::Bytes;
use harness_contracts::TenantId;
use harness_permission::{
    canonical_bytes, IntegrityAlgorithm, IntegrityError, IntegritySignature, StaticSignerStore,
};
use serde_json::json;

#[tokio::test]
async fn static_signer_signs_and_verifies_payload() {
    let store = StaticSignerStore::new();
    store.insert_key("jyowo-permission-test", vec![7_u8; 32]);
    let signer = store
        .signer("jyowo-permission-test", IntegrityAlgorithm::HmacSha256)
        .unwrap();
    let payload = br#"{"decision":"allow_once"}"#;

    let signature = signer.sign(payload).await.unwrap();

    signer.verify(payload, &signature).await.unwrap();
    assert_eq!(signature.key_id, "jyowo-permission-test");
    assert_eq!(signature.algorithm, IntegrityAlgorithm::HmacSha256);
}

#[tokio::test]
async fn static_signer_rejects_modified_payload() {
    let signer = StaticSignerStore::from_key(
        "jyowo-permission-test",
        vec![9_u8; 32],
        IntegrityAlgorithm::HmacSha256,
    )
    .unwrap();
    let signature = signer.sign(br#"{"decision":"allow_once"}"#).await.unwrap();

    let err = signer
        .verify(br#"{"decision":"deny_once"}"#, &signature)
        .await
        .unwrap_err();

    assert!(matches!(err, IntegrityError::Mismatch));
}

#[tokio::test]
async fn static_signer_rejects_unknown_key_id() {
    let signer = StaticSignerStore::from_key(
        "jyowo-permission-new",
        vec![1_u8; 32],
        IntegrityAlgorithm::HmacSha256,
    )
    .unwrap();
    let signature = IntegritySignature {
        algorithm: IntegrityAlgorithm::HmacSha256,
        key_id: "jyowo-permission-old".to_owned(),
        mac: Bytes::from_static(b"not-a-real-mac"),
        signed_at: chrono::Utc::now(),
    };

    let err = signer.verify(b"payload", &signature).await.unwrap_err();

    assert!(matches!(
        err,
        IntegrityError::UnknownKeyId(key_id) if key_id == "jyowo-permission-old"
    ));
}

#[tokio::test]
async fn static_signer_blocks_algorithm_downgrade() {
    let sha256_signer = StaticSignerStore::from_key(
        "jyowo-permission-test",
        vec![3_u8; 32],
        IntegrityAlgorithm::HmacSha256,
    )
    .unwrap();
    let sha512_signer = StaticSignerStore::from_key(
        "jyowo-permission-test",
        vec![3_u8; 32],
        IntegrityAlgorithm::HmacSha512,
    )
    .unwrap();
    let signature = sha256_signer.sign(b"payload").await.unwrap();

    let err = sha512_signer
        .verify(b"payload", &signature)
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        IntegrityError::AlgorithmDowngrade {
            stored: IntegrityAlgorithm::HmacSha256,
            expected: IntegrityAlgorithm::HmacSha512,
        }
    ));
}

#[test]
fn canonical_bytes_sorts_nested_objects_and_excludes_top_level_signature() {
    let record = json!({
        "signature": {
            "mac": "excluded"
        },
        "z": 2,
        "a": {
            "d": 4,
            "b": 1
        },
        "tenant_id": TenantId::SHARED,
    });

    let canonical = canonical_bytes(&record).unwrap();
    let canonical = String::from_utf8(canonical).unwrap();

    assert!(canonical.starts_with(r#"{"a":{"b":1,"d":4},"tenant_id":"#));
    assert!(canonical.ends_with(r#","z":2}"#));
    assert!(!canonical.contains("signature"));
}
