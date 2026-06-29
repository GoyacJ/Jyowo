import { describe, expect, it, vi } from 'vitest'

import {
  checkForAppUpdate,
  downloadAndInstallUpdate,
  relaunchApp,
  type UpdateHandle,
  type UpdaterClient,
} from './updater'

function createUpdate(overrides: Partial<UpdateHandle> = {}): UpdateHandle {
  return {
    body: 'Release notes',
    currentVersion: '0.1.0',
    date: '2026-06-29T00:00:00Z',
    downloadAndInstall: vi.fn(),
    version: '0.2.0',
    ...overrides,
  }
}

function createClient(update: UpdateHandle | null): UpdaterClient {
  return {
    check: vi.fn(async () => update),
    relaunch: vi.fn(async () => {}),
  }
}

describe('shared tauri updater wrapper', () => {
  it('reports current when no update is available', async () => {
    await expect(checkForAppUpdate(createClient(null))).resolves.toEqual({ kind: 'current' })
  })

  it('maps available update metadata without exposing raw plugin imports to components', async () => {
    const update = createUpdate()

    await expect(checkForAppUpdate(createClient(update))).resolves.toEqual({
      kind: 'available',
      update: {
        body: 'Release notes',
        currentVersion: '0.1.0',
        date: '2026-06-29T00:00:00Z',
        handle: update,
        version: '0.2.0',
      },
    })
  })

  it('normalizes download progress events', async () => {
    const update = createUpdate({
      downloadAndInstall: vi.fn(async (onEvent) => {
        onEvent?.({ event: 'Started', data: { contentLength: 100 } })
        onEvent?.({ event: 'Progress', data: { chunkLength: 40 } })
        onEvent?.({ event: 'Progress', data: { chunkLength: 60 } })
        onEvent?.({ event: 'Finished' })
      }),
    })
    const events: Array<unknown> = []

    await downloadAndInstallUpdate(
      { currentVersion: '0.1.0', handle: update, version: '0.2.0' },
      (event) => events.push(event),
    )

    expect(events).toEqual([
      { contentLength: 100, downloadedBytes: 0, kind: 'started' },
      { contentLength: 100, downloadedBytes: 40, kind: 'progress' },
      { contentLength: 100, downloadedBytes: 100, kind: 'progress' },
      { contentLength: 100, downloadedBytes: 100, kind: 'finished' },
    ])
  })

  it('relaunches through the wrapped process plugin client', async () => {
    const client = createClient(null)

    await relaunchApp(client)

    expect(client.relaunch).toHaveBeenCalledOnce()
  })
})
