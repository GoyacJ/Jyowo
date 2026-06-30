use super::*;

impl Harness {
    pub(super) fn hook_redactor(&self) -> Arc<dyn Redactor> {
        self.inner
            .observer
            .as_ref()
            .map(|observer| Arc::clone(&observer.redactor))
            .unwrap_or_else(default_hook_redactor)
    }
}

#[cfg(feature = "agents-team")]
pub(super) fn topology_kind(topology: harness_team::Topology) -> TopologyKind {
    match topology {
        harness_team::Topology::CoordinatorWorker => TopologyKind::CoordinatorWorker,
        harness_team::Topology::PeerToPeer => TopologyKind::PeerToPeer,
        harness_team::Topology::RoleRouted => TopologyKind::RoleRouted,
        harness_team::Topology::Custom => TopologyKind::Custom("sdk".to_owned()),
    }
}

fn redact_business_event(event: Event, redactor: &dyn Redactor) -> Event {
    let Ok(mut value) = serde_json::to_value(&event) else {
        return event;
    };
    redact_json_strings(&mut value, redactor);
    serde_json::from_value(value).unwrap_or(event)
}

pub(super) fn redact_business_event_for_display(event: Event, redactor: &dyn Redactor) -> Event {
    let event = redact_business_event(event, redactor);
    let default_redactor = default_hook_redactor();
    redact_business_event(event, default_redactor.as_ref())
}

