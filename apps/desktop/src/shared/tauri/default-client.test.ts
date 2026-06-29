import { afterEach, describe, expect, it, vi } from 'vitest'

import { createInvokeCommandClient, tauriCommandClient } from './commands'
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

  it('exposes provider capability route command methods', () => {
    const client = createDefaultCommandClient()

    expect(typeof client.listProviderCapabilityRoutes).toBe('function')
    expect(typeof client.listProviderCapabilityRouteOptions).toBe('function')
    expect(typeof client.saveProviderCapabilityRoute).toBe('function')
    expect(typeof client.deleteProviderCapabilityRoute).toBe('function')
  })

  it('forwards provider capability route command names', async () => {
    const invoke = vi.fn(async (command: string) => {
      switch (command) {
        case 'list_provider_capability_routes':
          return { version: 1, routes: [] }
        case 'list_provider_capability_route_options':
          return { options: [] }
        case 'save_provider_capability_route':
          return { version: 1, routes: [], status: 'saved' }
        case 'delete_provider_capability_route':
          return { version: 1, routes: [], status: 'deleted' }
        default:
          throw new Error(`unexpected command: ${command}`)
      }
    })
    const client = createInvokeCommandClient(invoke)

    await client.listProviderCapabilityRoutes()
    await client.listProviderCapabilityRouteOptions()
    await client.saveProviderCapabilityRoute({
      route: {
        kind: 'image_generation',
        configId: 'minimax-image',
        providerId: 'minimax',
        operationIds: ['minimax.image_generation'],
        enabled: false,
      },
    })
    await client.deleteProviderCapabilityRoute({
      kind: 'image_generation',
      configId: 'minimax-image',
      providerId: 'minimax',
    })

    expect(invoke).toHaveBeenCalledWith('list_provider_capability_routes')
    expect(invoke).toHaveBeenCalledWith('list_provider_capability_route_options')
    expect(invoke).toHaveBeenCalledWith('save_provider_capability_route', {
      route: {
        kind: 'image_generation',
        configId: 'minimax-image',
        providerId: 'minimax',
        operationIds: ['minimax.image_generation'],
        enabled: false,
      },
    })
    expect(invoke).toHaveBeenCalledWith('delete_provider_capability_route', {
      kind: 'image_generation',
      configId: 'minimax-image',
      providerId: 'minimax',
    })
  })
})
