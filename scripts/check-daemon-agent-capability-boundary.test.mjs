import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'

import { scanDaemonAgentCapabilityBoundary } from './check-daemon-agent-capability-boundary.mjs'

function fixture(files) {
  const root = join(tmpdir(), `jyowo-daemon-boundary-${randomUUID()}`)
  for (const [relativePath, content] of Object.entries(files)) {
    const absolutePath = join(root, relativePath)
    mkdirSync(join(absolutePath, '..'), { recursive: true })
    writeFileSync(absolutePath, content, 'utf8')
  }
  return root
}

const daemonAssembly = `
pub fn build() {
  Harness::builder()
    .with_mcp_config(mcp_config)
    .with_plugin_registry(plugin_registry)
    .with_skill_loader(skill_loader)
    .with_skill_config_snapshot(skill_config)
    .with_provider_capability_routes(provider_routes);
}
`

test('rejects task runtime assembly in Tauri', () => {
  const root = fixture({
    'apps/desktop/src-tauri/src/runtime.rs': 'let builder = Harness::builder();',
    'crates/jyowo-harness-daemon/src/sdk_run_factory.rs': daemonAssembly,
  })

  const result = scanDaemonAgentCapabilityBoundary(root)

  assert.equal(result.ok, false)
  assert.ok(result.violations.some(({ rule }) => rule === 'tauri-task-runtime-assembly'))
})

test('allows the non-task desktop settings runtime', () => {
  const root = fixture({
    'apps/desktop/src-tauri/src/runtime.rs': `
DesktopSettingsRuntime::builder()
  .with_mcp_config(mcp_config)
  .with_plugin_registry(plugin_registry)
  .with_skill_loader(skill_loader);
`,
    'crates/jyowo-harness-daemon/src/sdk_run_factory.rs': daemonAssembly,
  })

  assert.equal(scanDaemonAgentCapabilityBoundary(root).ok, true)
})

test('requires all task capability assembly calls in the daemon', () => {
  const root = fixture({
    'crates/jyowo-harness-daemon/src/sdk_run_factory.rs': `
Harness::builder()
  .with_mcp_config(mcp_config)
  .with_plugin_registry(plugin_registry);
`,
  })

  const result = scanDaemonAgentCapabilityBoundary(root)

  assert.equal(result.ok, false)
  assert.deepEqual(
    result.violations
      .filter(({ rule }) => rule === 'daemon-capability-assembly-missing')
      .map(({ token }) => token),
    [
      'with_provider_capability_routes',
      'with_skill_config_snapshot',
      'with_skill_loader',
    ],
  )
})
