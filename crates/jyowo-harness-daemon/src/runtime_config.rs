use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fmt, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use harness_contracts::{
    validate_agent_profile, validate_provider_capability_route, AgentProfile,
    AgentProfileSelectionRecord, ExecutionDefaultsRecord, ExecutionOverridesRecord,
    McpServerSource, PluginId, PluginSelectionRecord, ProviderCapabilityRoute,
    ProviderCapabilityRouteSettings, ProviderCredential, ProviderCredentialResolveContext,
    ProviderCredentialResolverCap, ProviderProfileDefinition, ProviderSecretEntry,
    ProviderSecretsRecord, ProviderSelectionRecord, SkillSelectionRecord, ToolError,
};
use harness_plugin::{
    DiscoverySource, FileManifestLoader, ManifestLoadReport, ManifestLoaderError, PluginConfig,
    PluginManifestLoader, PluginName, PluginRegistry, PluginRuntimeLoader,
};
use jyowo_harness_sdk::{
    builtin_agent_profiles,
    ext::{DirectorySourceKind, SkillLoader, SkillSourceConfig},
    SkillConfigSnapshot,
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;
use thiserror::Error;

use crate::{ProviderConfigError, ProviderConfigResolver, ResolvedProviderConfig};

const PROVIDER_SELECTION_FILE: &str = "provider-selection.json";
const EXECUTION_OVERRIDES_FILE: &str = "execution-overrides.json";
const PROVIDER_ROUTES_FILE: &str = "provider-capability-routes.json";
const MCP_SERVERS_FILE: &str = "mcp-servers.json";
const SKILLS_FILE: &str = "skills.json";
const PLUGINS_FILE: &str = "plugins.json";
const AGENT_PROFILES_FILE: &str = "agent-profiles.json";
const AGENT_PROFILE_SELECTION_FILE: &str = "agent-profile-selection.json";
const PROVIDER_PROFILES_FILE: &str = "provider-profiles.json";
const PROVIDER_SECRETS_FILE: &str = "provider-secrets.json";

/// Resolves immutable SDK factory inputs for one canonical workspace.
#[derive(Debug, Clone)]
pub struct RuntimeConfigResolver {
    global_config_root: PathBuf,
}

impl RuntimeConfigResolver {
    #[must_use]
    pub fn new(global_config_root: impl Into<PathBuf>) -> Self {
        Self {
            global_config_root: global_config_root.into(),
        }
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
        let provider = provider_resolver.resolve(selected_config_id)?;

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
        let mcp_servers = merge_mcp_servers(global_mcp, project_mcp)?;

        let global_skill_selection = read_optional_json::<SkillSelectionRecord>(
            &global_config_root.join(SKILLS_FILE),
            "global skill selection",
        )?
        .unwrap_or_default();
        let project_skill_selection = read_optional_json::<SkillSelectionRecord>(
            &project_config_root.join(SKILLS_FILE),
            "project skill selection",
        )?;
        let enabled_skill_ids = project_skill_selection
            .as_ref()
            .map(|selection| selection.enabled.iter().cloned().collect())
            .unwrap_or_else(|| global_skill_selection.enabled.iter().cloned().collect());
        let skill_loader = build_skill_loader(
            global_home,
            &workspace_root,
            &global_skill_selection,
            project_skill_selection.as_ref(),
        )
        .freeze_directory_sources()
        .map_err(|source| RuntimeConfigError::Invalid {
            kind: "skill packages",
            reason: source.to_string(),
        })?;

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
            "global plugin index",
        )?;
        validate_plugin_index_paths(
            &workspace_root.join(".jyowo/plugins/packages"),
            &project_plugin_records,
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

        let credentials = load_provider_credentials(&global_config_root)?;

        Ok(RuntimeConfigSnapshot {
            workspace_root: workspace_root.clone(),
            provider,
            execution_defaults,
            provider_routes: provider_routes.clone(),
            mcp_servers,
            plugin_snapshot,
            skill_loader,
            skill_config: SkillConfigSnapshot::new(),
            enabled_skill_ids,
            enabled_plugin_ids,
            allow_project_plugins,
            agent_profiles,
            default_agent_profile_id,
            memory_database_path: workspace_root.join(".jyowo/runtime/memory/memory.sqlite3"),
            provider_credential_resolver: Arc::new(DaemonProviderCredentialResolver {
                credentials,
                routes: provider_routes,
            }),
        })
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
        ensure_secure_directory_chain(
            &self.workspace_root,
            Path::new(".jyowo/runtime/memory"),
            "runtime memory directory",
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn with_plugin_runtime_loader(
        mut self,
        loader: Arc<dyn PluginRuntimeLoader>,
    ) -> Self {
        self.plugin_snapshot.runtime_loaders.push(loader);
        self
    }
}

#[derive(Clone)]
struct RuntimePluginSnapshot {
    config: PluginConfig,
    sources: Vec<FrozenPluginSource>,
    runtime_loaders: Vec<Arc<dyn PluginRuntimeLoader>>,
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

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeMcpServerConfig {
    pub enabled: bool,
    pub display_name: String,
    pub id: String,
    pub scope: String,
    #[serde(skip, default = "default_mcp_server_source")]
    pub(crate) source: McpServerSource,
    pub(crate) transport: RuntimeMcpTransport,
}

fn default_mcp_server_source() -> McpServerSource {
    McpServerSource::Workspace
}

impl fmt::Debug for RuntimeMcpServerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeMcpServerConfig")
            .field("enabled", &self.enabled)
            .field("display_name", &self.display_name)
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("source", &self.source)
            .field("transport", &self.transport)
            .finish()
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "camelCase")]
pub(crate) enum RuntimeMcpTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: Vec<SecretNameValue>,
        #[serde(default)]
        inherit_env: Vec<String>,
        #[serde(default)]
        working_dir: Option<String>,
    },
    Http {
        url: String,
        #[serde(default)]
        bearer_token_env_var: Option<String>,
        #[serde(default)]
        headers: Vec<SecretNameValue>,
        #[serde(default)]
        headers_from_env: Vec<HeaderFromEnv>,
    },
    InProcess,
}

