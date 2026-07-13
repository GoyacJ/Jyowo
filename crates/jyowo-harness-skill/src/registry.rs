use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
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
    pub candidates: BTreeMap<String, Vec<Arc<Skill>>>,
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
    pub logical_id: String,
    pub handler_id: String,
    pub skill_id: SkillId,
    pub skill_name: String,
    pub source: SkillSource,
    pub hook_id: String,
    pub events: Vec<HookEventKind>,
    pub transport: SkillHookTransport,
}

#[derive(Debug)]
pub enum SkillRegistryUpdateError<E> {
    Registry(SkillError),
    Reconcile(E),
}

impl SkillRegistry {
    #[must_use]
    pub fn builder() -> SkillRegistryBuilder {
        SkillRegistryBuilder::default()
    }

    pub fn register(&self, skill: Skill) -> Result<(), SkillError> {
        let mut guard = self.snapshot.write();
        let current = Arc::clone(&guard);
        let mut next = current.as_ref().clone();
        insert_skill(&mut next, skill)?;
        publish_if_changed(&mut guard, &current, &mut next);
        Ok(())
    }

    pub fn register_batch(&self, skills: Vec<Skill>) -> Result<SkillRegistrySnapshot, SkillError> {
        let mut guard = self.snapshot.write();
        let current = Arc::clone(&guard);
        let mut next = current.as_ref().clone();
        for skill in skills {
            insert_skill(&mut next, skill)?;
        }
        publish_if_changed(&mut guard, &current, &mut next);
        Ok(next)
    }

    pub fn replace_registrations(
        &self,
        registrations: &[SkillRegistration],
    ) -> Result<SkillRegistrySnapshot, SkillError> {
        match self.try_replace_registrations(registrations, |_, _| Ok::<_, Infallible>(())) {
            Ok(snapshot) => Ok(snapshot),
            Err(SkillRegistryUpdateError::Registry(error)) => Err(error),
            Err(SkillRegistryUpdateError::Reconcile(never)) => match never {},
        }
    }

    pub fn try_replace_registrations<E, F>(
        &self,
        registrations: &[SkillRegistration],
        reconcile: F,
    ) -> Result<SkillRegistrySnapshot, SkillRegistryUpdateError<E>>
    where
        F: FnOnce(&SkillRegistrySnapshot, &SkillRegistrySnapshot) -> Result<(), E>,
    {
        self.try_update(
            |next| {
                for registration in registrations {
                    insert_registration(next, registration)?;
                }
                Ok(())
            },
            reconcile,
        )
    }

    pub fn replace_source(
        &self,
        source: SkillSource,
        skills: Vec<Skill>,
    ) -> Result<SkillRegistrySnapshot, SkillError> {
        match self.try_replace_source(source, skills, |_, _| Ok::<_, Infallible>(())) {
            Ok(snapshot) => Ok(snapshot),
            Err(SkillRegistryUpdateError::Registry(error)) => Err(error),
            Err(SkillRegistryUpdateError::Reconcile(never)) => match never {},
        }
    }

    pub fn try_replace_source<E, F>(
        &self,
        source: SkillSource,
        mut skills: Vec<Skill>,
        reconcile: F,
    ) -> Result<SkillRegistrySnapshot, SkillRegistryUpdateError<E>>
    where
        F: FnOnce(&SkillRegistrySnapshot, &SkillRegistrySnapshot) -> Result<(), E>,
    {
        self.try_update(
            move |next| {
                for candidates in next.candidates.values_mut() {
                    candidates.retain(|skill| skill.source != source);
                }
                next.candidates
                    .retain(|_, candidates| !candidates.is_empty());
                for mut skill in skills.drain(..) {
                    skill.source = source.clone();
                    insert_skill_for_reload(next, skill)?;
                }
                rebuild_indexes(next);
                Ok(())
            },
            reconcile,
        )
    }

