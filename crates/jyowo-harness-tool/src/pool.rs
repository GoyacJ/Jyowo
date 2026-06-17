use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use harness_contracts::{
    DeferPolicy, ModelProvider, ProviderRestriction, ToolDescriptor, ToolError, ToolGroup,
    ToolOrigin, ToolSearchMode,
};
use parking_lot::{Mutex, MutexGuard};

use crate::{SchemaResolverContext, Tool, ToolRegistrySnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPoolModelProfile {
    pub provider: ModelProvider,
    pub max_context_tokens: Option<u32>,
}

impl Default for ToolPoolModelProfile {
    fn default() -> Self {
        Self {
            provider: ModelProvider("unknown".to_owned()),
            max_context_tokens: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolPoolFilter {
    pub allowlist: Option<HashSet<String>>,
    pub denylist: HashSet<String>,
    pub mcp_included: bool,
    pub plugin_included: bool,
    pub group_allowlist: Option<HashSet<ToolGroup>>,
    pub group_denylist: HashSet<ToolGroup>,
}

impl Default for ToolPoolFilter {
    fn default() -> Self {
        Self {
            allowlist: None,
            denylist: HashSet::new(),
            mcp_included: true,
            plugin_included: true,
            group_allowlist: None,
            group_denylist: HashSet::new(),
        }
    }
}

#[derive(Clone, Default)]
pub struct ToolPool {
    always_loaded: Vec<Arc<dyn Tool>>,
    deferred: Vec<Arc<dyn Tool>>,
    runtime_appended: Vec<Arc<dyn Tool>>,
    materialized_runtime_appended: Arc<Mutex<Vec<Arc<dyn Tool>>>>,
    descriptors: BTreeMap<String, Arc<ToolDescriptor>>,
}

impl ToolPool {
    pub async fn assemble(
        snapshot: &ToolRegistrySnapshot,
        filter: &ToolPoolFilter,
        search_mode: &ToolSearchMode,
        model_profile: &ToolPoolModelProfile,
        schema_resolver_ctx: &SchemaResolverContext,
    ) -> Result<Self, ToolError> {
        let mut pool = Self::default();
        let mut prepared = Vec::new();

        for (name, tool) in snapshot.iter_sorted() {
            let Some(snapshot_descriptor) = snapshot.descriptor(name) else {
                continue;
            };

            if !filter_allows(filter, snapshot_descriptor, model_profile) {
                continue;
            }

            let mut descriptor = snapshot_descriptor.as_ref().clone();
            if descriptor.dynamic_schema {
                descriptor.input_schema = tool.resolve_schema(schema_resolver_ctx).await?;
            }

            prepared.push(PreparedTool {
                tool: Arc::clone(tool),
                descriptor,
            });
        }

        let auto_defer_enabled = auto_defer_enabled(
            search_mode,
            model_profile,
            prepared.iter().map(|entry| &entry.descriptor),
        );

        for PreparedTool { tool, descriptor } in prepared {
            let partition = partition_for(&descriptor, search_mode, auto_defer_enabled)?;
            pool.descriptors
                .insert(descriptor.name.clone(), Arc::new(descriptor));

            match partition {
                ToolPoolPartition::AlwaysLoaded => pool.always_loaded.push(tool),
                ToolPoolPartition::Deferred => pool.deferred.push(tool),
            }
        }

        Ok(pool)
    }

    pub fn always_loaded(&self) -> &[Arc<dyn Tool>] {
        &self.always_loaded
    }

    pub fn deferred(&self) -> &[Arc<dyn Tool>] {
        &self.deferred
    }

    pub fn runtime_appended(&self) -> &[Arc<dyn Tool>] {
        &self.runtime_appended
    }

    pub fn materialized_runtime_appended(&self) -> Vec<Arc<dyn Tool>> {
        self.lock_materialized_runtime_appended().clone()
    }

    #[must_use]
    pub fn prompt_visible_descriptors(&self) -> Vec<ToolDescriptor> {
        self.always_loaded
            .iter()
            .chain(self.runtime_appended.iter())
            .cloned()
            .chain(self.materialized_runtime_appended())
            .map(|tool| tool.descriptor().clone())
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.iter()
            .find(|tool| tool.descriptor().name == name)
            .map(Arc::clone)
            .or_else(|| {
                self.lock_materialized_runtime_appended()
                    .iter()
                    .find(|tool| tool.descriptor().name == name)
                    .map(Arc::clone)
            })
    }

    pub fn append_runtime_tool(&mut self, tool: Arc<dyn Tool>) {
        let descriptor = tool.descriptor().clone();
        self.descriptors
            .insert(descriptor.name.clone(), Arc::new(descriptor));
        self.runtime_appended.push(tool);
    }

    pub fn materialize_deferred_tools(&self, names: &[String]) -> Vec<String> {
        let mut materialized = self.lock_materialized_runtime_appended();
        let mut added = Vec::new();
        for name in names {
            if self
                .always_loaded
                .iter()
                .any(|tool| tool.descriptor().name == *name)
                || self
                    .runtime_appended
                    .iter()
                    .any(|tool| tool.descriptor().name == *name)
                || materialized
                    .iter()
                    .any(|tool| tool.descriptor().name == *name)
            {
                added.push(name.clone());
                continue;
            }
            let Some(tool) = self
                .deferred
                .iter()
                .find(|tool| tool.descriptor().name == *name)
            else {
                continue;
            };
            materialized.push(Arc::clone(tool));
            added.push(name.clone());
        }
        added
    }

    #[must_use]
    pub fn filtered(&self, filter: &ToolPoolFilter) -> Self {
        let mut pool = Self::default();
        for tool in &self.always_loaded {
            if existing_filter_allows(filter, tool.descriptor()) {
                pool.descriptors.insert(
                    tool.descriptor().name.clone(),
                    Arc::new(tool.descriptor().clone()),
                );
                pool.always_loaded.push(Arc::clone(tool));
            }
        }
        for tool in &self.deferred {
            if existing_filter_allows(filter, tool.descriptor()) {
                pool.descriptors.insert(
                    tool.descriptor().name.clone(),
                    Arc::new(tool.descriptor().clone()),
                );
                pool.deferred.push(Arc::clone(tool));
            }
        }
        for tool in &self.runtime_appended {
            if existing_filter_allows(filter, tool.descriptor()) {
                pool.descriptors.insert(
                    tool.descriptor().name.clone(),
                    Arc::new(tool.descriptor().clone()),
                );
                pool.runtime_appended.push(Arc::clone(tool));
            }
        }
        for tool in self.materialized_runtime_appended() {
            if existing_filter_allows(filter, tool.descriptor()) {
                pool.descriptors.insert(
                    tool.descriptor().name.clone(),
                    Arc::new(tool.descriptor().clone()),
                );
                pool.lock_materialized_runtime_appended().push(tool);
            }
        }
        pool
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Tool>> {
        self.always_loaded
            .iter()
            .chain(self.deferred.iter())
            .chain(self.runtime_appended.iter())
    }

    pub fn descriptor(&self, name: &str) -> Option<&ToolDescriptor> {
        self.descriptors.get(name).map(std::convert::AsRef::as_ref)
    }

    fn lock_materialized_runtime_appended(&self) -> MutexGuard<'_, Vec<Arc<dyn Tool>>> {
        self.materialized_runtime_appended.lock()
    }
}

fn existing_filter_allows(filter: &ToolPoolFilter, descriptor: &ToolDescriptor) -> bool {
    if let Some(allowlist) = &filter.allowlist {
        if !allowlist.contains(&descriptor.name) {
            return false;
        }
    }

    if filter.denylist.contains(&descriptor.name) {
        return false;
    }

    if let Some(group_allowlist) = &filter.group_allowlist {
        if !group_allowlist.contains(&descriptor.group) {
            return false;
        }
    }

    if filter.group_denylist.contains(&descriptor.group) {
        return false;
    }

    match &descriptor.origin {
        ToolOrigin::Mcp(_) if !filter.mcp_included => return false,
        ToolOrigin::Plugin { .. } if !filter.plugin_included => return false,
        _ => {}
    }

    true
}

enum ToolPoolPartition {
    AlwaysLoaded,
    Deferred,
}

struct PreparedTool {
    tool: Arc<dyn Tool>,
    descriptor: ToolDescriptor,
}

fn filter_allows(
    filter: &ToolPoolFilter,
    descriptor: &ToolDescriptor,
    model_profile: &ToolPoolModelProfile,
) -> bool {
    if let Some(allowlist) = &filter.allowlist {
        if !allowlist.contains(&descriptor.name) {
            return false;
        }
    }

    if filter.denylist.contains(&descriptor.name) {
        return false;
    }

    if let Some(group_allowlist) = &filter.group_allowlist {
        if !group_allowlist.contains(&descriptor.group) {
            return false;
        }
    }

    if filter.group_denylist.contains(&descriptor.group) {
        return false;
    }

    match &descriptor.origin {
        ToolOrigin::Mcp(_) if !filter.mcp_included => return false,
        ToolOrigin::Plugin { .. } if !filter.plugin_included => return false,
        _ => {}
    }

    provider_allows(&descriptor.provider_restriction, &model_profile.provider)
}

fn provider_allows(restriction: &ProviderRestriction, provider: &ModelProvider) -> bool {
    match restriction {
        ProviderRestriction::Allowlist(providers) => providers.contains(provider),
        ProviderRestriction::Denylist(providers) => !providers.contains(provider),
        _ => true,
    }
}

fn partition_for(
    descriptor: &ToolDescriptor,
    search_mode: &ToolSearchMode,
    auto_defer_enabled: bool,
) -> Result<ToolPoolPartition, ToolError> {
    match descriptor.properties.defer_policy {
        DeferPolicy::AutoDefer => match search_mode {
            ToolSearchMode::Always => Ok(ToolPoolPartition::Deferred),
            ToolSearchMode::Auto { .. } if auto_defer_enabled => Ok(ToolPoolPartition::Deferred),
            ToolSearchMode::Disabled | ToolSearchMode::Auto { .. } => {
                Ok(ToolPoolPartition::AlwaysLoaded)
            }
            _ => Ok(ToolPoolPartition::AlwaysLoaded),
        },
        DeferPolicy::ForceDefer => match search_mode {
            ToolSearchMode::Disabled => Err(ToolError::DeferralRequired {
                tool: descriptor.name.clone(),
            }),
            ToolSearchMode::Always | ToolSearchMode::Auto { .. } => Ok(ToolPoolPartition::Deferred),
            _ => Err(ToolError::SchemaResolution(format!(
                "deferral required but tool search mode is unsupported: {}",
                descriptor.name
            ))),
        },
        _ => Ok(ToolPoolPartition::AlwaysLoaded),
    }
}

fn auto_defer_enabled<'a>(
    search_mode: &ToolSearchMode,
    model_profile: &ToolPoolModelProfile,
    descriptors: impl Iterator<Item = &'a ToolDescriptor>,
) -> bool {
    match search_mode {
        ToolSearchMode::Always => true,
        ToolSearchMode::Auto {
            ratio,
            min_absolute_tokens,
        } => {
            let Some(max_context_tokens) = model_profile.max_context_tokens else {
                return false;
            };
            let schema_chars: usize = descriptors
                .filter(|descriptor| descriptor.properties.defer_policy == DeferPolicy::AutoDefer)
                .map(auto_defer_schema_chars)
                .sum();
            let estimated_tokens = u64::try_from(schema_chars)
                .unwrap_or(u64::MAX)
                .saturating_mul(2)
                .saturating_add(4)
                / 5;
            let threshold_tokens =
                (f64::from(max_context_tokens) * f64::from(*ratio)).ceil() as u64;
            estimated_tokens >= threshold_tokens.max(u64::from(*min_absolute_tokens))
        }
        _ => false,
    }
}

fn auto_defer_schema_chars(descriptor: &ToolDescriptor) -> usize {
    descriptor.name.len()
        + descriptor.display_name.len()
        + descriptor.description.len()
        + descriptor.search_hint.as_ref().map_or(0, String::len)
        + serde_json::to_string(&descriptor.input_schema).map_or(0, |schema| schema.len())
        + descriptor
            .output_schema
            .as_ref()
            .and_then(|schema| serde_json::to_string(schema).ok())
            .map_or(0, |schema| schema.len())
}
