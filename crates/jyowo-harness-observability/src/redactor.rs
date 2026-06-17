use harness_contracts::{RedactPatternKind, RedactPatternSet, RedactRules, RedactScope, Redactor};
use parking_lot::RwLock;
use regex::Regex;

#[derive(Debug)]
pub struct DefaultRedactor {
    patterns: RwLock<Vec<RedactPattern>>,
}

impl DefaultRedactor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_pattern(&self, pattern: RedactPattern) {
        self.patterns.write().push(pattern);
    }

    #[must_use]
    pub fn pattern_count(&self) -> usize {
        self.patterns.read().len()
    }
}

impl Default for DefaultRedactor {
    fn default() -> Self {
        Self {
            patterns: RwLock::new(default_patterns()),
        }
    }
}

impl Redactor for DefaultRedactor {
    fn redact(&self, input: &str, rules: &RedactRules) -> String {
        if matches!(rules.pattern_set, RedactPatternSet::None) {
            return input.to_owned();
        }

        let mut output = input.to_owned();
        for pattern in self.patterns.read().iter() {
            if !pattern.enabled_for(rules) {
                continue;
            }
            let replacement = pattern.replacement.as_deref().unwrap_or(&rules.replacement);
            output = pattern.regex.replace_all(&output, replacement).into_owned();
        }
        output
    }
}

#[derive(Debug, Clone)]
pub struct RedactPattern {
    pub id: String,
    pub kind: RedactPatternKind,
    pub regex: Regex,
    pub replacement: Option<String>,
    pub scope: RedactScope,
}

impl RedactPattern {
    pub fn new(
        id: impl Into<String>,
        kind: RedactPatternKind,
        regex: Regex,
        replacement: Option<String>,
        scope: RedactScope,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            regex,
            replacement,
            scope,
        }
    }

    fn enabled_for(&self, rules: &RedactRules) -> bool {
        matches_scope(self.scope, rules.scope)
            && matches_pattern_set(&self.kind, &rules.pattern_set)
    }
}

fn matches_scope(pattern_scope: RedactScope, rule_scope: RedactScope) -> bool {
    matches!(rule_scope, RedactScope::All)
        || matches!(pattern_scope, RedactScope::All)
        || pattern_scope == rule_scope
}

fn matches_pattern_set(kind: &RedactPatternKind, set: &RedactPatternSet) -> bool {
    match set {
        RedactPatternSet::Default | RedactPatternSet::AllBuiltins => {
            !matches!(kind, RedactPatternKind::Custom(_))
        }
        RedactPatternSet::Only(kinds) => kinds.iter().any(|candidate| candidate == kind),
        _ => false,
    }
}

