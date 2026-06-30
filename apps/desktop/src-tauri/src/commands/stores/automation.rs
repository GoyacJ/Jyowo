use super::*;

#[derive(Clone)]
pub struct DesktopAutomationStore {
    retention_limit: usize,
    workspace_root: PathBuf,
}

impl DesktopAutomationStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::new_with_limit(workspace_root, AUTOMATION_RUN_RETENTION_LIMIT)
    }

    pub fn new_with_limit(workspace_root: PathBuf, retention_limit: usize) -> Self {
        Self {
            retention_limit,
            workspace_root,
        }
    }

    fn automations_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("automations.json")
    }

    fn runs_path(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("automation-runs.jsonl")
    }
}

impl AutomationStore for DesktopAutomationStore {
    fn load_automations(&self) -> Result<Vec<AutomationSpec>, CommandErrorPayload> {
        let automations_path = self.automations_path();
        ensure_no_symlink_components(&automations_path, "automation settings file")?;
        let bytes = match std::fs::read(&automations_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "automation settings read failed: {error}"
                )));
            }
        };
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(Vec::new());
        }
        let mut records =
            serde_json::from_slice::<Vec<AutomationSpec>>(&bytes).map_err(|error| {
                invalid_payload(format!("automation settings parse failed: {error}"))
            })?;
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
        ensure_no_symlink_components(&runs_path, "automation run ledger file")?;
        let content = match std::fs::read_to_string(&runs_path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "automation run ledger read failed: {error}"
                )));
            }
        };

        content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let record =
                    serde_json::from_str::<AutomationRunRecord>(line).map_err(|error| {
                        invalid_payload(format!("automation run ledger parse failed: {error}"))
                    })?;
                ensure_automation_run_record(&record)?;
                Ok(record)
            })
            .collect()
    }

    fn append_run_record(&self, record: &AutomationRunRecord) -> Result<(), CommandErrorPayload> {
        ensure_automation_run_record(record)?;
        let mut records = self.load_run_records()?;
        records.push(record.clone());
        let keep_from = records.len().saturating_sub(self.retention_limit);
        if keep_from > 0 {
            records.drain(0..keep_from);
        }
        write_automation_run_records(&self.runs_path(), &records)
    }
}
