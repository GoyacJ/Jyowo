use std::fmt;
use std::path::{Path, PathBuf};

use harness_contracts::SessionId;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum StorageScope {
    Project { workspace_root: PathBuf },
    GlobalConversation,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RuntimeScope {
    Project { workspace_root: PathBuf },
    GlobalConversation { conversation_id: SessionId },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ConfigScope {
    Project,
    GlobalOnly,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum StorageLayoutError {
    ConversationScopeMismatch {
        scope_conversation_id: SessionId,
        requested_conversation_id: SessionId,
    },
}

impl StorageLayoutError {
    pub fn scope_conversation_id(&self) -> Option<SessionId> {
        match self {
            Self::ConversationScopeMismatch {
                scope_conversation_id,
                ..
            } => Some(*scope_conversation_id),
        }
    }

    pub fn requested_conversation_id(&self) -> Option<SessionId> {
        match self {
            Self::ConversationScopeMismatch {
                requested_conversation_id,
                ..
            } => Some(*requested_conversation_id),
        }
    }
}

impl fmt::Display for StorageLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConversationScopeMismatch {
                scope_conversation_id,
                requested_conversation_id,
            } => write!(
                formatter,
                "global conversation scope id {scope_conversation_id} does not match requested workdir id {requested_conversation_id}"
            ),
        }
    }
}

impl std::error::Error for StorageLayoutError {}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct JyowoHome {
    root: PathBuf,
}

impl JyowoHome {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StorageLayout {
    home: JyowoHome,
}

impl StorageLayout {
    pub fn new(home: JyowoHome) -> Self {
        Self { home }
    }

    pub fn home(&self) -> &JyowoHome {
        &self.home
    }

    pub fn global_config_root(&self) -> PathBuf {
        self.home.root().join("config")
    }

    pub fn global_runtime_root(&self) -> PathBuf {
        self.home.root().join("runtime")
    }

    pub fn global_skills_root(&self) -> PathBuf {
        self.home.root().join("skills")
    }

    pub fn global_plugins_root(&self) -> PathBuf {
        self.home.root().join("plugins")
    }

    pub fn project_config_root(&self, workspace_root: impl AsRef<Path>) -> PathBuf {
        workspace_root.as_ref().join(".jyowo").join("config")
    }

    pub fn project_runtime_root(&self, workspace_root: impl AsRef<Path>) -> PathBuf {
        workspace_root.as_ref().join(".jyowo").join("runtime")
    }

    pub fn project_skills_root(&self, workspace_root: impl AsRef<Path>) -> PathBuf {
        workspace_root.as_ref().join(".jyowo").join("skills")
    }

    pub fn project_plugins_root(&self, workspace_root: impl AsRef<Path>) -> PathBuf {
        workspace_root.as_ref().join(".jyowo").join("plugins")
    }

    pub fn config_root_for(&self, scope: &StorageScope) -> PathBuf {
        match scope {
            StorageScope::Project { workspace_root } => self.project_config_root(workspace_root),
            StorageScope::GlobalConversation => self.global_config_root(),
        }
    }

    pub fn runtime_root_for_storage(&self, scope: &StorageScope) -> PathBuf {
        match scope {
            StorageScope::Project { workspace_root } => self.project_runtime_root(workspace_root),
            StorageScope::GlobalConversation => self.global_runtime_root(),
        }
    }

    pub fn skills_root_for(&self, scope: &StorageScope) -> PathBuf {
        match scope {
            StorageScope::Project { workspace_root } => self.project_skills_root(workspace_root),
            StorageScope::GlobalConversation => self.global_skills_root(),
        }
    }

    pub fn plugins_root_for(&self, scope: &StorageScope) -> PathBuf {
        match scope {
            StorageScope::Project { workspace_root } => self.project_plugins_root(workspace_root),
            StorageScope::GlobalConversation => self.global_plugins_root(),
        }
    }

    pub fn global_provider_profiles_file(&self) -> PathBuf {
        self.global_config_root().join("provider-profiles.json")
    }

    pub fn global_provider_secrets_file(&self) -> PathBuf {
        self.global_config_root().join("provider-secrets.json")
    }

    pub fn global_provider_selection_file(&self) -> PathBuf {
        self.global_config_root().join("provider-selection.json")
    }

    pub fn global_provider_routes_file(&self) -> PathBuf {
        self.global_config_root()
            .join("provider-capability-routes.json")
    }

