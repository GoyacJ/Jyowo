use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use harness_contracts::{
    validate_agent_profile, validate_persisted_mcp_server, validate_provider_capability_route,
    AgentProfile, AgentProfileSelectionRecord, CapabilityRouteKind, ExecutionDefaultsRecord,
    ExecutionOverridesRecord, LocalIsolationTag, McpServerConfigRecord, McpServerSource,
    McpServerTransportConfig, PluginId, PluginSelectionRecord, ProviderCapabilityRoute,
    ProviderCapabilityRouteSettings, ProviderCredential, ProviderCredentialResolveContext,
    ProviderCredentialResolverCap, ProviderProfileDefinition, ProviderSecretEntry,
    ProviderSecretsRecord, ProviderSelectionRecord, ProviderServiceCapability,
    ProviderServiceCategory, SandboxMode, SkillConfigDocument, SkillSelectionRecord, ToolError,
};
use harness_plugin::{
    CargoExtensionRuntimeLoader, DiscoverySource, FileManifestLoader, ManifestLoadReport,
    ManifestLoaderError, ManifestOrigin, Plugin, PluginConfig, PluginManifest,
    PluginManifestLoader, PluginName, PluginRegistry, PluginRuntimeLoader, RuntimeLoaderError,
};
use harness_sandbox::{LocalIsolation, LocalSandbox};
use jyowo_harness_sdk::{
    builtin_agent_profiles,
    ext::{DirectorySourceKind, SkillLoader, SkillSourceConfig},
    KeyringSkillSecretStore, SkillConfigSnapshot, SkillConfigStoreError, SkillSecretStore,
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

use crate::{ProviderConfigError, ProviderConfigResolver, ResolvedProviderConfig};

const PROVIDER_SELECTION_FILE: &str = "provider-selection.json";
const EXECUTION_OVERRIDES_FILE: &str = "execution-overrides.json";
const PROVIDER_ROUTES_FILE: &str = "provider-capability-routes.json";
const MCP_SERVERS_FILE: &str = "mcp-servers.json";
const SKILLS_FILE: &str = "skills.json";
const SKILL_CONFIG_FILE: &str = "skill-config.json";
const PLUGINS_FILE: &str = "plugins.json";
const AGENT_PROFILES_FILE: &str = "agent-profiles.json";
const AGENT_PROFILE_SELECTION_FILE: &str = "agent-profile-selection.json";
const PROVIDER_PROFILES_FILE: &str = "provider-profiles.json";
const PROVIDER_SECRETS_FILE: &str = "provider-secrets.json";
const MAX_FROZEN_PLUGIN_EXECUTABLE_BYTES: u64 = 64 * 1024 * 1024;

/// Resolves immutable SDK factory inputs for one canonical workspace.
#[derive(Clone)]
pub struct RuntimeConfigResolver {
    global_config_root: PathBuf,
    skill_secret_store: Arc<dyn SkillSecretStore>,
}

impl RuntimeConfigResolver {
    #[must_use]
    pub fn new(global_config_root: impl Into<PathBuf>) -> Self {
        Self {
            global_config_root: global_config_root.into(),
            skill_secret_store: Arc::new(KeyringSkillSecretStore),
        }
    }

    #[must_use]
    pub fn with_skill_secret_store(
        mut self,
        skill_secret_store: Arc<dyn SkillSecretStore>,
    ) -> Self {
        self.skill_secret_store = skill_secret_store;
        self
    }

    pub fn resolve_memory_database_path(
        &self,
        workspace_root: Option<&Path>,
    ) -> Result<PathBuf, RuntimeConfigError> {
        let global_config_root =
            self.global_config_root
                .canonicalize()
                .map_err(|source| RuntimeConfigError::Read {
                    kind: "global config root",
                    path: self.global_config_root.clone(),
                    source,
                })?;
        let global_home =
            global_config_root
                .parent()
                .ok_or_else(|| RuntimeConfigError::Invalid {
                    kind: "global config root",
                    reason: "config root has no Jyowo home parent".to_owned(),
                })?;
        let workspace_root = workspace_root.map(canonical_workspace_root).transpose()?;
        let memory_database_path =
            daemon_memory_database_path(global_home, workspace_root.as_deref());
        ensure_memory_parent_path(&memory_database_path, global_home)?;
        Ok(memory_database_path)
    }

    pub(crate) fn resolve_memory_export_directory(
        &self,
        workspace_root: Option<&Path>,
    ) -> Result<PathBuf, RuntimeConfigError> {
        let memory_database_path = self.resolve_memory_database_path(workspace_root)?;
        let global_config_root =
            self.global_config_root
                .canonicalize()
                .map_err(|source| RuntimeConfigError::Read {
                    kind: "global config root",
                    path: self.global_config_root.clone(),
                    source,
                })?;
        let global_home =
            global_config_root
                .parent()
                .ok_or_else(|| RuntimeConfigError::Invalid {
                    kind: "global config root",
                    reason: "config root has no Jyowo home parent".to_owned(),
                })?;
        let export_directory = memory_database_path
            .parent()
            .ok_or_else(|| RuntimeConfigError::Invalid {
                kind: "runtime memory export directory",
                reason: "memory database path has no parent".to_owned(),
            })?
            .join("exports");
        let relative = export_directory.strip_prefix(global_home).map_err(|_| {
            RuntimeConfigError::Invalid {
                kind: "runtime memory export directory",
                reason: "memory export path escaped daemon storage root".to_owned(),
            }
        })?;
        ensure_secure_directory_chain(global_home, relative, "runtime memory export directory")?;
        Ok(export_directory)
    }

    pub fn resolve(
        &self,
        workspace_root: &Path,
        model_config_id: Option<&str>,
    ) -> Result<RuntimeConfigSnapshot, RuntimeConfigError> {
        let workspace_root = canonical_workspace_root(workspace_root)?;
        let project_root = workspace_root.join(".jyowo");
        let project_config_root = project_root.join("config");
        reject_symlink_if_present(&project_root, "project settings root")?;
        reject_symlink_if_present(&project_config_root, "project config root")?;
        validate_runtime_path_roots(&workspace_root)?;

        let global_config_root =
            self.global_config_root
                .canonicalize()
                .map_err(|source| RuntimeConfigError::Read {
                    kind: "global config root",
                    path: self.global_config_root.clone(),
                    source,
                })?;
        let global_home =
            global_config_root
                .parent()
                .ok_or_else(|| RuntimeConfigError::Invalid {
                    kind: "global config root",
                    reason: "config root has no Jyowo home parent".to_owned(),
                })?;
        validate_existing_directory_chain(
            global_home,
            Path::new("skills/packages"),
            "global skill packages",
        )?;
        validate_existing_directory_chain(
            global_home,
            Path::new("plugins/packages"),
            "global plugin packages",
        )?;

        let project_provider_selection = read_optional_json::<ProviderSelectionRecord>(
            &project_config_root.join(PROVIDER_SELECTION_FILE),
            "project provider selection",
        )?;
        let project_selected_config_id = match &project_provider_selection {
            Some(selection) => match selection.default_config_id.as_deref() {
                Some(id) if !id.trim().is_empty() => Some(id),
                _ => {
                    return Err(RuntimeConfigError::Invalid {
                        kind: "project provider selection",
                        reason: "default config id is empty".to_owned(),
                    });
                }
            },
            None => None,
        };
        let selected_config_id = model_config_id.or(project_selected_config_id);
        let provider_resolver = ProviderConfigResolver::new(&global_config_root);
        let provider_generation_guard = provider_resolver.lock_generation_shared()?;
        let provider = provider_resolver.resolve_unlocked(selected_config_id)?;
        let credentials = load_provider_credentials(&global_config_root)?;
        drop(provider_generation_guard);

        let mut execution_defaults = provider_resolver.resolve_execution_defaults()?;
        if let Some(overrides) = read_optional_json::<ExecutionOverridesRecord>(
            &project_config_root.join(EXECUTION_OVERRIDES_FILE),
            "project execution overrides",
        )? {
            merge_execution_overrides(&mut execution_defaults, overrides);
        }

        let global_routes = read_optional_json::<ProviderCapabilityRouteSettings>(
            &global_config_root.join(PROVIDER_ROUTES_FILE),
            "global provider capability routes",
        )?
        .unwrap_or_else(empty_provider_routes);
        let project_routes = read_optional_json::<ProviderCapabilityRouteSettings>(
            &project_config_root.join(PROVIDER_ROUTES_FILE),
            "project provider capability routes",
        )?;
        validate_provider_routes(&global_routes, "global provider capability routes")?;
        if let Some(routes) = &project_routes {
            validate_provider_routes(routes, "project provider capability routes")?;
        }
        let provider_routes = merge_provider_routes(global_routes, project_routes);
        validate_provider_route_integrity(&provider_routes, &credentials)?;

        let mut global_mcp = read_optional_json::<Vec<RuntimeMcpServerConfig>>(
            &global_config_root.join(MCP_SERVERS_FILE),
            "global MCP servers",
        )?
        .unwrap_or_default();
        for server in &mut global_mcp {
            server.source = McpServerSource::User;
        }
        let mut project_mcp = read_optional_json::<Vec<RuntimeMcpServerConfig>>(
            &project_config_root.join(MCP_SERVERS_FILE),
            "project MCP servers",
        )?;
        if let Some(project_mcp) = &mut project_mcp {
            for server in project_mcp {
                server.source = McpServerSource::Project;
            }
        }
        let mut mcp_servers = merge_mcp_servers(global_mcp, project_mcp)?;
        freeze_mcp_working_directories(&mut mcp_servers, &workspace_root)?;

        let global_skill_selection = read_optional_json::<SkillSelectionRecord>(
            &global_config_root.join(SKILLS_FILE),
            "global skill selection",
        )?
        .unwrap_or_default();
        let project_skill_selection = read_optional_json::<SkillSelectionRecord>(
            &project_config_root.join(SKILLS_FILE),
            "project skill selection",
        )?;
        let global_skill_index = read_optional_json::<Vec<SkillIndexRecord>>(
            &global_home.join("skills/index.json"),
            "global skill index",
        )?
        .unwrap_or_default();
        let project_skill_index = read_optional_json::<Vec<SkillIndexRecord>>(
            &project_root.join("skills/index.json"),
            "project skill index",
        )?
        .unwrap_or_default();
        let enabled_skill_ids = project_skill_selection
            .as_ref()
            .map(|selection| selection.enabled.iter().cloned().collect())
            .unwrap_or_else(|| global_skill_selection.enabled.iter().cloned().collect());
        let skill_loader = build_skill_loader(
            global_home,
            &workspace_root,
            &global_skill_selection,
            project_skill_selection.as_ref(),
            &global_skill_index,
            &project_skill_index,
        )
        .freeze_directory_sources()
        .map_err(|source| RuntimeConfigError::Invalid {
            kind: "skill packages",
            reason: source.to_string(),
        })?;
        let skill_config_document = read_optional_json::<SkillConfigDocument>(
            &global_config_root.join(SKILL_CONFIG_FILE),
            "global skill config",
        )?
        .unwrap_or_default();
        let skill_config = SkillConfigSnapshot::from_document(
            skill_config_document,
            self.skill_secret_store.clone(),
        )?;

        let global_plugin_records = read_optional_json::<PluginSettingsFile>(
            &global_home.join("plugins/index.json"),
            "global plugin index",
        )?
        .unwrap_or_default();
        let project_plugin_records = read_optional_json::<PluginSettingsFile>(
            &project_root.join("plugins/index.json"),
            "project plugin index",
        )?
        .unwrap_or_default();
        let project_plugin_selection = read_optional_json::<PluginSelectionRecord>(
            &project_config_root.join(PLUGINS_FILE),
            "project plugin selection",
        )?;
        let (enabled_plugin_ids, allow_project_plugins) = effective_plugin_selection(
            &global_plugin_records,
            &project_plugin_records,
            project_plugin_selection.as_ref(),
        );
        validate_plugin_index_paths(
            &global_home.join("plugins/packages"),
            &global_plugin_records,
            &enabled_plugin_ids,
            true,
            "global plugin index",
        )?;
        validate_plugin_index_paths(
            &workspace_root.join(".jyowo/plugins/packages"),
            &project_plugin_records,
            &enabled_plugin_ids,
            allow_project_plugins,
            "project plugin index",
        )?;
        let plugin_snapshot = build_plugin_snapshot(
            global_home,
            &workspace_root,
            &global_plugin_records,
            &project_plugin_records,
            &enabled_plugin_ids,
            allow_project_plugins,
        )?;

        let mut agent_profiles = builtin_agent_profiles();
        let user_profiles = read_optional_json::<Vec<AgentProfile>>(
            &global_config_root.join(AGENT_PROFILES_FILE),
            "global agent profiles",
        )?
        .unwrap_or_default();
        validate_agent_profiles(&agent_profiles, &user_profiles)?;
        agent_profiles.extend(user_profiles);
        let agent_profile_selection = read_optional_json::<AgentProfileSelectionRecord>(
            &project_config_root.join(AGENT_PROFILE_SELECTION_FILE),
            "project agent profile selection",
        )?;
        let default_agent_profile_id = match agent_profile_selection {
            Some(selection) => match selection.default_profile_id {
                Some(id) if !id.trim().is_empty() => Some(id),
                _ => {
                    return Err(RuntimeConfigError::Invalid {
                        kind: "project agent profile selection",
                        reason: "default profile id is empty".to_owned(),
                    });
                }
            },
            None => None,
        };
        if let Some(selected) = &default_agent_profile_id {
            if !agent_profiles.iter().any(|profile| &profile.id == selected) {
                return Err(RuntimeConfigError::Invalid {
                    kind: "project agent profile selection",
                    reason: format!("selected profile `{selected}` is not defined"),
                });
            }
        }

        Ok(RuntimeConfigSnapshot {
            workspace_root: workspace_root.clone(),
            provider,
            execution_defaults,
            provider_routes: provider_routes.clone(),
            mcp_servers,
            plugin_snapshot,
            skill_loader,
            skill_config,
            enabled_skill_ids,
            enabled_plugin_ids,
            allow_project_plugins,
            agent_profiles,
            default_agent_profile_id,
            memory_database_path: daemon_memory_database_path(global_home, Some(&workspace_root)),
            memory_storage_root: global_home.to_owned(),
            provider_credential_resolver: Arc::new(DaemonProviderCredentialResolver {
                credentials,
                routes: provider_routes,
            }),
        })
    }
}

impl fmt::Debug for RuntimeConfigResolver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeConfigResolver")
            .field("global_config_root", &self.global_config_root)
            .field("skill_secret_store", &"SkillSecretStore")
            .finish()
    }
}

