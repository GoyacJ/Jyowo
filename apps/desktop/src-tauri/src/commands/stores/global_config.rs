use std::path::PathBuf;

use harness_contracts::{
    AgentProfile, ExecutionDefaultsRecord, McpPresetRecord, ProviderProfileDefinition,
    ProviderSecretEntry, ProviderSecretMetadata, ProviderSelectionRecord, SkillSelectionRecord,
};

use crate::commands::error::CommandErrorPayload;
use crate::storage_layout::{RuntimeScope, StorageLayout};

use super::{
    ensure_app_dir_no_symlink, read_json_file, read_secret_json_file, write_json_file_atomic,
    write_secret_json_file_atomic,
};

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
        Ok(read_json_file::<Vec<AgentProfile>>(&path, "agent profiles")?.unwrap_or_default())
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

    pub fn save_global_skill_selection(
        &self,
        record: &SkillSelectionRecord,
    ) -> Result<(), CommandErrorPayload> {
        let path = self.layout.global_skills_file();
        ensure_config_dir(&path, "skill selection")?;
        write_json_file_atomic(&path, "skill selection", record)
    }
}

/// Ensure the parent config directory exists without following symlinks.
fn ensure_config_dir(path: &PathBuf, label: &str) -> Result<(), CommandErrorPayload> {
    let parent = path.parent().ok_or_else(|| {
        crate::commands::error::runtime_operation_failed(format!(
            "{label} path has no parent directory"
        ))
    })?;
    ensure_app_dir_no_symlink(parent, &format!("{label} directory"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use harness_contracts::{
        AgentProfile, AgentProfileContextMode, AgentProfileMemoryScope,
        AgentProfileSandboxInheritance, AgentProfileScope, AgentWorkspaceIsolationMode,
        ExecutionDefaultsRecord, McpPresetRecord, McpPresetTransport, ModelProtocol,
        PermissionMode, ProviderProfileConversationCapability, ProviderProfileDefinition,
        ProviderProfileModelDescriptor, ProviderProfileModelLifecycle, ProviderSecretEntry,
        ProviderSelectionRecord, SkillSelectionRecord, ToolProfile,
    };

    use crate::storage_layout::{JyowoHome, StorageLayout};

    use super::GlobalConfigStore;

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
            base_url: None,
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
                base_url: None,
                model_descriptor: make_model_descriptor(),
            }])
            .expect("save");

        let expected_path = store.layout().global_provider_profiles_file();
        assert!(expected_path.exists(), "expected file at {expected_path:?}");
    }
}
