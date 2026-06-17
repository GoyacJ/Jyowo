use harness_contracts::JournalError;

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ObservabilityError {
    #[error("tracer init: {0}")]
    TracerInit(String),
    #[error("exporter: {0}")]
    Exporter(String),
    #[error("replay: {0}")]
    Replay(String),
    #[cfg(feature = "redactor")]
    #[error("redact regex: {0}")]
    Regex(#[from] regex::Error),
    #[error("journal: {0}")]
    Journal(#[from] JournalError),
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum TraceError {
    #[error("flush: {0}")]
    Flush(String),
}
