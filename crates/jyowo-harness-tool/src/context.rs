use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use harness_contracts::{
    AgentId, CapabilityRegistry, CorrelationId, MemoryThreadSettings, PermissionActorSource,
    Redactor, RunId, RunModelSnapshot, SessionId, TenantId, ToolCapability, ToolDescriptor,
    ToolError, ToolRuntimeSettings, ToolUseId,
};
use harness_sandbox::SandboxBackend;
use serde_json::Value;

pub const TOOL_RUNTIME_SETTINGS_CAPABILITY: &str = "jyowo.tool.runtime_settings";
pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 120_000;
pub const MIN_TOOL_TIMEOUT_MS: u64 = 1_000;
pub const MAX_TOOL_TIMEOUT_MS: u64 = 86_400_000;

#[derive(Debug, Clone, Default)]
pub struct ToolRuntimeSettingsRegistry {
    settings: BTreeMap<String, ToolRuntimeSettings>,
}

impl ToolRuntimeSettingsRegistry {
    #[must_use]
    pub fn new(settings: BTreeMap<String, ToolRuntimeSettings>) -> Self {
        Self { settings }
    }

    #[must_use]
    pub fn get(&self, tool_name: &str) -> Option<&ToolRuntimeSettings> {
        self.settings.get(tool_name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveToolRuntimeSettings {
    pub timeout_ms: u64,
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaResolverContext {
    pub run_id: RunId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentRunHandle {
    pub run_id: RunId,
    pub session_id: SessionId,
}

#[derive(Clone)]
pub struct ToolContext {
    pub tool_use_id: ToolUseId,
    pub run_id: RunId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub model: Option<RunModelSnapshot>,
    pub model_config_id: Option<String>,
    pub memory_thread_settings: Option<MemoryThreadSettings>,
    pub correlation_id: CorrelationId,
    pub agent_id: AgentId,
    pub subagent_depth: u8,
    pub workspace_root: PathBuf,
    pub project_workspace_root: Option<PathBuf>,
    pub sandbox: Option<Arc<dyn SandboxBackend>>,
    pub cap_registry: Arc<CapabilityRegistry>,
    pub redactor: Arc<dyn Redactor>,
    pub interrupt: InterruptToken,
    pub parent_run: Option<ParentRunHandle>,
    pub actor_source: PermissionActorSource,
}

impl ToolContext {
    pub fn capability<T>(&self, cap: ToolCapability) -> Result<Arc<T>, ToolError>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        self.cap_registry
            .get::<T>(&cap)
            .ok_or(ToolError::CapabilityMissing(cap))
    }

    #[must_use]
    pub fn configured_runtime_settings(
        &self,
        descriptor: &ToolDescriptor,
    ) -> Option<ToolRuntimeSettings> {
        let capability = ToolCapability::Custom(TOOL_RUNTIME_SETTINGS_CAPABILITY.to_owned());
        self.cap_registry
            .get::<ToolRuntimeSettingsRegistry>(&capability)
            .and_then(|registry| registry.get(&descriptor.name).cloned())
    }

    #[must_use]
    pub fn runtime_settings(&self, descriptor: &ToolDescriptor) -> EffectiveToolRuntimeSettings {
        let configured = self.configured_runtime_settings(descriptor);
        effective_tool_runtime_settings(descriptor, configured.as_ref())
    }
}

#[must_use]
pub fn effective_tool_runtime_settings(
    descriptor: &ToolDescriptor,
    configured: Option<&ToolRuntimeSettings>,
) -> EffectiveToolRuntimeSettings {
    let default_timeout_ms = descriptor
        .metadata
        .configuration
        .as_ref()
        .and_then(|configuration| configuration.default_timeout_ms)
        .or_else(|| {
            descriptor
                .properties
                .long_running
                .as_ref()
                .and_then(|policy| u64::try_from(policy.hard_timeout.as_millis()).ok())
        })
        .unwrap_or(DEFAULT_TOOL_TIMEOUT_MS);
    let mut parameters = descriptor
        .metadata
        .configuration
        .as_ref()
        .map_or_else(empty_parameters, |configuration| {
            configuration.default_parameters.clone()
        });
    if let Some(configured) = configured {
        merge_json_objects(&mut parameters, &configured.parameters);
    }
    EffectiveToolRuntimeSettings {
        timeout_ms: configured.map_or(default_timeout_ms, |settings| settings.timeout_ms),
        parameters,
    }
}

pub fn validate_tool_runtime_settings(
    descriptor: &ToolDescriptor,
    settings: &ToolRuntimeSettings,
) -> Result<(), String> {
    if !(MIN_TOOL_TIMEOUT_MS..=MAX_TOOL_TIMEOUT_MS).contains(&settings.timeout_ms) {
        return Err(format!(
            "timeoutMs must be between {MIN_TOOL_TIMEOUT_MS} and {MAX_TOOL_TIMEOUT_MS}"
        ));
    }
    if !settings.parameters.is_object() {
        return Err("tool parameters must be a JSON object".to_owned());
    }
    let effective = effective_tool_runtime_settings(descriptor, Some(settings));
    let Some(configuration) = descriptor.metadata.configuration.as_ref() else {
        return if effective
            .parameters
            .as_object()
            .is_some_and(serde_json::Map::is_empty)
        {
            Ok(())
        } else {
            Err("tool does not declare configurable parameters".to_owned())
        };
    };
    let validator = jsonschema::validator_for(&configuration.schema)
        .map_err(|error| format!("configuration schema is invalid: {error}"))?;
    if validator.is_valid(&effective.parameters) {
        Ok(())
    } else {
        Err(validator
            .iter_errors(&effective.parameters)
            .next()
            .map_or_else(
                || "parameters do not match the tool configuration schema".to_owned(),
                |error| error.to_string(),
            ))
    }
}

fn empty_parameters() -> Value {
    Value::Object(Default::default())
}

fn merge_json_objects(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(key) {
                    Some(existing) => merge_json_objects(existing, value),
                    None => {
                        base.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct InterruptToken {
    interrupted: Arc<AtomicBool>,
}

impl InterruptToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn interrupt(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
    }

    pub fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }
}
