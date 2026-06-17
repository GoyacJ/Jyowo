//! Current-working-directory side-channel marker protocol.

use std::path::PathBuf;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum CwdChannel {
    SideFd { fd: i32 },
    NamedPipe { name: String },
    Disabled,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct CwdMarkerLine {
    pub sequence: u64,
    pub cwd: PathBuf,
    pub at: DateTime<Utc>,
}
