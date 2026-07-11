import assert from 'node:assert/strict'
import test from 'node:test'

import { evaluateDaemonSidecarPolicy } from './check-daemon-sidecar.mjs'

test('accepts complete daemon sidecar wiring', () => {
  assert.deepEqual(
    evaluateDaemonSidecarPolicy({
      packageJson: {
        scripts: { 'build:daemon-sidecar': 'node scripts/build-daemon-sidecar.mjs' },
      },
      desktopPackageJson: {
        scripts: { tauri: 'pnpm --dir ../.. build:daemon-sidecar && tauri' },
      },
      tauriConfig: { bundle: { externalBin: ['binaries/jyowo-harness-daemon'] } },
      files: [
        'scripts/build-daemon-sidecar.mjs',
        'apps/desktop/src-tauri/binaries/README.md',
      ],
      buildRs: '"jyowo-harness-daemon"',
    }),
    [],
  )
})

test('rejects target-suffixed bundle entries and missing build wiring', () => {
  const errors = evaluateDaemonSidecarPolicy({
    packageJson: { scripts: {} },
    desktopPackageJson: { scripts: {} },
    tauriConfig: {
      bundle: { externalBin: ['binaries/jyowo-harness-daemon-x86_64-pc-windows-msvc.exe'] },
    },
    files: [],
    buildRs: '',
  })
  assert.match(errors.join('\n'), /build:daemon-sidecar/)
  assert.match(errors.join('\n'), /base path/)
  assert.match(errors.join('\n'), /build\.rs/)
})
