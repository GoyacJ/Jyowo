use std::sync::Arc;

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{TimeZone, Utc};
use harness_contracts::{PluginId, RejectionReason, TrustLevel};
use harness_plugin::{
    DiscoverySource, ManifestLoaderError, ManifestOrigin, ManifestRecord, ManifestSignature,
    ManifestSigner, PluginCapabilities, PluginError, PluginManifest, PluginManifestLoader,
    PluginName, PluginRegistry, SignatureAlgorithm, SignerId, SignerProvenance,
    StaticTrustedSignerStore, TrustedSigner,
};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::json;

#[tokio::test]
async fn admin_trusted_manifest_without_signature_is_rejected() {
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![record(manifest(
            "unsigned-admin",
            TrustLevel::AdminTrusted,
        ))])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert!(matches!(
        registry
            .state_detail(&PluginId("unsigned-admin@0.1.0".to_owned()))
            .and_then(|detail| detail.rejection_reason),
        Some(RejectionReason::SignatureInvalid { .. })
    ));
}

#[tokio::test]
async fn admin_trusted_manifest_signed_by_unknown_signer_is_rejected() {
    let keypair = keypair();
    let signed_at = Utc.with_ymd_and_hms(2026, 4, 27, 0, 0, 0).unwrap();
    let manifest = signed_manifest(
        manifest("unknown-signer", TrustLevel::AdminTrusted),
        &keypair,
        "acme-prod-r1",
        signed_at,
        SignatureAlgorithm::Ed25519,
    );
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![record(manifest)])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert!(matches!(
        registry
            .state_detail(&PluginId("unknown-signer@0.1.0".to_owned()))
            .and_then(|detail| detail.rejection_reason),
        Some(RejectionReason::UnknownSigner { signer }) if signer == "acme-prod-r1"
    ));
}

#[tokio::test]
async fn admin_trusted_manifest_rejects_algorithm_mismatch_and_bad_windows() {
    let keypair = keypair();
    let signer = trusted_signer(
        "acme-prod-r1",
        &keypair,
        SignatureAlgorithm::RsaPkcs1Sha256,
        None,
    );
    let store = Arc::new(StaticTrustedSignerStore::new(vec![signer]).unwrap());
    let signed_at = Utc.with_ymd_and_hms(2026, 4, 27, 0, 0, 0).unwrap();
    let registry = PluginRegistry::builder()
        .with_signer_store(store)
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![record(
            signed_manifest(
                manifest("algorithm-mismatch", TrustLevel::AdminTrusted),
                &keypair,
                "acme-prod-r1",
                signed_at,
                SignatureAlgorithm::Ed25519,
            ),
        )])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();
    assert!(discovered.is_empty());
    assert!(matches!(
        registry
            .state_detail(&PluginId("algorithm-mismatch@0.1.0".to_owned()))
            .and_then(|detail| detail.rejection_reason),
        Some(RejectionReason::SignatureInvalid { .. })
    ));

    let signer = trusted_signer("acme-prod-r2", &keypair, SignatureAlgorithm::Ed25519, None)
        .with_window(
            Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
            Some(Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap()),
        );
    let store = Arc::new(StaticTrustedSignerStore::new(vec![signer]).unwrap());
    let registry = PluginRegistry::builder()
        .with_signer_store(store)
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![record(
            signed_manifest(
                manifest("too-early", TrustLevel::AdminTrusted),
                &keypair,
                "acme-prod-r2",
                signed_at,
                SignatureAlgorithm::Ed25519,
            ),
        )])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();
    assert!(discovered.is_empty());
    assert!(matches!(
        registry
            .state_detail(&PluginId("too-early@0.1.0".to_owned()))
            .and_then(|detail| detail.rejection_reason),
        Some(RejectionReason::SignatureInvalid { .. })
    ));
}

