//! Versioned local IPC for the daemon.

mod codec;
mod server;
#[cfg(unix)]
mod transport_unix;
#[cfg(windows)]
mod transport_windows;

pub use codec::*;
pub use server::*;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("IPC frame length must be non-zero")]
    ZeroLengthFrame,
    #[error("IPC frame exceeds the {MAX_FRAME_BYTES} byte limit")]
    FrameTooLarge,
    #[error("invalid IPC JSON frame: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("IPC I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("task store failed: {0}")]
    Store(#[from] harness_journal::TaskStoreError),
    #[error("IPC server task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

pub use harness_contracts::MAX_DAEMON_FRAME_BYTES as MAX_FRAME_BYTES;
