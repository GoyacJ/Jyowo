import { describe, expect, it, vi } from 'vitest'

import type { ServerFrame } from '@/generated/daemon-protocol'

import { createDaemonClient, type DaemonTransport } from './client'

const taskId = '00000000000000000000000001'
const blobId = '00000000000000000000000002'
const contentHash = Array.from({ length: 32 }, (_, index) => index)

describe('daemon client', () => {
  it('sends typed task metadata commands with optimistic concurrency', async () => {
    const requests: unknown[] = []
    const invoke = vi.fn(async (_command: string, args?: Record<string, unknown>) => {
      const frame = args?.frame as { request: unknown; requestId: string }
      requests.push(frame.request)
      return {
        message: {
          commandId: taskId,
          committedOffset: 5,
          streamVersion: 8,
          taskId,
          type: 'command_accepted',
        },
        protocolVersion: 2,
        requestId: frame.requestId,
      }
    })
    const client = createDaemonClient(transport(invoke), { requestId: () => 'metadata-request' })

    await client.renameTask(taskId, 4, 'Renamed')
    await client.setTaskPinned(taskId, 5, true)
    await client.setTaskArchived(taskId, 6, true)
    await client.removeTask(taskId, 7)

    expect(requests).toEqual([
      {
        metadata: expect.objectContaining({ expectedStreamVersion: 4 }),
        taskId,
        title: 'Renamed',
        type: 'rename_task',
      },
      {
        metadata: expect.objectContaining({ expectedStreamVersion: 5 }),
        pinned: true,
        taskId,
        type: 'set_task_pinned',
      },
      {
        archived: true,
        metadata: expect.objectContaining({ expectedStreamVersion: 6 }),
        taskId,
        type: 'set_task_archived',
      },
      {
        metadata: expect.objectContaining({ expectedStreamVersion: 7 }),
        taskId,
        type: 'remove_task',
      },
    ])
  })

  it('rejects task metadata mutations that the daemon does not accept', async () => {
    const invoke = vi.fn(async (_command: string, args?: Record<string, unknown>) => {
      const frame = args?.frame as { requestId: string }
      return {
        message: {
          commandId: taskId,
          reason: 'wrong_expected_version',
          taskId,
          type: 'command_rejected',
        },
        protocolVersion: 2,
        requestId: frame.requestId,
      }
    })
    const client = createDaemonClient(transport(invoke))

    await expect(client.setTaskPinned(taskId, 1, true)).rejects.toThrow('wrong expected version')
  })

  it('validates every response and builds generated request frames', async () => {
    const invoke = vi.fn(async (_command: string, args?: Record<string, unknown>) => {
      if (_command === 'daemon_connect') {
        return taskListFrame()
      }
      const frame = args?.frame as { requestId: string }
      return {
        requestId: frame.requestId,
        protocolVersion: 2,
        message: { type: 'task_list', tasks: [] },
      }
    })
    const client = createDaemonClient(transport(invoke), {
      requestId: () => 'request-1',
    })

    await expect(client.connect()).resolves.toEqual(taskListFrame())
    await client.request({ type: 'list_tasks' })

    expect(invoke).toHaveBeenLastCalledWith('daemon_request', {
      frame: {
        protocolVersion: 2,
        request: { type: 'list_tasks' },
        requestId: 'request-1',
      },
    })
  })

  it('enters protocol error without dispatching an invalid subscription frame', async () => {
    let listener: ((payload: unknown) => void) | undefined
    const onFrame = vi.fn()
    const onProtocolError = vi.fn()
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === 'daemon_subscribe') return args?.subscriptionId
      if (command === 'daemon_unsubscribe') return null
      throw new Error(`unexpected command ${command}`)
    })
    const client = createDaemonClient(
      {
        invoke,
        listen: vi.fn(async (_event, handler) => {
          listener = handler
          return vi.fn()
        }),
      },
      { requestId: () => 'subscription-1' },
    )

    const unsubscribe = await client.subscribe(40, onFrame, onProtocolError)
    listener?.({ protocolVersion: 2, message: { type: 'future_event' } })

    expect(onFrame).not.toHaveBeenCalled()
    expect(onProtocolError).toHaveBeenCalledOnce()
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith('daemon_unsubscribe', {
        subscriptionId: 'subscription-1',
      }),
    )
    await unsubscribe()
    expect(invoke.mock.calls.filter(([command]) => command === 'daemon_unsubscribe')).toHaveLength(
      1,
    )
  })

  it('closes a subscription that receives a schema-valid non-event frame', async () => {
    let listener: ((payload: unknown) => void) | undefined
    const onFrame = vi.fn()
    const onProtocolError = vi.fn()
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === 'daemon_subscribe') return args?.subscriptionId
      if (command === 'daemon_unsubscribe') return null
      throw new Error(`unexpected command ${command}`)
    })
    const client = createDaemonClient(
      {
        invoke,
        listen: vi.fn(async (_event, handler) => {
          listener = handler
          return vi.fn()
        }),
      },
      { requestId: () => 'subscription-1' },
    )

    await client.subscribe(40, onFrame, onProtocolError)
    listener?.(taskListFrame())

    expect(onFrame).not.toHaveBeenCalled()
    expect(onProtocolError).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'Expected event_batch, received task_list' }),
    )
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith('daemon_unsubscribe', {
        subscriptionId: 'subscription-1',
      }),
    )
  })

  it('reads blobs only through validated blob ids', async () => {
    const invoke = vi.fn(async () => ({
      requestId: null,
      protocolVersion: 2,
      message: {
        type: 'blob',
        blobId,
        contentHash,
        mediaType: 'text/plain',
        missing: false,
        size: 3,
        base64Data: 'YWJj',
      },
    }))
    const client = createDaemonClient(transport(invoke))

    await expect(client.readBlob(blobId)).resolves.toEqual({
      blobId,
      bytes: new Uint8Array([97, 98, 99]),
      contentHash,
      mediaType: 'text/plain',
      missing: false,
      size: 3,
    })
    await expect(client.readBlob('/tmp/private')).rejects.toThrow('Invalid daemon client frame')
    expect(invoke).toHaveBeenCalledOnce()
  })

  it('rejects a blob response for a different blob id', async () => {
    const invoke = vi.fn(async () => ({
      requestId: null,
      protocolVersion: 2,
      message: {
        type: 'blob',
        blobId: '00000000000000000000000009',
        contentHash,
        mediaType: 'text/plain',
        missing: false,
        size: 3,
        base64Data: 'YWJj',
      },
    }))
    const client = createDaemonClient(transport(invoke))

    await expect(client.readBlob(blobId)).rejects.toThrow('another blob')
  })

  it('stages a local path through Tauri and returns a daemon-owned attachment', async () => {
    const invoke = vi.fn(async () => ({
      requestId: null,
      protocolVersion: 2,
      message: {
        type: 'blob',
        blobId,
        contentHash,
        mediaType: 'text/plain',
        missing: false,
        size: 5,
        base64Data: null,
      },
    }))
    const client = createDaemonClient(transport(invoke))

    await expect(client.stageBlobFromPath(taskId, '/tmp/notes.txt')).resolves.toEqual({
      attachment: {
        blobRef: {
          contentHash,
          contentType: 'text/plain',
          id: blobId,
          size: 5,
        },
        id: `attachment-${contentHash.map((byte) => byte.toString(16).padStart(2, '0')).join('')}`,
        mimeType: 'text/plain',
        name: 'notes.txt',
        sizeBytes: 5,
      },
    })
    expect(invoke).toHaveBeenCalledWith('daemon_stage_blob_from_path', {
      path: '/tmp/notes.txt',
      taskId,
    })
  })

  it('lists reference candidates through the task-native bridge', async () => {
    const payload = {
      artifacts: [],
      conversations: [],
      files: [{ label: 'src/main.ts', path: 'src/main.ts' }],
      memories: [],
      mcpServers: [],
      skills: [
        {
          id: 'workspace:review',
          label: 'review',
          source: 'workspace',
        },
        {
          id: 'plugin:release-notes',
          label: 'release-notes',
          source: { plugin: 'publisher@1.0.0' },
        },
      ],
      tools: [],
    }
    const invoke = vi.fn().mockResolvedValue(payload)
    const client = createDaemonClient(transport(invoke))

    await expect(client.listReferenceCandidates(taskId)).resolves.toEqual(payload)
    expect(invoke).toHaveBeenCalledWith('daemon_list_reference_candidates', { taskId })
  })

  it('rejects skill reference candidates without native source identity', async () => {
    const invoke = vi.fn().mockResolvedValue({
      artifacts: [],
      conversations: [],
      files: [],
      memories: [],
      mcpServers: [],
      skills: [{ id: 'workspace:review', label: 'review' }],
      tools: [],
    })
    const client = createDaemonClient(transport(invoke))

    await expect(client.listReferenceCandidates(taskId)).rejects.toThrow(
      'Invalid task reference candidate',
    )
  })

  it('rejects absolute paths from the task reference bridge', async () => {
    const invoke = vi.fn().mockResolvedValue({
      artifacts: [],
      conversations: [],
      files: [{ label: '/private/secret', path: '/private/secret' }],
      memories: [],
      mcpServers: [],
      skills: [],
      tools: [],
    })
    const client = createDaemonClient(transport(invoke))

    await expect(client.listReferenceCandidates(taskId)).rejects.toThrow(
      'Invalid task reference candidate',
    )
  })

  it('rejects invalid subscription offsets before opening a listener', async () => {
    const daemonTransport = transport(vi.fn())
    const client = createDaemonClient(daemonTransport)

    await expect(client.subscribe(-1, vi.fn())).rejects.toThrow('Invalid daemon client frame')
    expect(daemonTransport.invoke).not.toHaveBeenCalled()
  })

  it('isolates concurrent subscriptions on separate bridge event channels', async () => {
    const listenedEvents: string[] = []
    const requestIds = ['subscription-a', 'subscription-b']
    const client = createDaemonClient(
      {
        invoke: vi.fn(async (command, args) => {
          if (command === 'daemon_subscribe') return args?.subscriptionId
          if (command === 'daemon_unsubscribe') return null
          throw new Error(`unexpected command ${command}`)
        }),
        listen: vi.fn(async (event) => {
          listenedEvents.push(event)
          return vi.fn()
        }),
      },
      { requestId: () => requestIds.shift() as string },
    )

    const closeA = await client.subscribe(10, vi.fn())
    const closeB = await client.subscribe(20, vi.fn())

    expect(listenedEvents).toEqual([
      'jyowo://daemon-events/subscription-a',
      'jyowo://daemon-events/subscription-b',
    ])
    await closeA()
    await closeB()
  })

  it('rejects a snapshot for a different task', async () => {
    const invoke = vi.fn(async () => ({
      requestId: 'request-1',
      protocolVersion: 2,
      message: {
        type: 'task_snapshot',
        projection: {
          archived: false,
          lastGlobalOffset: 0,
          queue: [],
          state: 'idle',
          streamVersion: 1,
          taskId: '00000000000000000000000009',
          title: 'wrong task',
        },
        snapshotOffset: 0,
        timeline: [],
      },
    }))
    const client = createDaemonClient(transport(invoke), { requestId: () => 'request-1' })

    await expect(client.loadTask(taskId)).rejects.toThrow('another task')
  })

  it('loads bounded task audit pages with a backward cursor', async () => {
    const invoke = vi.fn(async (_command: string, args?: Record<string, unknown>) => {
      const frame = args?.frame as { request: unknown; requestId: string }
      return {
        requestId: frame.requestId,
        protocolVersion: 2,
        message: {
          events: [],
          nextBeforeOffset: 20,
          taskId,
          type: 'task_event_page',
        },
      }
    })
    const client = createDaemonClient(transport(invoke), { requestId: () => 'audit-request' })

    await expect(client.loadTaskEvents(taskId, 42)).resolves.toMatchObject({
      nextBeforeOffset: 20,
      taskId,
    })
    expect(invoke).toHaveBeenCalledWith('daemon_request', {
      frame: {
        protocolVersion: 2,
        request: {
          beforeGlobalOffset: 42,
          limit: 16,
          taskId,
          type: 'load_task_events',
        },
        requestId: 'audit-request',
      },
    })
  })
})

function transport(invoke: DaemonTransport['invoke']): DaemonTransport {
  return {
    invoke,
    listen: async () => () => undefined,
  }
}

function taskListFrame(): ServerFrame {
  return {
    requestId: null,
    protocolVersion: 2,
    message: { type: 'task_list', tasks: [] },
  }
}
