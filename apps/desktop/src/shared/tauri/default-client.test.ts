import { afterEach, describe, expect, it, vi } from 'vitest'

import { tauriCommandClient } from './commands'
import {
  createDefaultCommandClient,
  hasTauriRuntime,
  shouldUseMockCommandClient,
} from './default-client'

function setTauriRuntimeGlobals(values: { __TAURI__?: unknown; __TAURI_INTERNALS__?: unknown }) {
  Reflect.deleteProperty(window, '__TAURI__')
  Reflect.deleteProperty(window, '__TAURI_INTERNALS__')

  if ('__TAURI__' in values) {
    Object.defineProperty(window, '__TAURI__', {
      configurable: true,
      value: values.__TAURI__,
    })
  }

  if ('__TAURI_INTERNALS__' in values) {
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: values.__TAURI_INTERNALS__,
    })
  }
}

describe('default CommandClient selection', () => {
  afterEach(() => {
    vi.unstubAllEnvs()
    Reflect.deleteProperty(window, '__TAURI__')
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__')
  })

  it('allows the mock client only as an explicit dev opt-in outside Tauri', () => {
    expect(shouldUseMockCommandClient({ DEV: true, VITE_JYOWO_COMMAND_CLIENT: 'mock' })).toBe(true)
    expect(shouldUseMockCommandClient({ DEV: false, VITE_JYOWO_COMMAND_CLIENT: 'mock' })).toBe(
      false,
    )
  })

  it('does not auto-select mock data when direct Vite dev has no Tauri runtime', () => {
    expect(shouldUseMockCommandClient({ DEV: true }, { hasTauriRuntime: false })).toBe(false)
    expect(shouldUseMockCommandClient({ DEV: true }, { hasTauriRuntime: true })).toBe(false)
    expect(
      shouldUseMockCommandClient(
        { DEV: true, VITE_JYOWO_COMMAND_CLIENT: 'mock' },
        { hasTauriRuntime: true },
      ),
    ).toBe(false)
    expect(
      shouldUseMockCommandClient(
        { DEV: false, VITE_JYOWO_COMMAND_CLIENT: 'mock' },
        { hasTauriRuntime: false },
      ),
    ).toBe(false)
  })

  it('detects available Tauri runtime globals', () => {
    setTauriRuntimeGlobals({})
    expect(hasTauriRuntime()).toBe(false)

    setTauriRuntimeGlobals({ __TAURI__: {} })
    expect(hasTauriRuntime()).toBe(true)

    setTauriRuntimeGlobals({ __TAURI_INTERNALS__: {} })
    expect(hasTauriRuntime()).toBe(true)
  })

  it('uses the real Tauri client when the Tauri runtime is available', () => {
    setTauriRuntimeGlobals({ __TAURI__: {} })

    expect(createDefaultCommandClient()).toBe(tauriCommandClient)
  })

  it('uses the deferred mock client only for explicit dev web mock runtime', async () => {
    vi.stubEnv('DEV', true)
    vi.stubEnv('VITE_JYOWO_COMMAND_CLIENT', 'mock')
    setTauriRuntimeGlobals({})

    const client = createDefaultCommandClient()
    const evalCases = await client.listEvalCases()

    expect(client).not.toBe(tauriCommandClient)
    expect(evalCases.cases[0]?.id).toBe('regression-smoke')
  })
})
