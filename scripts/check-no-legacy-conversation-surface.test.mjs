import assert from 'node:assert/strict'
import test from 'node:test'

import {
  findLegacyContractViolations,
  findLegacyInvokeViolations,
  legacyContractIdentifiers,
  legacyInvokeNames,
} from './check-no-legacy-conversation-surface.mjs'

test('recognizes all 36 deleted Tauri invoke names', () => {
  assert.equal(legacyInvokeNames.length, 36)
})

test('rejects a legacy Tauri invoke fixture', () => {
  assert.deepEqual(
    findLegacyInvokeViolations("const command = 'get_conversation'\nreturn invoke(command)"),
    ['get_conversation'],
  )
})

test('recognizes deleted desktop contract identifiers', () => {
  assert.equal(legacyContractIdentifiers.length, 12)
  assert.deepEqual(findLegacyContractViolations('struct ConversationMetadataStore;'), [
    'ConversationMetadataStore',
  ])
})

test('allows daemon-only requests and retained Tauri settings commands', () => {
  const source = [
    "daemonClient.request({ type: 'list_automations' })",
    "const command = 'get_execution_settings'",
  ].join('\n')
  assert.deepEqual(findLegacyInvokeViolations(source), [])
})