/// Immutable inputs shared by foreground and child Harness construction.
#[derive(Clone)]
pub struct RuntimeConfigSnapshot {
    pub workspace_root: PathBuf,
    pub provider: ResolvedProviderConfig,
    pub execution_defaults: ExecutionDefaultsRecord,
    pub provider_routes: ProviderCapabilityRouteSettings,
    pub mcp_servers: Vec<RuntimeMcpServerConfig>,
    plugin_snapshot: RuntimePluginSnapshot,
    pub skill_loader: SkillLoader,
    pub skill_config: SkillConfigSnapshot,
    pub enabled_skill_ids: BTreeSet<String>,
    pub enabled_plugin_ids: BTreeSet<String>,
    pub allow_project_plugins: bool,
    pub agent_profiles: Vec<AgentProfile>,
    pub default_agent_profile_id: Option<String>,
    pub memory_database_path: PathBuf,
    memory_storage_root: PathBuf,
    pub provider_credential_resolver: Arc<dyn ProviderCredentialResolverCap>,
}

impl fmt::Debug for RuntimeConfigSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeConfigSnapshot")
            .field("workspace_root", &self.workspace_root)
            .field("provider", &self.provider)
            .field("execution_defaults", &self.execution_defaults)
            .field("provider_routes", &self.provider_routes)
            .field("mcp_servers", &self.mcp_servers)
            .field("plugin_snapshot", &self.plugin_snapshot)
            .field("skill_loader", &"SkillLoader")
            .field("skill_config", &self.skill_config)
            .field("enabled_skill_ids", &self.enabled_skill_ids)
            .field("enabled_plugin_ids", &self.enabled_plugin_ids)
            .field("allow_project_plugins", &self.allow_project_plugins)
            .field("agent_profiles", &self.agent_profiles)
            .field("default_agent_profile_id", &self.default_agent_profile_id)
            .field("memory_database_path", &self.memory_database_path)
            .finish_non_exhaustive()
    }
}

