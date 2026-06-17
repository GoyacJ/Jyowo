use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use harness_contracts::{
    AgentId, HookEventKind, PluginId, SkillFilter, SkillId, SkillParameterInfo, SkillStatus,
    SkillSummary, SkillView, TrustLevel,
};
use parking_lot::RwLock;

use crate::{
    Skill, SkillError, SkillHookTransport, SkillParamType, SkillRegistration, SkillSource,
};

#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    snapshot: Arc<RwLock<Arc<SkillRegistrySnapshot>>>,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRegistrySnapshot {
    pub generation: u64,
    pub entries: BTreeMap<String, Arc<Skill>>,
    pub by_source: HashMap<SkillSource, Vec<SkillId>>,
    pub status: BTreeMap<SkillId, SkillStatus>,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRegistryBuilder {
    skills: Vec<Skill>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillHookBinding {
    pub handler_id: String,
    pub skill_id: SkillId,
    pub skill_name: String,
    pub source: SkillSource,
    pub hook_id: String,
    pub events: Vec<HookEventKind>,
    pub transport: SkillHookTransport,
}

impl SkillRegistry {
    #[must_use]
    pub fn builder() -> SkillRegistryBuilder {
        SkillRegistryBuilder::default()
    }

    pub fn register(&self, skill: Skill) -> Result<(), SkillError> {
        let current = self.snapshot();
        let mut next = (*current).clone();
        insert_skill(&mut next, skill)?;
        if next.entries != current.entries {
            next.generation = current.generation.saturating_add(1);
        }
        *self.snapshot.write() = Arc::new(next);
        Ok(())
    }

    pub fn register_batch(&self, skills: Vec<Skill>) -> Result<SkillRegistrySnapshot, SkillError> {
        let current = self.snapshot();
        let mut next = (*current).clone();
        for skill in skills {
            insert_skill(&mut next, skill)?;
        }
        if next.entries != current.entries {
            next.generation = current.generation.saturating_add(1);
        }
        *self.snapshot.write() = Arc::new(next.clone());
        Ok(next)
    }

    pub fn candidate_snapshot(
        &self,
        registrations: &[SkillRegistration],
    ) -> Result<SkillRegistrySnapshot, SkillError> {
        let current = self.snapshot();
        let mut next = (*current).clone();
        for registration in registrations {
            insert_registration(&mut next, registration)?;
        }
        if next.entries != current.entries {
            next.generation = current.generation.saturating_add(1);
        }
        Ok(next)
    }

    pub fn commit_snapshot(&self, snapshot: SkillRegistrySnapshot) {
        *self.snapshot.write() = Arc::new(snapshot);
    }

    pub fn register_from_plugin(
        &self,
        plugin_id: PluginId,
        trust: TrustLevel,
        mut skill: Skill,
    ) -> Result<(), SkillError> {
        skill.source = SkillSource::Plugin { plugin_id, trust };
        self.register(skill)
    }

    pub fn deregister_from_plugin(&self, plugin_id: &PluginId, name: &str) -> Vec<String> {
        let current = self.snapshot();
        let Some(skill) = current.entries.get(name) else {
            return Vec::new();
        };
        if !matches!(&skill.source, SkillSource::Plugin { plugin_id: owner, .. } if owner == plugin_id)
        {
            return Vec::new();
        }
        let handler_ids = skill
            .frontmatter
            .hooks
            .iter()
            .map(|hook| format!("skill:{}:{}", skill.name, hook.id))
            .collect::<Vec<_>>();
        let mut next = (*current).clone();
        next.entries.remove(name);
        rebuild_indexes(&mut next);
        next.generation = current.generation.saturating_add(1);
        *self.snapshot.write() = Arc::new(next);
        handler_ids
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.snapshot.read().entries.is_empty()
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<Skill>> {
        self.snapshot.read().entries.get(name).cloned()
    }

    #[must_use]
    pub fn snapshot(&self) -> Arc<SkillRegistrySnapshot> {
        Arc::clone(&self.snapshot.read())
    }

    #[must_use]
    pub fn list_available_for_agent(&self, agent: &AgentId) -> Vec<Arc<Skill>> {
        self.list_available_for_agent_in_snapshot(agent, &self.snapshot())
    }

    #[must_use]
    pub fn list_available_for_agent_in_snapshot(
        &self,
        agent: &AgentId,
        snapshot: &SkillRegistrySnapshot,
    ) -> Vec<Arc<Skill>> {
        snapshot
            .entries
            .values()
            .filter(|skill| visible_to_agent(skill, agent))
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn list_summaries_for_agent(
        &self,
        agent: &AgentId,
        filter: SkillFilter,
    ) -> Vec<SkillSummary> {
        self.list_summaries_for_agent_in_snapshot(agent, filter, &self.snapshot())
    }

    #[must_use]
    pub fn list_summaries_for_agent_in_snapshot(
        &self,
        agent: &AgentId,
        filter: SkillFilter,
        snapshot: &SkillRegistrySnapshot,
    ) -> Vec<SkillSummary> {
        snapshot
            .entries
            .values()
            .filter(|skill| visible_to_agent(skill, agent))
            .filter_map(|skill| summary_for(skill, &snapshot.status, &filter))
            .collect()
    }

    #[must_use]
    pub fn view(&self, agent: &AgentId, name: &str, full: bool) -> Option<SkillView> {
        self.view_in_snapshot(agent, name, full, &self.snapshot())
    }

    #[must_use]
    pub fn view_in_snapshot(
        &self,
        agent: &AgentId,
        name: &str,
        full: bool,
        snapshot: &SkillRegistrySnapshot,
    ) -> Option<SkillView> {
        let skill = snapshot.entries.get(name)?;
        if !visible_to_agent(skill, agent) {
            return None;
        }
        let status = snapshot
            .status
            .get(&skill.id)
            .cloned()
            .unwrap_or(SkillStatus::Ready);
        Some(SkillView {
            summary: SkillSummary {
                name: skill.name.clone(),
                description: skill.description.clone(),
                tags: skill.frontmatter.tags.clone(),
                category: skill.frontmatter.category.clone(),
                source: skill.source.to_kind(),
                status,
            },
            parameters: skill
                .frontmatter
                .parameters
                .iter()
                .map(|parameter| SkillParameterInfo {
                    name: parameter.name.clone(),
                    param_type: param_type_name(parameter.param_type).to_owned(),
                    required: parameter.required,
                    default: parameter.default.clone(),
                    description: parameter.description.clone(),
                })
                .collect(),
            config_keys: skill
                .frontmatter
                .config
                .iter()
                .map(|config| config.key.clone())
                .collect(),
            body_preview: preview_chars(&skill.body, 1024),
            body_full: full.then(|| skill.body.clone()),
        })
    }

    #[must_use]
    pub fn hook_bindings(&self) -> Vec<SkillHookBinding> {
        self.hook_bindings_in_snapshot(&self.snapshot())
    }

    #[must_use]
    pub fn hook_bindings_in_snapshot(
        &self,
        snapshot: &SkillRegistrySnapshot,
    ) -> Vec<SkillHookBinding> {
        snapshot
            .entries
            .values()
            .flat_map(|skill| {
                skill.frontmatter.hooks.iter().map(|hook| SkillHookBinding {
                    handler_id: format!("skill:{}:{}", skill.name, hook.id),
                    skill_id: skill.id.clone(),
                    skill_name: skill.name.clone(),
                    source: skill.source.clone(),
                    hook_id: hook.id.clone(),
                    events: hook.events.clone(),
                    transport: hook.transport.clone(),
                })
            })
            .collect()
    }
}

impl SkillRegistryBuilder {
    #[must_use]
    pub fn with_skill(mut self, skill: Skill) -> Self {
        self.skills.push(skill);
        self
    }

    #[must_use]
    pub fn with_skills(mut self, skills: Vec<Skill>) -> Self {
        self.skills.extend(skills);
        self
    }

    #[must_use]
    pub fn build(self) -> SkillRegistry {
        let registry = SkillRegistry::default();
        for skill in self.skills {
            let _ = registry.register(skill);
        }
        registry
    }
}

fn insert_skill(snapshot: &mut SkillRegistrySnapshot, skill: Skill) -> Result<(), SkillError> {
    insert_skill_with_policy(snapshot, skill, SameSourcePolicy::IdempotentOnly)
}

fn insert_skill_for_reload(
    snapshot: &mut SkillRegistrySnapshot,
    skill: Skill,
) -> Result<(), SkillError> {
    insert_skill_with_policy(snapshot, skill, SameSourcePolicy::Replace)
}

#[derive(Debug, Clone, Copy)]
enum SameSourcePolicy {
    IdempotentOnly,
    Replace,
}

fn insert_skill_with_policy(
    snapshot: &mut SkillRegistrySnapshot,
    skill: Skill,
    same_source_policy: SameSourcePolicy,
) -> Result<(), SkillError> {
    if let Some(existing) = snapshot.entries.get(&skill.name) {
        if existing.source == skill.source {
            if existing.as_ref() == &skill {
                return Ok(());
            }
            if matches!(same_source_policy, SameSourcePolicy::Replace) {
                snapshot.entries.insert(skill.name.clone(), Arc::new(skill));
                rebuild_indexes(snapshot);
                return Ok(());
            }
            return Err(SkillError::Duplicate(skill.name));
        }
        if source_rank(&existing.source) > source_rank(&skill.source) {
            return Ok(());
        }
    }
    snapshot.entries.insert(skill.name.clone(), Arc::new(skill));
    rebuild_indexes(snapshot);
    Ok(())
}

fn insert_registration(
    snapshot: &mut SkillRegistrySnapshot,
    registration: &SkillRegistration,
) -> Result<(), SkillError> {
    let mut skill = registration.skill.clone();
    if let Some(allowlist) = &registration.force_allowlist {
        skill.frontmatter.allowlist_agents =
            Some(allowlist.iter().map(ToString::to_string).collect());
    }
    insert_skill_for_reload(snapshot, skill)
}

fn rebuild_indexes(snapshot: &mut SkillRegistrySnapshot) {
    snapshot.by_source.clear();
    snapshot.status.clear();
    for skill in snapshot.entries.values() {
        snapshot
            .by_source
            .entry(skill.source.clone())
            .or_default()
            .push(skill.id.clone());
        snapshot.status.insert(skill.id.clone(), status_for(skill));
    }
}

fn visible_to_agent(skill: &Skill, agent: &AgentId) -> bool {
    skill
        .frontmatter
        .allowlist_agents
        .as_ref()
        .map(|list| list.iter().any(|candidate| candidate == &agent.to_string()))
        .unwrap_or(true)
}

fn summary_for(
    skill: &Skill,
    status: &BTreeMap<SkillId, SkillStatus>,
    filter: &SkillFilter,
) -> Option<SkillSummary> {
    let status = status.get(&skill.id).cloned().unwrap_or(SkillStatus::Ready);
    if !filter.include_prerequisite_missing
        && matches!(status, SkillStatus::PrerequisiteMissing { .. })
    {
        return None;
    }
    if let Some(tag) = &filter.tag {
        if !skill
            .frontmatter
            .tags
            .iter()
            .any(|candidate| candidate == tag)
        {
            return None;
        }
    }
    if let Some(category) = &filter.category {
        if skill.frontmatter.category.as_ref() != Some(category) {
            return None;
        }
    }
    Some(SkillSummary {
        name: skill.name.clone(),
        description: skill.description.clone(),
        tags: skill.frontmatter.tags.clone(),
        category: skill.frontmatter.category.clone(),
        source: skill.source.to_kind(),
        status,
    })
}

fn status_for(skill: &Skill) -> SkillStatus {
    let missing = skill
        .frontmatter
        .prerequisites
        .env_vars
        .iter()
        .filter(|name| std::env::var_os(name).is_none())
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        SkillStatus::Ready
    } else {
        SkillStatus::PrerequisiteMissing { env_vars: missing }
    }
}

fn source_rank(source: &SkillSource) -> u8 {
    match source {
        SkillSource::Bundled => 0,
        SkillSource::Plugin { .. } => 1,
        SkillSource::Mcp(_) => 2,
        SkillSource::User(_) => 3,
        SkillSource::Workspace(_) => 4,
    }
}

fn param_type_name(param_type: SkillParamType) -> &'static str {
    match param_type {
        SkillParamType::String => "string",
        SkillParamType::Number => "number",
        SkillParamType::Boolean => "boolean",
        SkillParamType::Path => "path",
        SkillParamType::Url => "url",
    }
}

fn preview_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}
