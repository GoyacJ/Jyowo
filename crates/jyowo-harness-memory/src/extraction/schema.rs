//! Typed schema for extraction model output.
//!
//! The extraction model produces structured output that must parse into
//! this schema. Unparsable output is a retryable failure.

use harness_contracts::{MemoryId, MemoryKind};
use serde::{Deserialize, Serialize};

/// Model output from an extraction run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractionOutput {
    /// New memory candidates discovered.
    #[serde(default)]
    pub candidates: Vec<ExtractedCandidate>,
    /// IDs of existing memories to update/consolidate.
    #[serde(default)]
    pub consolidations: Vec<ExtractedConsolidation>,
    /// Summary of the session (may be used for indexing).
    #[serde(default)]
    pub summary: Option<String>,
}

/// A candidate memory entry extracted from conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedCandidate {
    pub kind: ExtractionMemoryKind,
    pub visibility: ExtractionVisibility,
    pub content: String,
    #[serde(default)]
    pub confidence: f32,
}

/// Memory kind as reported by the extraction model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionMemoryKind {
    ProjectFact,
    UserPreference,
    Reference,
    Feedback,
    AgentSelfNote,
}

impl From<ExtractionMemoryKind> for MemoryKind {
    fn from(k: ExtractionMemoryKind) -> Self {
        match k {
            ExtractionMemoryKind::ProjectFact => MemoryKind::ProjectFact,
            ExtractionMemoryKind::UserPreference => MemoryKind::UserPreference,
            ExtractionMemoryKind::Reference => MemoryKind::Reference,
            ExtractionMemoryKind::Feedback => MemoryKind::Feedback,
            ExtractionMemoryKind::AgentSelfNote => MemoryKind::AgentSelfNote,
        }
    }
}

/// Visibility as reported by the extraction model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionVisibility {
    User,
    Tenant,
}

/// A consolidation action for existing memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedConsolidation {
    /// ID of the memory to update.
    pub memory_id: MemoryId,
    /// Consolidation action requested by the extractor.
    #[serde(default)]
    pub action: ExtractedConsolidationAction,
    /// New content (replaces existing).
    pub content: String,
    /// Reason for consolidation.
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractedConsolidationAction {
    #[default]
    Merge,
    Demote,
    Expire,
}
