//! Local process sandbox backend.

mod exec;

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use harness_contracts::ShellKind;

use crate::{EventSink, SandboxBaseConfig};

pub use exec::LocalActivity;

#[derive(Clone)]
pub struct LocalSandbox {
    pub(crate) base: SandboxBaseConfig,
    pub(crate) root: PathBuf,
    pub(crate) shell: ShellKind,
    pub(crate) isolation: LocalIsolation,
    pub(crate) snapshot_event_sink: Option<Arc<dyn EventSink>>,
}

impl LocalSandbox {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_base(root, SandboxBaseConfig::default())
    }

    pub fn with_base(root: impl Into<PathBuf>, base: SandboxBaseConfig) -> Self {
        Self {
            base,
            root: root.into(),
            shell: ShellKind::System,
            isolation: LocalIsolation::None,
            snapshot_event_sink: None,
        }
    }

    #[must_use]
    pub fn with_shell(mut self, shell: ShellKind) -> Self {
        self.shell = shell;
        self
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn base(&self) -> &SandboxBaseConfig {
        &self.base
    }

    pub fn isolation(&self) -> LocalIsolation {
        self.isolation
    }

    #[must_use]
    pub fn with_isolation(mut self, isolation: LocalIsolation) -> Self {
        self.isolation = isolation;
        self
    }

    #[must_use]
    pub fn with_snapshot_event_sink(mut self, event_sink: Arc<dyn EventSink>) -> Self {
        self.snapshot_event_sink = Some(event_sink);
        self
    }
}

impl fmt::Debug for LocalSandbox {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalSandbox")
            .field("base", &self.base)
            .field("root", &self.root)
            .field("shell", &self.shell)
            .field("isolation", &self.isolation)
            .field("snapshot_event_sink", &self.snapshot_event_sink.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum LocalIsolation {
    None,
    Bubblewrap,
    Seatbelt,
    JobObject,
}

impl LocalIsolation {
    pub fn for_current_platform() -> Self {
        if cfg!(target_os = "linux") {
            Self::Bubblewrap
        } else if cfg!(target_os = "macos") {
            Self::Seatbelt
        } else if cfg!(target_os = "windows") {
            Self::JobObject
        } else {
            Self::None
        }
    }

    pub(crate) fn is_os_level(self) -> bool {
        !matches!(self, Self::None)
    }
}
