use std::sync::Arc;

use async_trait::async_trait;

use crate::{ManifestOrigin, Plugin, PluginManifest, PluginRuntimeLoader, RuntimeLoaderError};

#[derive(Debug, Default, Clone)]
pub struct WasmRuntimeLoader;

#[async_trait]
impl PluginRuntimeLoader for WasmRuntimeLoader {
    fn can_load(&self, _manifest: &PluginManifest, origin: &ManifestOrigin) -> bool {
        matches!(origin, ManifestOrigin::File { path } if is_wasm_module(path))
    }

    async fn load(
        &self,
        _manifest: &PluginManifest,
        _origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        Err(RuntimeLoaderError::LoadFailed(
            "wasm-runtime is unsupported: no Wasm runtime is linked in M5".to_owned(),
        ))
    }
}

fn is_wasm_module(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(std::ffi::OsStr::to_str),
        Some("wasm")
    )
}