fn default_patterns() -> Vec<RedactPattern> {
    let specs = [
        (
            "openai_api_key",
            RedactPatternKind::ApiKey,
            r"sk-[A-Za-z0-9_-]{20,}",
            RedactScope::All,
        ),
        (
            "anthropic_api_key",
            RedactPatternKind::ApiKey,
            r"sk-ant-[A-Za-z0-9_-]{20,}",
            RedactScope::All,
        ),
        (
            "github_token",
            RedactPatternKind::ApiKey,
            r"gh[pousr]_[A-Za-z0-9_]{20,}",
            RedactScope::All,
        ),
        (
            "slack_token",
            RedactPatternKind::ApiKey,
            r"xox[baprs]-[A-Za-z0-9-]{10,}",
            RedactScope::All,
        ),
        (
            "aws_access_key",
            RedactPatternKind::ApiKey,
            r"\b(A3T[A-Z0-9]|AKIA|ASIA)[A-Z0-9]{16}\b",
            RedactScope::All,
        ),
        (
            "google_api_key",
            RedactPatternKind::ApiKey,
            r"AIza[0-9A-Za-z_-]{35}",
            RedactScope::All,
        ),
        (
            "stripe_live_key",
            RedactPatternKind::ApiKey,
            r"sk_live_[0-9A-Za-z]{16,}",
            RedactScope::All,
        ),
        (
            "stripe_restricted_key",
            RedactPatternKind::ApiKey,
            r"rk_live_[0-9A-Za-z]{16,}",
            RedactScope::All,
        ),
        (
            "npm_token",
            RedactPatternKind::ApiKey,
            r"npm_[A-Za-z0-9]{20,}",
            RedactScope::All,
        ),
        (
            "linear_api_key",
            RedactPatternKind::ApiKey,
            r"lin_api_[A-Za-z0-9]{20,}",
            RedactScope::All,
        ),
        (
            "notion_token",
            RedactPatternKind::ApiKey,
            r"secret_[A-Za-z0-9]{20,}",
            RedactScope::All,
        ),
        (
            "bearer_token",
            RedactPatternKind::BearerToken,
            r"Bearer\s+[A-Za-z0-9._~+/=-]{16,}",
            RedactScope::All,
        ),
        (
            "basic_auth",
            RedactPatternKind::BearerToken,
            r"Basic\s+[A-Za-z0-9+/=]{16,}",
            RedactScope::All,
        ),
        (
            "jwt",
            RedactPatternKind::BearerToken,
            r"eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
            RedactScope::All,
        ),
        (
            "oauth_code",
            RedactPatternKind::OAuthCode,
            r"(?i)(code|oauth_code)=([A-Za-z0-9._~-]{12,})",
            RedactScope::All,
        ),
        (
            "oauth_refresh_token",
            RedactPatternKind::OAuthCode,
            r"(?i)refresh_token=([A-Za-z0-9._~-]{12,})",
            RedactScope::All,
        ),
        (
            "oauth_access_token",
            RedactPatternKind::OAuthCode,
            r"(?i)access_token=([A-Za-z0-9._~-]{12,})",
            RedactScope::All,
        ),
        (
            "ssh_private_key",
            RedactPatternKind::PrivateKey,
            r"-----BEGIN OPENSSH PRIVATE KEY-----[\s\S]*?-----END OPENSSH PRIVATE KEY-----",
            RedactScope::All,
        ),
        (
            "rsa_private_key",
            RedactPatternKind::PrivateKey,
            r"-----BEGIN RSA PRIVATE KEY-----[\s\S]*?-----END RSA PRIVATE KEY-----",
            RedactScope::All,
        ),
        (
            "ec_private_key",
            RedactPatternKind::PrivateKey,
            r"-----BEGIN EC PRIVATE KEY-----[\s\S]*?-----END EC PRIVATE KEY-----",
            RedactScope::All,
        ),
        (
            "generic_private_key",
            RedactPatternKind::PrivateKey,
            r"-----BEGIN PRIVATE KEY-----[\s\S]*?-----END PRIVATE KEY-----",
            RedactScope::All,
        ),
        (
            "postgres_url",
            RedactPatternKind::DatabaseUrl,
            r"postgres(?:ql)?://[^@\s]+@[^/\s]+/[^\s]+",
            RedactScope::All,
        ),
        (
            "mysql_url",
            RedactPatternKind::DatabaseUrl,
            r"mysql://[^@\s]+@[^/\s]+/[^\s]+",
            RedactScope::All,
        ),
        (
            "mongodb_url",
            RedactPatternKind::DatabaseUrl,
            r"mongodb(?:\+srv)?://[^@\s]+@[^/\s]+/[^\s]+",
            RedactScope::All,
        ),
        (
            "redis_url",
            RedactPatternKind::DatabaseUrl,
            r"redis://[^@\s]+@[^/\s]+",
            RedactScope::All,
        ),
        (
            "amqp_url",
            RedactPatternKind::DatabaseUrl,
            r"amqps?://[^@\s]+@[^/\s]+",
            RedactScope::All,
        ),
        (
            "private_ipv4_10",
            RedactPatternKind::PrivateIp,
            r"\b10(?:\.\d{1,3}){3}\b",
            RedactScope::TraceOnly,
        ),
        (
            "private_ipv4_172",
            RedactPatternKind::PrivateIp,
            r"\b172\.(?:1[6-9]|2\d|3[01])(?:\.\d{1,3}){2}\b",
            RedactScope::TraceOnly,
        ),
        (
            "private_ipv4_192",
            RedactPatternKind::PrivateIp,
            r"\b192\.168(?:\.\d{1,3}){2}\b",
            RedactScope::TraceOnly,
        ),
        (
            "loopback_ipv4",
            RedactPatternKind::PrivateIp,
            r"\b127(?:\.\d{1,3}){3}\b",
            RedactScope::TraceOnly,
        ),
        (
            "email",
            RedactPatternKind::Email,
            r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b",
            RedactScope::LogOnly,
        ),
        (
            "password_assignment",
            RedactPatternKind::ApiKey,
            r#"(?i)(password|passwd|pwd)\s*[:=]\s*["']?[^"'\s]{8,}"#,
            RedactScope::All,
        ),
        (
            "secret_assignment",
            RedactPatternKind::ApiKey,
            r#"(?i)(secret|client_secret)\s*[:=]\s*["']?[^"'\s]{8,}"#,
            RedactScope::All,
        ),
    ];

    specs
        .into_iter()
        .map(|(id, kind, pattern, scope)| {
            RedactPattern::new(
                id,
                kind,
                Regex::new(pattern).expect("default redactor regex compiles"),
                None,
                scope,
            )
        })
        .collect()
}
