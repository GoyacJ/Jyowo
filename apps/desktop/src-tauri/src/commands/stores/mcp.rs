use super::*;
use crate::storage_layout::StorageLayout;

/// Project-scoped MCP server store.
///
/// Stores enabled/custom MCP server configurations under
/// `<workspace>/.jyowo/config/mcp-servers.json`.
///
/// New saves through this store are validated by the command layer and should
/// not contain raw inline secrets.
#[derive(Clone)]
pub(crate) struct DesktopMcpServerStore {
    layout: StorageLayout,
    workspace_root: PathBuf,
}

impl DesktopMcpServerStore {
    pub(crate) fn new(layout: StorageLayout, workspace_root: PathBuf) -> Self {
        Self {
            layout,
            workspace_root,
        }
    }

    fn settings_path(&self) -> PathBuf {
        self.layout.project_mcp_servers_file(&self.workspace_root)
    }
}

impl McpServerStore for DesktopMcpServerStore {
    fn load_records(&self) -> Result<Vec<McpServerConfigRecord>, CommandErrorPayload> {
        let settings_path = self.settings_path();
        read_secret_json_file(&settings_path, "mcp server settings")
            .map(|records| records.unwrap_or_default())
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

#[derive(Clone, Default)]
pub(crate) struct NoWorkspaceMcpServerStore;

impl McpServerStore for NoWorkspaceMcpServerStore {
    fn load_records(&self) -> Result<Vec<McpServerConfigRecord>, CommandErrorPayload> {
        Ok(Vec::new())
    }

    fn save_record(&self, _record: &McpServerConfigRecord) -> Result<(), CommandErrorPayload> {
        Err(invalid_payload(
            "custom MCP servers require an active project workspace".to_owned(),
        ))
    }

    fn delete_record(&self, _id: &str) -> Result<(), CommandErrorPayload> {
        Err(invalid_payload(
            "custom MCP servers require an active project workspace".to_owned(),
        ))
    }
}

#[derive(Clone)]
pub struct DesktopMcpDiagnosticStore {
    retention_limit: usize,
    runtime_root: PathBuf,
}

impl DesktopMcpDiagnosticStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::new_with_limit(workspace_root, MCP_DIAGNOSTIC_RETENTION_LIMIT)
    }

    pub fn new_with_limit(workspace_root: PathBuf, retention_limit: usize) -> Self {
        let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
        Self {
            retention_limit,
            runtime_root: workspace_root.join(".jyowo").join("runtime"),
        }
    }

    pub fn new_runtime_root(runtime_root: PathBuf) -> Self {
        Self::new_runtime_root_with_limit(runtime_root, MCP_DIAGNOSTIC_RETENTION_LIMIT)
    }

    pub fn new_runtime_root_with_limit(runtime_root: PathBuf, retention_limit: usize) -> Self {
        Self {
            retention_limit,
            runtime_root,
        }
    }

    fn diagnostics_path(&self) -> PathBuf {
        self.runtime_root.join("mcp-diagnostics.jsonl")
    }
}

impl McpDiagnosticStore for DesktopMcpDiagnosticStore {
    fn load_records(&self) -> Result<Vec<McpDiagnosticRecord>, CommandErrorPayload> {
        let diagnostics_path = self.diagnostics_path();
        read_jsonl_records_locked(
            &diagnostics_path,
            "mcp diagnostics",
            |error| runtime_operation_failed(format!("mcp diagnostics parse failed: {error}")),
            |_| Ok(()),
        )
    }

    fn append_record(&self, record: &McpDiagnosticRecord) -> Result<(), CommandErrorPayload> {
        append_jsonl_record_with_retention_locked(
            &self.diagnostics_path(),
            "mcp diagnostics",
            record,
            self.retention_limit,
            |error| runtime_operation_failed(format!("mcp diagnostics parse failed: {error}")),
            |_| Ok(()),
        )
    }

    fn clear_records(&self, server_id: Option<&str>) -> Result<(), CommandErrorPayload> {
        update_jsonl_records_locked(
            &self.diagnostics_path(),
            "mcp diagnostics",
            |records: &mut Vec<McpDiagnosticRecord>| match server_id {
                Some(server_id) => records.retain(|record| record.server_id != server_id),
                None => records.clear(),
            },
            |error| runtime_operation_failed(format!("mcp diagnostics parse failed: {error}")),
            |_| Ok(()),
        )
    }
}