impl RuntimeConfigSnapshot {
    pub fn materialize_plugin_registry(
        &self,
    ) -> Result<PluginRegistry, harness_plugin::PluginError> {
        self.plugin_snapshot.materialize()
    }

    pub fn ensure_memory_parent(&self) -> Result<(), RuntimeConfigError> {
        ensure_memory_parent_path(&self.memory_database_path, &self.memory_storage_root)
    }
}

fn ensure_memory_parent_path(
    memory_database_path: &Path,
    memory_storage_root: &Path,
) -> Result<(), RuntimeConfigError> {
    let parent = memory_database_path
        .parent()
        .ok_or_else(|| RuntimeConfigError::Invalid {
            kind: "runtime memory directory",
            reason: "memory database path has no parent".to_owned(),
        })?;
    let relative =
        parent
            .strip_prefix(memory_storage_root)
            .map_err(|_| RuntimeConfigError::Invalid {
                kind: "runtime memory directory",
                reason: "memory database path escaped daemon storage root".to_owned(),
            })?;
    ensure_secure_directory_chain(memory_storage_root, relative, "runtime memory directory")?;
    for path in [
        memory_database_path.to_owned(),
        path_with_suffix(memory_database_path, "-wal"),
        path_with_suffix(memory_database_path, "-shm"),
    ] {
        reject_symlink_path_if_present(&path, "runtime memory sqlite file")?;
    }
    Ok(())
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

fn reject_symlink_path_if_present(
    path: &Path,
    kind: &'static str,
) -> Result<(), RuntimeConfigError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(RuntimeConfigError::ConfigSymlink {
                kind,
                path: path.to_owned(),
            })
        }
        Ok(_) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(RuntimeConfigError::Read {
            kind,
            path: path.to_owned(),
            source,
        }),
    }
}

fn daemon_memory_database_path(global_home: &Path, workspace_root: Option<&Path>) -> PathBuf {
    let runtime_root = global_home.join("runtime");
    match workspace_root {
        Some(workspace_root) => {
            let workspace_key =
                blake3::hash(workspace_root.as_os_str().as_encoded_bytes()).to_hex();
            runtime_root
                .join("workspaces")
                .join(workspace_key.as_str())
                .join("memory/memory.sqlite3")
        }
        None => runtime_root.join("memory/memory.sqlite3"),
    }
}

#[derive(Clone)]
struct RuntimePluginSnapshot {
    config: PluginConfig,
    sources: Vec<FrozenPluginSource>,
    runtime_loaders: Vec<Arc<dyn PluginRuntimeLoader>>,
}

#[derive(Debug)]
struct FrozenPluginRuntimeRoot {
    path: PathBuf,
}

impl Drop for FrozenPluginRuntimeRoot {
    fn drop(&mut self) {
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let _ = fs::remove_file(&self.path);
            }
            Ok(metadata) if metadata.is_dir() => {
                let _ = fs::remove_dir_all(&self.path);
            }
            Ok(_) => {
                let _ = fs::remove_file(&self.path);
            }
            Err(_) => {}
        }
    }
}

#[derive(Debug)]
struct FrozenPluginExecutableStore {
    global_home: PathBuf,
    runtime_root: Option<Arc<FrozenPluginRuntimeRoot>>,
    next_file_id: u64,
}

impl FrozenPluginExecutableStore {
    fn new(global_home: &Path) -> Self {
        Self {
            global_home: global_home.to_owned(),
            runtime_root: None,
            next_file_id: 0,
        }
    }

    fn runtime_root(&self) -> Option<Arc<FrozenPluginRuntimeRoot>> {
        self.runtime_root.clone()
    }

    fn freeze(&mut self, source: &Path) -> Result<PathBuf, RuntimeConfigError> {
        let bytes = read_regular_executable_no_follow(source)?;
        let content_hash = blake3::hash(&bytes).to_hex();
        let root = match &self.runtime_root {
            Some(root) => Arc::clone(root),
            None => {
                let root = Arc::new(FrozenPluginRuntimeRoot {
                    path: create_frozen_plugin_runtime_root(&self.global_home)?,
                });
                self.runtime_root = Some(Arc::clone(&root));
                root
            }
        };
        self.next_file_id = self.next_file_id.saturating_add(1);
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| value.eq_ignore_ascii_case("exe"))
            .map(|_| ".exe")
            .unwrap_or_default();
        let destination = root.path.join(format!(
            "{:04}-{content_hash}{extension}",
            self.next_file_id
        ));
        write_frozen_plugin_executable(&destination, &bytes)?;
        Ok(destination)
    }
}

impl fmt::Debug for RuntimePluginSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimePluginSnapshot")
            .field("sources", &self.sources)
            .field("runtime_loader_count", &self.runtime_loaders.len())
            .finish_non_exhaustive()
    }
}

impl RuntimePluginSnapshot {
    fn materialize(&self) -> Result<PluginRegistry, harness_plugin::PluginError> {
        let mut builder = PluginRegistry::builder().with_config(self.config.clone());
        for source in &self.sources {
            builder = builder
                .with_source(source.source.clone())
                .with_manifest_loader(Arc::new(FrozenPluginManifestLoader {
                    source: source.source.clone(),
                    report: source.report.clone(),
                }));
        }
        for loader in &self.runtime_loaders {
            builder = builder.with_runtime_loader(Arc::clone(loader));
        }
        builder.build()
    }
}

#[derive(Debug, Clone)]
struct DaemonPluginRuntimeLoader {
    runtime_root: Option<Arc<FrozenPluginRuntimeRoot>>,
    workspace_root: PathBuf,
}

impl DaemonPluginRuntimeLoader {
    fn sandbox_root(&self, origin: &ManifestOrigin) -> Option<PathBuf> {
        let ManifestOrigin::CargoExtension { binary, .. } = origin else {
            return None;
        };
        let runtime_root = self.runtime_root.as_ref()?;
        binary
            .starts_with(&runtime_root.path)
            .then(|| runtime_root.path.clone())
    }
}

#[async_trait::async_trait]
impl PluginRuntimeLoader for DaemonPluginRuntimeLoader {
    fn can_load(&self, _manifest: &PluginManifest, origin: &ManifestOrigin) -> bool {
        self.sandbox_root(origin).is_some()
    }

    async fn load(
        &self,
        manifest: &PluginManifest,
        origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        let sandbox_root = self.sandbox_root(origin).ok_or_else(|| {
            RuntimeLoaderError::UnsupportedOrigin("sidecar path is outside plugin roots".to_owned())
        })?;
        let isolation = LocalIsolation::for_current_platform();
        let mode = SandboxMode::OsLevel(match isolation {
            LocalIsolation::None => LocalIsolationTag::None,
            LocalIsolation::Bubblewrap => LocalIsolationTag::Bubblewrap,
            LocalIsolation::Seatbelt => LocalIsolationTag::Seatbelt,
            LocalIsolation::JobObject => LocalIsolationTag::JobObject,
        });
        CargoExtensionRuntimeLoader::new()
            .with_sandbox(
                Arc::new(LocalSandbox::new(sandbox_root).with_isolation(isolation)),
                mode,
                self.workspace_root.clone(),
            )
            .load(manifest, origin)
            .await
    }
}

#[derive(Debug, Clone)]
struct FrozenPluginSource {
    source: DiscoverySource,
    report: ManifestLoadReport,
}

#[derive(Debug, Clone)]
struct FrozenPluginManifestLoader {
    source: DiscoverySource,
    report: ManifestLoadReport,
}