    fn try_update<E, M, F>(
        &self,
        mutate: M,
        reconcile: F,
    ) -> Result<SkillRegistrySnapshot, SkillRegistryUpdateError<E>>
    where
        M: FnOnce(&mut SkillRegistrySnapshot) -> Result<(), SkillError>,
        F: FnOnce(&SkillRegistrySnapshot, &SkillRegistrySnapshot) -> Result<(), E>,
    {
        let mut guard = self.snapshot.write();
        let current = Arc::clone(&guard);
        let mut next = current.as_ref().clone();
        mutate(&mut next).map_err(SkillRegistryUpdateError::Registry)?;
        set_next_generation(&current, &mut next);
        reconcile(&current, &next).map_err(SkillRegistryUpdateError::Reconcile)?;
        if next.candidates != current.candidates {
            *guard = Arc::new(next.clone());
        }
        Ok(next)
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
        let mut guard = self.snapshot.write();
        let current = Arc::clone(&guard);
        let mut next = current.as_ref().clone();
        let Some(candidates) = next.candidates.get_mut(name) else {
            return Vec::new();
        };
        let removed = candidates
            .iter()
            .filter(|skill| matches!(&skill.source, SkillSource::Plugin { plugin_id: owner, .. } if owner == plugin_id))
            .cloned()
            .collect::<Vec<_>>();
        if removed.is_empty() {
            return Vec::new();
        }
        candidates.retain(|skill| {
            !matches!(&skill.source, SkillSource::Plugin { plugin_id: owner, .. } if owner == plugin_id)
        });
        if candidates.is_empty() {
            next.candidates.remove(name);
        }
        rebuild_indexes(&mut next);
        set_next_generation(&current, &mut next);
        *guard = Arc::new(next);
        let handler_ids = removed
            .iter()
            .flat_map(|skill| hook_bindings_for_skill(skill))
            .map(|binding| binding.handler_id)
            .collect();
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
            .flat_map(|skill| hook_bindings_for_skill(skill))
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
    let candidates = snapshot.candidates.entry(skill.name.clone()).or_default();
    if let Some(existing_index) = candidates
        .iter()
        .position(|existing| existing.source == skill.source)
    {
        let existing = &candidates[existing_index];
        if existing.as_ref() == &skill {
            return Ok(());
        }
        if matches!(same_source_policy, SameSourcePolicy::Replace) {
            candidates[existing_index] = Arc::new(skill);
            rebuild_indexes(snapshot);
            return Ok(());
        }
        return Err(SkillError::Duplicate(skill.name));
    }
    candidates.push(Arc::new(skill));
    rebuild_indexes(snapshot);
    Ok(())
}

fn publish_if_changed(
    guard: &mut Arc<SkillRegistrySnapshot>,
    current: &SkillRegistrySnapshot,
    next: &mut SkillRegistrySnapshot,
) {
    set_next_generation(current, next);
    if next.candidates != current.candidates {
        *guard = Arc::new(next.clone());
    }
}

fn set_next_generation(current: &SkillRegistrySnapshot, next: &mut SkillRegistrySnapshot) {
    next.generation = if next.candidates == current.candidates {
        current.generation
    } else {
        current.generation.saturating_add(1)
    };
}

fn hook_bindings_for_skill(skill: &Skill) -> Vec<SkillHookBinding> {
    skill
        .frontmatter
        .hooks
        .iter()
        .map(|hook| {
            let logical_id = format!("skill:{}:{}", skill.name, hook.id);
            let declaration = format!("{}|{hook:?}", skill.source.fingerprint_identity());
            let fingerprint = blake3::hash(declaration.as_bytes()).to_hex();
            SkillHookBinding {
                handler_id: format!("{logical_id}:{}", &fingerprint[..16]),
                logical_id,
                skill_id: skill.id.clone(),
                skill_name: skill.name.clone(),
                source: skill.source.clone(),
                hook_id: hook.id.clone(),
                events: hook.events.clone(),
                transport: hook.transport.clone(),
            }
        })
        .collect()
}

// Candidate order is insertion order. The newest candidate wins equal-rank ties.
fn active_candidate(candidates: &[Arc<Skill>]) -> Option<Arc<Skill>> {
    candidates
        .iter()
        .enumerate()
        .max_by_key(|(index, skill)| (source_rank(&skill.source), *index))
        .map(|(_, skill)| Arc::clone(skill))
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
    snapshot.entries.clear();
    snapshot.by_source.clear();
    snapshot.status.clear();
    for (name, candidates) in &snapshot.candidates {
        if let Some(active) = active_candidate(candidates) {
            snapshot.entries.insert(name.clone(), active);
        }
        for skill in candidates {
            snapshot
                .by_source
                .entry(skill.source.clone())
                .or_default()
                .push(skill.id.clone());
            snapshot.status.insert(skill.id.clone(), status_for(skill));
        }
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
