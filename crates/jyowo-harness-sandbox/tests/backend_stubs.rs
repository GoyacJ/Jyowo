#![cfg(any(feature = "docker", feature = "ssh"))]

use std::sync::Arc;

use harness_contracts::{Event, NetworkAccess, SandboxError, SandboxPolicy};
use harness_sandbox::{EventSink, ExecContext, ExecSpec, SandboxBackend};

#[derive(Default)]
struct RecordingSink;

impl EventSink for RecordingSink {
    fn emit(&self, _event: Event) -> Result<(), SandboxError> {
        Ok(())
    }
}

async fn assert_feature_backend_detects_unavailable_host(
    backend: Arc<dyn SandboxBackend>,
    backend_id: &str,
) {
    assert_eq!(backend.backend_id(), backend_id);
    assert!(backend.capabilities().supports_streaming);
    let spec = ExecSpec {
        policy: SandboxPolicy {
            network: NetworkAccess::Unrestricted,
            ..ExecSpec::default().policy
        },
        ..ExecSpec::default()
    };
    let error = match backend
        .execute(spec, ExecContext::for_test(Arc::new(RecordingSink)))
        .await
    {
        Ok(_) => panic!("{backend_id} execute should reject"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::Unavailable { ref backend, .. } if backend == backend_id
    ));
    backend.shutdown().await.expect("shutdown should succeed");
}

#[cfg(feature = "docker")]
#[tokio::test]
async fn docker_sandbox_feature_backend_is_object_safe() {
    assert_feature_backend_detects_unavailable_host(
        Arc::new(
            harness_sandbox::DockerSandbox::builder()
                .docker_binary("/definitely/missing/docker")
                .build()
                .unwrap(),
        ),
        "docker",
    )
    .await;
}

#[cfg(feature = "ssh")]
#[tokio::test]
async fn ssh_sandbox_feature_backend_is_object_safe() {
    assert_feature_backend_detects_unavailable_host(
        Arc::new(
            harness_sandbox::SshSandbox::builder()
                .ssh_binary("/definitely/missing/ssh")
                .build()
                .unwrap(),
        ),
        "ssh",
    )
    .await;
}
