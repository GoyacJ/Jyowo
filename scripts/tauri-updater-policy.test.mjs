import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import test from 'node:test'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

function read(path) {
  return readFileSync(join(repoRoot, path), 'utf8')
}

function listFiles(dir, extensions) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)

    if (entry.isDirectory()) {
      return listFiles(path, extensions)
    }

    return extensions.some((extension) => path.endsWith(extension)) ? [path] : []
  })
}

test('tauri config creates updater artifacts from GitHub Releases', () => {
  const config = JSON.parse(read('apps/desktop/src-tauri/tauri.conf.json'))

  assert.equal(config.bundle.createUpdaterArtifacts, true)
  assert.equal(
    config.plugins.updater.endpoints[0],
    'https://github.com/GoyacJ/Jyowo/releases/latest/download/latest.json',
  )
  assert.match(config.plugins.updater.pubkey, /^[A-Za-z0-9+/=]+$/)
})

test('default capability allows updater and process plugins', () => {
  const capability = JSON.parse(read('apps/desktop/src-tauri/capabilities/default.json'))

  assert.ok(capability.permissions.includes('updater:default'))
  assert.ok(capability.permissions.includes('process:default'))
})

test('desktop shell registers updater and process plugins', () => {
  const lib = read('apps/desktop/src-tauri/src/lib.rs')
  const manifest = read('apps/desktop/src-tauri/Cargo.toml')
  const workspace = read('Cargo.toml')

  assert.match(workspace, /tauri-plugin-updater\s*=\s*"2\.10\.1"/)
  assert.match(workspace, /tauri-plugin-process\s*=\s*"2\.3\.1"/)
  assert.match(manifest, /tauri-plugin-updater\s*=\s*\{\s*workspace\s*=\s*true\s*\}/)
  assert.match(manifest, /tauri-plugin-process\s*=\s*\{\s*workspace\s*=\s*true\s*\}/)
  assert.match(lib, /tauri_plugin_updater::Builder::new\(\)\.build\(\)/)
  assert.match(lib, /tauri_plugin_process::init\(\)/)
})

test('frontend imports raw updater and process plugins only from shared tauri wrapper', () => {
  const sourceFiles = listFiles(join(repoRoot, 'apps', 'desktop', 'src'), ['.ts', '.tsx'])
  const violations = sourceFiles.filter((file) => {
    if (file.includes(join('shared', 'tauri'))) {
      return false
    }

    return /@tauri-apps\/plugin-(?:updater|process)/.test(readFileSync(file, 'utf8'))
  })

  assert.deepEqual(
    violations.map((file) => file.slice(repoRoot.length + 1)),
    [],
  )
})