#[async_trait::async_trait]
impl PluginManifestLoader for FrozenPluginManifestLoader {
    async fn enumerate(
        &self,
        source: &DiscoverySource,
    ) -> Result<Vec<harness_plugin::ManifestRecord>, ManifestLoaderError> {
        Ok(if source == &self.source {
            self.report.records.clone()
        } else {
            Vec::new()
        })
    }

    async fn load_report(
        &self,
        source: &DiscoverySource,
    ) -> Result<ManifestLoadReport, ManifestLoaderError> {
        Ok(if source == &self.source {
            self.report.clone()
        } else {
            ManifestLoadReport::default()
        })
    }
}

#[derive(Clone)]
pub struct RuntimeMcpServerConfig {
    config: McpServerConfigRecord,
    pub(crate) source: McpServerSource,
}

fn default_mcp_server_source() -> McpServerSource {
    McpServerSource::User
}

impl<'de> Deserialize<'de> for RuntimeMcpServerConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self {
            config: McpServerConfigRecord::deserialize(deserializer)?,
            source: default_mcp_server_source(),
        })
    }
}

impl std::ops::Deref for RuntimeMcpServerConfig {
    type Target = McpServerConfigRecord;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

fn create_frozen_plugin_runtime_root(global_home: &Path) -> Result<PathBuf, RuntimeConfigError> {
    let parent = ensure_secure_directory_chain(
        global_home,
        Path::new("runtime/plugin-snapshots"),
        "plugin runtime snapshot directory",
    )?;
    for _ in 0..16 {
        let path = parent.join(uuid::Uuid::new_v4().to_string());
        #[cfg(unix)]
        let result = {
            use std::os::unix::fs::DirBuilderExt;

            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            builder.create(&path)
        };
        #[cfg(not(unix))]
        let result = fs::create_dir(&path);
        match result {
            Ok(()) => {
                let metadata =
                    fs::symlink_metadata(&path).map_err(|source| RuntimeConfigError::Read {
                        kind: "plugin runtime snapshot directory",
                        path: path.clone(),
                        source,
                    })?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(RuntimeConfigError::Invalid {
                        kind: "plugin runtime snapshot directory",
                        reason: "snapshot root is not a private directory".to_owned(),
                    });
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(RuntimeConfigError::Read {
                    kind: "plugin runtime snapshot directory",
                    path,
                    source,
                });
            }
        }
    }
    Err(RuntimeConfigError::Invalid {
        kind: "plugin runtime snapshot directory",
        reason: "unable to allocate a unique snapshot root".to_owned(),
    })
}

#[cfg(unix)]
fn read_regular_executable_no_follow(path: &Path) -> Result<Vec<u8>, RuntimeConfigError> {
    use std::os::unix::fs::PermissionsExt;

    let mut components = Vec::new();
    let mut absolute = false;
    for component in path.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::ParentDir => {
                return Err(RuntimeConfigError::Invalid {
                    kind: "plugin sidecar executable",
                    reason: "executable path is not normalized".to_owned(),
                });
            }
            std::path::Component::RootDir => absolute = true,
            std::path::Component::CurDir => {}
            std::path::Component::Normal(value) => components.push(value.to_os_string()),
        }
    }
    let file_name = components
        .pop()
        .ok_or_else(|| RuntimeConfigError::Invalid {
            kind: "plugin sidecar executable",
            reason: "executable path has no file name".to_owned(),
        })?;
    let mut directory = fs::File::open(if absolute {
        Path::new("/")
    } else {
        Path::new(".")
    })
    .map_err(|source| RuntimeConfigError::Read {
        kind: "plugin sidecar executable",
        path: path.to_owned(),
        source,
    })?;
    for component in components {
        let fd = rustix::fs::openat(
            &directory,
            Path::new(&component),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| RuntimeConfigError::Invalid {
            kind: "plugin sidecar executable",
            reason: if matches!(error, rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) {
                "executable path must not use symbolic links".to_owned()
            } else {
                "executable parent directory is unavailable".to_owned()
            },
        })?;
        directory = fs::File::from(fd);
    }
    let fd = rustix::fs::openat(
        &directory,
        Path::new(&file_name),
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| RuntimeConfigError::Invalid {
        kind: "plugin sidecar executable",
        reason: if matches!(error, rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) {
            "executable must not be a symbolic link".to_owned()
        } else {
            "executable is unavailable".to_owned()
        },
    })?;
    let mut file = fs::File::from(fd);
    let metadata = file.metadata().map_err(|source| RuntimeConfigError::Read {
        kind: "plugin sidecar executable",
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(RuntimeConfigError::Invalid {
            kind: "plugin sidecar executable",
            reason: "sidecar must be a regular executable file".to_owned(),
        });
    }
    if metadata.len() > MAX_FROZEN_PLUGIN_EXECUTABLE_BYTES {
        return Err(RuntimeConfigError::Invalid {
            kind: "plugin sidecar executable",
            reason: "sidecar executable is too large".to_owned(),
        });
    }
    read_executable_bytes_bounded(
        &mut file,
        metadata.len(),
        MAX_FROZEN_PLUGIN_EXECUTABLE_BYTES,
        path,
    )
}

#[cfg(windows)]
fn read_regular_executable_no_follow(path: &Path) -> Result<Vec<u8>, RuntimeConfigError> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|source| RuntimeConfigError::Read {
            kind: "plugin sidecar executable",
            path: path.to_owned(),
            source,
        })?;
    let metadata = file.metadata().map_err(|source| RuntimeConfigError::Read {
        kind: "plugin sidecar executable",
        path: path.to_owned(),
        source,
    })?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !metadata.is_file() {
        return Err(RuntimeConfigError::Invalid {
            kind: "plugin sidecar executable",
            reason: "sidecar must be a regular non-symlink file".to_owned(),
        });
    }
    if metadata.len() > MAX_FROZEN_PLUGIN_EXECUTABLE_BYTES {
        return Err(RuntimeConfigError::Invalid {
            kind: "plugin sidecar executable",
            reason: "sidecar executable is too large".to_owned(),
        });
    }
    read_executable_bytes_bounded(
        &mut file,
        metadata.len(),
        MAX_FROZEN_PLUGIN_EXECUTABLE_BYTES,
        path,
    )
}

#[cfg(all(not(unix), not(windows)))]
fn read_regular_executable_no_follow(path: &Path) -> Result<Vec<u8>, RuntimeConfigError> {
    let mut file = fs::File::open(path).map_err(|source| RuntimeConfigError::Read {
        kind: "plugin sidecar executable",
        path: path.to_owned(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| RuntimeConfigError::Read {
        kind: "plugin sidecar executable",
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() || fs::symlink_metadata(path).is_ok_and(|item| item.is_symlink()) {
        return Err(RuntimeConfigError::Invalid {
            kind: "plugin sidecar executable",
            reason: "sidecar must be a regular non-symlink file".to_owned(),
        });
    }
    read_executable_bytes_bounded(
        &mut file,
        metadata.len(),
        MAX_FROZEN_PLUGIN_EXECUTABLE_BYTES,
        path,
    )
}

fn read_executable_bytes_bounded(
    reader: &mut impl Read,
    initial_len: u64,
    max_bytes: u64,
    path: &Path,
) -> Result<Vec<u8>, RuntimeConfigError> {
    let mut bytes = Vec::with_capacity(initial_len.min(max_bytes) as usize);
    reader
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| RuntimeConfigError::Read {
            kind: "plugin sidecar executable",
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > max_bytes {
        return Err(RuntimeConfigError::Invalid {
            kind: "plugin sidecar executable",
            reason: "sidecar executable is too large".to_owned(),
        });
    }
    Ok(bytes)
}

fn write_frozen_plugin_executable(path: &Path, bytes: &[u8]) -> Result<(), RuntimeConfigError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o500);
    }
    let mut file = options
        .open(path)
        .map_err(|source| RuntimeConfigError::Read {
            kind: "frozen plugin sidecar executable",
            path: path.to_owned(),
            source,
        })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| RuntimeConfigError::Read {
            kind: "frozen plugin sidecar executable",
            path: path.to_owned(),
            source,
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o500)).map_err(|source| {
            RuntimeConfigError::Read {
                kind: "frozen plugin sidecar executable",
                path: path.to_owned(),
                source,
            }
        })?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| RuntimeConfigError::Read {
        kind: "frozen plugin sidecar executable",
        path: path.to_owned(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RuntimeConfigError::Invalid {
            kind: "frozen plugin sidecar executable",
            reason: "snapshot executable is not a regular file".to_owned(),
        });
    }
    Ok(())
}

impl fmt::Debug for RuntimeMcpServerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeMcpServerConfig")
            .field("config", &self.config)
            .field("source", &self.source)
            .finish()
    }
}

