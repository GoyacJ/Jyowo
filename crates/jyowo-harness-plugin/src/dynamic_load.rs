use std::sync::Arc;

use async_trait::async_trait;

use crate::{ManifestOrigin, Plugin, PluginManifest, PluginRuntimeLoader, RuntimeLoaderError};

#[derive(Debug, Default, Clone)]
pub struct DylibRuntimeLoader;

#[async_trait]
impl PluginRuntimeLoader for DylibRuntimeLoader {
    fn can_load(&self, _manifest: &PluginManifest, origin: &ManifestOrigin) -> bool {
        matches!(origin, ManifestOrigin::File { path } if is_dynamic_library(path))
    }

    async fn load(
        &self,
        _manifest: &PluginManifest,
        _origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        Err(RuntimeLoaderError::LoadFailed(
            "dynamic-load is unsupported: real dlopen requires a separate unsafe governance decision"
                .to_owned(),
        ))
    }
}

fn is_dynamic_library(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(std::ffi::OsStr::to_str),
        Some("dylib" | "so" | "dll")
    )
}
