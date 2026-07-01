import assert from 'node:assert/strict'
import test from 'node:test'

import {
  isScopedProductionPath,
  scanAgentOrchestrationContent,
} from './check-agent-orchestration-no-fakes.mjs'

test('scopes only agent orchestration production paths', () => {
  assert.equal(isScopedProductionPath('crates/jyowo-harness-subagent/src/lib.rs'), true)
  assert.equal(isScopedProductionPath('apps/desktop/src-tauri/src/commands.rs'), true)
  assert.equal(isScopedProductionPath('crates/jyowo-harness-subagent/tests/contract.rs'), false)
  assert.equal(isScopedProductionPath('docs/plans/agent.md'), false)
})

test('rejects agent-context placeholder labels', () => {
  const failures = scanAgentOrchestrationContent(
    `export const label = 'Background agent coming soon'`,
  )

  assert.equal(failures.length, 1)
  assert.match(failures[0].reason, /placeholder/)
})

test('rejects fake agent production surfaces', () => {
  const failures = scanAgentOrchestrationContent('class FakeBackgroundAgentRunner {}')

  assert.equal(failures.length, 1)
  assert.match(failures[0].reason, /fake\/mock/)
})

test('rejects fixed success agent handlers', () => {
  const failures = scanAgentOrchestrationContent(`
    async fn start_background_agent() -> Result<(), Error> {
      Ok(())
    }
  `)

  assert.equal(failures.length, 1)
  assert.match(failures[0].reason, /fixed success/)
})

test('allows unrelated placeholder and ordinary test doubles outside agent context', () => {
  assert.deepEqual(scanAgentOrchestrationContent("const label = 'Coming soon'"), [])
  assert.deepEqual(scanAgentOrchestrationContent('struct FakeClock;'), [])
})
