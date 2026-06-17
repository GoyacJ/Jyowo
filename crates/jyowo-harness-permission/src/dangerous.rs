use harness_contracts::{
    default_unix_dangerous_pattern_specs, default_windows_dangerous_pattern_specs,
    DangerousPatternSpec, Severity, ShellKind,
};
use regex::{Regex, RegexBuilder};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum DangerousPatternKind {
    Command,
    Path,
    Url,
}

#[derive(Debug, Clone)]
pub struct DangerousPatternLibrary {
    patterns: Vec<DangerousPatternRule>,
}

#[derive(Debug, Clone)]
pub struct DangerousPatternRule {
    pub id: String,
    pub pattern: Regex,
    pub severity: Severity,
    pub description: String,
    pub kind: DangerousPatternKind,
}

impl DangerousPatternLibrary {
    pub fn default_unix() -> Self {
        let mut patterns = compile_patterns(default_unix_dangerous_pattern_specs());
        patterns.extend(compile_local_patterns(
            DangerousPatternKind::Path,
            UNIX_PATH_DANGEROUS_PATTERNS,
        ));
        patterns.extend(common_url_patterns());
        Self { patterns }
    }

    pub fn default_windows() -> Self {
        let mut patterns = compile_patterns(default_windows_dangerous_pattern_specs());
        patterns.extend(compile_local_patterns(
            DangerousPatternKind::Path,
            WINDOWS_PATH_DANGEROUS_PATTERNS,
        ));
        patterns.extend(common_url_patterns());
        Self { patterns }
    }

    pub fn default_all() -> Self {
        let mut patterns = compile_patterns(default_unix_dangerous_pattern_specs());
        patterns.extend(compile_patterns(default_windows_dangerous_pattern_specs()));
        patterns.extend(compile_local_patterns(
            DangerousPatternKind::Path,
            UNIX_PATH_DANGEROUS_PATTERNS,
        ));
        patterns.extend(compile_local_patterns(
            DangerousPatternKind::Path,
            WINDOWS_PATH_DANGEROUS_PATTERNS,
        ));
        patterns.extend(common_url_patterns());
        Self { patterns }
    }

    pub fn for_shell_kind(shell_kind: ShellKind) -> Self {
        match shell_kind {
            ShellKind::Bash(_) | ShellKind::Zsh(_) => Self::default_unix(),
            ShellKind::PowerShell => Self::default_windows(),
            ShellKind::System | _ => Self::default_all(),
        }
    }

    pub fn detect(&self, command: &str) -> Option<&DangerousPatternRule> {
        self.detect_command(command)
    }

    pub fn detect_command(&self, command: &str) -> Option<&DangerousPatternRule> {
        self.detect_normalized(DangerousPatternKind::Command, command)
    }

    pub fn detect_path(&self, path: impl AsRef<str>) -> Option<&DangerousPatternRule> {
        self.detect_normalized(DangerousPatternKind::Path, path.as_ref())
    }

    pub fn detect_url(&self, url: impl AsRef<str>) -> Option<&DangerousPatternRule> {
        self.detect_normalized(DangerousPatternKind::Url, url.as_ref())
    }

    fn detect_normalized(
        &self,
        kind: DangerousPatternKind,
        value: &str,
    ) -> Option<&DangerousPatternRule> {
        let normalized = normalize_for_detection(value);
        self.patterns
            .iter()
            .filter(|rule| rule.kind == kind)
            .find(|rule| rule.pattern.is_match(&normalized))
    }

    pub fn patterns(&self) -> &[DangerousPatternRule] {
        &self.patterns
    }
}

fn compile_patterns(specs: &[DangerousPatternSpec]) -> Vec<DangerousPatternRule> {
    specs
        .iter()
        .map(|spec| DangerousPatternRule {
            id: spec.id.to_owned(),
            pattern: RegexBuilder::new(spec.pattern)
                .case_insensitive(true)
                .build()
                .expect("builtin dangerous pattern should compile"),
            severity: spec.severity,
            description: spec.description.to_owned(),
            kind: DangerousPatternKind::Command,
        })
        .collect()
}

fn compile_local_patterns(
    kind: DangerousPatternKind,
    specs: &[LocalDangerousPatternSpec],
) -> Vec<DangerousPatternRule> {
    specs
        .iter()
        .map(|spec| DangerousPatternRule {
            id: spec.id.to_owned(),
            pattern: RegexBuilder::new(spec.pattern)
                .case_insensitive(true)
                .build()
                .expect("builtin dangerous pattern should compile"),
            severity: spec.severity,
            description: spec.description.to_owned(),
            kind,
        })
        .collect()
}

fn common_url_patterns() -> Vec<DangerousPatternRule> {
    compile_local_patterns(DangerousPatternKind::Url, URL_DANGEROUS_PATTERNS)
}

fn normalize_for_detection(value: &str) -> String {
    let stripped = strip_ansi_escapes::strip_str(value);
    stripped.nfkc().collect()
}

#[derive(Debug, Clone, Copy)]
struct LocalDangerousPatternSpec {
    id: &'static str,
    pattern: &'static str,
    severity: Severity,
    description: &'static str,
}

const UNIX_PATH_DANGEROUS_PATTERNS: &[LocalDangerousPatternSpec] = &[
    local_spec(
        "path-unix-system-auth-db",
        r"(^|/)(etc/(passwd|shadow|sudoers)|etc/sudoers\.d/[^/]+)$",
        Severity::Critical,
        "Unix account or sudoers database path",
    ),
    local_spec(
        "path-unix-ssh-credential",
        r"(^|/)\.ssh/(id_[a-z0-9_]+|authorized_keys|known_hosts)$",
        Severity::High,
        "SSH credential or trust database path",
    ),
];

const WINDOWS_PATH_DANGEROUS_PATTERNS: &[LocalDangerousPatternSpec] = &[local_spec(
    "path-windows-system32",
    r"^[a-z]:\\windows\\system32\\",
    Severity::Critical,
    "Windows System32 path",
)];

const URL_DANGEROUS_PATTERNS: &[LocalDangerousPatternSpec] = &[
    local_spec(
        "url-cloud-metadata",
        r"^https?://(169\.254\.169\.254|metadata\.google\.internal|100\.100\.100\.200)([:/]|$)",
        Severity::High,
        "cloud instance metadata service URL",
    ),
    local_spec(
        "url-local-loopback",
        r"^https?://(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])([:/]|$)",
        Severity::Medium,
        "local loopback URL",
    ),
];

const fn local_spec(
    id: &'static str,
    pattern: &'static str,
    severity: Severity,
    description: &'static str,
) -> LocalDangerousPatternSpec {
    LocalDangerousPatternSpec {
        id,
        pattern,
        severity,
        description,
    }
}
