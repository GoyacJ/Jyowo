use std::sync::Arc;

use harness_contracts::{BlobReaderCapAdapter, BlobStore, CapabilityRegistry, ToolCapability};
use harness_journal::{EventStore, EventStoreOffloadedBlobAuthorizer};

pub(crate) fn assemble_capability_registry(
    base: Option<&Arc<CapabilityRegistry>>,
    event_store: &Arc<dyn EventStore>,
    blob_store: Option<&Arc<dyn BlobStore>>,
    overrides: &CapabilityRegistry,
) -> Arc<CapabilityRegistry> {
    let mut registry = base.map_or_else(CapabilityRegistry::default, |base| base.as_ref().clone());

    if let Some(blob_store) = blob_store {
        registry.install::<dyn harness_contracts::BlobReaderCap>(
            ToolCapability::BlobReader,
            Arc::new(BlobReaderCapAdapter::new(Arc::clone(blob_store))),
        );
        registry.install::<dyn harness_contracts::OffloadedBlobAuthorizerCap>(
            ToolCapability::OffloadedBlobAuthorizer,
            Arc::new(EventStoreOffloadedBlobAuthorizer::new(Arc::clone(
                event_store,
            ))),
        );
    }

    registry.overlay_from(overrides);
    Arc::new(registry)
}
