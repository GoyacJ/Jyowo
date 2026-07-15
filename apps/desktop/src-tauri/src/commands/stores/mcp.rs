use super::*;
use crate::storage_layout::StorageLayout;

const RETIRED_BROWSER_MCP_SERVER_IDS: [&str; 2] = ["browser-playwright", "browser-chrome-devtools"];

/// Scope-aware MCP server store.
///
/// Global settings live under `~/.jyowo/config/mcp-servers.json`.
/// Project settings support is kept for the future project settings module.
///
/// New saves through this store are validated by the command layer and should
/// not contain raw inline secrets.
#[derive(Clone)]
pub(crate) struct DesktopMcpServerStore {
    layout: StorageLayout,
    workspace_root: Option<PathBuf>,
}

impl DesktopMcpServerStore {
    #[allow(dead_code)]
    pub(crate) fn new(layout: StorageLayout, workspace_root: PathBuf) -> Self {
        Self {
            layout,
            workspace_root: Some(workspace_root),
        }
    }

    pub(crate) fn global(layout: StorageLayout) -> Self {
        Self {
            layout,
            workspace_root: None,
        }
    }

    fn settings_path(&self) -> PathBuf {
        match &self.workspace_root {
            Some(workspace_root) => self.layout.project_mcp_servers_file(workspace_root),
            None => self.layout.global_mcp_servers_file(),
        }
    }
}

impl McpServerStore for DesktopMcpServerStore {
    fn load_records(&self) -> Result<Vec<McpServerConfigRecord>, CommandErrorPayload> {
        let settings_path = self.settings_path();
        let mut records: Vec<McpServerConfigRecord> =
            read_secret_json_file(&settings_path, "mcp server settings")?.unwrap_or_default();
        let stored_count = records.len();
        records.retain(|record| !RETIRED_BROWSER_MCP_SERVER_IDS.contains(&record.id.as_str()));
        if records.len() != stored_count {
            write_mcp_server_records(&settings_path, &records)?;
        }
        Ok(records)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_layout::JyowoHome;

    #[test]
    fn load_migrates_retired_browser_mcp_records() {
        let home = tempfile::tempdir().expect("temporary home");
        let home_root = home.path().canonicalize().expect("canonical tempdir");
        let layout = StorageLayout::new(JyowoHome::new(home_root.join(".jyowo")));
        let store = DesktopMcpServerStore::global(layout);
        let mut retained = browser_mcp_preset_record(BrowserMcpPresetId::Playwright, false);
        retained.id = "custom-browser-server".to_owned();
        retained.display_name = "Custom browser server".to_owned();
        store.save_record(&retained).expect("save retained server");
        store
            .save_record(&browser_mcp_preset_record(
                BrowserMcpPresetId::ChromeDevtools,
                true,
            ))
            .expect("save retired browser preset");

        let loaded = store.load_records().expect("load migrated records");
        assert_eq!(loaded, vec![retained.clone()]);

        let persisted: Vec<McpServerConfigRecord> =
            read_secret_json_file(&store.settings_path(), "mcp server settings")
                .expect("read migrated settings")
                .expect("settings exist");
        assert_eq!(persisted, vec![retained]);
    }
}

#[derive(Clone)]
pub struct DesktopMcpDiagnosticStore {
    clear_lock: Arc<ParkingMutex<()>>,
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
            clear_lock: Arc::new(ParkingMutex::new(())),
            retention_limit,
            runtime_root: workspace_root.join(".jyowo").join("runtime"),
        }
    }

    pub fn new_runtime_root(runtime_root: PathBuf) -> Self {
        Self::new_runtime_root_with_limit(runtime_root, MCP_DIAGNOSTIC_RETENTION_LIMIT)
    }

    pub fn new_runtime_root_with_limit(runtime_root: PathBuf, retention_limit: usize) -> Self {
        Self {
            clear_lock: Arc::new(ParkingMutex::new(())),
            retention_limit,
            runtime_root,
        }
    }

    fn diagnostics_path(&self) -> PathBuf {
        self.runtime_root.join("mcp-diagnostics.jsonl")
    }

    fn task_clear_watermarks_path(&self) -> PathBuf {
        self.runtime_root
            .join("mcp-task-diagnostic-clear-watermarks.json")
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

    fn load_task_clear_watermarks(
        &self,
    ) -> Result<McpTaskDiagnosticClearWatermarks, CommandErrorPayload> {
        let _guard = self.clear_lock.lock();
        read_json_file(
            &self.task_clear_watermarks_path(),
            "MCP task diagnostic clear watermarks",
        )
        .map(Option::unwrap_or_default)
    }

    fn clear_records(
        &self,
        server_id: Option<&str>,
        task_cleared_at: DateTime<Utc>,
    ) -> Result<(), CommandErrorPayload> {
        let _guard = self.clear_lock.lock();
        let mut watermarks: McpTaskDiagnosticClearWatermarks = read_json_file(
            &self.task_clear_watermarks_path(),
            "MCP task diagnostic clear watermarks",
        )?
        .unwrap_or_default();
        match server_id {
            Some(server_id) => {
                watermarks
                    .servers
                    .insert(server_id.to_owned(), task_cleared_at);
            }
            None => {
                watermarks.all = Some(task_cleared_at);
                watermarks.servers.clear();
            }
        }
        update_jsonl_records_locked(
            &self.diagnostics_path(),
            "mcp diagnostics",
            |records: &mut Vec<McpDiagnosticRecord>| match server_id {
                Some(server_id) => records.retain(|record| record.server_id != server_id),
                None => records.clear(),
            },
            |error| runtime_operation_failed(format!("mcp diagnostics parse failed: {error}")),
            |_| Ok(()),
        )?;
        write_json_file_atomic(
            &self.task_clear_watermarks_path(),
            "MCP task diagnostic clear watermarks",
            &watermarks,
        )
    }
}
