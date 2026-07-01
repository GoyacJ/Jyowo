import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'

import {
  AGENT_CONTEXT_PATTERNS,
  hasAgentContextNearby,
  scanAgentOrchestrationNoFakes,
} from './check-agent-orchestration-no-fakes.mjs'

function writeFixture({ relativePath, content }) {
  const root = join(tmpdir(), `jyowo-agent-no-fakes-${randomUUID()}`)
  const absolutePath = join(root, relativePath)
  mkdirSync(join(absolutePath, '..'), { recursive: true })
  writeFileSync(absolutePath, content, 'utf8')
  return { root, relativePath }
}

test('hasAgentContextNearby detects subagent context within radius', () => {
  const lines = [
    '// unrelated',
    'export function renderSubagentPanel() {',
    '  return null',
    '}',
    '  label: "Coming soon"',
  ]

  assert.equal(hasAgentContextNearby(lines, 4), true)
})

test('fails agent-related experimental placeholder in scoped production file', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'apps/desktop/src/features/conversation/Composer.tsx',
    content: `
export function Composer() {
  return <p>Subagent support is experimental and not ready.</p>
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, false)
  assert.match(result.violations[0].rule, /experimental-label/)
})

test('fails fake background provider description in agent runtime crate', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'crates/jyowo-harness-agent-runtime/src/lib.rs',
    content: `
//! Fake background provider for agent orchestration demos.
pub fn bootstrap() {}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, false)
  assert.ok(
    result.violations.some((violation) => violation.rule === 'fake-background-provider'),
  )
})

test('fails fake agent runtime filename in scoped production file', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'apps/desktop/src/features/conversation/FakeSubagentRunner.ts',
    content: `
export function runSubagent() {
  return 'delegated'
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'fake-filename'))
})

test('fails noop agent tauri command returning fixed success', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'apps/desktop/src-tauri/src/commands/agents.rs',
    content: `
#[tauri::command]
pub async fn list_agent_profiles() -> Result<Vec<String>, String> {
  Ok(vec![])
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, false)
  assert.equal(result.violations[0].rule, 'noop-agent-command')
})

test('passes agent command that delegates to runtime', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'apps/desktop/src-tauri/src/commands/agents.rs',
    content: `
#[tauri::command]
pub async fn list_agent_profiles(state: State<'_, RuntimeState>) -> Result<Vec<String>, String> {
  state.harness.list_agent_profiles().await.map_err(|err| err.to_string())
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, true)
})

test('does not fail unrelated placeholder outside scoped production surfaces', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'apps/desktop/src/features/settings/AboutSettings.tsx',
    content: `
export function AboutSettings() {
  return <p>Coming soon: theme customization placeholder.</p>
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, true)
})

test('does not fail generic mock fixtures in test files', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'apps/desktop/src/features/conversation/Composer.test.tsx',
    content: `
test('mock agent runtime fixture', () => {
  const fakeSubagentRunner = { spawn: () => {} }
  assert.ok(fakeSubagentRunner)
})
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, true)
})

test('agent context patterns cover orchestration keywords', () => {
  const sample = 'background agent runtime orchestration'
  assert.ok(AGENT_CONTEXT_PATTERNS.some((pattern) => pattern.test(sample)))
})

test('fails hardcoded subagent availability assignment outside resolver', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'apps/desktop/src-tauri/src/commands/providers.rs',
    content: `
pub fn broken() {
  let subagents_available = false;
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, false)
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'hardcoded-subagents-unavailable',
    ),
  )
})

test('fails hardcoded background availability assignment inside policy resolver', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'crates/jyowo-harness-agent-runtime/src/policy.rs',
    content: `
fn broken() {
  let background_agents_available = false;
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, false)
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'hardcoded-background-unavailable',
    ),
  )
})

test('fails naked background availability assignment outside policy resolver', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'apps/desktop/src-tauri/src/commands/providers.rs',
    content: `
pub fn broken() {
  let background_agents_available = false;
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, false)
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'hardcoded-background-unavailable',
    ),
  )
})

test('scans final agent orchestration production surfaces including json and package metadata', () => {
  const { root } = writeFixture({
    relativePath: 'apps/desktop/src-tauri/tauri.conf.json',
    content: `
{
  "bundle": {
    "externalBin": ["binaries/jyowo-agent-supervisor"]
  },
  "notes": "background agent coming soon"
}
`,
  })
  writeFixtureAtRoot({
    root,
    relativePath: 'package.json',
    content: `
{
  "scripts": {
    "check": "echo agent orchestration placeholder"
  }
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.file.endsWith('tauri.conf.json')))
  assert.ok(result.violations.some((violation) => violation.file === 'package.json'))
})

test('fails struct-field hardcoded agent availability false values', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'crates/jyowo-harness-agent-runtime/src/policy.rs',
    content: `
fn broken() -> ResolvedAgentCapabilityPolicy {
  ResolvedAgentCapabilityPolicy {
    subagents_available: false,
    agent_teams_available: false,
    background_agents_available: false,
    unavailable_reasons: Vec::new(),
  }
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, false)
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'hardcoded-subagents-unavailable',
    ),
  )
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'hardcoded-agent-teams-unavailable',
    ),
  )
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'hardcoded-background-unavailable',
    ),
  )
})

test('fails temporary scanner allowlist for agent capability availability fields', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'scripts/check-agent-orchestration-no-fakes.mjs',
    content: `
const temporaryAllowlistForHardcodedAgentCapabilityAvailability = [
  'subagents_available = false',
  'agent_teams_available = false',
]
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, false)
  assert.ok(result.violations.some((violation) => violation.rule === 'temporary-availability-allowlist'))
})

test('fails agent runtime command returning fixed success payload without SDK runtime delegation', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'apps/desktop/src-tauri/src/commands/background_agents.rs',
    content: `
#[tauri::command]
pub async fn resume_background_agent() -> Result<BackgroundAgentActionResponse, CommandErrorPayload> {
  Ok(BackgroundAgentActionResponse {
    status: "resumed".to_owned(),
  })
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, false)
  assert.equal(result.violations[0].rule, 'noop-agent-command')
})

test('fails frontend-only hardcoded agent availability state', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'apps/desktop/src/features/conversation/Composer.tsx',
    content: `
export function Composer() {
  const localAgentCapabilities = {
    subagentsAvailable: true,
    agentTeamsAvailable: true,
    backgroundAgentsAvailable: true,
  }
  return localAgentCapabilities.subagentsAvailable ? null : null
}
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, false)
  assert.ok(
    result.violations.some(
      (violation) => violation.rule === 'frontend-only-agent-capability-state',
    ),
  )
})

test('requires agent context near generic fake mock noop todo markers', () => {
  const { root, relativePath } = writeFixture({
    relativePath: 'apps/desktop/src/features/conversation/ConversationWorkspace.tsx',
    content: `
export const copy = [
  'Mock layout copy for a generic non-agent panel',
  'TODO: replace unrelated style token',
  'noop render branch for empty decorative chrome',
]
`,
  })

  const result = scanAgentOrchestrationNoFakes(root, { scopedPaths: [relativePath] })

  assert.equal(result.ok, true)
})

function writeFixtureAtRoot({ root, relativePath, content }) {
  const absolutePath = join(root, relativePath)
  mkdirSync(join(absolutePath, '..'), { recursive: true })
  writeFileSync(absolutePath, content, 'utf8')
  return { root, relativePath }
}
