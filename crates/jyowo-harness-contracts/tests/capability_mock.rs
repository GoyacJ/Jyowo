#![cfg(feature = "testing")]

use std::sync::Arc;

use bytes::Bytes;
use futures::{future::BoxFuture, stream::BoxStream};
use harness_contracts::{
    BlobReaderCap, BlobRef, MockCapabilityRegistry, TenantId, ToolCapability, ToolError,
};

#[test]
fn mock_capability_registry_is_testing_only_and_converts_to_runtime_registry() {
    let blob_reader: Arc<dyn BlobReaderCap> = Arc::new(FakeBlobReader);
    let registry = MockCapabilityRegistry::new()
        .with_capability(ToolCapability::BlobReader, blob_reader.clone())
        .into_registry();

    assert!(registry.contains(&ToolCapability::BlobReader));
    assert!(registry
        .get::<dyn BlobReaderCap>(&ToolCapability::BlobReader)
        .is_some());
}

struct FakeBlobReader;

impl BlobReaderCap for FakeBlobReader {
    fn read_blob<'a>(
        &'a self,
        _tenant_id: TenantId,
        _blob: BlobRef,
    ) -> BoxFuture<'a, Result<BoxStream<'static, Bytes>, ToolError>> {
        Box::pin(async { Ok(Box::pin(futures::stream::empty()) as BoxStream<'static, Bytes>) })
    }
}
