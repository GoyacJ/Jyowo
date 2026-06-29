import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'

import { checkReleaseVersion } from './check-release-version.mjs'

function writeFixture({ cargoVersion = '0.1.0', desktopVersion = '0.1.0', rootVersion = '0.1.0', tauriVersion = '0.1.0' } = {}) {
  const root = join(tmpdir(), `jyowo-release-version-${randomUUID()}`)

  mkdirSync(join(root, 'apps', 'desktop', 'src-tauri'), { recursive: true })
  writeFileSync(join(root, 'package.json'), JSON.stringify({ version: rootVersion }), 'utf8')
  writeFileSync(
    join(root, 'apps', 'desktop', 'package.json'),
    JSON.stringify({ version: desktopVersion }),
    'utf8',
  )
  writeFileSync(
    join(root, 'Cargo.toml'),
    `[workspace.package]\nversion = "${cargoVersion}"\n`,
    'utf8',
  )
  writeFileSync(
    join(root, 'apps', 'desktop', 'src-tauri', 'tauri.conf.json'),
    JSON.stringify({ version: tauriVersion }),
    'utf8',
  )

  return root
}

test('passes when project versions and tag match', () => {
  const result = checkReleaseVersion(writeFixture(), {
    GITHUB_REF_NAME: 'v0.1.0',
    GITHUB_REF_TYPE: 'tag',
  })

  assert.deepEqual(result, { ok: true, version: '0.1.0' })
})

test('fails when project versions differ', () => {
  const result = checkReleaseVersion(writeFixture({ desktopVersion: '0.2.0' }), {})

  assert.equal(result.ok, false)
  assert.match(result.message, /desktop package.json: 0\.2\.0/)
  assert.match(result.message, /root package.json: 0\.1\.0/)
})

test('fails when tag version differs from project version', () => {
  const result = checkReleaseVersion(writeFixture(), {
    GITHUB_REF_NAME: 'v0.2.0',
    GITHUB_REF_TYPE: 'tag',
  })

  assert.equal(result.ok, false)
  assert.match(result.message, /tag v0\.2\.0/)
  assert.match(result.message, /project version 0\.1\.0/)
})
