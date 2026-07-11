import assert from 'node:assert/strict'
import { join } from 'node:path'
import test from 'node:test'

import {
  cargoBuiltBinaryPath,
  parseRustHostTriple,
  sidecarFilenameForTarget,
  sidecarOutputPath,
} from './daemon-sidecar-utils.mjs'

test('maps target triples to daemon sidecar filenames', () => {
  assert.equal(
    sidecarFilenameForTarget('aarch64-apple-darwin'),
    'jyowo-harness-daemon-aarch64-apple-darwin',
  )
  assert.equal(
    sidecarFilenameForTarget('x86_64-pc-windows-msvc'),
    'jyowo-harness-daemon-x86_64-pc-windows-msvc.exe',
  )
})

test('computes daemon build and bundle paths', () => {
  assert.equal(
    sidecarOutputPath({ repoRoot: '/repo', target: 'aarch64-apple-darwin' }),
    join(
      '/repo',
      'apps',
      'desktop',
      'src-tauri',
      'binaries',
      'jyowo-harness-daemon-aarch64-apple-darwin',
    ),
  )
  assert.equal(
    cargoBuiltBinaryPath({ repoRoot: '/repo', target: 'x86_64-pc-windows-msvc' }),
    join('/repo', 'target', 'x86_64-pc-windows-msvc', 'debug', 'jyowo-harness-daemon.exe'),
  )
})

test('parses the active Rust host target', () => {
  assert.equal(
    parseRustHostTriple('rustc 1.96.0\nhost: aarch64-apple-darwin\n'),
    'aarch64-apple-darwin',
  )
})