impl fmt::Debug for RuntimeMcpTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stdio {
                command,
                args,
                env,
                inherit_env,
                working_dir,
            } => formatter
                .debug_struct("Stdio")
                .field("command", command)
                .field("args", args)
                .field(
                    "env_keys",
                    &env.iter().map(|item| &item.key).collect::<Vec<_>>(),
                )
                .field("inherit_env", inherit_env)
                .field("working_dir", working_dir)
                .finish(),
            Self::Http {
                url,
                bearer_token_env_var,
                headers,
                headers_from_env,
            } => formatter
                .debug_struct("Http")
                .field("url", url)
                .field("bearer_token_env_var", bearer_token_env_var)
                .field(
                    "header_keys",
                    &headers.iter().map(|item| &item.key).collect::<Vec<_>>(),
                )
                .field("headers_from_env", headers_from_env)
                .finish(),
            Self::InProcess => formatter.write_str("InProcess"),
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SecretNameValue {
    pub(crate) key: String,
    pub(crate) value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct HeaderFromEnv {
    pub(crate) key: String,
    pub(crate) env_var: String,
}

#[derive(Debug, Error)]
pub enum RuntimeConfigError {
    #[error(transparent)]
    Provider(#[from] ProviderConfigError),
    #[error("failed to read {kind} from {path}")]
    Read {
        kind: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {kind} from {path}")]
    Decode {
        kind: &'static str,
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("workspace root must not be a symbolic link: {path}")]
    WorkspaceSymlink { path: PathBuf },
    #[error("{kind} must not be a symbolic link: {path}")]
    ConfigSymlink { kind: &'static str, path: PathBuf },
    #[error("invalid {kind}: {reason}")]
    Invalid { kind: &'static str, reason: String },
    #[error("failed to initialize plugin registry")]
    PluginRegistry {
        #[source]
        source: harness_plugin::PluginError,
    },
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
        (".jyowo/runtime/memory", "runtime memory directory"),
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
                let metadata =
                    fs::symlink_metadata(&current).map_err(|source| RuntimeConfigError::Read {
                        kind,
                        path: current.clone(),
                        source,
                    })?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(RuntimeConfigError::ConfigSymlink {
                        kind,
                        path: current,
                    });
                }
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
        if server.id.trim().is_empty() {
            return Err(RuntimeConfigError::Invalid {
                kind,
                reason: "server id is empty".to_owned(),
            });
        }
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

fn build_skill_loader(
    global_home: &Path,
    workspace_root: &Path,
    global: &SkillSelectionRecord,
    project: Option<&SkillSelectionRecord>,
) -> SkillLoader {
    let global_allowed = project
        .map(|selection| selection.enabled.iter().cloned().collect())
        .unwrap_or_else(|| global.enabled.iter().cloned().collect());
    let project_allowed = project
        .map(|selection| selection.enabled.iter().cloned().collect())
        .unwrap_or_default();
    SkillLoader::default()
        .with_source(SkillSourceConfig::DirectoryPackages {
            path: global_home.join("skills/packages"),
            source_kind: DirectorySourceKind::User,
            allowed_package_ids: Some(global_allowed),
        })
        .with_source(SkillSourceConfig::DirectoryPackages {
            path: workspace_root.join(".jyowo/skills/packages"),
            source_kind: DirectorySourceKind::Workspace,
            allowed_package_ids: Some(project_allowed),
        })
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
    let mut disabled_plugins = BTreeSet::new();
    let mut entries = BTreeMap::new();
    for record in global.records.iter().chain(project.records.iter()) {
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
        if enabled_ids.contains(&record.plugin_id.0) && record.enabled {
            allowed_user_plugins.insert(name);
        } else {
            disabled_plugins.insert(name);
        }
    }
    let config = PluginConfig {
        allow_project_plugins,
        allowed_user_plugins: Some(allowed_user_plugins),
        disabled_plugins,
        entries,
        workspace_root: Some(workspace_root.to_owned()),
        ..PluginConfig::default()
    };
    let loader = FileManifestLoader;
    let sources = [
        DiscoverySource::User(global_home.join("plugins/packages")),
        DiscoverySource::Workspace(workspace_root.join(".jyowo/plugins/packages")),
    ]
    .into_iter()
    .map(|source| {
        let report = loader.load_source_report(&source).map_err(|source| {
            RuntimeConfigError::PluginRegistry {
                source: source.into(),
            }
        })?;
        Ok(FrozenPluginSource { source, report })
    })
    .collect::<Result<Vec<_>, RuntimeConfigError>>()?;
    Ok(RuntimePluginSnapshot {
        config,
        sources,
        runtime_loaders: Vec::new(),
    })
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

fn validate_plugin_index_paths(
    packages_root: &Path,
    settings: &PluginSettingsFile,
    kind: &'static str,
) -> Result<(), RuntimeConfigError> {
    for record in &settings.records {
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
