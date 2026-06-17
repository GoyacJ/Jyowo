use harness_contracts::HarnessError;

#[derive(Debug, thiserror::Error)]
pub enum ToolLoadingError {
    #[error("reload handle missing: inline backend requires session.reload_with handle")]
    ReloadHandleMissing,
    #[error("requested tool not in deferred set: {0}")]
    NotInDeferredSet(String),
    #[error("backend internal: {0}")]
    Backend(String),
    #[error("coalescer closed")]
    CoalescerClosed,
    #[error("reload rejected: {0}")]
    ReloadRejected(String),
}

impl From<ToolLoadingError> for HarnessError {
    fn from(error: ToolLoadingError) -> Self {
        Self::Internal(error.to_string())
    }
}
