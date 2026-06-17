import { describe, expect, it, vi } from 'vitest'

import {
  createInvokeCommandClient,
  getAppInfo,
  getHarnessHealthcheck,
  TauriCommandPayloadError,
} from './commands'
import { createMockCommandClient } from './mock-client'

describe('CommandClient', () => {
  it('normalizes get_app_info through Zod validation', async () => {
    const invoke = vi.fn().mockResolvedValue({
      name: 'Jyowo',
      version: '0.1.0',
      shell: 'tauri2-react',
      harness: {
        sdkCrate: 'jyowo_harness_sdk',
        mode: 'in-process',
      },
    })
    const client = createInvokeCommandClient(invoke)

    await expect(getAppInfo(client)).resolves.toMatchObject({
      name: 'Jyowo',
      shell: 'tauri2-react',
      harness: {
        sdkCrate: 'jyowo_harness_sdk',
      },
    })
    expect(invoke).toHaveBeenCalledWith('get_app_info')
  })

  it('normalizes harness_healthcheck through Zod validation', async () => {
    const invoke = vi.fn().mockResolvedValue({
      status: 'available',
      sdkCrate: 'jyowo_harness_sdk',
    })
    const client = createInvokeCommandClient(invoke)

    await expect(getHarnessHealthcheck(client)).resolves.toEqual({
      status: 'available',
      sdkCrate: 'jyowo_harness_sdk',
    })
    expect(invoke).toHaveBeenCalledWith('harness_healthcheck')
  })

  it('throws a schema error for invalid IPC payloads', async () => {
    const client = createInvokeCommandClient(vi.fn().mockResolvedValue({ name: 'Jyowo' }))

    await expect(getAppInfo(client)).rejects.toThrow(TauriCommandPayloadError)
  })

  it('supports mock clients outside the Tauri runtime', async () => {
    const client = createMockCommandClient()

    await expect(getAppInfo(client)).resolves.toMatchObject({
      name: 'Jyowo',
      shell: 'tauri2-react',
    })
    await expect(getHarnessHealthcheck(client)).resolves.toEqual({
      status: 'available',
      sdkCrate: 'jyowo_harness_sdk',
    })
  })
})