const MAX_RUNTIME_CONFIG_DIAGNOSTIC_BYTES: usize = 512;

#[derive(Debug)]
pub enum RuntimeConfigError {
    Provider(ProviderConfigError),
    Read {
        kind: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    Decode {
        kind: &'static str,
        path: PathBuf,
        source: serde_json::Error,
    },
    WorkspaceSymlink {
        path: PathBuf,
    },
    ConfigSymlink {
        kind: &'static str,
        path: PathBuf,
    },
    Invalid {
        kind: &'static str,
        reason: String,
    },
    PluginRegistry {
        source: harness_plugin::PluginError,
    },
    SkillConfigStore {
        source: SkillConfigStoreError,
    },
}

impl From<ProviderConfigError> for RuntimeConfigError {
    fn from(source: ProviderConfigError) -> Self {
        Self::Provider(source)
    }
}

impl From<SkillConfigStoreError> for RuntimeConfigError {
    fn from(source: SkillConfigStoreError) -> Self {
        match source {
            SkillConfigStoreError::SecretStoreUnavailable => Self::SkillConfigStore { source },
            SkillConfigStoreError::UnsupportedDocumentVersion(version) => Self::Invalid {
                kind: "global skill config",
                reason: format!("unsupported document version {version}"),
            },
        }
    }
}

impl fmt::Display for RuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Provider(_) => "invalid provider configuration".to_owned(),
            Self::Read { kind, .. } => format!("failed to read {kind} from configured storage"),
            Self::Decode { kind, .. } => {
                format!("failed to parse {kind} from configured storage")
            }
            Self::WorkspaceSymlink { .. } => {
                "workspace root must not be a symbolic link".to_owned()
            }
            Self::ConfigSymlink { kind, .. } => {
                format!("{kind} must not be a symbolic link")
            }
            Self::Invalid { kind, .. } => format!("invalid {kind}"),
            Self::PluginRegistry { .. } => "failed to initialize plugin registry".to_owned(),
            Self::SkillConfigStore { .. } => {
                "failed to load skill configuration from secure storage".to_owned()
            }
        };
        formatter.write_str(&bounded_runtime_config_diagnostic(message))
    }
}

impl std::error::Error for RuntimeConfigError {}

fn bounded_runtime_config_diagnostic(mut message: String) -> String {
    if message.len() <= MAX_RUNTIME_CONFIG_DIAGNOSTIC_BYTES {
        return message;
    }
    let mut boundary = MAX_RUNTIME_CONFIG_DIAGNOSTIC_BYTES.saturating_sub(3);
    while !message.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    message.truncate(boundary);
    message.push_str("...");
    message
}

fn canonical_workspace_root(path: &Path) -> Result<PathBuf, RuntimeConfigError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| RuntimeConfigError::Read {
        kind: "workspace root",
        path: path.to_owned(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(RuntimeConfigError::WorkspaceSymlink {
            path: path.to_owned(),
        });
    }
    if !metadata.is_dir() {
        return Err(RuntimeConfigError::Invalid {
            kind: "workspace root",
            reason: "path is not a directory".to_owned(),
        });
    }
    path.canonicalize()
        .map_err(|source| RuntimeConfigError::Read {
            kind: "workspace root",
            path: path.to_owned(),
            source,
        })
}

fn reject_symlink_if_present(path: &Path, kind: &'static str) -> Result<(), RuntimeConfigError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(RuntimeConfigError::ConfigSymlink {
                kind,
                path: path.to_owned(),
            })
        }
        Ok(metadata) if !metadata.is_dir() => Err(RuntimeConfigError::Invalid {
            kind,
            reason: "path is not a directory".to_owned(),
        }),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(RuntimeConfigError::Read {
            kind,
            path: path.to_owned(),
            source,
        }),
    }
}

fn validate_runtime_path_roots(workspace_root: &Path) -> Result<(), RuntimeConfigError> {
    for (relative, kind) in [
        (".jyowo/skills/packages", "project skill packages"),
        (".jyowo/plugins/packages", "project plugin packages"),
    ] {
        validate_existing_directory_chain(workspace_root, Path::new(relative), kind)?;
    }
    let plugin_index = workspace_root.join(".jyowo/plugins/index.json");
    if fs::symlink_metadata(&plugin_index).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(RuntimeConfigError::ConfigSymlink {
            kind: "project plugin index",
            path: plugin_index,
        });
    }
    Ok(())
}

fn validate_existing_directory_chain(
    root: &Path,
    relative: &Path,
    kind: &'static str,
) -> Result<(), RuntimeConfigError> {
    let mut current = root.to_owned();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(RuntimeConfigError::Invalid {
                kind,
                reason: "path must contain only relative directory components".to_owned(),
            });
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(RuntimeConfigError::ConfigSymlink {
                    kind,
                    path: current,
                });
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(RuntimeConfigError::Invalid {
                    kind,
                    reason: "path component is not a directory".to_owned(),
                });
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(RuntimeConfigError::Read {
                    kind,
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_secure_directory_chain(
    root: &Path,
    relative: &Path,
    kind: &'static str,
) -> Result<PathBuf, RuntimeConfigError> {
    let root_fd = rustix::fs::open(
        root,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| {
        if matches!(error, rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) {
            RuntimeConfigError::ConfigSymlink {
                kind,
                path: root.to_owned(),
            }
        } else {
            RuntimeConfigError::Read {
                kind,
                path: root.to_owned(),
                source: std::io::Error::other(error.to_string()),
            }
        }
    })?;
    let mut directory = fs::File::from(root_fd);
    let mut current = root.to_owned();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(RuntimeConfigError::Invalid {
                kind,
                reason: "path must contain only relative directory components".to_owned(),
            });
        };
        current.push(component);
        match rustix::fs::mkdirat(
            &directory,
            Path::new(component),
            rustix::fs::Mode::from_raw_mode(0o700),
        ) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => {
                return Err(RuntimeConfigError::Read {
                    kind,
                    path: current.clone(),
                    source: std::io::Error::other(error.to_string()),
                });
            }
        }
        let fd = rustix::fs::openat(
            &directory,
            Path::new(component),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| {
            if matches!(error, rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) {
                RuntimeConfigError::ConfigSymlink {
                    kind,
                    path: current.clone(),
                }
            } else {
                RuntimeConfigError::Read {
                    kind,
                    path: current.clone(),
                    source: std::io::Error::other(error.to_string()),
                }
            }
        })?;
        directory = fs::File::from(fd);
    }
    Ok(current)
}

#[cfg(not(unix))]
fn ensure_secure_directory_chain(
    root: &Path,
    relative: &Path,
    kind: &'static str,
) -> Result<PathBuf, RuntimeConfigError> {
    let mut current = root.to_owned();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(RuntimeConfigError::Invalid {
                kind,
                reason: "path must contain only relative directory components".to_owned(),
            });
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(RuntimeConfigError::ConfigSymlink {
                    kind,
                    path: current,
                });
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(RuntimeConfigError::Invalid {
                    kind,
                    reason: "path component is not a directory".to_owned(),
                });
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|source| RuntimeConfigError::Read {
                    kind,
                    path: current.clone(),
                    source,
                })?;
            }
            Err(source) => {
                return Err(RuntimeConfigError::Read {
                    kind,
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(current)
}

fn read_optional_json<T: DeserializeOwned>(
    path: &Path,
    kind: &'static str,
) -> Result<Option<T>, RuntimeConfigError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(RuntimeConfigError::ConfigSymlink {
                kind,
                path: path.to_owned(),
            });
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(RuntimeConfigError::Invalid {
                kind,
                reason: "path is not a regular file".to_owned(),
            });
        }
        Ok(_) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(RuntimeConfigError::Read {
                kind,
                path: path.to_owned(),
                source,
            });
        }
    }
    let bytes = fs::read(path).map_err(|source| RuntimeConfigError::Read {
        kind,
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|source| RuntimeConfigError::Decode {
            kind,
            path: path.to_owned(),
            source,
        })
}

