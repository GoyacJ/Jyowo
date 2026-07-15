use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use harness_contracts::{
    DeferPolicy, ModelProvider, ProviderRestriction, ToolDescriptor, ToolError, ToolGroup,
    ToolOrigin, ToolProfile, ToolSearchMode,
};
use parking_lot::{Mutex, MutexGuard};

use crate::{SchemaResolverContext, Tool, ToolJournalAuthority, ToolRegistrySnapshot};

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

#[derive(Debug, Clone, PartialEq, Eq)]
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

impl ToolPoolFilter {
    #[must_use]
    pub fn from_profile(profile: &ToolProfile) -> Self {
        match profile {
            ToolProfile::Minimal => Self::minimal(),
            ToolProfile::Coding => Self {
                mcp_included: false,
                plugin_included: false,
                group_allowlist: Some(HashSet::from([
                    ToolGroup::Agent,
                    ToolGroup::Clarification,
                    ToolGroup::Coordinator,
                    ToolGroup::FileSystem,
                    ToolGroup::Memory,
                    ToolGroup::Meta,
                    ToolGroup::Search,
                    ToolGroup::Shell,
                ])),
                ..Self::default()
            },
            ToolProfile::Full => Self::default(),
            ToolProfile::Custom {
                allowlist,
                denylist,
                group_allowlist,
                group_denylist,
                mcp_included,
                plugin_included,
            } => Self {
                allowlist: (!allowlist.is_empty()).then(|| allowlist.iter().cloned().collect()),
                denylist: denylist.iter().cloned().collect(),
                mcp_included: *mcp_included,
                plugin_included: *plugin_included,
                group_allowlist: (!group_allowlist.is_empty())
                    .then(|| group_allowlist.iter().cloned().collect()),
                group_denylist: group_denylist.iter().cloned().collect(),
            },
            _ => Self::minimal(),
        }
    }

    fn minimal() -> Self {
        Self {
            mcp_included: false,
            plugin_included: false,
            group_allowlist: Some(HashSet::from([
                ToolGroup::Clarification,
                ToolGroup::Coordinator,
                ToolGroup::Meta,
            ])),
            ..Self::default()
        }
    }

    pub fn intersect_with(&mut self, profile_filter: Self) {
        self.allowlist = intersect_optional_sets(self.allowlist.take(), profile_filter.allowlist);
        self.denylist.extend(profile_filter.denylist);
        self.mcp_included &= profile_filter.mcp_included;
        self.plugin_included &= profile_filter.plugin_included;
        self.group_allowlist =
            intersect_optional_sets(self.group_allowlist.take(), profile_filter.group_allowlist);
        self.group_denylist.extend(profile_filter.group_denylist);
    }

    /// Returns whether this filter admits a descriptor before provider-specific
    /// restrictions are applied.
    #[must_use]
    pub fn allows_descriptor(&self, descriptor: &ToolDescriptor) -> bool {
        existing_filter_allows(self, descriptor)
    }
}

fn intersect_optional_sets<T>(
    left: Option<HashSet<T>>,
    right: Option<HashSet<T>>,
) -> Option<HashSet<T>>
where
    T: Clone + Eq + std::hash::Hash,
{
    match (left, right) {
        (Some(left), Some(right)) => Some(left.intersection(&right).cloned().collect()),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

#[derive(Clone, Default)]
pub struct ToolPool {
    always_loaded: Vec<Arc<dyn Tool>>,
    deferred: Vec<Arc<dyn Tool>>,
    runtime_appended: Vec<Arc<dyn Tool>>,
    materialized_runtime_appended: Arc<Mutex<Vec<Arc<dyn Tool>>>>,
    descriptors: BTreeMap<String, Arc<ToolDescriptor>>,
    journal_authorities: BTreeMap<String, ToolJournalAuthority>,
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
                journal_authority: snapshot.journal_authority(name),
            });
        }

        let auto_defer_enabled = auto_defer_enabled(
            search_mode,
            model_profile,
            prepared.iter().map(|entry| &entry.descriptor),
        );

        for PreparedTool {
            tool,
            descriptor,
            journal_authority,
        } in prepared
        {
            let partition = partition_for(&descriptor, search_mode, auto_defer_enabled)?;
            pool.descriptors
                .insert(descriptor.name.clone(), Arc::new(descriptor));
            pool.journal_authorities
                .insert(tool.descriptor().name.clone(), journal_authority);

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
        let mut seen = std::collections::HashSet::new();
        let deferred_names = self
            .deferred
            .iter()
            .map(|tool| tool.descriptor().name.clone())
            .collect::<std::collections::HashSet<_>>();
        let mut descriptors = Vec::new();
        for tool in &self.always_loaded {
            if seen.insert(tool.descriptor().name.clone()) {
                descriptors.push(tool.descriptor().clone());
            }
        }
        for tool in self.materialized_runtime_appended() {
            if seen.insert(tool.descriptor().name.clone()) {
                descriptors.push(tool.descriptor().clone());
            }
        }
        for tool in &self.runtime_appended {
            if !deferred_names.contains(&tool.descriptor().name)
                && seen.insert(tool.descriptor().name.clone())
            {
                descriptors.push(tool.descriptor().clone());
            }
        }
        descriptors
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
            .entry(descriptor.name.clone())
            .or_insert_with(|| Arc::new(descriptor));
        self.journal_authorities
            .entry(tool.descriptor().name.clone())
            .or_insert(ToolJournalAuthority::None);
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
                if self
                    .runtime_appended
                    .iter()
                    .any(|tool| tool.descriptor().name == *name)
                {
                    added.push(name.clone());
                }
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
                pool.journal_authorities.insert(
                    tool.descriptor().name.clone(),
                    self.journal_authority(&tool.descriptor().name),
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
                pool.journal_authorities.insert(
                    tool.descriptor().name.clone(),
                    self.journal_authority(&tool.descriptor().name),
                );
                pool.deferred.push(Arc::clone(tool));
            }
        }
        for tool in &self.runtime_appended {
            if !self.has_partitioned_tool(&tool.descriptor().name)
                && existing_filter_allows(filter, tool.descriptor())
            {
                pool.descriptors
                    .entry(tool.descriptor().name.clone())
                    .or_insert_with(|| Arc::new(tool.descriptor().clone()));
                pool.journal_authorities
                    .entry(tool.descriptor().name.clone())
                    .or_insert_with(|| self.journal_authority(&tool.descriptor().name));
                pool.runtime_appended.push(Arc::clone(tool));
            }
        }
        for tool in self.materialized_runtime_appended() {
            if existing_filter_allows(filter, tool.descriptor()) {
                pool.descriptors
                    .entry(tool.descriptor().name.clone())
                    .or_insert_with(|| Arc::new(tool.descriptor().clone()));
                pool.journal_authorities
                    .entry(tool.descriptor().name.clone())
                    .or_insert_with(|| self.journal_authority(&tool.descriptor().name));
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

    pub fn journal_authority(&self, name: &str) -> ToolJournalAuthority {
        self.journal_authorities
            .get(name)
            .copied()
            .unwrap_or_default()
    }

    fn lock_materialized_runtime_appended(&self) -> MutexGuard<'_, Vec<Arc<dyn Tool>>> {
        self.materialized_runtime_appended.lock()
    }

    fn has_partitioned_tool(&self, name: &str) -> bool {
        self.always_loaded
            .iter()
            .chain(self.deferred.iter())
            .any(|tool| tool.descriptor().name == name)
            || self
                .lock_materialized_runtime_appended()
                .iter()
                .any(|tool| tool.descriptor().name == name)
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
    journal_authority: ToolJournalAuthority,
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
