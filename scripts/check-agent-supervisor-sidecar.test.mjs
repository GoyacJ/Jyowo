import assert from 'node:assert/strict'
import test from 'node:test'

import { evaluateAgentSupervisorSidecarPolicy } from './check-agent-supervisor-sidecar.mjs'

test('passes when supervisor sidecar is not configured and no generated binary is tracked', () => {
  const errors = evaluateAgentSupervisorSidecarPolicy({
    packageJson: { scripts: {} },
    tauriConfig: { bundle: {} },
    files: [],
    buildRs: '',
  })

  assert.deepEqual(errors, [])
})

test('fails tracked supervisor binaries without bundle policy', () => {
  const errors = evaluateAgentSupervisorSidecarPolicy({
    packageJson: { scripts: {} },
    tauriConfig: { bundle: {} },
    files: ['apps/desktop/src-tauri/binaries/jyowo-agent-supervisor-x86_64-apple-darwin'],
    buildRs: '',
  })

  assert.deepEqual(errors, [
    'tracked supervisor sidecar binary exists without bundle.externalBin policy',
  ])
})

test('validates complete configured supervisor sidecar policy', () => {
  const errors = evaluateAgentSupervisorSidecarPolicy({
    packageJson: {
      scripts: {
        'build:agent-supervisor-sidecar': 'node scripts/build-agent-supervisor-sidecar.mjs',
      },
    },
    tauriConfig: { bundle: { externalBin: ['binaries/jyowo-agent-supervisor'] } },
    files: [
      'scripts/build-agent-supervisor-sidecar.mjs',
      'apps/desktop/src-tauri/binaries/README.md',
    ],
    buildRs: 'const SUPERVISOR: &str = "jyowo-agent-supervisor";',
  })

  assert.deepEqual(errors, [])
})

test('fails partial configured supervisor sidecar policy', () => {
  const errors = evaluateAgentSupervisorSidecarPolicy({
    packageJson: { scripts: {} },
    tauriConfig: { bundle: { externalBin: ['binaries/jyowo-agent-supervisor-x86_64.exe'] } },
    files: [],
    buildRs: '',
  })

  assert.match(errors.join('\n'), /build:agent-supervisor-sidecar/)
  assert.match(errors.join('\n'), /base path/)
  assert.match(errors.join('\n'), /build\.rs/)
})
