use std::sync::Arc;

use futures::executor::block_on;
use jyowo_harness_sdk::builtin::*;
use jyowo_harness_sdk::prelude::*;
use jyowo_harness_sdk::testing::{InMemoryEventStore, MockProvider, NoopRedactor};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    block_on(async {
        let workspace =
            std::env::temp_dir().join(format!("jyowo-harness-quickstart-{}", SessionId::new()));
        std::fs::create_dir_all(&workspace)?;

        let harness = Harness::builder()
            .with_model(MockProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await?;

        let _session = harness
            .create_session(SessionOptions::new(&workspace))
            .await?;

        Ok(())
    })
}
