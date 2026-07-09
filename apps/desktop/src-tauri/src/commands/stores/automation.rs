use super::*;
use crate::storage_layout::{JyowoHome, StorageLayout};

#[derive(Clone)]
pub struct DesktopAutomationStore {
    layout: StorageLayout,
    retention_limit: usize,
    workspace_root: Option<PathBuf>,
}

impl DesktopAutomationStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::new_with_layout_and_limit(
            StorageLayout::new(JyowoHome::new(default_jyowo_home())),
            workspace_root,
            AUTOMATION_RUN_RETENTION_LIMIT,
        )
    }

    pub fn new_with_layout(layout: StorageLayout, workspace_root: PathBuf) -> Self {
        Self::new_with_layout_and_limit(layout, workspace_root, AUTOMATION_RUN_RETENTION_LIMIT)
    }

    pub fn global_with_layout(layout: StorageLayout) -> Self {
        Self::global_with_layout_and_limit(layout, AUTOMATION_RUN_RETENTION_LIMIT)
    }

    pub fn global_with_layout_and_limit(layout: StorageLayout, retention_limit: usize) -> Self {
        Self {
            layout,
            retention_limit,
            workspace_root: None,
        }
    }

    pub fn new_with_layout_and_limit(
        layout: StorageLayout,
        workspace_root: PathBuf,
        retention_limit: usize,
    ) -> Self {
        Self {
            layout,
            retention_limit,
            workspace_root: Some(workspace_root),
        }
    }

    pub fn new_with_limit(workspace_root: PathBuf, retention_limit: usize) -> Self {
        Self {
            layout: StorageLayout::new(JyowoHome::new(default_jyowo_home())),
            retention_limit,
            workspace_root: Some(workspace_root),
        }
    }

    fn automations_path(&self) -> PathBuf {
        match &self.workspace_root {
            Some(workspace_root) => self.layout.project_automations_file(workspace_root),
            None => self.layout.global_automations_file(),
        }
    }

    fn runs_path(&self) -> PathBuf {
        match &self.workspace_root {
            Some(workspace_root) => workspace_root
                .join(".jyowo")
                .join("runtime")
                .join("automation-runs.jsonl"),
            None => self.layout.global_automation_runs_file(),
        }
    }
}

fn default_jyowo_home() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".jyowo")
}

impl AutomationStore for DesktopAutomationStore {
    fn load_automations(&self) -> Result<Vec<AutomationSpec>, CommandErrorPayload> {
        let automations_path = self.automations_path();
        let mut records: Vec<AutomationSpec> =
            read_secret_json_file_or_default_on_blank(&automations_path, "automation settings")?;
        for record in &records {
            ensure_automation_spec(record)?;
        }
        records.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(records)
    }

    fn save_automations(&self, records: &[AutomationSpec]) -> Result<(), CommandErrorPayload> {
        for record in records {
            ensure_automation_spec(record)?;
        }
        let mut records = records.to_vec();
        records.sort_by(|left, right| left.id.cmp(&right.id));
        write_automation_specs(&self.automations_path(), &records)
    }

    fn load_run_records(&self) -> Result<Vec<AutomationRunRecord>, CommandErrorPayload> {
        let runs_path = self.runs_path();
        read_jsonl_records_locked(
            &runs_path,
            "automation run ledger",
            |error| invalid_payload(format!("automation run ledger parse failed: {error}")),
            ensure_automation_run_record,
        )
    }

    fn append_run_record(&self, record: &AutomationRunRecord) -> Result<(), CommandErrorPayload> {
        ensure_automation_run_record(record)?;
        append_jsonl_record_with_retention_locked(
            &self.runs_path(),
            "automation run ledger",
            record,
            self.retention_limit,
            |error| invalid_payload(format!("automation run ledger parse failed: {error}")),
            ensure_automation_run_record,
        )
    }
}

#[derive(Clone, Default)]
pub struct NoWorkspaceAutomationStore;

impl AutomationStore for NoWorkspaceAutomationStore {
    fn load_automations(&self) -> Result<Vec<AutomationSpec>, CommandErrorPayload> {
        Ok(Vec::new())
    }

    fn save_automations(&self, _records: &[AutomationSpec]) -> Result<(), CommandErrorPayload> {
        Err(invalid_payload(
            "project-scoped automations require an active project workspace".to_owned(),
        ))
    }

    fn load_run_records(&self) -> Result<Vec<AutomationRunRecord>, CommandErrorPayload> {
        Ok(Vec::new())
    }

    fn append_run_record(&self, _record: &AutomationRunRecord) -> Result<(), CommandErrorPayload> {
        Err(invalid_payload(
            "project-scoped automation runs require an active project workspace".to_owned(),
        ))
    }
}
