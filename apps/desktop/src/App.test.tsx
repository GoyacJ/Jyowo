import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import App from './App'
import { getAppInfo, getHarnessHealthcheck } from './tauri/client'

const invokeMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

describe('Tauri client', () => {
  beforeEach(() => {
    invokeMock.mockReset()
  })

  it('requests app info through the get_app_info command', async () => {
    invokeMock.mockResolvedValueOnce({
      name: 'Jyowo',
      version: '0.1.0',
      shell: 'tauri2-react',
      harness: {
        sdkCrate: 'jyowo_harness_sdk',
        mode: 'in-process',
      },
    })

    await expect(getAppInfo()).resolves.toMatchObject({
      name: 'Jyowo',
      shell: 'tauri2-react',
      harness: {
        sdkCrate: 'jyowo_harness_sdk',
        mode: 'in-process',
      },
    })
    expect(invokeMock).toHaveBeenCalledWith('get_app_info')
  })

  it('requests harness status through the harness_healthcheck command', async () => {
    invokeMock.mockResolvedValueOnce({
      status: 'available',
      sdkCrate: 'jyowo_harness_sdk',
    })

    await expect(getHarnessHealthcheck()).resolves.toEqual({
      status: 'available',
      sdkCrate: 'jyowo_harness_sdk',
    })
    expect(invokeMock).toHaveBeenCalledWith('harness_healthcheck')
  })
})

describe('App', () => {
  beforeEach(() => {
    invokeMock.mockReset()
  })

  it('renders Jyowo app info and harness health from Tauri commands', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_app_info') {
        return {
          name: 'Jyowo',
          version: '0.1.0',
          shell: 'tauri2-react',
          harness: {
            sdkCrate: 'jyowo_harness_sdk',
            mode: 'in-process',
          },
        }
      }

      if (command === 'harness_healthcheck') {
        return {
          status: 'available',
          sdkCrate: 'jyowo_harness_sdk',
        }
      }

      throw new Error(`unexpected command: ${command}`)
    })

    render(<App />)

    expect(await screen.findByText('Jyowo')).toBeInTheDocument()
    expect(screen.getByText('tauri2-react')).toBeInTheDocument()
    expect(screen.getByText('jyowo_harness_sdk')).toBeInTheDocument()
    expect(screen.getByText('available')).toBeInTheDocument()
  })
})