#[tokio::test]
async fn revoked_signer_rejects_manifest_even_when_signature_is_valid() {
    let keypair = keypair();
    let revoked_at = Utc.with_ymd_and_hms(2026, 4, 27, 1, 0, 0).unwrap();
    let signer = trusted_signer(
        "acme-revoked-r1",
        &keypair,
        SignatureAlgorithm::Ed25519,
        Some(revoked_at),
    );
    let store = Arc::new(StaticTrustedSignerStore::new(vec![signer]).unwrap());
    let registry = PluginRegistry::builder()
        .with_signer_store(store)
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![record(
            signed_manifest(
                manifest("revoked", TrustLevel::AdminTrusted),
                &keypair,
                "acme-revoked-r1",
                Utc.with_ymd_and_hms(2026, 4, 27, 0, 0, 0).unwrap(),
                SignatureAlgorithm::Ed25519,
            ),
        )])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert!(matches!(
        registry
            .state_detail(&PluginId("revoked@0.1.0".to_owned()))
            .and_then(|detail| detail.rejection_reason),
        Some(RejectionReason::SignerRevoked { signer, revoked_at: at })
            if signer == "acme-revoked-r1" && at == revoked_at
    ));
}

#[tokio::test]
async fn signature_rejection_does_not_abort_later_valid_discovery() {
    let keypair = keypair();
    let signer = trusted_signer("acme-prod-r1", &keypair, SignatureAlgorithm::Ed25519, None);
    let store = Arc::new(StaticTrustedSignerStore::new(vec![signer]).unwrap());
    let signed_at = Utc.with_ymd_and_hms(2026, 4, 27, 0, 0, 0).unwrap();
    let bad = signed_manifest(
        manifest("bad-admin", TrustLevel::AdminTrusted),
        &keypair,
        "unknown-signer",
        signed_at,
        SignatureAlgorithm::Ed25519,
    );
    let good = signed_manifest(
        manifest("good-admin", TrustLevel::AdminTrusted),
        &keypair,
        "acme-prod-r1",
        signed_at,
        SignatureAlgorithm::Ed25519,
    );
    let registry = PluginRegistry::builder()
        .with_signer_store(store)
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![
            record(bad),
            record(good),
        ])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert_eq!(discovered.len(), 1);
    assert_eq!(
        discovered[0].record.manifest.plugin_id().0,
        "good-admin@0.1.0"
    );
    assert!(matches!(
        registry.state(&PluginId("bad-admin@0.1.0".to_owned())),
        Some(harness_plugin::PluginLifecycleState::Rejected(_))
    ));
}

#[tokio::test]
async fn valid_admin_trusted_signature_passes_discovery() {
    let keypair = keypair();
    let signer = trusted_signer("acme-prod-r1", &keypair, SignatureAlgorithm::Ed25519, None);
    let store = Arc::new(StaticTrustedSignerStore::new(vec![signer]).unwrap());
    let registry = PluginRegistry::builder()
        .with_signer_store(store)
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![record(
            signed_manifest(
                manifest("signed-admin", TrustLevel::AdminTrusted),
                &keypair,
                "acme-prod-r1",
                Utc.with_ymd_and_hms(2026, 4, 27, 0, 0, 0).unwrap(),
                SignatureAlgorithm::Ed25519,
            ),
        )])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert_eq!(discovered.len(), 1);
    assert_eq!(
        discovered[0].record.manifest.plugin_id().0,
        "signed-admin@0.1.0"
    );
}

#[tokio::test]
async fn valid_rsa_pkcs1_sha256_signature_passes_discovery() {
    let signed_at = Utc.with_ymd_and_hms(2026, 4, 27, 0, 0, 0).unwrap();
    let manifest = rsa_signed_manifest(manifest("rsa-admin", TrustLevel::AdminTrusted), signed_at);
    let store =
        Arc::new(StaticTrustedSignerStore::new(vec![rsa_trusted_signer("acme-rsa-r1")]).unwrap());
    let registry = PluginRegistry::builder()
        .with_signer_store(store)
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![record(manifest)])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert_eq!(discovered.len(), 1);
    assert_eq!(
        discovered[0].record.manifest.plugin_id().0,
        "rsa-admin@0.1.0"
    );
}

