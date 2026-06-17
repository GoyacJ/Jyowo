use std::path::PathBuf;

use harness_contracts::{PluginId, TrustLevel};

use crate::{
    DirectorySourceKind, LoadReport, SkillError, SkillLoader, SkillPlatform, SkillSourceConfig,
};

#[derive(Debug, Clone)]
pub struct PluginSource {
    plugin_id: PluginId,
    plugin_root: PathBuf,
    trust: TrustLevel,
}

impl PluginSource {
    #[must_use]
    pub fn new(plugin_id: PluginId, plugin_root: PathBuf) -> Self {
        Self {
            plugin_id,
            plugin_root,
            trust: TrustLevel::UserControlled,
        }
    }

    #[must_use]
    pub fn new_with_trust(plugin_id: PluginId, plugin_root: PathBuf, trust: TrustLevel) -> Self {
        Self {
            plugin_id,
            plugin_root,
            trust,
        }
    }

    pub async fn load(&self, runtime_platform: SkillPlatform) -> Result<LoadReport, SkillError> {
        SkillLoader::default()
            .with_source(SkillSourceConfig::Directory {
                path: self.plugin_root.join("skills"),
                source_kind: DirectorySourceKind::Plugin {
                    plugin_id: self.plugin_id.clone(),
                    trust: self.trust,
                },
            })
            .with_runtime_platform(runtime_platform)
            .load_all()
            .await
    }
}
