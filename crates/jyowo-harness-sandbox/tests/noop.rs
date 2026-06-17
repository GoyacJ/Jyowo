#![cfg(feature = "noop")]

use std::sync::Arc;

use harness_contracts::{Event, SandboxError};
use harness_sandbox::{EventSink, ExecContext, ExecSpec, NoopSandbox, SandboxBackend};

#[derive(Default)]
struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: Event) -> Result<(), SandboxError> {
        Ok(())
    }
}

#[tokio::test]
async fn noop_sandbox_rejects_exec_and_records_spec() {
    let sandbox = Arc::new(NoopSandbox::new());
    let backend: Arc<dyn SandboxBackend> = sandbox.clone();
    let spec = ExecSpec {
        command: "echo".to_owned(),
        args: vec!["hello".to_owned()],
        ..ExecSpec::default()
    };

    let error = match backend
        .execute(spec.clone(), ExecContext::for_test(Arc::new(NullSink)))
        .await
    {
        Ok(_) => panic!("noop execute should reject"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        SandboxError::CapabilityMismatch {
            ref capability,
            ..
        } if capability == "execute"
    ));
    assert_eq!(sandbox.recorded_execs(), vec![spec]);
    backend.shutdown().await.expect("shutdown should succeed");
}
