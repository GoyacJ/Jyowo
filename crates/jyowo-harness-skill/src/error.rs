use harness_contracts::{ThreatCategory, TrustLevel};

use crate::SkillPlatform;

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("parse frontmatter: {0}")]
    ParseFrontmatter(String),
    #[error("missing required parameter: {0}")]
    MissingParam(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("duplicate name: {0}")]
    Duplicate(String),
    #[error("threat detected: pattern={pattern_id} category={category:?}")]
    ThreatDetected {
        pattern_id: String,
        category: ThreatCategory,
    },
    #[error("platform mismatch: required={required:?}")]
    PlatformMismatch { required: Vec<SkillPlatform> },
    #[error("hook transport not permitted for trust={trust:?}")]
    HookTransportNotPermitted { trust: TrustLevel },
    #[error("name too long: {0} > 64")]
    NameTooLong(usize),
    #[error("description too long: {0} > 1024")]
    DescriptionTooLong(usize),
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("missing required parameter: {0}")]
    MissingParam(String),
    #[error("invalid parameter `{name}`: expected {expected}")]
    InvalidParam {
        name: String,
        expected: &'static str,
    },
    #[error("unknown config key: {0}")]
    UnknownConfigKey(String),
    #[error("config resolve: {0}")]
    ConfigResolve(#[from] ConfigResolveError),
    #[error("shell not allowed: {0}")]
    ShellNotAllowed(String),
    #[error("shell exec: {0}")]
    ShellExec(#[from] std::io::Error),
    #[error("skill not visible: {0}")]
    SkillNotVisible(String),
    #[error("skill `{skill_id}` is missing required config: {config_keys:?}")]
    MissingConfig {
        skill_id: String,
        config_keys: Vec<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigResolveError {
    #[error("unknown config key: {0}")]
    UnknownKey(String),
    #[error("skill config resolver is bound to `{expected_skill_id}`, not `{actual_skill_id}`")]
    SkillIdentityMismatch {
        expected_skill_id: String,
        actual_skill_id: String,
    },
    #[error("skill `{skill_id}` is missing required config `{key}`")]
    MissingRequiredConfig { skill_id: String, key: String },
    #[error("secret config `{key}` for skill `{skill_id}` cannot be interpolated")]
    SecretInterpolationForbidden { skill_id: String, key: String },
    #[error("invalid config `{key}` for skill `{skill_id}`: expected {expected}")]
    InvalidType {
        skill_id: String,
        key: String,
        expected: &'static str,
    },
    #[error("{0}")]
    Message(String),
}
