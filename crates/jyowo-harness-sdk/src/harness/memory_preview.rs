//! Model request preview.
//!
//! Generates a redacted, metadata-only preview of the memory context
//! that will be injected into the model request. Never exposes raw system
//! prompt or full memory content.

use harness_contracts::{
    ContentHash, GetModelRequestPreviewResponse, MemoryId, MemoryModelRequestPreview,
    MemoryModelRequestPreviewSection, MemorySource, MemoryTraceId, RunId, SessionId,
};

/// Build a redacted preview of the model request's memory sections.
#[derive(Debug, Default)]
pub struct ModelRequestPreviewBuilder {
    trace_id: Option<MemoryTraceId>,
    sections: Vec<MemoryModelRequestPreviewSection>,
    tool_names: Vec<String>,
    policy_decisions: Vec<String>,
}

impl ModelRequestPreviewBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_trace_id(mut self, trace_id: Option<MemoryTraceId>) -> Self {
        self.trace_id = trace_id;
        self
    }

    #[must_use]
    pub fn with_tool_names(mut self, tool_names: Vec<String>) -> Self {
        self.tool_names = tool_names;
        self.tool_names.sort();
        self.tool_names.dedup();
        self
    }

    #[must_use]
    pub fn with_policy_decisions(mut self, policy_decisions: Vec<String>) -> Self {
        self.policy_decisions = policy_decisions;
        self.policy_decisions.sort();
        self.policy_decisions.dedup();
        self
    }

    /// Add a memory section to the preview with redacted content.
    pub fn add_section(
        mut self,
        source: MemorySource,
        provider_id: Option<String>,
        memory_ids: Vec<MemoryId>,
        redacted_content: String,
    ) -> Self {
        self.sections.push(MemoryModelRequestPreviewSection {
            source,
            provider_id,
            memory_ids,
            redacted_content,
        });
        self
    }

    /// Build the final preview.
    #[must_use]
    pub fn build(self, session_id: SessionId, run_id: RunId) -> MemoryModelRequestPreview {
        let redacted_count = self.sections.len() as u32;
        let token_estimate = estimate_preview_tokens(&self.sections, &self.tool_names);
        let content_hash = compute_preview_hash(&self.sections);
        MemoryModelRequestPreview {
            session_id,
            run_id,
            trace_id: self.trace_id,
            sections: self.sections,
            redacted_count,
            token_estimate,
            tool_names: self.tool_names,
            policy_decisions: self.policy_decisions,
            content_hash,
        }
    }
}

pub(super) fn compute_preview_hash(sections: &[MemoryModelRequestPreviewSection]) -> ContentHash {
    let mut hasher = blake3::Hasher::new();
    for section in sections {
        hasher.update(format!("{:?}", section.source).as_bytes());
        hasher.update(section.redacted_content.as_bytes());
    }
    let hash = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(hash.as_bytes());
    ContentHash(bytes)
}

pub(super) fn estimate_preview_tokens(
    sections: &[MemoryModelRequestPreviewSection],
    tool_names: &[String],
) -> u64 {
    sections
        .iter()
        .map(|section| section.redacted_content.len() as u64)
        .chain(tool_names.iter().map(|name| name.len() as u64))
        .map(|chars| chars.saturating_add(3) / 4)
        .sum()
}

/// Generate a `GetModelRequestPreviewResponse` from the preview builder.
pub fn build_preview_response(
    session_id: SessionId,
    run_id: RunId,
    builder: ModelRequestPreviewBuilder,
) -> GetModelRequestPreviewResponse {
    GetModelRequestPreviewResponse {
        preview: builder.build(session_id, run_id),
    }
}