    pub fn global_execution_defaults_file(&self) -> PathBuf {
        self.global_config_root().join("execution-defaults.json")
    }

    pub fn global_mcp_servers_file(&self) -> PathBuf {
        self.global_config_root().join("mcp-servers.json")
    }

    pub fn global_mcp_presets_file(&self) -> PathBuf {
        self.global_config_root().join("mcp-presets.json")
    }

    pub fn global_automations_file(&self) -> PathBuf {
        self.global_config_root().join("automations.json")
    }

    pub fn global_automation_runs_file(&self) -> PathBuf {
        self.global_runtime_root().join("automation-runs.jsonl")
    }

    pub fn global_agent_profiles_file(&self) -> PathBuf {
        self.global_config_root().join("agent-profiles.json")
    }

    pub fn global_skills_file(&self) -> PathBuf {
        self.global_config_root().join("skills.json")
    }

    pub fn global_skills_index_file(&self) -> PathBuf {
        self.global_skills_root().join("index.json")
    }

    pub fn global_plugins_index_file(&self) -> PathBuf {
        self.global_plugins_root().join("index.json")
    }

    pub fn project_provider_selection_file(&self, workspace_root: impl AsRef<Path>) -> PathBuf {
        self.project_config_root(workspace_root)
            .join("provider-selection.json")
    }

    pub fn project_provider_routes_file(&self, workspace_root: impl AsRef<Path>) -> PathBuf {
        self.project_config_root(workspace_root)
            .join("provider-capability-routes.json")
    }

    pub fn project_execution_overrides_file(&self, workspace_root: impl AsRef<Path>) -> PathBuf {
        self.project_config_root(workspace_root)
            .join("execution-overrides.json")
    }

    pub fn project_mcp_servers_file(&self, workspace_root: impl AsRef<Path>) -> PathBuf {
        self.project_config_root(workspace_root)
            .join("mcp-servers.json")
    }

    pub fn project_automations_file(&self, workspace_root: impl AsRef<Path>) -> PathBuf {
        self.project_config_root(workspace_root)
            .join("automations.json")
    }

    pub fn project_skills_file(&self, workspace_root: impl AsRef<Path>) -> PathBuf {
        self.project_config_root(workspace_root).join("skills.json")
    }

    pub fn project_plugins_file(&self, workspace_root: impl AsRef<Path>) -> PathBuf {
        self.project_config_root(workspace_root)
            .join("plugins.json")
    }

    pub fn project_agent_profile_selection_file(
        &self,
        workspace_root: impl AsRef<Path>,
    ) -> PathBuf {
        self.project_config_root(workspace_root)
            .join("agent-profile-selection.json")
    }

    pub fn runtime_root_for(&self, scope: &RuntimeScope) -> PathBuf {
        match scope {
            RuntimeScope::Project { workspace_root } => self.project_runtime_root(workspace_root),
            RuntimeScope::GlobalConversation { .. } => {
                self.global_runtime_root().join("global-conversations")
            }
        }
    }

    pub fn conversation_workdir_for(
        &self,
        scope: &RuntimeScope,
        conversation_id: SessionId,
    ) -> Result<PathBuf, StorageLayoutError> {
        match scope {
            RuntimeScope::Project { workspace_root } => Ok(workspace_root.clone()),
            RuntimeScope::GlobalConversation {
                conversation_id: scope_conversation_id,
            } => {
                if *scope_conversation_id != conversation_id {
                    return Err(StorageLayoutError::ConversationScopeMismatch {
                        scope_conversation_id: *scope_conversation_id,
                        requested_conversation_id: conversation_id,
                    });
                }

                Ok(self
                    .runtime_root_for(scope)
                    .join("workdir")
                    .join(scope_conversation_id.to_string()))
            }
        }
    }

    pub fn runtime_memory_file_for(&self, scope: &RuntimeScope) -> PathBuf {
        self.runtime_root_for(scope)
            .join("memory")
            .join("memory.sqlite3")
    }

    pub fn runtime_events_dir_for(&self, scope: &RuntimeScope) -> PathBuf {
        self.runtime_root_for(scope).join("events")
    }

    pub fn runtime_blobs_dir_for(&self, scope: &RuntimeScope) -> PathBuf {
        self.runtime_root_for(scope).join("blobs")
    }

