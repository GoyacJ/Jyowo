use harness_contracts::{RedactRules, Redactor};

pub struct RedactorContractTest;

impl RedactorContractTest {
    pub fn assert_idempotent(redactor: &dyn Redactor, input: &str, rules: &RedactRules) {
        let once = redactor.redact(input, rules);
        let twice = redactor.redact(&once, rules);
        assert_eq!(once, twice);
    }

    pub fn assert_noop_compatible(redactor: &dyn Redactor) {
        let input = "ordinary non-secret text";
        assert_eq!(redactor.redact(input, &RedactRules::default()), input);
    }
}
