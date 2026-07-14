use std::path::Path;

use harness_contracts::{
    AgentId, ListSkillReferenceCandidatesResponse, SkillReferenceCandidate, SkillStatus,
};
use harness_plugin::PluginCapabilityRegistries;
use jyowo_harness_sdk::apply_skill_config_statuses;
use jyowo_harness_sdk::ext::SkillRegistry;
use thiserror::Error;

use crate::{RuntimeConfigError, RuntimeConfigResolver};

/// Resolves Composer skill references from the same effective workspace runtime
/// configuration used to construct task sessions.
#[derive(Clone, Debug)]
pub struct SkillReferenceCandidateService {
    runtime_config: RuntimeConfigResolver,
}

impl SkillReferenceCandidateService {
    #[must_use]
    pub fn new(runtime_config: RuntimeConfigResolver) -> Self {
        Self { runtime_config }
    }

    pub async fn list(
        &self,
        workspace_root: &Path,
    ) -> Result<ListSkillReferenceCandidatesResponse, SkillReferenceCandidateError> {
        let runtime = self.runtime_config.resolve(workspace_root, None)?;
        let report = runtime
            .skill_loader
            .load_all()
            .await
            .map_err(|error| SkillReferenceCandidateError::Skill(error.to_string()))?;
        let registry = SkillRegistry::default();
        registry
            .register_batch(report.loaded)
            .map_err(|error| SkillReferenceCandidateError::Skill(error.to_string()))?;
        let plugins = runtime.materialize_plugin_registry()?;
        plugins.set_capability_registries(
            PluginCapabilityRegistries::default().with_skill_registry(registry.clone()),
        );
        for plugin in plugins.discover().await? {
            let plugin_id = plugin.record.manifest.plugin_id();
            if plugins.is_plugin_enabled(&plugin_id) == Some(false) {
                continue;
            }
            plugins.activate(&plugin_id).await?;
        }
        let mut snapshot = (*registry.snapshot()).clone();
        apply_skill_config_statuses(&mut snapshot, &runtime.skill_config)?;

        // Selected skill resolution currently uses this stable root-agent identity.
        // Candidate visibility must use the same identity or Composer can advertise
        // a skill that the turn assembler will reject.
        let agent = AgentId::from_u128(1);
        let skills = registry
            .list_available_for_agent_in_snapshot(&agent, &snapshot)
            .into_iter()
            .filter(|skill| {
                matches!(
                    snapshot.status.get(&skill.id),
                    None | Some(SkillStatus::Ready)
                )
            })
            .map(|skill| SkillReferenceCandidate {
                skill_id: skill.id.clone(),
                label: skill.name.clone(),
                source: skill.source.to_kind(),
            })
            .collect();

        Ok(ListSkillReferenceCandidatesResponse { skills })
    }
}

#[derive(Debug, Error)]
pub enum SkillReferenceCandidateError {
    #[error("runtime configuration failed: {0}")]
    RuntimeConfig(#[from] RuntimeConfigError),
    #[error("skill resolution failed: {0}")]
    Skill(String),
    #[error("plugin resolution failed: {0}")]
    Plugin(#[from] harness_plugin::PluginError),
    #[error("skill configuration status failed: {0}")]
    SkillConfig(#[from] jyowo_harness_sdk::SkillConfigStoreError),
}
