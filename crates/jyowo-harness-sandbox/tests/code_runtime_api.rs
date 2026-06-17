#![cfg(feature = "code-runtime")]

use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::SandboxError;
use harness_sandbox::{
    CodeSandbox, CodeSandboxCapabilities, CodeSandboxResult, CodeSandboxRunContext, CompiledScript,
};

struct TestCodeSandbox;

#[async_trait]
impl CodeSandbox for TestCodeSandbox {
    fn capabilities(&self) -> CodeSandboxCapabilities {
        CodeSandboxCapabilities::default()
    }

    async fn run(
        &self,
        _script: &CompiledScript,
        _ctx: CodeSandboxRunContext,
    ) -> Result<CodeSandboxResult, SandboxError> {
        Ok(CodeSandboxResult::default())
    }
}

#[test]
fn code_sandbox_is_object_safe() {
    let sandbox: Arc<dyn CodeSandbox> = Arc::new(TestCodeSandbox);
    assert_eq!(sandbox.capabilities().max_call_depth, 32);
}