fn merge_execution_overrides(
    defaults: &mut ExecutionDefaultsRecord,
    overrides: ExecutionOverridesRecord,
) {
    if let Some(value) = overrides.permission_mode {
        defaults.permission_mode = value;
    }
    if let Some(value) = overrides.tool_profile {
        defaults.tool_profile = value;
    }
    if let Some(value) = overrides.context_compression_trigger_ratio {
        defaults.context_compression_trigger_ratio = value;
    }
    if let Some(value) = overrides.subagents_enabled {
        defaults.subagents_enabled = value;
    }
    if let Some(value) = overrides.agent_teams_enabled {
        defaults.agent_teams_enabled = value;
    }
    if let Some(value) = overrides.background_agents_enabled {
        defaults.background_agents_enabled = value;
    }
}

fn empty_provider_routes() -> ProviderCapabilityRouteSettings {
    ProviderCapabilityRouteSettings {
        version: 1,
        routes: Vec::new(),
    }
}

fn merge_provider_routes(
    global: ProviderCapabilityRouteSettings,
    project: Option<ProviderCapabilityRouteSettings>,
) -> ProviderCapabilityRouteSettings {
    let Some(project) = project else {
        return global;
    };
    let mut operations = Vec::<(String, ProviderCapabilityRoute)>::new();
    for route in global.routes {
        for operation_id in route.operation_ids.iter().cloned() {
            let mut route = route.clone();
            route.operation_ids = vec![operation_id.clone()];
            operations.push((operation_id, route));
        }
    }
    for route in project.routes {
        for operation_id in route.operation_ids.iter().cloned() {
            operations.retain(|(configured, _)| configured != &operation_id);
            let mut route = route.clone();
            route.operation_ids = vec![operation_id.clone()];
            operations.push((operation_id, route));
        }
    }
    operations.sort_by(|left, right| left.0.cmp(&right.0));
    ProviderCapabilityRouteSettings {
        version: global.version.max(project.version),
        routes: operations.into_iter().map(|(_, route)| route).collect(),
    }
}

fn merge_mcp_servers(
    global: Vec<RuntimeMcpServerConfig>,
    project: Option<Vec<RuntimeMcpServerConfig>>,
) -> Result<Vec<RuntimeMcpServerConfig>, RuntimeConfigError> {
    let mut servers = BTreeMap::new();
    insert_unique_mcp_servers(&mut servers, global, "global MCP servers")?;
    if let Some(project) = project {
        let mut project_servers = BTreeMap::new();
        insert_unique_mcp_servers(&mut project_servers, project, "project MCP servers")?;
        servers.extend(project_servers);
    }
    Ok(servers.into_values().collect())
}

fn insert_unique_mcp_servers(
    output: &mut BTreeMap<String, RuntimeMcpServerConfig>,
    servers: Vec<RuntimeMcpServerConfig>,
    kind: &'static str,
) -> Result<(), RuntimeConfigError> {
    let mut ids = HashSet::new();
    for server in servers {
        validate_persisted_mcp_server(&server.config).map_err(|error| {
            RuntimeConfigError::Invalid {
                kind,
                reason: error.to_string(),
            }
        })?;
        if !ids.insert(server.id.clone()) {
            return Err(RuntimeConfigError::Invalid {
                kind,
                reason: format!("server id `{}` is duplicated", server.id),
            });
        }
        output.insert(server.id.clone(), server);
    }
    Ok(())
}

fn freeze_mcp_working_directories(
    servers: &mut [RuntimeMcpServerConfig],
    workspace_root: &Path,
) -> Result<(), RuntimeConfigError> {
    for server in servers {
        let McpServerTransportConfig::Stdio { working_dir, .. } = &mut server.config.transport
        else {
            continue;
        };
        let Some(working_dir) = working_dir else {
            continue;
        };
        let configured = PathBuf::from(&*working_dir);
        let candidate = if configured.is_absolute() {
            configured
        } else {
            workspace_root.join(configured)
        };
        let canonical = candidate
            .canonicalize()
            .map_err(|_| RuntimeConfigError::Invalid {
                kind: "MCP server working directory",
                reason: "configured directory is unavailable".to_owned(),
            })?;
        if !canonical.starts_with(workspace_root) {
            return Err(RuntimeConfigError::Invalid {
                kind: "MCP server working directory",
                reason: "configured directory escapes the workspace".to_owned(),
            });
        }
        if !canonical.is_dir() {
            return Err(RuntimeConfigError::Invalid {
                kind: "MCP server working directory",
                reason: "configured path is not a directory".to_owned(),
            });
        }
        *working_dir = canonical
            .to_str()
            .ok_or_else(|| RuntimeConfigError::Invalid {
                kind: "MCP server working directory",
                reason: "canonical directory is not valid UTF-8".to_owned(),
            })?
            .to_owned();
    }
    Ok(())
}

fn build_skill_loader(
    global_home: &Path,
    workspace_root: &Path,
    global: &SkillSelectionRecord,
    project: Option<&SkillSelectionRecord>,
    global_index: &[SkillIndexRecord],
    project_index: &[SkillIndexRecord],
) -> SkillLoader {
    let global_enabled = project.unwrap_or(global);
    let global_expected = expected_skill_package_hashes(global_index, global_enabled);
    let project_expected = project
        .map(|selection| expected_skill_package_hashes(project_index, selection))
        .unwrap_or_default();
    SkillLoader::default()
        .with_source(SkillSourceConfig::DirectoryPackages {
            path: global_home.join("skills/packages"),
            source_kind: DirectorySourceKind::User,
            expected_package_hashes: global_expected,
        })
        .with_source(SkillSourceConfig::DirectoryPackages {
            path: workspace_root.join(".jyowo/skills/packages"),
            source_kind: DirectorySourceKind::Workspace,
            expected_package_hashes: project_expected,
        })
}