#[test]
fn canonical_payload_sorts_nested_objects_and_strips_signature() {
    let mut manifest = manifest("canonical-admin", TrustLevel::AdminTrusted);
    manifest.capabilities.configuration_schema = Some(json!({
        "z": true,
        "a": {
            "y": 1,
            "b": 2
        }
    }));
    manifest.signature = Some(ManifestSignature {
        algorithm: SignatureAlgorithm::Ed25519,
        signer: "acme-prod-r1".to_owned(),
        signature: vec![1, 2, 3],
        timestamp: "2026-04-27T00:00:00Z".to_owned(),
    });

    let payload = String::from_utf8(ManifestSigner::canonical_payload(&manifest).unwrap()).unwrap();

    assert!(!payload.contains("\"signature\""));
    assert!(payload.find("\"a\"").unwrap() < payload.find("\"z\"").unwrap());
    assert!(payload.find("\"b\"").unwrap() < payload.find("\"y\"").unwrap());
}

#[tokio::test]
async fn user_controlled_signature_does_not_upgrade_trust_level() {
    let keypair = keypair();
    let signer = trusted_signer("acme-prod-r1", &keypair, SignatureAlgorithm::Ed25519, None);
    let store = Arc::new(StaticTrustedSignerStore::new(vec![signer]).unwrap());
    let registry = PluginRegistry::builder()
        .with_signer_store(store)
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![record(
            signed_manifest(
                manifest("signed-user", TrustLevel::UserControlled),
                &keypair,
                "acme-prod-r1",
                Utc.with_ymd_and_hms(2026, 4, 27, 0, 0, 0).unwrap(),
                SignatureAlgorithm::Ed25519,
            ),
        )])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert_eq!(
        discovered[0].record.manifest.trust_level,
        TrustLevel::UserControlled
    );
}

#[tokio::test]
async fn static_signer_store_tracks_active_and_revoked_signers() {
    let keypair = keypair();
    let revoked_at = Utc.with_ymd_and_hms(2026, 4, 27, 0, 0, 0).unwrap();
    let store = StaticTrustedSignerStore::new(vec![
        trusted_signer(
            "acme-active-r1",
            &keypair,
            SignatureAlgorithm::Ed25519,
            None,
        ),
        trusted_signer(
            "acme-revoked-r1",
            &keypair,
            SignatureAlgorithm::Ed25519,
            Some(revoked_at),
        ),
    ])
    .unwrap();

    let active = store.list_active().await.unwrap();

    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id.as_str(), "acme-active-r1");
    assert!(store
        .is_revoked(&SignerId::new("acme-revoked-r1").unwrap(), Utc::now())
        .await
        .unwrap());
    assert!(store
        .get(&SignerId::new("acme-active-r1").unwrap())
        .await
        .unwrap()
        .is_some());
}

#[test]
fn signer_store_and_trusted_signer_builder_modes_are_mutually_exclusive() {
    let keypair = keypair();
    let store = Arc::new(
        StaticTrustedSignerStore::new(vec![trusted_signer(
            "acme-prod-r1",
            &keypair,
            SignatureAlgorithm::Ed25519,
            None,
        )])
        .unwrap(),
    );

    let error = PluginRegistry::builder()
        .with_signer_store(store)
        .with_trusted_signer(keypair.public_key().as_ref().to_vec())
        .build()
        .unwrap_err();

    assert!(matches!(error, PluginError::Builder(_)));
}

struct StaticManifestLoader {
    records: Vec<ManifestRecord>,
}

impl StaticManifestLoader {
    fn new(records: Vec<ManifestRecord>) -> Self {
        Self { records }
    }
}

#[async_trait]
impl PluginManifestLoader for StaticManifestLoader {
    async fn enumerate(
        &self,
        _source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        Ok(self.records.clone())
    }
}

fn keypair() -> Ed25519KeyPair {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
}

fn trusted_signer(
    id: &str,
    keypair: &Ed25519KeyPair,
    algorithm: SignatureAlgorithm,
    revoked_at: Option<chrono::DateTime<Utc>>,
) -> TrustedSigner {
    TrustedSigner {
        id: SignerId::new(id).unwrap(),
        algorithm,
        public_key: keypair.public_key().as_ref().to_vec(),
        activated_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        retired_at: None,
        revoked_at,
        provenance: SignerProvenance::BuilderInjected,
    }
}

trait TrustedSignerTestExt {
    fn with_window(
        self,
        activated_at: chrono::DateTime<Utc>,
        retired_at: Option<chrono::DateTime<Utc>>,
    ) -> Self;
}

