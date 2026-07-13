use std::path::{Path, PathBuf};

use harness_contracts::{
    AgentProfile, ExecutionDefaultsRecord, McpPresetRecord, ProviderProfileDefinition,
    ProviderSecretEntry, ProviderSecretMetadata, ProviderSelectionRecord, SkillSelectionRecord,
};

use crate::commands::error::CommandErrorPayload;
use crate::storage_layout::StorageLayout;

use super::{
    ensure_app_dir_no_symlink, read_file_no_follow, read_json_file, read_json_file_invalid_payload,
    read_secret_json_file, remove_invalid_json_file, write_bytes_file_atomic,
    write_json_file_atomic, write_secret_json_file_atomic,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ProviderGenerationFile {
    Profiles,
    Secrets,
    Selection,
}

struct ProviderGenerationWrite {
    file: ProviderGenerationFile,
    path: PathBuf,
    label: &'static str,
    secret: bool,
    new_bytes: Vec<u8>,
    old_bytes: Option<Vec<u8>>,
}

/// Typed store for global configuration under `~/.jyowo/config/`.
///
/// Uses [`StorageLayout`] for path resolution and the existing atomic I/O helpers
/// from the stores module. This struct is intentionally lightweight — it owns no
/// cached state and delegates all persistence to the helpers.
#[derive(Debug, Clone)]
pub struct GlobalConfigStore {
    layout: StorageLayout,
}

impl GlobalConfigStore {
    pub fn new(layout: StorageLayout) -> Self {
        Self { layout }
    }

    /// Returns a reference to the underlying [`StorageLayout`].
    pub fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    pub(crate) fn lock_provider_generation_shared(
        &self,
    ) -> Result<harness_fs::AdvisoryFileLock, CommandErrorPayload> {
        harness_fs::lock_provider_generation_for_read(&self.layout.global_config_root())
            .map_err(provider_generation_lock_error)
    }

    pub(crate) fn lock_provider_generation_exclusive(
        &self,
    ) -> Result<harness_fs::AdvisoryFileLock, CommandErrorPayload> {
        harness_fs::lock_provider_generation_for_write(&self.layout.global_config_root())
            .map_err(provider_generation_lock_error)
    }

    // ── Provider profiles ──────────────────────────────────────────────

    pub fn load_provider_profiles(
        &self,
    ) -> Result<Vec<ProviderProfileDefinition>, CommandErrorPayload> {
        let path = self.layout.global_provider_profiles_file();
        ensure_config_dir(&path, "provider profiles")?;
        Ok(
            read_json_file::<Vec<ProviderProfileDefinition>>(&path, "provider profiles")?
                .unwrap_or_default(),
        )
    }

    pub fn save_provider_profiles(
        &self,
        profiles: &[ProviderProfileDefinition],
    ) -> Result<(), CommandErrorPayload> {
        let path = self.layout.global_provider_profiles_file();
        ensure_config_dir(&path, "provider profiles")?;
        write_json_file_atomic(&path, "provider profiles", profiles)
    }

    // ── Provider secrets ───────────────────────────────────────────────

    /// Returns redacted metadata for every stored secret. Never exposes raw key material.
    pub fn load_provider_secrets_metadata(
        &self,
    ) -> Result<Vec<ProviderSecretMetadata>, CommandErrorPayload> {
        let path = self.layout.global_provider_secrets_file();
        ensure_config_dir(&path, "provider secrets")?;
        let record = read_secret_json_file::<Vec<ProviderSecretEntry>>(&path, "provider secrets")?
            .unwrap_or_default();
        Ok(record
            .into_iter()
            .map(|entry| ProviderSecretMetadata {
                config_id: entry.config_id.clone(),
                has_api_key: !entry.api_key.is_empty(),
                has_official_quota_api_key: entry
                    .official_quota_api_key
                    .as_ref()
                    .is_some_and(|key| !key.is_empty()),
            })
            .collect())
    }

    /// Returns the raw secret entry for a given `config_id`, if present.
    /// This is the explicit reveal path and must only be called after user authorization.
    pub fn load_provider_secret(
        &self,
        config_id: &str,
    ) -> Result<Option<ProviderSecretEntry>, CommandErrorPayload> {
        let path = self.layout.global_provider_secrets_file();
        ensure_config_dir(&path, "provider secrets")?;
        let record = read_secret_json_file::<Vec<ProviderSecretEntry>>(&path, "provider secrets")?
            .unwrap_or_default();
        Ok(record
            .into_iter()
            .find(|entry| entry.config_id == config_id))
    }

    pub fn save_provider_secret(
        &self,
        entry: &ProviderSecretEntry,
    ) -> Result<(), CommandErrorPayload> {
        let path = self.layout.global_provider_secrets_file();
        ensure_config_dir(&path, "provider secrets")?;
        let mut entries =
            read_secret_json_file::<Vec<ProviderSecretEntry>>(&path, "provider secrets")?
                .unwrap_or_default();
        if let Some(existing) = entries.iter_mut().find(|e| e.config_id == entry.config_id) {
            *existing = entry.clone();
        } else {
            entries.push(entry.clone());
        }
        write_secret_json_file_atomic(&path, "provider secrets", &entries)
    }

    pub fn delete_provider_secret(&self, config_id: &str) -> Result<(), CommandErrorPayload> {
        let path = self.layout.global_provider_secrets_file();
        ensure_config_dir(&path, "provider secrets")?;
        let mut entries =
            read_secret_json_file::<Vec<ProviderSecretEntry>>(&path, "provider secrets")?
                .unwrap_or_default();
        entries.retain(|entry| entry.config_id != config_id);
        write_secret_json_file_atomic(&path, "provider secrets", &entries)
    }

    // ── Global provider selection ──────────────────────────────────────

    pub fn load_global_provider_selection(
        &self,
    ) -> Result<ProviderSelectionRecord, CommandErrorPayload> {
        let path = self.layout.global_provider_selection_file();
        ensure_config_dir(&path, "provider selection")?;
        Ok(
            read_json_file::<ProviderSelectionRecord>(&path, "provider selection")?
                .unwrap_or_default(),
        )
    }

    pub fn save_global_provider_selection(
        &self,
        record: &ProviderSelectionRecord,
    ) -> Result<(), CommandErrorPayload> {
        let path = self.layout.global_provider_selection_file();
        ensure_config_dir(&path, "provider selection")?;
        write_json_file_atomic(&path, "provider selection", record)
    }

    pub(crate) fn save_provider_generation(
        &self,
        profiles: &[ProviderProfileDefinition],
        secrets: &[ProviderSecretEntry],
        selection: &ProviderSelectionRecord,
    ) -> Result<(), CommandErrorPayload> {
        let _generation_guard = self.lock_provider_generation_exclusive()?;
        self.save_provider_generation_with_writer_locked(
            profiles,
            secrets,
            selection,
            |_, path, label, bytes, secret| {
                write_bytes_file_atomic(path, "store.json", label, bytes, secret)
            },
        )
    }

    #[cfg(test)]
    fn save_provider_generation_with_writer<F>(
        &self,
        profiles: &[ProviderProfileDefinition],
        secrets: &[ProviderSecretEntry],
        selection: &ProviderSelectionRecord,
        writer: F,
    ) -> Result<(), CommandErrorPayload>
    where
        F: FnMut(
            ProviderGenerationFile,
            &Path,
            &str,
            &[u8],
            bool,
        ) -> Result<(), CommandErrorPayload>,
    {
        let _generation_guard = self.lock_provider_generation_exclusive()?;
        self.save_provider_generation_with_writer_locked(profiles, secrets, selection, writer)
    }

    pub(crate) fn save_provider_generation_locked(
        &self,
        profiles: &[ProviderProfileDefinition],
        secrets: &[ProviderSecretEntry],
        selection: &ProviderSelectionRecord,
    ) -> Result<(), CommandErrorPayload> {
        self.save_provider_generation_with_writer_locked(
            profiles,
            secrets,
            selection,
            |_, path, label, bytes, secret| {
                write_bytes_file_atomic(path, "store.json", label, bytes, secret)
            },
        )
    }

    fn save_provider_generation_with_writer_locked<F>(
        &self,
        profiles: &[ProviderProfileDefinition],
        secrets: &[ProviderSecretEntry],
        selection: &ProviderSelectionRecord,
        mut writer: F,
    ) -> Result<(), CommandErrorPayload>
    where
        F: FnMut(
            ProviderGenerationFile,
            &Path,
            &str,
            &[u8],
            bool,
        ) -> Result<(), CommandErrorPayload>,
    {
        let profiles_bytes = serde_json::to_vec_pretty(profiles).map_err(|error| {
            crate::commands::error::runtime_operation_failed(format!(
                "provider profiles serialization failed: {error}"
            ))
        })?;
        let secrets_bytes = serde_json::to_vec_pretty(secrets).map_err(|error| {
            crate::commands::error::runtime_operation_failed(format!(
                "provider secrets serialization failed: {error}"
            ))
        })?;
        let selection_bytes = serde_json::to_vec_pretty(selection).map_err(|error| {
            crate::commands::error::runtime_operation_failed(format!(
                "provider selection serialization failed: {error}"
            ))
        })?;
        let paths = [
            (
                ProviderGenerationFile::Profiles,
                self.layout.global_provider_profiles_file(),
                "provider profiles",
                false,
                profiles_bytes,
            ),
            (
                ProviderGenerationFile::Secrets,
                self.layout.global_provider_secrets_file(),
                "provider secrets",
                true,
                secrets_bytes,
            ),
            (
                ProviderGenerationFile::Selection,
                self.layout.global_provider_selection_file(),
                "provider selection",
                false,
                selection_bytes,
            ),
        ];
        let mut writes = Vec::with_capacity(paths.len());
        for (file, path, label, secret, new_bytes) in paths {
            ensure_config_dir(&path, label)?;
            let old_bytes = read_file_no_follow(&path, label)?;
            writes.push(ProviderGenerationWrite {
                file,
                path,
                label,
                secret,
                new_bytes,
                old_bytes,
            });
        }
        harness_fs::begin_provider_generation_write(
            &self.layout.global_config_root(),
            writes[0].old_bytes.as_deref(),
            writes[1].old_bytes.as_deref(),
            writes[2].old_bytes.as_deref(),
        )
        .map_err(provider_generation_lock_error)?;

        for index in 0..writes.len() {
            let write = &writes[index];
            if let Err(commit_error) = writer(
                write.file,
                &write.path,
                write.label,
                &write.new_bytes,
                write.secret,
            ) {
                let mut rollback_errors = Vec::new();
                for rollback in writes[..=index].iter().rev() {
                    let result = match rollback.old_bytes.as_deref() {
                        Some(bytes) => writer(
                            rollback.file,
                            &rollback.path,
                            rollback.label,
                            bytes,
                            rollback.secret,
                        ),
                        None => remove_invalid_json_file(&rollback.path, rollback.label),
                    };
                    if let Err(error) = result {
                        rollback_errors.push(format!("{}: {}", rollback.label, error.message));
                    }
                }
                if rollback_errors.is_empty() {
                    self.finish_provider_generation_write()?;
                    return Err(commit_error);
                }
                return Err(crate::commands::error::runtime_operation_failed(format!(
                    "{}; provider settings rollback failed: {}",
                    commit_error.message,
                    rollback_errors.join("; ")
                )));
            }
        }
        self.finish_provider_generation_write()?;
        Ok(())
    }

    fn finish_provider_generation_write(&self) -> Result<(), CommandErrorPayload> {
        match harness_fs::finish_provider_generation_write(&self.layout.global_config_root())
            .map_err(provider_generation_lock_error)?
        {
            harness_fs::ProviderGenerationFinishOutcome::Committed => {}
            harness_fs::ProviderGenerationFinishOutcome::CommittedWithoutDirectorySync {
                source,
            } => {
                log::warn!(
                    "provider generation committed, but recovery marker directory sync failed: {source}"
                );
            }
        }
        Ok(())
    }

    // ── Execution defaults ─────────────────────────────────────────────

    pub fn load_execution_defaults(&self) -> Result<ExecutionDefaultsRecord, CommandErrorPayload> {
        let path = self.layout.global_execution_defaults_file();
        ensure_config_dir(&path, "execution defaults")?;
        Ok(
            read_json_file::<ExecutionDefaultsRecord>(&path, "execution defaults")?
                .unwrap_or_default(),
        )
    }

    pub fn save_execution_defaults(
        &self,
        record: &ExecutionDefaultsRecord,
    ) -> Result<(), CommandErrorPayload> {
        let path = self.layout.global_execution_defaults_file();
        ensure_config_dir(&path, "execution defaults")?;
        write_json_file_atomic(&path, "execution defaults", record)
    }

    // ── MCP presets ────────────────────────────────────────────────────

    pub fn load_mcp_presets(&self) -> Result<Vec<McpPresetRecord>, CommandErrorPayload> {
        let path = self.layout.global_mcp_presets_file();
        ensure_config_dir(&path, "MCP presets")?;
        Ok(read_json_file::<Vec<McpPresetRecord>>(&path, "MCP presets")?.unwrap_or_default())
    }

    pub fn save_mcp_presets(&self, presets: &[McpPresetRecord]) -> Result<(), CommandErrorPayload> {
        let path = self.layout.global_mcp_presets_file();
        ensure_config_dir(&path, "MCP presets")?;
        write_secret_json_file_atomic(&path, "MCP presets", presets)
    }

    // ── Global agent profiles ──────────────────────────────────────────

    pub fn load_global_agent_profiles(&self) -> Result<Vec<AgentProfile>, CommandErrorPayload> {
        let path = self.layout.global_agent_profiles_file();
        ensure_config_dir(&path, "agent profiles")?;
        Ok(
            read_json_file_invalid_payload::<Vec<AgentProfile>>(&path, "agent profiles")?
                .unwrap_or_default(),
        )
    }

    pub fn save_global_agent_profiles(
        &self,
        profiles: &[AgentProfile],
    ) -> Result<(), CommandErrorPayload> {
        let path = self.layout.global_agent_profiles_file();
        ensure_config_dir(&path, "agent profiles")?;
        write_json_file_atomic(&path, "agent profiles", profiles)
    }

    // ── Global skill selection ─────────────────────────────────────────

    pub fn load_global_skill_selection(&self) -> Result<SkillSelectionRecord, CommandErrorPayload> {
        let path = self.layout.global_skills_file();
        ensure_config_dir(&path, "skill selection")?;
        Ok(read_json_file::<SkillSelectionRecord>(&path, "skill selection")?.unwrap_or_default())
    }

    pub fn load_global_skill_selection_if_present(
        &self,
    ) -> Result<Option<SkillSelectionRecord>, CommandErrorPayload> {
        let path = self.layout.global_skills_file();
        read_json_file::<SkillSelectionRecord>(&path, "skill selection")
    }

    pub fn save_global_skill_selection(
        &self,
        record: &SkillSelectionRecord,
    ) -> Result<(), CommandErrorPayload> {
        let path = self.layout.global_skills_file();
        ensure_config_dir(&path, "skill selection")?;
        write_json_file_atomic(&path, "skill selection", record)
    }
}

fn provider_generation_lock_error(error: harness_fs::FsError) -> CommandErrorPayload {
    crate::commands::error::runtime_operation_failed(format!(
        "provider generation lock failed: {error}"
    ))
}

/// Ensure the parent config directory exists without following symlinks.
fn ensure_config_dir(path: &Path, label: &str) -> Result<(), CommandErrorPayload> {
    let parent = path.parent().ok_or_else(|| {
        crate::commands::error::runtime_operation_failed(format!(
            "{label} path has no parent directory"
        ))
    })?;
    ensure_app_dir_no_symlink(parent, &format!("{label} directory"))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use harness_contracts::{
        AgentProfile, AgentProfileContextMode, AgentProfileMemoryScope,
        AgentProfileSandboxInheritance, AgentProfileScope, AgentWorkspaceIsolationMode,
        ExecutionDefaultsRecord, McpPresetRecord, McpPresetTransport, ModelProtocol,
        PermissionMode, ProviderProfileConversationCapability, ProviderProfileDefinition,
        ProviderProfileModelDescriptor, ProviderProfileModelLifecycle, ProviderSecretEntry,
        ProviderSelectionRecord, SkillSelectionRecord, ToolProfile,
    };

    use crate::storage_layout::{JyowoHome, StorageLayout};

    use crate::commands::{DesktopProviderSettingsStore, ProviderSettingsStore};

    use super::{GlobalConfigStore, ProviderGenerationFile};

    const PROVIDER_GENERATION_READER_CHILD: &str =
        "commands::stores::global_config::tests::provider_generation_reader_child";

    fn store() -> (GlobalConfigStore, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home_root = temp
            .path()
            .canonicalize()
            .expect("canonical tempdir")
            .join(".jyowo");
        let layout = StorageLayout::new(JyowoHome::new(&home_root));
        (GlobalConfigStore::new(layout), temp)
    }

    fn make_model_descriptor() -> ProviderProfileModelDescriptor {
        ProviderProfileModelDescriptor {
            protocol: ModelProtocol::ChatCompletions,
            context_window: 128000,
            display_name: "GPT-5".to_owned(),
            lifecycle: ProviderProfileModelLifecycle::Stable,
            max_output_tokens: 16384,
            model_id: "gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            conversation_capability: ProviderProfileConversationCapability {
                input_modalities: vec!["text".to_owned()],
                output_modalities: vec!["text".to_owned()],
                context_window: 128000,
                max_output_tokens: 16384,
                streaming: true,
                tool_calling: true,
                reasoning: true,
                prompt_cache: false,
                structured_output: true,
            },
            runtime_semantics: None,
        }
    }

    fn provider_profile(id: &str) -> ProviderProfileDefinition {
        ProviderProfileDefinition {
            id: id.to_owned(),
            display_name: id.to_owned(),
            provider_id: "openai".to_owned(),
            model_id: "gpt-5".to_owned(),
            protocol: ModelProtocol::ChatCompletions,
            model_options: harness_contracts::ModelRequestOptions::default(),
            base_url: None,
            provider_defaults: None,
            model_descriptor: make_model_descriptor(),
        }
    }

    fn provider_generation_paths(store: &GlobalConfigStore) -> [PathBuf; 3] {
        [
            store.layout().global_provider_profiles_file(),
            store.layout().global_provider_secrets_file(),
            store.layout().global_provider_selection_file(),
        ]
    }

    fn spawn_provider_generation_reader(
        store: &GlobalConfigStore,
        started_path: &Path,
        output_path: &Path,
    ) -> Child {
        Command::new(std::env::current_exe().expect("current test executable"))
            .arg("--exact")
            .arg(PROVIDER_GENERATION_READER_CHILD)
            .arg("--nocapture")
            .env(
                "JYOWO_PROVIDER_GENERATION_CONFIG_ROOT",
                store.layout().global_config_root(),
            )
            .env("JYOWO_PROVIDER_GENERATION_READER_STARTED", started_path)
            .env("JYOWO_PROVIDER_GENERATION_READER_OUTPUT", output_path)
            .env("JYOWO_PROVIDER_GENERATION_READER_EXPECT_BLOCKED", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn provider generation reader")
    }

    fn wait_for_path(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(Instant::now() < deadline, "timed out waiting for {path:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn read_generation_result(path: &Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(path).expect("read generation result"))
            .expect("decode generation result")
    }

    #[test]
    fn provider_generation_reader_child() {
        let Some(config_root) = std::env::var_os("JYOWO_PROVIDER_GENERATION_CONFIG_ROOT") else {
            return;
        };
        let config_root = PathBuf::from(config_root);
        let started_path = PathBuf::from(
            std::env::var_os("JYOWO_PROVIDER_GENERATION_READER_STARTED")
                .expect("reader started path"),
        );
        let output_path = PathBuf::from(
            std::env::var_os("JYOWO_PROVIDER_GENERATION_READER_OUTPUT")
                .expect("reader output path"),
        );
        let lock_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(config_root.join("provider-generation.lock"))
            .expect("open provider generation lock");
        let error = fs2::FileExt::try_lock_shared(&lock_file)
            .expect_err("writer must hold the provider generation lock");
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        std::fs::write(&started_path, b"started").expect("signal reader start");
        let layout = StorageLayout::new(JyowoHome::new(
            config_root.parent().expect("config root parent"),
        ));
        let record = DesktopProviderSettingsStore::global_only_with_layout(layout)
            .load_record()
            .expect("load provider generation")
            .expect("provider generation exists");
        let selected = record.configs.first().expect("provider config exists");
        std::fs::write(
            output_path,
            serde_json::to_vec(&serde_json::json!({
                "configId": selected.id,
                "apiKey": selected.api_key,
                "defaultConfigId": record.default_config_id,
            }))
            .expect("serialize generation result"),
        )
        .expect("write generation result");
    }

    #[test]
    fn provider_generation_reader_waits_for_complete_commit() {
        let (store, temp) = store();
        store
            .save_provider_generation(
                &[provider_profile("old")],
                &[ProviderSecretEntry {
                    config_id: "old".to_owned(),
                    api_key: "old-secret".to_owned(),
                    official_quota_api_key: None,
                }],
                &ProviderSelectionRecord {
                    default_config_id: Some("old".to_owned()),
                },
            )
            .expect("seed old generation");
        let writer_store = store.clone();
        let (profiles_written_tx, profiles_written_rx) = mpsc::sync_channel(0);
        let (resume_tx, resume_rx) = mpsc::sync_channel(0);
        let writer = std::thread::spawn(move || {
            writer_store.save_provider_generation_with_writer(
                &[provider_profile("new")],
                &[ProviderSecretEntry {
                    config_id: "new".to_owned(),
                    api_key: "new-secret".to_owned(),
                    official_quota_api_key: None,
                }],
                &ProviderSelectionRecord {
                    default_config_id: Some("new".to_owned()),
                },
                |file, path, label, bytes, secret| {
                    super::super::write_bytes_file_atomic(
                        path,
                        "store.json",
                        label,
                        bytes,
                        secret,
                    )?;
                    if file == ProviderGenerationFile::Profiles {
                        profiles_written_tx.send(()).expect("signal profiles write");
                        resume_rx.recv().expect("resume generation commit");
                    }
                    Ok(())
                },
            )
        });
        profiles_written_rx.recv().expect("profiles written");

        let started_path = temp.path().join("reader-started");
        let output_path = temp.path().join("reader-output.json");
        let mut child = spawn_provider_generation_reader(&store, &started_path, &output_path);
        wait_for_path(&started_path);

        resume_tx.send(()).expect("resume writer");
        writer
            .join()
            .expect("join writer")
            .expect("commit generation");
        assert!(child.wait().expect("wait reader").success());
        let result = read_generation_result(&output_path);

        assert_eq!(result["configId"], "new");
        assert_eq!(result["apiKey"], "new-secret");
        assert_eq!(result["defaultConfigId"], "new");
    }

    #[test]
    fn provider_generation_reader_waits_for_complete_rollback() {
        let (store, temp) = store();
        store
            .save_provider_generation(
                &[provider_profile("old")],
                &[ProviderSecretEntry {
                    config_id: "old".to_owned(),
                    api_key: "old-secret".to_owned(),
                    official_quota_api_key: None,
                }],
                &ProviderSelectionRecord {
                    default_config_id: Some("old".to_owned()),
                },
            )
            .expect("seed old generation");
        let writer_store = store.clone();
        let (profiles_written_tx, profiles_written_rx) = mpsc::sync_channel(0);
        let (resume_tx, resume_rx) = mpsc::sync_channel(0);
        let writer = std::thread::spawn(move || {
            let mut inject_failure = true;
            writer_store.save_provider_generation_with_writer(
                &[provider_profile("new")],
                &[ProviderSecretEntry {
                    config_id: "new".to_owned(),
                    api_key: "new-secret".to_owned(),
                    official_quota_api_key: None,
                }],
                &ProviderSelectionRecord {
                    default_config_id: Some("new".to_owned()),
                },
                |file, path, label, bytes, secret| {
                    if file == ProviderGenerationFile::Secrets && inject_failure {
                        inject_failure = false;
                        return Err(crate::commands::error::runtime_operation_failed(
                            "injected provider secrets write failure".to_owned(),
                        ));
                    }
                    super::super::write_bytes_file_atomic(
                        path,
                        "store.json",
                        label,
                        bytes,
                        secret,
                    )?;
                    if file == ProviderGenerationFile::Profiles && inject_failure {
                        profiles_written_tx.send(()).expect("signal profiles write");
                        resume_rx.recv().expect("resume generation rollback");
                    }
                    Ok(())
                },
            )
        });
        profiles_written_rx.recv().expect("profiles written");

        let started_path = temp.path().join("rollback-reader-started");
        let output_path = temp.path().join("rollback-reader-output.json");
        let mut child = spawn_provider_generation_reader(&store, &started_path, &output_path);
        wait_for_path(&started_path);

        resume_tx.send(()).expect("resume writer");
        writer
            .join()
            .expect("join writer")
            .expect_err("generation commit must roll back");
        assert!(child.wait().expect("wait reader").success());
        let result = read_generation_result(&output_path);

        assert_eq!(result["configId"], "old");
        assert_eq!(result["apiKey"], "old-secret");
        assert_eq!(result["defaultConfigId"], "old");
    }

    #[test]
    fn provider_generation_rolls_back_every_file_when_each_commit_step_fails() {
        for failed_file in [
            ProviderGenerationFile::Profiles,
            ProviderGenerationFile::Secrets,
            ProviderGenerationFile::Selection,
        ] {
            let (store, _temp) = store();
            store
                .save_provider_generation(
                    &[provider_profile("old")],
                    &[ProviderSecretEntry {
                        config_id: "old".to_owned(),
                        api_key: "old-secret".to_owned(),
                        official_quota_api_key: None,
                    }],
                    &ProviderSelectionRecord {
                        default_config_id: Some("old".to_owned()),
                    },
                )
                .expect("seed old generation");
            let paths = provider_generation_paths(&store);
            let old_bytes = paths
                .clone()
                .map(|path| std::fs::read(path).expect("read old generation"));
            let mut injected = false;

            let error = store
                .save_provider_generation_with_writer(
                    &[provider_profile("new")],
                    &[ProviderSecretEntry {
                        config_id: "new".to_owned(),
                        api_key: "new-secret".to_owned(),
                        official_quota_api_key: None,
                    }],
                    &ProviderSelectionRecord {
                        default_config_id: Some("new".to_owned()),
                    },
                    |file, path, label, bytes, secret| {
                        if file == failed_file && !injected {
                            injected = true;
                            return Err(crate::commands::error::runtime_operation_failed(format!(
                                "injected {file:?} write failure"
                            )));
                        }
                        super::super::write_bytes_file_atomic(
                            path,
                            "store.json",
                            label,
                            bytes,
                            secret,
                        )
                    },
                )
                .expect_err("generation commit should fail");

            assert!(error.message.contains("injected"));
            for (path, expected) in paths.iter().zip(old_bytes.iter()) {
                assert_eq!(
                    std::fs::read(path).expect("read rolled back generation"),
                    *expected,
                    "{failed_file:?} failure left a mixed generation at {path:?}"
                );
            }
        }
    }

    // ── Provider profiles ──────────────────────────────────────────

    #[test]
    fn saves_and_loads_provider_profiles() {
        let (store, _temp) = store();
        let profile = ProviderProfileDefinition {
            id: "p1".to_owned(),
            display_name: "GPT-5".to_owned(),
            provider_id: "openai".to_owned(),
            model_id: "gpt-5".to_owned(),
            protocol: ModelProtocol::ChatCompletions,
            model_options: harness_contracts::ModelRequestOptions::default(),
            base_url: None,
            provider_defaults: None,
            model_descriptor: make_model_descriptor(),
        };

        store
            .save_provider_profiles(&[profile.clone()])
            .expect("save profiles");
        let loaded = store.load_provider_profiles().expect("load profiles");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "p1");
    }

    #[test]
    fn load_provider_profiles_returns_empty_when_file_missing() {
        let (store, _temp) = store();
        let loaded = store.load_provider_profiles().expect("load profiles");
        assert!(loaded.is_empty());
    }

    // ── Provider secrets ───────────────────────────────────────────

    #[test]
    fn saves_and_loads_provider_secret() {
        let (store, _temp) = store();
        let entry = ProviderSecretEntry {
            config_id: "c1".to_owned(),
            api_key: "sk-test".to_owned(),
            official_quota_api_key: None,
        };

        store.save_provider_secret(&entry).expect("save secret");
        let loaded = store
            .load_provider_secret("c1")
            .expect("load secret")
            .expect("present");
        assert_eq!(loaded.api_key, "sk-test");
    }

    #[test]
    fn provider_secrets_metadata_hides_raw_keys() {
        let (store, _temp) = store();
        store
            .save_provider_secret(&ProviderSecretEntry {
                config_id: "c1".to_owned(),
                api_key: "sk-secret".to_owned(),
                official_quota_api_key: Some("quota-key".to_owned()),
            })
            .expect("save secret");

        let metadata = store
            .load_provider_secrets_metadata()
            .expect("load metadata");
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].config_id, "c1");
        assert!(metadata[0].has_api_key);
        assert!(metadata[0].has_official_quota_api_key);
    }

    #[test]
    fn delete_provider_secret_removes_entry() {
        let (store, _temp) = store();
        store
            .save_provider_secret(&ProviderSecretEntry {
                config_id: "c1".to_owned(),
                api_key: "sk-test".to_owned(),
                official_quota_api_key: None,
            })
            .expect("save secret");

        store.delete_provider_secret("c1").expect("delete secret");
        assert!(store.load_provider_secret("c1").expect("load").is_none());
    }

    #[test]
    fn save_provider_secret_updates_existing_entry() {
        let (store, _temp) = store();
        store
            .save_provider_secret(&ProviderSecretEntry {
                config_id: "c1".to_owned(),
                api_key: "old-key".to_owned(),
                official_quota_api_key: None,
            })
            .expect("save");

        store
            .save_provider_secret(&ProviderSecretEntry {
                config_id: "c1".to_owned(),
                api_key: "new-key".to_owned(),
                official_quota_api_key: None,
            })
            .expect("update");

        let loaded = store
            .load_provider_secret("c1")
            .expect("load")
            .expect("present");
        assert_eq!(loaded.api_key, "new-key");
    }

    #[test]
    #[cfg(unix)]
    fn provider_secrets_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let (store, _temp) = store();
        store
            .save_provider_secret(&ProviderSecretEntry {
                config_id: "c1".to_owned(),
                api_key: "sk-test".to_owned(),
                official_quota_api_key: None,
            })
            .expect("save secret");

        let path = store.layout().global_provider_secrets_file();
        let metadata = std::fs::metadata(&path).expect("metadata");
        let mode = metadata.permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "secret file must be owner-only");
    }

    // ── Provider selection ─────────────────────────────────────────

    #[test]
    fn saves_and_loads_global_provider_selection() {
        let (store, _temp) = store();
        let record = ProviderSelectionRecord {
            default_config_id: Some("p1".to_owned()),
        };
        store.save_global_provider_selection(&record).expect("save");
        let loaded = store.load_global_provider_selection().expect("load");
        assert_eq!(loaded.default_config_id.as_deref(), Some("p1"));
    }

    #[test]
    fn load_global_provider_selection_returns_default_when_missing() {
        let (store, _temp) = store();
        let loaded = store.load_global_provider_selection().expect("load");
        assert_eq!(loaded.default_config_id, None);
    }

    // ── Execution defaults ─────────────────────────────────────────

    #[test]
    fn saves_and_loads_execution_defaults() {
        let (store, _temp) = store();
        let record = ExecutionDefaultsRecord {
            permission_mode: PermissionMode::Auto,
            tool_profile: ToolProfile::Minimal,
            context_compression_trigger_ratio: 0.75,
            subagents_enabled: true,
            agent_teams_enabled: false,
            background_agents_enabled: true,
        };
        store.save_execution_defaults(&record).expect("save");
        let loaded = store.load_execution_defaults().expect("load");
        assert_eq!(loaded.permission_mode, PermissionMode::Auto);
        assert_eq!(loaded.tool_profile, ToolProfile::Minimal);
        assert!((loaded.context_compression_trigger_ratio - 0.75).abs() < f32::EPSILON);
        assert!(loaded.subagents_enabled);
        assert!(!loaded.agent_teams_enabled);
        assert!(loaded.background_agents_enabled);
    }

    #[test]
    fn load_execution_defaults_returns_default_when_missing() {
        let (store, _temp) = store();
        let loaded = store.load_execution_defaults().expect("load");
        assert_eq!(loaded, ExecutionDefaultsRecord::default());
    }

    // ── MCP presets ────────────────────────────────────────────────

    #[test]
    fn saves_and_loads_mcp_presets() {
        let (store, _temp) = store();
        let preset = McpPresetRecord {
            id: "browser".to_owned(),
            display_name: "Browser".to_owned(),
            description: "Browser MCP server".to_owned(),
            transport: McpPresetTransport::Http {
                url: "http://localhost:3000".to_owned(),
                headers: vec![],
                headers_from_env: vec![],
                bearer_token_env_var: None,
            },
        };
        store.save_mcp_presets(&[preset.clone()]).expect("save");
        let loaded = store.load_mcp_presets().expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "browser");
    }

    #[test]
    fn load_mcp_presets_returns_empty_when_missing() {
        let (store, _temp) = store();
        let loaded = store.load_mcp_presets().expect("load");
        assert!(loaded.is_empty());
    }

    // ── Agent profiles ─────────────────────────────────────────────

    #[test]
    fn saves_and_loads_global_agent_profiles() {
        let (store, _temp) = store();
        let profile = AgentProfile {
            id: "ap1".to_owned(),
            scope: AgentProfileScope::User,
            role: "Developer".to_owned(),
            description: "A developer agent".to_owned(),
            model_config_override: None,
            tool_allowlist: None,
            tool_blocklist: vec![],
            sandbox_inheritance: AgentProfileSandboxInheritance::InheritParent,
            memory_scope: AgentProfileMemoryScope::ReadWrite,
            context_mode: AgentProfileContextMode::Focused,
            max_turns: 100,
            max_depth: 3,
            default_workspace_isolation: AgentWorkspaceIsolationMode::GitWorktree,
        };
        store
            .save_global_agent_profiles(&[profile.clone()])
            .expect("save");
        let loaded = store.load_global_agent_profiles().expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "ap1");
    }

    #[test]
    fn load_global_agent_profiles_returns_empty_when_missing() {
        let (store, _temp) = store();
        let loaded = store.load_global_agent_profiles().expect("load");
        assert!(loaded.is_empty());
    }

    // ── Skill selection ────────────────────────────────────────────

    #[test]
    fn saves_and_loads_global_skill_selection() {
        let (store, _temp) = store();
        let record = SkillSelectionRecord {
            enabled: vec!["skill-a".to_owned(), "skill-b".to_owned()],
        };
        store.save_global_skill_selection(&record).expect("save");
        let loaded = store.load_global_skill_selection().expect("load");
        assert_eq!(loaded.enabled.len(), 2);
        assert!(loaded.enabled.contains(&"skill-a".to_owned()));
    }

    #[test]
    fn load_global_skill_selection_returns_default_when_missing() {
        let (store, _temp) = store();
        let loaded = store.load_global_skill_selection().expect("load");
        assert!(loaded.enabled.is_empty());
    }

    // ── Path resolution ────────────────────────────────────────────

    #[test]
    fn resolves_all_global_config_file_paths() {
        let layout = StorageLayout::new(JyowoHome::new(Path::new("/home/alice/.jyowo")));
        let store = GlobalConfigStore::new(layout);

        assert_eq!(
            store.layout().global_provider_profiles_file(),
            Path::new("/home/alice/.jyowo/config/provider-profiles.json")
        );
        assert_eq!(
            store.layout().global_provider_secrets_file(),
            Path::new("/home/alice/.jyowo/config/provider-secrets.json")
        );
        assert_eq!(
            store.layout().global_provider_selection_file(),
            Path::new("/home/alice/.jyowo/config/provider-selection.json")
        );
        assert_eq!(
            store.layout().global_execution_defaults_file(),
            Path::new("/home/alice/.jyowo/config/execution-defaults.json")
        );
        assert_eq!(
            store.layout().global_mcp_presets_file(),
            Path::new("/home/alice/.jyowo/config/mcp-presets.json")
        );
        assert_eq!(
            store.layout().global_agent_profiles_file(),
            Path::new("/home/alice/.jyowo/config/agent-profiles.json")
        );
        assert_eq!(
            store.layout().global_skills_file(),
            Path::new("/home/alice/.jyowo/config/skills.json")
        );
    }

    #[test]
    fn store_methods_resolve_under_temp_home() {
        let (store, _temp) = store();

        store
            .save_provider_profiles(&[ProviderProfileDefinition {
                id: "p1".to_owned(),
                display_name: "Test".to_owned(),
                provider_id: "openai".to_owned(),
                model_id: "gpt-5".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                model_options: harness_contracts::ModelRequestOptions::default(),
                base_url: None,
                provider_defaults: None,
                model_descriptor: make_model_descriptor(),
            }])
            .expect("save");

        let expected_path = store.layout().global_provider_profiles_file();
        assert!(expected_path.exists(), "expected file at {expected_path:?}");
    }
}
