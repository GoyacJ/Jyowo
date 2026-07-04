//! Model request preview.
//!
//! Generates a redacted, metadata-only preview of the memory context
//! that will be injected into the model request. Never exposes raw system
//! prompt or full memory content.

use harness_contracts::{
    ContentHash, GetModelRequestPreviewResponse, MemoryId, MemoryModelRequestPreview,
    MemoryModelRequestPreviewSection, MemoryRecallTraceSummary, MemorySource, RunId, SessionId,
};

/// Build a redacted preview of the model request's memory sections.
#[derive(Debug, Default)]
pub struct ModelRequestPreviewBuilder {
    sections: Vec<MemoryModelRequestPreviewSection>,
}

impl ModelRequestPreviewBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
        let content_hash = compute_preview_hash(&self.sections);
        MemoryModelRequestPreview {
            session_id,
            run_id,
            sections: self.sections,
            redacted_count,
            content_hash,
        }
    }
}

fn compute_preview_hash(sections: &[MemoryModelRequestPreviewSection]) -> ContentHash {
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