impl TrustedSignerTestExt for TrustedSigner {
    fn with_window(
        mut self,
        activated_at: chrono::DateTime<Utc>,
        retired_at: Option<chrono::DateTime<Utc>>,
    ) -> Self {
        self.activated_at = activated_at;
        self.retired_at = retired_at;
        self
    }
}

fn signed_manifest(
    mut manifest: PluginManifest,
    keypair: &Ed25519KeyPair,
    signer: &str,
    signed_at: chrono::DateTime<Utc>,
    algorithm: SignatureAlgorithm,
) -> PluginManifest {
    let payload = ManifestSigner::canonical_payload(&manifest).unwrap();
    let signature = keypair.sign(&payload);
    manifest.signature = Some(ManifestSignature {
        algorithm,
        signer: signer.to_owned(),
        signature: signature.as_ref().to_vec(),
        timestamp: signed_at.to_rfc3339(),
    });
    manifest
}

fn rsa_signed_manifest(
    mut manifest: PluginManifest,
    signed_at: chrono::DateTime<Utc>,
) -> PluginManifest {
    manifest.signature = Some(ManifestSignature {
        algorithm: SignatureAlgorithm::RsaPkcs1Sha256,
        signer: "acme-rsa-r1".to_owned(),
        signature: STANDARD.decode(RSA_SIGNATURE_B64).unwrap(),
        timestamp: signed_at.to_rfc3339(),
    });
    manifest
}

fn rsa_trusted_signer(id: &str) -> TrustedSigner {
    TrustedSigner {
        id: SignerId::new(id).unwrap(),
        algorithm: SignatureAlgorithm::RsaPkcs1Sha256,
        public_key: STANDARD.decode(RSA_PUBLIC_KEY_B64).unwrap(),
        activated_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        retired_at: None,
        revoked_at: None,
        provenance: SignerProvenance::BuilderInjected,
    }
}

const RSA_PUBLIC_KEY_B64: &str = "MIIBCgKCAQEAkqC9D2WUiR1984HrLBKxWaNKsF3buix+bX+rEJtgUJaEKgzarVZRj8w7/v9Svpj2B3Af8iErapfERZJ0JyTgpwE3/g52FKZH/Fz1ko+fxKXAmt26CMANE0U9wocbv6xBDA/S5XJBHxp31vahw7rIujjH01IyflFmXApuTTzvQBugy530jVC8uAPFhiTadryCF2g8TazC/Ppseq0cK+pKI6H0J5iRtRrVUKA95auWxNR+rRRxNfABzpDMaaw65/796cEZH8cq9BY8pz1oCmvZZ3eyFt/MaznbCTjQbTGQV84AUBCkWxDPqodJUgqie/+IzPysKf/6MPXneI6mQNz3LQIDAQAB";

const RSA_SIGNATURE_B64: &str = "XxH81o1di3vh4UnePsJ5KBulbLeWQJ8OK8DNQgmk0jjewF6OaCYhKoI8A+XNBwtEIvJFXiC7wXcL8ETV4IaZ9BItEOGz8qJUN5RBzOE9BlRcuRZTdsCMaS70JtPXuKF418JcVfjWHRfGFN25X/6IQrWJNT/Ix7a/jG0awNWIpFMJkyBdhR9CVEjgCX+aeq6cwolm6uLu+fMwmrbTbJZHd7r9rQYp0jCBLvX1i146aS8ic6lTQ40qp5BXWLQjVmjAfVTf7OCZ/sxruC32Djs1+TDvwy9buihPXoZVpcFtJwXZG6r1EFPWFtmpmjne0b22jfFB0oncbI2sDfbi39JgXQ==";

fn record(manifest: PluginManifest) -> ManifestRecord {
    ManifestRecord::new(
        manifest,
        ManifestOrigin::File {
            path: "/plugins/plugin.json".into(),
        },
        [9; 32],
    )
    .unwrap()
}

fn manifest(name: &str, trust_level: TrustLevel) -> PluginManifest {
    PluginManifest {
        name: PluginName::new(name).unwrap(),
        version: semver::Version::parse("0.1.0").unwrap(),
        trust_level,
        description: None,
        authors: Vec::new(),
        repository: None,
        signature: None,
        capabilities: PluginCapabilities::default(),
        dependencies: Vec::new(),
        min_harness_version: semver::VersionReq::parse(">=0.0.0").unwrap(),
    }
}