    pub fn runtime_provider_continuations_file_for(&self, scope: &RuntimeScope) -> PathBuf {
        self.runtime_root_for(scope)
            .join("provider-continuations.jsonl")
    }

    pub fn runtime_permission_decisions_file_for(&self, scope: &RuntimeScope) -> PathBuf {
        self.runtime_root_for(scope)
            .join("permission-decisions.json")
    }

    pub fn runtime_agent_worktrees_dir_for(&self, scope: &RuntimeScope) -> PathBuf {
        self.runtime_root_for(scope).join("agent-worktrees")
    }

    pub fn runtime_provider_diagnostics_file_for(&self, scope: &RuntimeScope) -> PathBuf {
        self.runtime_root_for(scope)
            .join("provider-diagnostics.json")
    }

    pub fn runtime_provider_quota_cache_file_for(&self, scope: &RuntimeScope) -> PathBuf {
        self.runtime_root_for(scope)
            .join("provider-quota-cache.json")
    }

    pub fn runtime_layout_for_project(&self, workspace_root: impl AsRef<Path>) -> RuntimeLayout {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let scope = RuntimeScope::Project {
            workspace_root: workspace_root.clone(),
        };
        RuntimeLayout {
            workspace_root: Some(workspace_root.clone()),
            runtime_root: self.runtime_root_for(&scope),
            conversation_cwd: workspace_root,
            config_scope: ConfigScope::Project,
            scope,
        }
    }