fn redact_json_strings(value: &mut Value, redactor: &dyn Redactor) {
    match value {
        Value::String(text) => {
            *text = redactor.redact(text, &business_event_redact_rules());
        }
        Value::Array(items) => {
            for item in items {
                redact_json_strings(item, redactor);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                redact_json_strings(item, redactor);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn business_event_redact_rules() -> RedactRules {
    RedactRules {
        scope: RedactScope::EventBody,
        replacement: "[REDACTED]".to_owned(),
        pattern_set: RedactPatternSet::Default,
    }
}

#[cfg(feature = "observability-redactor")]
fn default_hook_redactor() -> Arc<dyn Redactor> {
    Arc::new(DefaultRedactor::default())
}

#[cfg(not(feature = "observability-redactor"))]
fn default_hook_redactor() -> Arc<dyn Redactor> {
    Arc::new(MinimalHookRedactor)
}

#[cfg(not(feature = "observability-redactor"))]
struct MinimalHookRedactor;

#[cfg(not(feature = "observability-redactor"))]
impl Redactor for MinimalHookRedactor {
    fn redact(&self, input: &str, rules: &RedactRules) -> String {
        if matches!(rules.pattern_set, RedactPatternSet::None) {
            return input.to_owned();
        }
        let mut output = input.to_owned();
        if minimal_pattern_enabled(RedactScope::All, RedactPatternKind::PrivateKey, rules) {
            output = redact_private_key_blocks(&output, &rules.replacement);
        }
        if minimal_pattern_enabled(RedactScope::All, RedactPatternKind::ApiKey, rules) {
            for prefix in [
                "sk-ant-",
                "sk-",
                "ghp_",
                "gho_",
                "ghu_",
                "ghs_",
                "ghr_",
                "xoxb-",
                "xoxp_",
                "xoxp-",
                "xoxa-",
                "xoxr-",
                "xoxs-",
                "github_pat_",
                "npm_",
                "lin_api_",
                "secret_",
                "sk_live_",
                "rk_live_",
            ] {
                output = redact_prefixed_tokens(&output, prefix, &rules.replacement);
            }
            for (prefix, min_len) in [("AKIA", 16), ("ASIA", 16), ("A3T", 17), ("AIza", 35)] {
                output = redact_prefixed_tokens_min(&output, prefix, min_len, &rules.replacement);
            }
            output = redact_secret_assignments(
                &output,
                &["password", "passwd", "pwd", "secret", "client_secret"],
                &rules.replacement,
            );
        }
        if minimal_pattern_enabled(RedactScope::All, RedactPatternKind::BearerToken, rules) {
            output = redact_auth_scheme_tokens(&output, "Bearer", &rules.replacement);
            output = redact_auth_scheme_tokens(&output, "Basic", &rules.replacement);
            output = redact_jwt_like_tokens(&output, &rules.replacement);
        }
        if minimal_pattern_enabled(RedactScope::All, RedactPatternKind::OAuthCode, rules) {
            output = redact_secret_assignments(
                &output,
                &["code", "oauth_code", "refresh_token", "access_token"],
                &rules.replacement,
            );
        }
        if minimal_pattern_enabled(RedactScope::All, RedactPatternKind::DatabaseUrl, rules) {
            output = redact_database_urls(&output, &rules.replacement);
        }
        if minimal_pattern_enabled(RedactScope::TraceOnly, RedactPatternKind::PrivateIp, rules) {
            output = redact_private_ip_addresses(&output, &rules.replacement);
        }
        if minimal_pattern_enabled(RedactScope::LogOnly, RedactPatternKind::Email, rules) {
            output = redact_email_addresses(&output, &rules.replacement);
        }
        if minimal_default_event_body_patterns_enabled(rules) {
            output = redact_private_absolute_paths(&output, &rules.replacement);
        }
        output
    }
}

#[cfg(not(feature = "observability-redactor"))]
fn minimal_default_event_body_patterns_enabled(rules: &RedactRules) -> bool {
    let scope_matches = matches!(rules.scope, RedactScope::All | RedactScope::EventBody);
    let pattern_matches = matches!(
        rules.pattern_set,
        RedactPatternSet::Default | RedactPatternSet::AllBuiltins
    );
    scope_matches && pattern_matches
}

#[cfg(not(feature = "observability-redactor"))]
fn minimal_pattern_enabled(
    pattern_scope: RedactScope,
    kind: RedactPatternKind,
    rules: &RedactRules,
) -> bool {
    let scope_matches = matches!(rules.scope, RedactScope::All)
        || matches!(pattern_scope, RedactScope::All)
        || pattern_scope == rules.scope;
    if !scope_matches {
        return false;
    }
    match &rules.pattern_set {
        RedactPatternSet::Default | RedactPatternSet::AllBuiltins => true,
        RedactPatternSet::Only(kinds) => kinds.iter().any(|candidate| candidate == &kind),
        RedactPatternSet::None => false,
        _ => false,
    }
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_prefixed_tokens(input: &str, prefix: &str, replacement: &str) -> String {
    redact_prefixed_tokens_min(input, prefix, 1, replacement)
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_auth_scheme_tokens(input: &str, scheme: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let remaining = &input[cursor..];
        if remaining
            .get(..scheme.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(scheme))
        {
            let mut offset = scheme.len();
            let whitespace_len = ascii_whitespace_prefix_len(&remaining[offset..]);
            if whitespace_len > 0 {
                offset += whitespace_len;
                let token_len = remaining[offset..]
                    .char_indices()
                    .take_while(|(_, ch)| {
                        ch.is_ascii_alphanumeric()
                            || matches!(*ch, '_' | '-' | '.' | '~' | '+' | '/' | '=')
                    })
                    .last()
                    .map_or(0, |(index, ch)| index + ch.len_utf8());
                if token_len > 0 {
                    output.push_str(replacement);
                    cursor += offset + token_len;
                    continue;
                }
            }
        }
        let ch = remaining
            .chars()
            .next()
            .expect("cursor should point to char boundary");
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_prefixed_tokens_min(
    input: &str,
    prefix: &str,
    min_token_len: usize,
    replacement: &str,
) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let remaining = &input[cursor..];
        if remaining
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        {
            let token_len = remaining[prefix.len()..]
                .char_indices()
                .take_while(|(_, ch)| {
                    ch.is_ascii_alphanumeric()
                        || matches!(*ch, '_' | '-' | '.' | '~' | '+' | '/' | '=')
                })
                .last()
                .map_or(0, |(index, ch)| index + ch.len_utf8());
            if token_len >= min_token_len {
                output.push_str(replacement);
                cursor += prefix.len() + token_len;
                continue;
            }
        }
        let ch = remaining
            .chars()
            .next()
            .expect("cursor should point to char boundary");
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_private_key_blocks(input: &str, replacement: &str) -> String {
    let mut output = input.to_owned();
    for (begin, end) in [
        (
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "-----END OPENSSH PRIVATE KEY-----",
        ),
        (
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----END RSA PRIVATE KEY-----",
        ),
        (
            "-----BEGIN EC PRIVATE KEY-----",
            "-----END EC PRIVATE KEY-----",
        ),
        ("-----BEGIN PRIVATE KEY-----", "-----END PRIVATE KEY-----"),
    ] {
        while let Some(start) = output.find(begin) {
            let end_index = output[start..]
                .find(end)
                .map_or(output.len(), |relative_end| {
                    start + relative_end + end.len()
                });
            output.replace_range(start..end_index, replacement);
        }
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_jwt_like_tokens(input: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let remaining = &input[cursor..];
        if remaining.starts_with("eyJ") {
            let token_len = remaining
                .char_indices()
                .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || matches!(*ch, '_' | '-' | '.'))
                .last()
                .map_or(0, |(index, ch)| index + ch.len_utf8());
            let token = &remaining[..token_len];
            let parts = token.split('.').collect::<Vec<_>>();
            if parts.len() >= 3 && parts[0].len() >= 8 && parts[1].len() >= 8 && parts[2].len() >= 8
            {
                output.push_str(replacement);
                cursor += token_len;
                continue;
            }
        }
        let ch = remaining
            .chars()
            .next()
            .expect("cursor should point to char boundary");
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_secret_assignments(input: &str, names: &[&str], replacement: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    'scan: while cursor < input.len() {
        let remaining = &input[cursor..];
        for name in names.iter().copied() {
            if let Some(match_len) = assignment_match_len(remaining, name) {
                output.push_str(replacement);
                cursor += match_len;
                continue 'scan;
            }
        }
        let ch = remaining
            .chars()
            .next()
            .expect("cursor should point to char boundary");
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_private_ip_addresses(input: &str, replacement: &str) -> String {
    replace_matching_tokens(input, replacement, is_private_ipv4)
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_email_addresses(input: &str, replacement: &str) -> String {
    replace_matching_tokens(input, replacement, is_email_like)
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_private_absolute_paths(input: &str, replacement: &str) -> String {
    replace_matching_tokens(input, replacement, is_private_absolute_path_like)
}

#[cfg(not(feature = "observability-redactor"))]
fn replace_matching_tokens(
    input: &str,
    replacement: &str,
    matches_token: impl Fn(&str) -> bool,
) -> String {
    let mut output = String::with_capacity(input.len());
    let mut token_start: Option<usize> = None;
    for (index, ch) in input.char_indices() {
        if ch.is_ascii_whitespace() {
            if let Some(start) = token_start.take() {
                let token = &input[start..index];
                if matches_token(token) {
                    output.push_str(replacement);
                } else {
                    output.push_str(token);
                }
            }
            output.push(ch);
        } else if token_start.is_none() {
            token_start = Some(index);
        }
    }
    if let Some(start) = token_start {
        let token = &input[start..];
        if matches_token(token) {
            output.push_str(replacement);
        } else {
            output.push_str(token);
        }
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn is_private_ipv4(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | ':' | ')' | '(' | '.' | '[' | ']' | '<' | '>' | '"' | '\''
        )
    });
    let octets = token
        .split('.')
        .map(str::parse::<u8>)
        .collect::<Result<Vec<_>, _>>();
    let Ok(octets) = octets else {
        return false;
    };
    if octets.len() != 4 {
        return false;
    }
    matches!(
        octets.as_slice(),
        [10, _, _, _] | [172, 16..=31, _, _] | [192, 168, _, _] | [127, _, _, _]
    )
}

#[cfg(not(feature = "observability-redactor"))]
fn is_email_like(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | ':' | ')' | '(' | '.' | '[' | ']' | '<' | '>' | '"' | '\''
        )
    });
    let Some((local, domain)) = token.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain
            .rsplit_once('.')
            .is_some_and(|(name, tld)| !name.is_empty() && tld.len() >= 2)
}

#[cfg(not(feature = "observability-redactor"))]
fn is_private_absolute_path_like(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | ':' | ')' | '(' | '[' | ']' | '<' | '>' | '"' | '\''
        )
    });
    token.contains("/Users/")
        || token.contains("/home/")
        || token.contains("/private/var/")
        || token.as_bytes().windows(3).any(|window| {
            window[0].is_ascii_alphabetic()
                && window[1] == b':'
                && matches!(window[2], b'\\' | b'/')
        })
}

#[cfg(not(feature = "observability-redactor"))]
fn assignment_match_len(input: &str, name: &str) -> Option<usize> {
    if !input
        .get(..name.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        return None;
    }

    let mut offset = name.len();
    offset += ascii_whitespace_prefix_len(&input[offset..]);
    let delimiter = input[offset..].chars().next()?;
    if !matches!(delimiter, ':' | '=') {
        return None;
    }
    offset += delimiter.len_utf8();
    offset += ascii_whitespace_prefix_len(&input[offset..]);

    let quote = input[offset..]
        .chars()
        .next()
        .filter(|ch| matches!(*ch, '"' | '\''));
    if let Some(quote) = quote {
        offset += quote.len_utf8();
    }

    let value_len = input[offset..]
        .char_indices()
        .take_while(|(_, ch)| match quote {
            Some(quote) => *ch != quote,
            None => !ch.is_ascii_whitespace() && !matches!(*ch, '"' | '\''),
        })
        .last()
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    if value_len < 8 {
        return None;
    }
    Some(offset + value_len)
}

#[cfg(not(feature = "observability-redactor"))]
fn ascii_whitespace_prefix_len(input: &str) -> usize {
    input
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_whitespace())
        .last()
        .map_or(0, |(index, ch)| index + ch.len_utf8())
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_database_urls(input: &str, replacement: &str) -> String {
    let schemes = [
        "postgres://",
        "postgresql://",
        "mysql://",
        "mongodb://",
        "mongodb+srv://",
        "redis://",
        "amqp://",
        "amqps://",
    ];
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    'scan: while cursor < input.len() {
        let remaining = &input[cursor..];
        for scheme in schemes {
            if remaining
                .get(..scheme.len())
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(scheme))
            {
                let url_len = remaining
                    .char_indices()
                    .take_while(|(_, ch)| !ch.is_ascii_whitespace())
                    .last()
                    .map_or(0, |(index, ch)| index + ch.len_utf8());
                let url = &remaining[..url_len];
                if url.contains('@') {
                    output.push_str(replacement);
                    cursor += url_len;
                    continue 'scan;
                }
            }
        }
        let ch = remaining
            .chars()
            .next()
            .expect("cursor should point to char boundary");
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

#[cfg(all(test, not(feature = "observability-redactor")))]
mod tests {
    use super::*;

    #[test]
    fn default_hook_redactor_redacts_without_observability_redactor_feature() {
        let redactor = default_hook_redactor();
        let redacted = redactor.redact(
            "token sk-abcdefghijklmnopqrstuvwxyz and ghp_abcdefghijklmnopqrstuvwxyz \
             bearer synthetic-token Basic synthetic-basic \
             jwt eyJabcdefgh.eyJijklmnop.eyJqrstuvwx \
             bearer\twhitespace-token \
             db postgres://user:password@example.com/app \
             paths /Users/goya/.ssh/config C:/Users/goya/.ssh/config \
             password=supersecret client_secret: verysecretvalue \
             google AIzaabcdefghijklmnopqrstuvwxyz123456789 \
             stripe rk_live_abcdefghijklmnop \
             -----BEGIN OPENSSH PRIVATE KEY-----truncated",
            &RedactRules::default(),
        );

        assert!(!redacted.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(!redacted.contains("ghp_abcdefghijklmnopqrstuvwxyz"));
        assert!(!redacted.contains("synthetic-token"));
        assert!(!redacted.contains("synthetic-basic"));
        assert!(!redacted.contains("whitespace-token"));
        assert!(!redacted.contains("eyJabcdefgh.eyJijklmnop.eyJqrstuvwx"));
        assert!(!redacted.contains("postgres://user:password@example.com/app"));
        assert!(!redacted.contains("/Users/goya/.ssh/config"));
        assert!(!redacted.contains("C:/Users/goya/.ssh/config"));
        assert!(!redacted.contains("supersecret"));
        assert!(!redacted.contains("verysecretvalue"));
        assert!(!redacted.contains("AIzaabcdefghijklmnopqrstuvwxyz123456789"));
        assert!(!redacted.contains("rk_live_abcdefghijklmnop"));
        assert!(!redacted.contains("BEGIN OPENSSH PRIVATE KEY"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn default_hook_redactor_honors_rules_without_observability_redactor_feature() {
        let redactor = default_hook_redactor();
        let only_database_url = redactor.redact(
            "token sk-abcdefghijklmnopqrstuvwxyz db postgres://user:password@example.com/app",
            &RedactRules {
                pattern_set: RedactPatternSet::Only(vec![RedactPatternKind::DatabaseUrl]),
                ..RedactRules::default()
            },
        );

        assert!(only_database_url.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(!only_database_url.contains("postgres://user:password@example.com/app"));

        let event_body = redactor.redact(
            "email user@example.com ip 10.1.2.3",
            &RedactRules::default(),
        );
        assert!(event_body.contains("user@example.com"));
        assert!(event_body.contains("10.1.2.3"));

        let log_only = redactor.redact(
            "email <user@example.com> ip [10.1.2.3]",
            &RedactRules {
                scope: RedactScope::LogOnly,
                ..RedactRules::default()
            },
        );
        assert!(!log_only.contains("user@example.com"));
        assert!(log_only.contains("10.1.2.3"));

        let trace_only = redactor.redact(
            "email \"user@example.com\" ip [10.1.2.3]",
            &RedactRules {
                scope: RedactScope::TraceOnly,
                ..RedactRules::default()
            },
        );
        assert!(trace_only.contains("user@example.com"));
        assert!(!trace_only.contains("10.1.2.3"));
    }
}
