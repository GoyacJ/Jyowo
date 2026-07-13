use std::path::PathBuf;

use harness_contracts::{ThreatCategory, TrustLevel};

use crate::{SkillError, SkillPlatform, SkillSource};

#[derive(Debug, Clone, PartialEq)]
pub struct SkillRejection {
    pub source: SkillSource,
    pub raw_path: Option<PathBuf>,
    pub reason: SkillRejectReason,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SkillRejectReason {
    ParseFrontmatter(String),
    PlatformMismatch {
        required: Vec<SkillPlatform>,
    },
    ThreatDetected {
        pattern_id: String,
        category: ThreatCategory,
    },
    NameTooLong(usize),
    DescriptionTooLong(usize),
    HookTransportNotPermitted {
        trust: TrustLevel,
    },
    Duplicate,
    Io(String),
}

impl SkillRejectReason {
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::ParseFrontmatter(_) => "parse_frontmatter",
            Self::PlatformMismatch { .. } => "platform_mismatch",
            Self::ThreatDetected { .. } => "threat_detected",
            Self::NameTooLong(_) => "name_too_long",
            Self::DescriptionTooLong(_) => "description_too_long",
            Self::HookTransportNotPermitted { .. } => "hook_transport_not_permitted",
            Self::Duplicate => "duplicate",
            Self::Io(_) => "io",
        }
    }

    #[must_use]
    pub fn from_error(error: &SkillError) -> Self {
        match error {
            SkillError::ParseFrontmatter(message)
            | SkillError::InvalidScriptDeclaration(message) => {
                Self::ParseFrontmatter(message.clone())
            }
            SkillError::PlatformMismatch { required } => Self::PlatformMismatch {
                required: required.clone(),
            },
            SkillError::ThreatDetected {
                pattern_id,
                category,
            } => Self::ThreatDetected {
                pattern_id: pattern_id.clone(),
                category: *category,
            },
            SkillError::NameTooLong(size) => Self::NameTooLong(*size),
            SkillError::DescriptionTooLong(size) => Self::DescriptionTooLong(*size),
            SkillError::HookTransportNotPermitted { trust } => {
                Self::HookTransportNotPermitted { trust: *trust }
            }
            SkillError::Duplicate(_) => Self::Duplicate,
            SkillError::MissingParam(name) => {
                Self::ParseFrontmatter(format!("missing param: {name}"))
            }
            SkillError::Io(error) => Self::Io(error.to_string()),
        }
    }
}
