import assert from 'node:assert/strict'
import { join } from 'node:path'
import test from 'node:test'

import {
  cargoBuiltBinaryPath,
  parseRustHostTriple,
  sidecarFilenameForTarget,
  sidecarOutputPath,
} from './agent-supervisor-sidecar-utils.mjs'

test('maps target triples to Tauri sidecar filenames', () => {
  assert.equal(
    sidecarFilenameForTarget('x86_64-apple-darwin'),
    'jyowo-agent-supervisor-x86_64-apple-darwin',
  )
  assert.equal(
    sidecarFilenameForTarget('aarch64-apple-darwin'),
    'jyowo-agent-supervisor-aarch64-apple-darwin',
  )
  assert.equal(
    sidecarFilenameForTarget('x86_64-pc-windows-msvc'),
    'jyowo-agent-supervisor-x86_64-pc-windows-msvc.exe',
  )
  assert.equal(
    sidecarFilenameForTarget('x86_64-unknown-linux-gnu'),
    'jyowo-agent-supervisor-x86_64-unknown-linux-gnu',
  )
})

test('computes copied sidecar output path under src-tauri binaries', () => {
  assert.equal(
    sidecarOutputPath({ repoRoot: '/repo', target: 'aarch64-apple-darwin' }),
    join(
      '/repo',
      'apps',
      'desktop',
      'src-tauri',
      'binaries',
      'jyowo-agent-supervisor-aarch64-apple-darwin',
    ),
  )
})

test('computes cargo-built binary source path for target triple', () => {
  assert.equal(
    cargoBuiltBinaryPath({ repoRoot: '/repo', target: 'x86_64-pc-windows-msvc' }),
    join('/repo', 'target', 'x86_64-pc-windows-msvc', 'debug', 'jyowo-agent-supervisor.exe'),
  )
})

test('parses active host target from rustc verbose version output', () => {
  assert.equal(
    parseRustHostTriple(`rustc 1.96.0\nhost: aarch64-apple-darwin\nrelease: 1.96.0\n`),
    'aarch64-apple-darwin',
  )
})
