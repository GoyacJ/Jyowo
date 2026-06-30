use super::*;

#[derive(Clone)]
pub(crate) struct DesktopMcpServerStore {
    workspace_root: PathBuf,
}

impl DesktopMcpServerStore {
    pub(crate) fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn settings_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("mcp-servers.json")
    }
}

impl McpServerStore for DesktopMcpServerStore {
    fn load_records(&self) -> Result<Vec<McpServerConfigRecord>, CommandErrorPayload> {
        let settings_path = self.settings_path();
        match std::fs::read(&settings_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                runtime_operation_failed(format!("mcp server settings parse failed: {error}"))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(runtime_operation_failed(format!(
                "mcp server settings read failed: {error}"
            ))),
        }
    }

    fn save_record(&self, record: &McpServerConfigRecord) -> Result<(), CommandErrorPayload> {
        let mut records = self.load_records()?;
        records.retain(|existing| existing.id != record.id);
        records.push(record.clone());
        records.sort_by(|left, right| left.id.cmp(&right.id));
        write_mcp_server_records(&self.settings_path(), &records)
    }

    fn delete_record(&self, id: &str) -> Result<(), CommandErrorPayload> {
        let mut records = self.load_records()?;
        records.retain(|existing| existing.id != id);
        write_mcp_server_records(&self.settings_path(), &records)
    }
}

#[derive(Clone)]
pub struct DesktopMcpDiagnosticStore {
    retention_limit: usize,
    workspace_root: PathBuf,
}

impl DesktopMcpDiagnosticStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::new_with_limit(workspace_root, MCP_DIAGNOSTIC_RETENTION_LIMIT)
    }

    pub fn new_with_limit(workspace_root: PathBuf, retention_limit: usize) -> Self {
        let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
        Self {
            retention_limit,
            workspace_root,
        }
    }

    fn diagnostics_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("mcp-diagnostics.jsonl")
    }
}

impl McpDiagnosticStore for DesktopMcpDiagnosticStore {
    fn load_records(&self) -> Result<Vec<McpDiagnosticRecord>, CommandErrorPayload> {
        let diagnostics_path = self.diagnostics_path();
        let content = match std::fs::read_to_string(&diagnostics_path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "mcp diagnostics read failed: {error}"
                )));
            }
        };

        content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<McpDiagnosticRecord>(line).map_err(|error| {
                    runtime_operation_failed(format!("mcp diagnostics parse failed: {error}"))
                })
            })
            .collect()
    }

    fn append_record(&self, record: &McpDiagnosticRecord) -> Result<(), CommandErrorPayload> {
        let mut records = self.load_records()?;
        records.push(record.clone());
        let keep_from = records.len().saturating_sub(self.retention_limit);
        if keep_from > 0 {
            records.drain(0..keep_from);
        }
        write_mcp_diagnostic_records(&self.diagnostics_path(), &records)
    }

    fn clear_records(&self, server_id: Option<&str>) -> Result<(), CommandErrorPayload> {
        let records = match server_id {
            Some(server_id) => self
                .load_records()?
                .into_iter()
                .filter(|record| record.server_id != server_id)
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };
        write_mcp_diagnostic_records(&self.diagnostics_path(), &records)
    }
}
