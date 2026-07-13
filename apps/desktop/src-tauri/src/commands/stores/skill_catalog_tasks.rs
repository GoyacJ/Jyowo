use super::*;

const SKILL_CATALOG_TASKS_FILE: &str = "skill-catalog-install-tasks.json";
const SKILL_CATALOG_TASKS_LABEL: &str = "skill catalog install tasks";

#[derive(Clone)]
pub(crate) struct DesktopSkillCatalogTaskStore {
    runtime_root: PathBuf,
}

impl DesktopSkillCatalogTaskStore {
    pub(crate) fn new(runtime_root: PathBuf) -> Self {
        Self { runtime_root }
    }

    fn path(&self) -> PathBuf {
        self.runtime_root.join(SKILL_CATALOG_TASKS_FILE)
    }

    fn load_unlocked(
        &self,
    ) -> Result<HashMap<String, SkillCatalogInstallTaskPayload>, CommandErrorPayload> {
        Ok(read_json_file(&self.path(), SKILL_CATALOG_TASKS_LABEL)?.unwrap_or_default())
    }

    fn save_unlocked(
        &self,
        tasks: &HashMap<String, SkillCatalogInstallTaskPayload>,
    ) -> Result<(), CommandErrorPayload> {
        write_json_file_atomic(&self.path(), SKILL_CATALOG_TASKS_LABEL, tasks)
    }

    fn with_lock<T>(
        &self,
        action: impl FnOnce() -> Result<T, CommandErrorPayload>,
    ) -> Result<T, CommandErrorPayload> {
        with_jsonl_file_lock(&self.path(), SKILL_CATALOG_TASKS_LABEL, action)
    }

    pub(crate) fn load(
        &self,
    ) -> Result<HashMap<String, SkillCatalogInstallTaskPayload>, CommandErrorPayload> {
        self.with_lock(|| self.load_unlocked())
    }

    pub(crate) fn create_running(
        &self,
        task: SkillCatalogInstallTaskPayload,
    ) -> Result<(SkillCatalogInstallTaskPayload, bool), CommandErrorPayload> {
        self.with_lock(|| {
            let mut tasks = self.load_unlocked()?;
            if let Some(existing) = tasks.get(&task.operation_id) {
                if existing.status == "running" {
                    if existing.source_id != task.source_id
                        || existing.entry_id != task.entry_id
                        || existing.version != task.version
                    {
                        return Err(invalid_payload(
                            "catalog install operation identity changed".to_owned(),
                        ));
                    }
                    return Ok((existing.clone(), false));
                }
                return Err(invalid_payload(
                    "operationId already identifies a terminal install operation".to_owned(),
                ));
            }
            tasks.insert(task.operation_id.clone(), task.clone());
            self.save_unlocked(&tasks)?;
            Ok((task, true))
        })
    }

    pub(crate) fn record_progress(
        &self,
        payload: SkillCatalogInstallProgressPayload,
    ) -> Result<SkillCatalogInstallTaskPayload, CommandErrorPayload> {
        self.with_lock(|| {
            let mut tasks = self.load_unlocked()?;
            let task = tasks.get_mut(&payload.operation_id).ok_or_else(|| {
                invalid_payload("catalog install operation was not found".to_owned())
            })?;
            if task.source_id != payload.source_id
                || task.entry_id != payload.entry_id
                || task.version != payload.version
            {
                return Err(invalid_payload(
                    "catalog install operation identity changed".to_owned(),
                ));
            }
            if task.status != "running" {
                return Ok(task.clone());
            }
            if payload.percent < task.percent || stage_rank(payload.stage) < stage_rank(&task.stage)
            {
                return Ok(task.clone());
            }
            task.stage = payload.stage.to_owned();
            task.percent = payload.percent.min(100);
            task.status = match payload.stage {
                "completed" => "completed",
                "failed" => "failed",
                _ => "running",
            }
            .to_owned();
            task.message = payload.message;
            task.updated_at = now().to_rfc3339();
            let task = task.clone();
            self.save_unlocked(&tasks)?;
            Ok(task)
        })
    }

    pub(crate) fn interrupt_running(
        &self,
    ) -> Result<Vec<SkillCatalogInstallTaskPayload>, CommandErrorPayload> {
        self.with_lock(|| {
            let mut tasks = self.load_unlocked()?;
            let now = now().to_rfc3339();
            let mut interrupted = Vec::new();
            for task in tasks.values_mut() {
                if task.status == "running" {
                    task.status = "interrupted".to_owned();
                    task.stage = "interrupted".to_owned();
                    task.message =
                        Some("install operation was interrupted by runtime restart".to_owned());
                    task.updated_at = now.clone();
                    interrupted.push(task.clone());
                }
            }
            if !interrupted.is_empty() {
                self.save_unlocked(&tasks)?;
            }
            Ok(interrupted)
        })
    }
}

fn stage_rank(stage: &str) -> u8 {
    match stage {
        "preparing" => 0,
        "resolving" => 1,
        "checking" => 2,
        "downloading" => 3,
        "validating" => 4,
        "copying" => 5,
        "reloading" => 6,
        "completed" | "failed" | "interrupted" => 7,
        _ => 0,
    }
}
