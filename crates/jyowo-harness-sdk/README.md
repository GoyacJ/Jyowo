# jyowo-harness-sdk

L4 facade crate for Jyowo Agent Harness SDK.

SPEC: `docs/architecture/harness/crates/harness-sdk.md`

## Quickstart

```rust
use std::sync::Arc;

use jyowo_harness_sdk::builtin::*;
use jyowo_harness_sdk::prelude::*;
use jyowo_harness_sdk::testing::{InMemoryEventStore, MockProvider, NoopRedactor};

# futures::executor::block_on(async {
let harness = Harness::builder()
    .with_model(MockProvider::default())
    .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
    .with_sandbox(NoopSandbox::new())
    .build()
    .await?;

let session = harness
    .create_session(SessionOptions::new(std::env::current_dir()?))
    .await?;
# Ok::<(), Box<dyn std::error::Error>>(())
# });
```

`HarnessBuilder` uses type-state for required dependencies. Calling `build()` before
`with_model`, `with_store`, and `with_sandbox` is a compile-time error.

Import surfaces:

- `prelude::*`: business-facing default API.
- `ext::*`: traits for custom providers, stores, sandboxes, tools, hooks, and plugins.
- `builtin::*`: feature-gated built-in implementations.
- `testing::*`: mock and noop implementations behind the `testing` feature.