    pub fn runtime_layout_for_global_conversation(
        &self,
        conversation_id: SessionId,
    ) -> RuntimeLayout {
        let scope = RuntimeScope::GlobalConversation { conversation_id };
        RuntimeLayout {
            workspace_root: None,
            runtime_root: self.runtime_root_for(&scope),
            conversation_cwd: self
                .conversation_workdir_for(&scope, conversation_id)
                .expect("runtime layout uses matching global conversation id"),
            config_scope: ConfigScope::GlobalOnly,
            scope,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimeLayout {
    pub scope: RuntimeScope,
    pub workspace_root: Option<PathBuf>,
    pub runtime_root: PathBuf,
    pub conversation_cwd: PathBuf,
    pub config_scope: ConfigScope,
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use harness_contracts::SessionId;

    use super::{ConfigScope, JyowoHome, RuntimeScope, StorageLayout, StorageScope};

    fn layout() -> StorageLayout {
        StorageLayout::new(JyowoHome::new(PathBuf::from("/home/alice/.jyowo")))
    }

    fn workspace() -> PathBuf {
        PathBuf::from("/workspaces/jyowo")
    }

    #[test]
    fn storage_roots_resolve_global_and_project_scopes() {
        let layout = layout();
        let workspace = workspace();
        let project_scope = StorageScope::Project {
            workspace_root: workspace.clone(),
        };
        let global_scope = StorageScope::GlobalConversation;

        assert_eq!(
            layout.global_config_root(),
            Path::new("/home/alice/.jyowo/config")
        );
        assert_eq!(
            layout.global_runtime_root(),
            Path::new("/home/alice/.jyowo/runtime")
        );
        assert_eq!(
            layout.global_skills_root(),
            Path::new("/home/alice/.jyowo/skills")
        );
        assert_eq!(
            layout.global_plugins_root(),
            Path::new("/home/alice/.jyowo/plugins")
        );
        assert_eq!(
            layout.config_root_for(&global_scope),
            layout.global_config_root()
        );
        assert_eq!(
            layout.runtime_root_for_storage(&global_scope),
            layout.global_runtime_root()
        );
        assert_eq!(
            layout.skills_root_for(&global_scope),
            layout.global_skills_root()
        );
        assert_eq!(
            layout.plugins_root_for(&global_scope),
            layout.global_plugins_root()
        );

        assert_eq!(
            layout.project_config_root(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/config")
        );
        assert_eq!(
            layout.project_runtime_root(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/runtime")
        );
        assert_eq!(
            layout.project_skills_root(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/skills")
        );
        assert_eq!(
            layout.project_plugins_root(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/plugins")
        );
        assert_eq!(
            layout.config_root_for(&project_scope),
            layout.project_config_root(&workspace)
        );
        assert_eq!(
            layout.runtime_root_for_storage(&project_scope),
            layout.project_runtime_root(&workspace)
        );
        assert_eq!(
            layout.skills_root_for(&project_scope),
            layout.project_skills_root(&workspace)
        );
        assert_eq!(
            layout.plugins_root_for(&project_scope),
            layout.project_plugins_root(&workspace)
        );
    }

    #[test]
    fn config_files_resolve_to_target_scopes() {
        let layout = layout();
        let workspace = workspace();

        assert_eq!(
            layout.global_provider_profiles_file(),
            Path::new("/home/alice/.jyowo/config/provider-profiles.json")
        );
        assert_eq!(
            layout.global_provider_secrets_file(),
            Path::new("/home/alice/.jyowo/config/provider-secrets.json")
        );
        assert_eq!(
            layout.global_provider_selection_file(),
            Path::new("/home/alice/.jyowo/config/provider-selection.json")
        );
        assert_eq!(
            layout.global_provider_routes_file(),
            Path::new("/home/alice/.jyowo/config/provider-capability-routes.json")
        );
        assert_eq!(
            layout.global_execution_defaults_file(),
            Path::new("/home/alice/.jyowo/config/execution-defaults.json")
        );
        assert_eq!(
            layout.global_mcp_servers_file(),
            Path::new("/home/alice/.jyowo/config/mcp-servers.json")
        );
        assert_eq!(
            layout.global_mcp_presets_file(),
            Path::new("/home/alice/.jyowo/config/mcp-presets.json")
        );
        assert_eq!(
            layout.global_automations_file(),
            Path::new("/home/alice/.jyowo/config/automations.json")
        );
        assert_eq!(
            layout.global_automation_runs_file(),
            Path::new("/home/alice/.jyowo/runtime/automation-runs.jsonl")
        );
        assert_eq!(
            layout.global_agent_profiles_file(),
            Path::new("/home/alice/.jyowo/config/agent-profiles.json")
        );
        assert_eq!(
            layout.global_skills_file(),
            Path::new("/home/alice/.jyowo/config/skills.json")
        );
        assert_eq!(
            layout.global_skills_index_file(),
            Path::new("/home/alice/.jyowo/skills/index.json")
        );
        assert_eq!(
            layout.global_plugins_index_file(),
            Path::new("/home/alice/.jyowo/plugins/index.json")
        );

        assert_eq!(
            layout.project_provider_selection_file(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/config/provider-selection.json")
        );
        assert_eq!(
            layout.project_provider_routes_file(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/config/provider-capability-routes.json")
        );
        assert_eq!(
            layout.project_execution_overrides_file(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/config/execution-overrides.json")
        );
        assert_eq!(
            layout.project_mcp_servers_file(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/config/mcp-servers.json")
        );
        assert_eq!(
            layout.project_automations_file(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/config/automations.json")
        );
        assert_eq!(
            layout.project_skills_file(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/config/skills.json")
        );
        assert_eq!(
            layout.project_plugins_file(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/config/plugins.json")
        );
        assert_eq!(
            layout.project_agent_profile_selection_file(&workspace),
            Path::new("/workspaces/jyowo/.jyowo/config/agent-profile-selection.json")
        );
    }

    #[test]
    fn runtime_paths_resolve_project_and_global_conversation_scopes() {
        let layout = layout();
        let workspace = workspace();
        let conversation_id = SessionId::from_u128(42);
        let project_scope = RuntimeScope::Project {
            workspace_root: workspace.clone(),
        };
        let global_scope = RuntimeScope::GlobalConversation { conversation_id };

        assert_eq!(
            layout.runtime_root_for(&project_scope),
            Path::new("/workspaces/jyowo/.jyowo/runtime")
        );
        assert_eq!(
            layout.runtime_root_for(&global_scope),
            Path::new("/home/alice/.jyowo/runtime/global-conversations")
        );
        assert_eq!(
            layout
                .conversation_workdir_for(&project_scope, conversation_id)
                .expect("project workdir should resolve"),
            Path::new("/workspaces/jyowo")
        );
        assert_eq!(
            layout
                .conversation_workdir_for(&global_scope, conversation_id)
                .expect("global conversation workdir should resolve"),
            Path::new("/home/alice/.jyowo/runtime/global-conversations/workdir/0000000000000000000000001A")
        );

        assert_eq!(
            layout.runtime_events_dir_for(&global_scope),
            Path::new("/home/alice/.jyowo/runtime/global-conversations/events")
        );
        assert_eq!(
            layout.runtime_blobs_dir_for(&global_scope),
            Path::new("/home/alice/.jyowo/runtime/global-conversations/blobs")
        );
        assert_eq!(
            layout.runtime_provider_continuations_file_for(&global_scope),
            Path::new(
                "/home/alice/.jyowo/runtime/global-conversations/provider-continuations.jsonl"
            )
        );
        assert_eq!(
            layout.runtime_permission_decisions_file_for(&global_scope),
            Path::new("/home/alice/.jyowo/runtime/global-conversations/permission-decisions.json")
        );
        assert_eq!(
            layout.runtime_memory_file_for(&project_scope),
            Path::new("/workspaces/jyowo/.jyowo/runtime/memory/memory.sqlite3")
        );
        assert_eq!(
            layout.runtime_memory_file_for(&global_scope),
            Path::new("/home/alice/.jyowo/runtime/global-conversations/memory/memory.sqlite3")
        );
        assert_eq!(
            layout.runtime_agent_worktrees_dir_for(&global_scope),
            Path::new("/home/alice/.jyowo/runtime/global-conversations/agent-worktrees")
        );
        assert_eq!(
            layout.runtime_provider_diagnostics_file_for(&project_scope),
            Path::new("/workspaces/jyowo/.jyowo/runtime/provider-diagnostics.json")
        );
        assert_eq!(
            layout.runtime_provider_diagnostics_file_for(&global_scope),
            Path::new("/home/alice/.jyowo/runtime/global-conversations/provider-diagnostics.json")
        );
        assert_eq!(
            layout.runtime_provider_quota_cache_file_for(&project_scope),
            Path::new("/workspaces/jyowo/.jyowo/runtime/provider-quota-cache.json")
        );
        assert_eq!(
            layout.runtime_provider_quota_cache_file_for(&global_scope),
            Path::new("/home/alice/.jyowo/runtime/global-conversations/provider-quota-cache.json")
        );
    }

    #[test]
    fn runtime_layouts_preserve_scope_and_cwd_rules() {
        let layout = layout();
        let workspace = workspace();
        let conversation_id = SessionId::from_u128(42);
        let other_conversation_id = SessionId::from_u128(43);

        let project_layout = layout.runtime_layout_for_project(&workspace);
        assert_eq!(
            project_layout.workspace_root.as_deref(),
            Some(workspace.as_path())
        );
        assert_eq!(
            project_layout.runtime_root,
            Path::new("/workspaces/jyowo/.jyowo/runtime")
        );
        assert_eq!(project_layout.conversation_cwd, workspace);
        assert_eq!(project_layout.config_scope, ConfigScope::Project);

        let global_layout = layout.runtime_layout_for_global_conversation(conversation_id);
        let other_global_layout =
            layout.runtime_layout_for_global_conversation(other_conversation_id);
        assert_eq!(global_layout.workspace_root, None);
        assert_eq!(
            global_layout.runtime_root,
            Path::new("/home/alice/.jyowo/runtime/global-conversations")
        );
        assert_eq!(
            global_layout.conversation_cwd,
            Path::new("/home/alice/.jyowo/runtime/global-conversations/workdir/0000000000000000000000001A")
        );
        assert_eq!(global_layout.config_scope, ConfigScope::GlobalOnly);
        assert_ne!(
            global_layout.runtime_root,
            Path::new("/home/alice/.jyowo/unconfigured")
        );
        assert_ne!(global_layout.runtime_root, Path::new("/home/alice"));
        assert_ne!(
            global_layout.conversation_cwd,
            Path::new("/home/alice/.jyowo/unconfigured")
        );
        assert_ne!(global_layout.conversation_cwd, Path::new("/home/alice"));
        assert_ne!(
            global_layout.conversation_cwd,
            other_global_layout.conversation_cwd
        );
    }

    #[test]
    fn global_conversation_workdir_fails_on_mismatched_conversation_id() {
        let layout = layout();
        let scope = RuntimeScope::GlobalConversation {
            conversation_id: SessionId::from_u128(42),
        };

        let error = layout
            .conversation_workdir_for(&scope, SessionId::from_u128(43))
            .expect_err("mismatched conversation id should fail");

        assert_eq!(
            error.scope_conversation_id(),
            Some(SessionId::from_u128(42))
        );
        assert_eq!(
            error.requested_conversation_id(),
            Some(SessionId::from_u128(43))
        );
    }
}