fn expected_skill_package_hashes(
    index: &[SkillIndexRecord],
    selection: &SkillSelectionRecord,
) -> BTreeMap<String, String> {
    let enabled = selection.enabled.iter().collect::<HashSet<_>>();
    index
        .iter()
        .filter(|record| enabled.contains(&record.id))
        .map(|record| (record.id.clone(), record.content_hash.clone()))
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillIndexRecord {
    id: String,
    content_hash: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PluginSettingsFile {
    #[serde(default)]
    allow_project_plugins: bool,
    #[serde(default)]
    records: Vec<PluginIndexRecord>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PluginIndexRecord {
    plugin_id: PluginId,
    name: String,
    version: String,
    enabled: bool,
    package_dir: String,
    source_path: String,
    content_hash: String,
    imported_at: String,
    updated_at: String,
    #[serde(default)]
    config: Value,
    #[serde(default)]
    last_validation_error: Option<String>,
}

fn effective_plugin_selection(
    global: &PluginSettingsFile,
    project: &PluginSettingsFile,
    selection: Option<&PluginSelectionRecord>,
) -> (BTreeSet<String>, bool) {
    if let Some(selection) = selection {
        return (
            selection.enabled.iter().cloned().collect(),
            selection.allow_project_plugins,
        );
    }
    let enabled = global
        .records
        .iter()
        .chain(project.records.iter())
        .filter(|record| record.enabled)
        .map(|record| record.plugin_id.0.clone())
        .collect();
    (enabled, project.allow_project_plugins)
}

fn build_plugin_snapshot(
    global_home: &Path,
    workspace_root: &Path,
    global: &PluginSettingsFile,
    project: &PluginSettingsFile,
    enabled_ids: &BTreeSet<String>,
    allow_project_plugins: bool,
) -> Result<RuntimePluginSnapshot, RuntimeConfigError> {
    let mut allowed_user_plugins = BTreeSet::new();
    let mut entries = BTreeMap::new();
    for (record, source_enabled) in global.records.iter().map(|record| (record, true)).chain(
        project
            .records
            .iter()
            .map(|record| (record, allow_project_plugins)),
    ) {
        if !source_enabled || !record.enabled || !enabled_ids.contains(&record.plugin_id.0) {
            continue;
        }
        let _ = (
            &record.version,
            &record.package_dir,
            &record.source_path,
            &record.content_hash,
            &record.imported_at,
            &record.updated_at,
            &record.last_validation_error,
        );
        let name =
            PluginName::new(record.name.clone()).map_err(|source| RuntimeConfigError::Invalid {
                kind: "plugin index",
                reason: source.to_string(),
            })?;
        entries.insert(name.clone(), record.config.clone());
        allowed_user_plugins.insert(name);
    }
    let config = PluginConfig {
        allow_project_plugins,
        allowed_user_plugins: Some(allowed_user_plugins),
        disabled_plugins: BTreeSet::new(),
        entries,
        workspace_root: Some(workspace_root.to_owned()),
        ..PluginConfig::default()
    };
    let loader = FileManifestLoader;
    let mut executable_store = FrozenPluginExecutableStore::new(global_home);
    let sources = vec![
        freeze_plugin_source(
            &loader,
            DiscoverySource::User(global_home.join("plugins/packages")),
            &global_home.join("plugins/packages"),
            global,
            enabled_ids,
            true,
            &mut executable_store,
            "global plugin index",
        )?,
        freeze_plugin_source(
            &loader,
            DiscoverySource::Project(workspace_root.join(".jyowo/plugins/packages")),
            &workspace_root.join(".jyowo/plugins/packages"),
            project,
            enabled_ids,
            allow_project_plugins,
            &mut executable_store,
            "project plugin index",
        )?,
    ];
    Ok(RuntimePluginSnapshot {
        config,
        sources,
        runtime_loaders: vec![Arc::new(DaemonPluginRuntimeLoader {
            runtime_root: executable_store.runtime_root(),
            workspace_root: workspace_root.to_owned(),
        })],
    })
}

fn freeze_plugin_source(
    loader: &FileManifestLoader,
    source: DiscoverySource,
    packages_root: &Path,
    settings: &PluginSettingsFile,
    enabled_ids: &BTreeSet<String>,
    source_enabled: bool,
    executable_store: &mut FrozenPluginExecutableStore,
    kind: &'static str,
) -> Result<FrozenPluginSource, RuntimeConfigError> {
    let mut report = ManifestLoadReport::default();
    for index_record in settings.records.iter().filter(|record| {
        source_enabled && record.enabled && enabled_ids.contains(&record.plugin_id.0)
    }) {
        let package_report = loader
            .load_package_report_sync(&packages_root.join(&index_record.package_dir))
            .map_err(|source| RuntimeConfigError::PluginRegistry {
                source: source.into(),
            })?;
        if package_report.records.is_empty() && package_report.failures.is_empty() {
            return Err(RuntimeConfigError::Invalid {
                kind,
                reason: "indexed plugin package has no manifest".to_owned(),
            });
        }
        if !package_report.failures.is_empty() {
            return Err(RuntimeConfigError::Invalid {
                kind,
                reason: "selected plugin package manifest is invalid".to_owned(),
            });
        }
        for manifest_record in &package_report.records {
            if manifest_record.manifest.plugin_id() != index_record.plugin_id
                || manifest_record.manifest.name.as_str() != index_record.name
            {
                return Err(RuntimeConfigError::Invalid {
                    kind,
                    reason: "indexed plugin identity does not match package manifest".to_owned(),
                });
            }
        }
        let mut package_records = package_report.records;
        for manifest_record in &mut package_records {
            if let ManifestOrigin::CargoExtension { binary, .. } = &mut manifest_record.origin {
                *binary = executable_store.freeze(binary)?;
            }
        }
        report.records.extend(package_records);
        report.failures.extend(package_report.failures);
    }
    Ok(FrozenPluginSource { source, report })
}

fn validate_provider_routes(
    settings: &ProviderCapabilityRouteSettings,
    kind: &'static str,
) -> Result<(), RuntimeConfigError> {
    for route in &settings.routes {
        validate_provider_capability_route(route)
            .map_err(|reason| RuntimeConfigError::Invalid { kind, reason })?;
    }
    Ok(())
}

fn validate_provider_route_integrity(
    settings: &ProviderCapabilityRouteSettings,
    credentials: &BTreeMap<String, ProviderCredential>,
) -> Result<(), RuntimeConfigError> {
    let catalog = harness_model::provider_catalog_entries();
    let adapter_availability = harness_tool::provider_service_adapter_availability_from_snapshot(
        &harness_tool::ToolRegistryBuilder::new()
            .with_builtin_toolset(harness_tool::BuiltinToolset::Default)
            .build()
            .map_err(|_| RuntimeConfigError::Invalid {
                kind: "provider capability routes",
                reason: "runtime adapter registry is unavailable".to_owned(),
            })?
            .snapshot(),
    );
    let mut enabled_kind_targets = HashMap::new();
    for route in &settings.routes {
        if !route.enabled {
            continue;
        }
        let credential =
            credentials
                .get(&route.config_id)
                .ok_or_else(|| RuntimeConfigError::Invalid {
                    kind: "provider capability routes",
                    reason: "route config is missing or has no usable secret".to_owned(),
                })?;
        if credential.provider_id != route.provider_id {
            return Err(RuntimeConfigError::Invalid {
                kind: "provider capability routes",
                reason: "route provider does not match its config".to_owned(),
            });
        }
        let provider = catalog
            .iter()
            .find(|provider| provider.provider_id == route.provider_id)
            .ok_or_else(|| RuntimeConfigError::Invalid {
                kind: "provider capability routes",
                reason: "route provider is not present in the provider catalog".to_owned(),
            })?;
        for operation_id in &route.operation_ids {
            let capability = provider
                .service_capabilities
                .iter()
                .find(|capability| capability.operation_id == *operation_id)
                .ok_or_else(|| RuntimeConfigError::Invalid {
                    kind: "provider capability routes",
                    reason: "route operation is not declared by the provider catalog".to_owned(),
                })?;
            if capability_route_kind(capability) != Some(route.kind) {
                return Err(RuntimeConfigError::Invalid {
                    kind: "provider capability routes",
                    reason: "route kind does not match its operation".to_owned(),
                });
            }
            if !adapter_availability.bindings.iter().any(|binding| {
                binding.provider_id == route.provider_id
                    && binding.operation_id == *operation_id
                    && binding.route_kind == route.kind
            }) {
                return Err(RuntimeConfigError::Invalid {
                    kind: "provider capability routes",
                    reason: "route operation has no runtime adapter".to_owned(),
                });
            }
        }
        match enabled_kind_targets.insert(route.kind, (&route.config_id, &route.provider_id)) {
            Some((config_id, provider_id))
                if config_id != &route.config_id || provider_id != &route.provider_id =>
            {
                return Err(RuntimeConfigError::Invalid {
                    kind: "provider capability routes",
                    reason: "route kind cannot target multiple provider configs".to_owned(),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn capability_route_kind(capability: &ProviderServiceCapability) -> Option<CapabilityRouteKind> {
    match capability.category {
        ProviderServiceCategory::Image => Some(CapabilityRouteKind::ImageGeneration),
        ProviderServiceCategory::Video => Some(CapabilityRouteKind::VideoGeneration),
        ProviderServiceCategory::ThreeD => Some(CapabilityRouteKind::ThreeDGeneration),
        ProviderServiceCategory::Embedding => Some(CapabilityRouteKind::EmbeddingGeneration),
        ProviderServiceCategory::File => Some(CapabilityRouteKind::FileOperation),
        ProviderServiceCategory::Music => Some(CapabilityRouteKind::MusicGeneration),
        ProviderServiceCategory::Moderation => Some(CapabilityRouteKind::Moderation),
        ProviderServiceCategory::Upload => Some(CapabilityRouteKind::FileManagement),
        ProviderServiceCategory::VectorStore => Some(CapabilityRouteKind::VectorStoreManagement),
        ProviderServiceCategory::Batch => Some(CapabilityRouteKind::BatchJob),
        ProviderServiceCategory::FineTuning => Some(CapabilityRouteKind::FineTuningJob),
        ProviderServiceCategory::Eval | ProviderServiceCategory::Grader => {
            Some(CapabilityRouteKind::EvalRun)
        }
        ProviderServiceCategory::Container => Some(CapabilityRouteKind::ContainerSession),
        ProviderServiceCategory::Realtime => Some(CapabilityRouteKind::RealtimeSession),
        ProviderServiceCategory::Admin => Some(CapabilityRouteKind::AdminOperation),
        ProviderServiceCategory::Webhook => Some(CapabilityRouteKind::WebhookVerification),
        ProviderServiceCategory::Audio if operation_is_speech_to_text(&capability.operation_id) => {
            Some(CapabilityRouteKind::SpeechToText)
        }
        ProviderServiceCategory::Audio => Some(CapabilityRouteKind::TextToSpeech),
        ProviderServiceCategory::Conversation | ProviderServiceCategory::Model => None,
    }
}

fn operation_is_speech_to_text(operation_id: &str) -> bool {
    operation_id.contains("speech_to_text")
        || operation_id.contains("speech-to-text")
        || operation_id.contains("transcription")
}

fn validate_plugin_index_paths(
    packages_root: &Path,
    settings: &PluginSettingsFile,
    enabled_ids: &BTreeSet<String>,
    source_enabled: bool,
    kind: &'static str,
) -> Result<(), RuntimeConfigError> {
    for record in settings.records.iter().filter(|record| {
        source_enabled && record.enabled && enabled_ids.contains(&record.plugin_id.0)
    }) {
        let package_dir = Path::new(&record.package_dir);
        if package_dir.components().count() != 1
            || !matches!(
                package_dir.components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return Err(RuntimeConfigError::Invalid {
                kind,
                reason: "package directory must be a single relative path component".to_owned(),
            });
        }
        validate_existing_directory_chain(packages_root, package_dir, kind)?;
    }
    Ok(())
}

fn validate_agent_profiles(
    builtin: &[AgentProfile],
    user: &[AgentProfile],
) -> Result<(), RuntimeConfigError> {
    let mut ids = builtin
        .iter()
        .map(|profile| profile.id.as_str())
        .collect::<HashSet<_>>();
    for profile in user {
        validate_agent_profile(profile).map_err(|source| RuntimeConfigError::Invalid {
            kind: "global agent profiles",
            reason: source.to_string(),
        })?;
        if !ids.insert(profile.id.as_str()) {
            return Err(RuntimeConfigError::Invalid {
                kind: "global agent profiles",
                reason: format!("profile id `{}` is duplicated", profile.id),
            });
        }
    }
    Ok(())
}

fn load_provider_credentials(
    global_config_root: &Path,
) -> Result<BTreeMap<String, ProviderCredential>, RuntimeConfigError> {
    let profiles = read_optional_json::<Vec<ProviderProfileDefinition>>(
        &global_config_root.join(PROVIDER_PROFILES_FILE),
        "provider profiles",
    )?
    .ok_or_else(|| RuntimeConfigError::Invalid {
        kind: "provider profiles",
        reason: "configuration is missing".to_owned(),
    })?;
    let secrets = read_optional_json::<ProviderSecretsFile>(
        &global_config_root.join(PROVIDER_SECRETS_FILE),
        "provider secrets",
    )?
    .ok_or_else(|| RuntimeConfigError::Invalid {
        kind: "provider secrets",
        reason: "configuration is missing".to_owned(),
    })?;
    let secrets = match secrets {
        ProviderSecretsFile::Record(record) => record.entries,
        ProviderSecretsFile::Legacy(entries) => entries,
    };
    let secrets = secrets
        .into_iter()
        .map(|secret| (secret.config_id.clone(), secret))
        .collect::<BTreeMap<_, _>>();
    Ok(profiles
        .into_iter()
        .filter_map(|profile| {
            let secret = secrets.get(&profile.id)?;
            (!secret.api_key.trim().is_empty()).then(|| {
                (
                    profile.id.clone(),
                    ProviderCredential {
                        provider_id: profile.provider_id,
                        config_id: profile.id,
                        api_key: secret.api_key.clone(),
                        base_url: profile.base_url,
                    },
                )
            })
        })
        .collect())
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ProviderSecretsFile {
    Record(ProviderSecretsRecord),
    Legacy(Vec<ProviderSecretEntry>),
}

struct DaemonProviderCredentialResolver {
    credentials: BTreeMap<String, ProviderCredential>,
    routes: ProviderCapabilityRouteSettings,
}

impl ProviderCredentialResolverCap for DaemonProviderCredentialResolver {
    fn resolve_provider_credential(
        &self,
        context: ProviderCredentialResolveContext,
    ) -> futures::future::BoxFuture<'_, Result<ProviderCredential, ToolError>> {
        Box::pin(async move {
            let config_id = match (context.operation_id.as_deref(), context.route_kind) {
                (Some(operation), Some(kind)) => self
                    .routes
                    .routes
                    .iter()
                    .find(|route| {
                        route.enabled
                            && route.kind == kind
                            && route.provider_id == context.provider_id
                            && route
                                .operation_ids
                                .iter()
                                .any(|configured| configured == operation)
                    })
                    .map(|route| route.config_id.as_str()),
                _ => context.model_config_id.as_deref(),
            }
            .ok_or_else(|| {
                ToolError::PermissionDenied(
                    "provider service credential resolution is unavailable".to_owned(),
                )
            })?;
            let credential = self.credentials.get(config_id).filter(|credential| {
                credential.provider_id == context.provider_id
                    && !credential.api_key.trim().is_empty()
            });
            credential.cloned().ok_or_else(|| {
                ToolError::PermissionDenied(
                    "provider service credential resolution is unavailable".to_owned(),
                )
            })
        })
    }
}

#[cfg(test)]
mod security_tests {
    use std::{io::Cursor, path::Path};

    use harness_contracts::McpServerSource;
    use serde_json::json;

    #[test]
    fn bounded_sidecar_read_rejects_growth_beyond_initial_metadata_length() {
        let mut bytes = Cursor::new(vec![0_u8; 9]);
        let error =
            super::read_executable_bytes_bounded(&mut bytes, 4, 8, Path::new("fixture-sidecar"))
                .expect_err("reader growth beyond the cap must fail closed");

        assert!(matches!(error, super::RuntimeConfigError::Invalid { .. }));
    }

    #[test]
    fn persisted_mcp_records_default_to_user_source() {
        let record = serde_json::from_value::<super::RuntimeMcpServerConfig>(json!({
            "enabled": true,
            "displayName": "global",
            "id": "shared",
            "scope": "global",
            "transport": {"kind": "stdio", "command": "node"}
        }))
        .expect("deserialize global MCP record");

        assert_eq!(record.source, McpServerSource::User);
    }

    #[test]
    fn project_mcp_override_preserves_required_policy_and_source() {
        let mut global = serde_json::from_value::<super::RuntimeMcpServerConfig>(json!({
            "enabled": true,
            "required": false,
            "displayName": "global",
            "id": "shared",
            "scope": "global",
            "transport": {"kind": "stdio", "command": "node"}
        }))
        .expect("deserialize global MCP record");
        global.source = McpServerSource::User;
        let mut project = serde_json::from_value::<super::RuntimeMcpServerConfig>(json!({
            "enabled": true,
            "required": true,
            "displayName": "project",
            "id": "shared",
            "scope": "session",
            "transport": {"kind": "stdio", "command": "node"}
        }))
        .expect("deserialize project MCP record");
        project.source = McpServerSource::Project;

        let merged =
            super::merge_mcp_servers(vec![global], Some(vec![project])).expect("merge MCP records");

        assert_eq!(merged.len(), 1);
        assert!(merged[0].required);
        assert_eq!(merged[0].source, McpServerSource::Project);
    }
}
