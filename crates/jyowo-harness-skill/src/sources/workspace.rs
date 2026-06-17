use std::path::PathBuf;

use crate::{
    DirectorySourceKind, LoadReport, SkillError, SkillLoader, SkillPlatform, SkillSourceConfig,
};

#[derive(Debug, Clone)]
pub struct WorkspaceSource {
    path: PathBuf,
}

impl WorkspaceSource {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub async fn load(&self, runtime_platform: SkillPlatform) -> Result<LoadReport, SkillError> {
        SkillLoader::default()
            .with_source(SkillSourceConfig::Directory {
                path: self.path.clone(),
                source_kind: DirectorySourceKind::Workspace,
            })
            .with_runtime_platform(runtime_platform)
            .load_all()
            .await
    }
}
