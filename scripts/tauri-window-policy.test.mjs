import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const readJson = (relativePath) =>
  JSON.parse(readFileSync(new URL(`../${relativePath}`, import.meta.url), 'utf8'))

test('hidden main window grants the frontend permission to show it', () => {
  const config = readJson('apps/desktop/src-tauri/tauri.conf.json')
  const capability = readJson('apps/desktop/src-tauri/capabilities/default.json')
  const mainWindow = config.app.windows.find((window) => window.label === 'main')

  assert.ok(mainWindow, 'main window must be configured')
  assert.equal(mainWindow.visible, false)
  assert.ok(capability.windows.includes('main'))
  assert.ok(capability.permissions.includes('core:window:allow-show'))
})
