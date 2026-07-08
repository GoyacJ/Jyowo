#![cfg(feature = "redactor")]

use harness_contracts::{
    NoopRedactor, RedactPatternKind, RedactPatternSet, RedactRules, RedactScope, Redactor,
};
use harness_observability::{DefaultRedactor, RedactPattern, RedactorContractTest};
use regex::Regex;

#[test]
fn default_redactor_has_thirty_plus_builtin_patterns() {
    let redactor = DefaultRedactor::default();

    assert!(redactor.pattern_count() >= 30);
}

#[test]
fn default_redactor_redacts_common_secret_shapes_and_is_idempotent() {
    let redactor = DefaultRedactor::default();
    let rules = RedactRules {
        scope: RedactScope::All,
        replacement: "[REDACTED]".to_owned(),
        pattern_set: RedactPatternSet::AllBuiltins,
    };
    let input = concat!(
        "Authorization: Bearer abcdefghijklmnopqrstuvwxyz ",
        "postgres://user:password@example.internal/app ",
        "email alice@example.com ",
        "10.1.2.3 ",
        "sk-abcdefghijklmnopqrstuvwxyz"
    );

    let redacted = redactor.redact(input, &rules);

    assert!(!redacted.contains("Bearer abcdefghijklmnopqrstuvwxyz"));
    assert!(!redacted.contains("postgres://user:password@example.internal/app"));
    assert!(!redacted.contains("alice@example.com"));
    assert!(!redacted.contains("10.1.2.3"));
    assert!(!redacted.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    RedactorContractTest::assert_idempotent(&redactor, input, &rules);
}

#[test]
fn default_redactor_preserves_existing_replacement_token() {
    let redactor = DefaultRedactor::default();
    let rules = RedactRules {
        scope: RedactScope::EventBody,
        replacement: "[REDACTED]".to_owned(),
        pattern_set: RedactPatternSet::Default,
    };

    assert_eq!(
        redactor.redact("already [REDACTED]", &rules),
        "already [REDACTED]"
    );
}

#[test]
fn pattern_set_only_limits_redaction_to_selected_kind() {
    let redactor = DefaultRedactor::default();
    let rules = RedactRules {
        scope: RedactScope::All,
        replacement: "[REDACTED]".to_owned(),
        pattern_set: RedactPatternSet::Only(vec![RedactPatternKind::Email]),
    };

    let redacted = redactor.redact("alice@example.com sk-abcdefghijklmnopqrstuvwxyz", &rules);

    assert!(!redacted.contains("alice@example.com"));
    assert!(redacted.contains("sk-abcdefghijklmnopqrstuvwxyz"));
}

#[test]
fn custom_patterns_can_use_pattern_specific_replacement() {
    let redactor = DefaultRedactor::default();
    redactor.add_pattern(RedactPattern::new(
        "internal_user_id",
        RedactPatternKind::Custom("internal_user_id".to_owned()),
        Regex::new(r"user-\d{10}").unwrap(),
        Some("[USER_ID]".to_owned()),
        RedactScope::All,
    ));

    let rules = RedactRules {
        scope: RedactScope::All,
        replacement: "[REDACTED]".to_owned(),
        pattern_set: RedactPatternSet::Only(vec![RedactPatternKind::Custom(
            "internal_user_id".to_owned(),
        )]),
    };

    assert_eq!(
        redactor.redact("actor=user-1234567890", &rules),
        "actor=[USER_ID]"
    );
}

#[test]
fn redactor_contract_accepts_noop_for_non_secret_text() {
    RedactorContractTest::assert_noop_contract(&NoopRedactor);
    RedactorContractTest::assert_noop_contract(&DefaultRedactor::default());
}
