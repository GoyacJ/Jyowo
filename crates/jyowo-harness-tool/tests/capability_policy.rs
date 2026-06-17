use harness_contracts::{ToolCapability, TrustLevel};
use harness_tool::{CapabilityPolicy, CapabilityPolicyRule};

#[test]
fn capability_policy_is_auditable_and_matches_current_defaults() {
    let policy = CapabilityPolicy::default();
    let matrix = policy.describe();

    assert!(matrix.iter().any(|entry| {
        entry.capability == ToolCapability::BlobReader
            && entry.rule == CapabilityPolicyRule::AnyTrust
    }));
    assert!(matrix.iter().any(|entry| {
        entry.capability == ToolCapability::Custom("*".to_owned())
            && entry.rule == CapabilityPolicyRule::AdminTrustedOnly
    }));

    assert!(policy.allows(TrustLevel::UserControlled, &ToolCapability::BlobReader));
    assert!(!policy.allows(TrustLevel::UserControlled, &ToolCapability::CodeRuntime));
    assert!(policy.allows(TrustLevel::AdminTrusted, &ToolCapability::CodeRuntime));
    assert!(!policy.allows(
        TrustLevel::UserControlled,
        &ToolCapability::Custom("private".to_owned())
    ));
    assert!(policy.allows(
        TrustLevel::AdminTrusted,
        &ToolCapability::Custom("private".to_owned())
    ));
}
