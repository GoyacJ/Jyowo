import { describe, expect, it } from 'vitest'

import { shouldUseMockCommandClient } from './default-client'

describe('default CommandClient selection', () => {
  it('allows the mock client only in dev runtimes', () => {
    expect(shouldUseMockCommandClient({ DEV: true, VITE_JYOWO_COMMAND_CLIENT: 'mock' })).toBe(true)
    expect(shouldUseMockCommandClient({ DEV: false, VITE_JYOWO_COMMAND_CLIENT: 'mock' })).toBe(
      false,
    )
  })
})
