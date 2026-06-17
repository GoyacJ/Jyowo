//! Command wrapper shared by process-style sandbox backends.

use futures::stream::BoxStream;
use tokio::process::Command;

use crate::cwd::CwdMarkerLine;

pub struct WrappedCommand {
    command: Command,
    cwd_marker: Option<BoxStream<'static, CwdMarkerLine>>,
}

impl WrappedCommand {
    pub fn new(command: Command, cwd_marker: Option<BoxStream<'static, CwdMarkerLine>>) -> Self {
        Self {
            command,
            cwd_marker,
        }
    }

    pub fn into_parts(self) -> (Command, Option<BoxStream<'static, CwdMarkerLine>>) {
        (self.command, self.cwd_marker)
    }
}
