use std::{path::Path, sync::Arc};

use harness_provider_state::{
    FileProviderContinuationStore, ProviderContinuationStore, ProviderContinuationStoreError,
};
use jyowo_harness_sdk::HarnessBuilder;

pub(crate) fn with_file_provider_continuation_store<M, S, SB>(
    builder: HarnessBuilder<M, S, SB>,
    runtime_root: &Path,
) -> Result<HarnessBuilder<M, S, SB>, ProviderContinuationStoreError> {
    let store: Arc<dyn ProviderContinuationStore> = Arc::new(
        FileProviderContinuationStore::open_runtime_dir(runtime_root)?,
    );
    Ok(builder.with_provider_continuation_store_arc(store))
}
