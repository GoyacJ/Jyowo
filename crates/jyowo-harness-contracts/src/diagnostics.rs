use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Event, RunId, SessionId, TenantId};

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsRunnerKind {
    Rust,
    DesktopTs,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsRequest {
    pub runner: DiagnosticsRunnerKind,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLanguage {
    Rust,
    TypeScript,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticItem {
    pub language: DiagnosticLanguage,
    pub severity: DiagnosticSeverity,
    pub code: Option<String>,
    pub message: String,
    pub relative_path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsResult {
    pub diagnostics: Vec<DiagnosticItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsRunRequest {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub workspace_root: PathBuf,
    pub runner: DiagnosticsRunnerKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticsRawOutput {
    pub runner: DiagnosticsRunnerKind,
    pub stdout: String,
    pub stderr: String,
    pub sandbox_events: Vec<Event>,
}
