import { tauriCommandClient } from './commands'
import { createMockCommandClient } from './mock-client'

interface CommandClientEnv {
  DEV: boolean
  VITE_JYOWO_COMMAND_CLIENT?: string
}

export function shouldUseMockCommandClient(env: CommandClientEnv) {
  return env.DEV && env.VITE_JYOWO_COMMAND_CLIENT === 'mock'
}

export function createDefaultCommandClient() {
  if (shouldUseMockCommandClient(import.meta.env)) {
    return createMockCommandClient()
  }

  return tauriCommandClient
}
