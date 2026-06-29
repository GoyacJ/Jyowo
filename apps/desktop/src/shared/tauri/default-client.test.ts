import { afterEach, describe, expect, it, vi } from 'vitest'

import { tauriCommandClient } from './commands'
import { createDefaultCommandClient } from './default-client'

describe('default CommandClient selection', () => {
  afterEach(() => {
    vi.unstubAllEnvs()
  })

  it('uses the real Tauri client', () => {
    expect(createDefaultCommandClient()).toBe(tauriCommandClient)
  })

  it('ignores the retired fixture runtime env outside Tauri', () => {
    vi.stubEnv('DEV', true)
    vi.stubEnv('VITE_JYOWO_COMMAND_CLIENT', 'fixture')

    expect(createDefaultCommandClient()).toBe(tauriCommandClient)
  })
})
