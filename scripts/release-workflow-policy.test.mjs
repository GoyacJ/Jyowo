import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import test from 'node:test'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))
const workflow = readFileSync(join(repoRoot, '.github', 'workflows', 'release.yml'), 'utf8')

test('release workflow is triggered only by semantic version tags', () => {
  assert.match(workflow, /push:\s*\n\s*tags:\s*\n\s*-\s*['"]v\*\.\*\.\*['"]/)
})

test('release workflow checks versions before matrix builds', () => {
  assert.match(workflow, /needs:\s*version/)
  assert.match(workflow, /pnpm check:release-version/)
})

test('release workflow builds all supported desktop platforms', () => {
  assert.match(workflow, /windows-latest/)
  assert.match(workflow, /macos-latest/)
  assert.match(workflow, /ubuntu-22\.04/)
  assert.match(workflow, /libwebkit2gtk-4\.1-dev/)
  assert.match(workflow, /libayatana-appindicator3-dev/)
})

test('release workflow uploads Tauri artifacts with updater signing secrets', () => {
  assert.match(workflow, /tauri-apps\/tauri-action@v0/)
  assert.match(workflow, /projectPath:\s*apps\/desktop/)
  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY:/)
  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY_PASSWORD:/)
  assert.match(workflow, /includeUpdaterJson:\s*true/)
  assert.doesNotMatch(workflow, /uploadUpdaterJson:/)
  assert.match(workflow, /releaseDraft:\s*false/)
})
