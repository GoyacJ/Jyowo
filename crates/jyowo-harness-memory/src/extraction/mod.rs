//! Extraction and consolidation worker.
//!
//! Delayed memory generation that runs after sessions end or are idle.
//! Respects policy for active sessions, external context, and quota.
//! Creates inbox candidates, not direct long-term records.

pub mod job;
pub mod schema;
pub mod worker;

pub use job::{
    ExtractionJob, ExtractionJobConfig, ExtractionJobKind, ExtractionJobQueue, ExtractionJobState,
    JobId,
};
pub use schema::{
    ExtractedCandidate, ExtractedConsolidation, ExtractionMemoryKind, ExtractionOutput,
    ExtractionVisibility,
};
pub use worker::{ExtractionRunOutcome, ExtractionWorker, ExtractionWorkerConfig};
