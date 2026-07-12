import assert from 'node:assert/strict'
import test from 'node:test'

import { compareTauriCommandRegistration } from './check-tauri-command-registration.mjs'

const registration = (...commands) => `
  .invoke_handler(tauri::generate_handler![
    ${commands.map(command => `commands::${command},`).join('\n')}
  ])
`

test('reports a Tauri invoke missing from generate_handler', () => {
  assert.deepEqual(
    compareTauriCommandRegistration({
      clientSources: ["const command = 'missing_command'\ninvoke(command)"],
      registrationSource: registration('registered_command'),
    }),
    { missing: ['missing_command'], orphaned: ['registered_command'] },
  )
})

test('accepts a registered direct or daemon transport invoke', () => {
  assert.deepEqual(
    compareTauriCommandRegistration({
      clientSources: [
        "const command = 'registered_command'\ninvoke(command)",
        "transport.invoke('daemon_request', { frame })",
      ],
      registrationSource: registration('registered_command', 'daemon_request'),
    }),
    { missing: [], orphaned: [] },
  )
})

test('ignores daemon protocol requests that do not invoke Tauri', () => {
  assert.deepEqual(
    compareTauriCommandRegistration({
      clientSources: ["daemonClient.request({ type: 'list_automations' })"],
      registrationSource: '',
    }),
    { missing: [], orphaned: [] },
  )
})
