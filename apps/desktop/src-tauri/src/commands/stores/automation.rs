use super::*;
use crate::storage_layout::{JyowoHome, StorageLayout};

#[derive(Clone)]
pub struct DesktopAutomationStore {
    layout: StorageLayout,
    retention_limit: usize,
    workspace_root: PathBuf,
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

    pub fn new_with_layout_and_limit(
        layout: StorageLayout,
        workspace_root: PathBuf,
        retention_limit: usize,
    ) -> Self {
        Self {
            layout,
            retention_limit,
            workspace_root,
        }
    }

    pub fn new_with_limit(workspace_root: PathBuf, retention_limit: usize) -> Self {
        Self {
            layout: StorageLayout::new(JyowoHome::new(default_jyowo_home())),
            retention_limit,
            workspace_root,
        }
    }

    fn automations_path(&self) -> PathBuf {
        self.layout.project_automations_file(&self.workspace_root)
    }

    fn runs_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("automation-runs.jsonl")
    }
}

fn default_jyowo_home() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".jyowo")
}

/// Migrate automations from the old runtime path to the new project config path.
///
/// Old path: `<workspace>/.jyowo/runtime/automations.json`
/// New path: `<workspace>/.jyowo/config/automations.json`
///
/// Automation run logs stay under runtime and are not touched.
pub(crate) fn migrate_automations_from_runtime(
    layout: &StorageLayout,
    workspace_root: &Path,
) -> Result<MigrationResult, CommandErrorPayload> {
    let old_path = workspace_root
        .join(".jyowo")
        .join("runtime")
        .join("automations.json");
    let new_path = layout.project_automations_file(workspace_root);

    migrate_secret_json_file_with::<Vec<AutomationSpec>, _>(
        &old_path,
        &new_path,
        "automation settings",
        true,
        |old, new| old == new,
    )
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
