import assert from 'node:assert/strict'
import test from 'node:test'

import { countRustTestFunctionsFromContent } from './audit-tests.mjs'
import { isTrackedTestFile } from './check-test-architecture.mjs'

test('audit inventory counts tokio tests with attributes', () => {
  const count = countRustTestFunctionsFromContent(`
#[tokio::test(flavor = "current_thread")]
async fn async_policy_holds() {}

#[tokio::test]
async fn async_default_holds() {}

#[test]
fn sync_policy_holds() {}
`)

  assert.equal(count, 3)
})

test('architecture gate tracks nested Rust test support files', () => {
  assert.equal(
    isTrackedTestFile('crates/jyowo-harness-sdk/tests/runtime_assembly_support/mod.rs'),
    true,
  )
  assert.equal(
    isTrackedTestFile('crates/jyowo-harness-sdk/tests/runtime_assembly_support/observability.rs'),
    true,
  )
})
