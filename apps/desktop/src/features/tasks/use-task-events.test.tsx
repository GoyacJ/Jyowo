import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { DaemonClient, DaemonSubscriptionHandler } from '@/shared/daemon/client'
import { DaemonClientProvider } from '@/shared/tauri/react'

import { createTaskStore, type TaskSnapshot } from './task-store'
import { useTaskEvents } from './use-task-events'

const taskId = '00000000000000000000000001'

describe('useTaskEvents', () => {
  it('requests one authoritative resnapshot for a gap and resumes from its offset', async () => {
    const store = createTaskStore(taskId)
    const subscriptions: Array<{ afterOffset: number; handler: DaemonSubscriptionHandler }> = []
    const unsubscribers: Array<ReturnType<typeof vi.fn>> = []
    const snapshots = [snapshot(40, 'initial'), snapshot(50, 'resynced')]
    const client = {
      connect: vi.fn(async () => taskListFrame()),
      loadTask: vi.fn(async () => snapshots.shift() as TaskSnapshot),
      subscribe: vi.fn(async (afterOffset, handler) => {
        const unsubscribe = vi.fn(async () => undefined)
        subscriptions.push({ afterOffset, handler })
        unsubscribers.push(unsubscribe)
        return unsubscribe
      }),
    } as unknown as DaemonClient

    renderHook(() => useTaskEvents(taskId, store), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <DaemonClientProvider client={client}>{children}</DaemonClientProvider>
      ),
    })

    await waitFor(() => expect(subscriptions.map((item) => item.afterOffset)).toEqual([40]))

    act(() => {
      subscriptions[0]?.handler(eventBatch([event(41), event(42), event(42), event(44)], 40, 44))
    })

    await waitFor(() => expect(subscriptions.map((item) => item.afterOffset)).toEqual([40, 50]))
    expect(client.loadTask).toHaveBeenCalledTimes(2)
    expect(unsubscribers[0]).toHaveBeenCalledOnce()
    expect(store.getState()).toMatchObject({
      connectionState: 'connected',
      lastAppliedOffset: 50,
      snapshot: { projection: { title: 'resynced' } },
    })
  })

  it('moves to protocol_error without applying partial state', async () => {
    const store = createTaskStore(taskId)
    let protocolError: ((error: Error) => void) | undefined
    const client = {
      connect: vi.fn(async () => taskListFrame()),
      loadTask: vi.fn(async () => snapshot(40, 'valid snapshot')),
      subscribe: vi.fn(async (_afterOffset, _handler, onProtocolError) => {
        protocolError = onProtocolError
        return vi.fn(async () => undefined)
      }),
    } as unknown as DaemonClient

    renderHook(() => useTaskEvents(taskId, store), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <DaemonClientProvider client={client}>{children}</DaemonClientProvider>
      ),
    })
    await waitFor(() => expect(protocolError).toBeTypeOf('function'))

    act(() => protocolError?.(new Error('Invalid daemon server frame')))

    expect(store.getState()).toMatchObject({
      connectionError: 'Invalid daemon server frame',
      connectionState: 'protocol_error',
      events: [],
      lastAppliedOffset: 40,
      snapshot: { projection: { title: 'valid snapshot' } },
    })
  })

  it('discards queued frames when a later frame enters protocol_error', async () => {
    const store = createTaskStore(taskId)
    let handler: DaemonSubscriptionHandler | undefined
    let renderFrame: FrameRequestCallback | undefined
    const animationFrame = vi
      .spyOn(window, 'requestAnimationFrame')
      .mockImplementation((callback) => {
        renderFrame = callback
        return 1
      })
    const client = {
      connect: vi.fn(async () => taskListFrame()),
      loadTask: vi.fn(async () => snapshot(40, 'valid snapshot')),
      subscribe: vi.fn(async (_afterOffset, onFrame) => {
        handler = onFrame
        return vi.fn(async () => undefined)
      }),
    } as unknown as DaemonClient

    renderHook(() => useTaskEvents(taskId, store), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <DaemonClientProvider client={client}>{children}</DaemonClientProvider>
      ),
    })
    await waitFor(() => expect(handler).toBeTypeOf('function'))

    act(() => {
      handler?.(eventBatch([event(41)], 40, 41))
      handler?.(taskListFrame())
    })
    expect(store.getState()).toMatchObject({
      connectionError: 'Expected event_batch, received task_list',
      connectionState: 'protocol_error',
    })

    act(() => renderFrame?.(0))
    animationFrame.mockRestore()

    expect(store.getState()).toMatchObject({
      connectionState: 'protocol_error',
      events: [],
      lastAppliedOffset: 40,
    })
  })

  it('keeps protocol_error terminal after an in-flight resnapshot completes', async () => {
    const store = createTaskStore(taskId)
    let handler: DaemonSubscriptionHandler | undefined
    let protocolError: ((error: Error) => void) | undefined
    let renderFrame: FrameRequestCallback | undefined
    const resnapshot = deferred<TaskSnapshot>()
    const animationFrame = vi
      .spyOn(window, 'requestAnimationFrame')
      .mockImplementation((callback) => {
        renderFrame = callback
        return 1
      })
    const client = {
      connect: vi.fn(async () => taskListFrame()),
      loadTask: vi
        .fn<DaemonClient['loadTask']>()
        .mockResolvedValueOnce(snapshot(40, 'initial'))
        .mockImplementationOnce(() => resnapshot.promise),
      subscribe: vi.fn(async (_afterOffset, onFrame, onProtocolError) => {
        handler = onFrame
        protocolError = onProtocolError
        return vi.fn(async () => undefined)
      }),
    } as unknown as DaemonClient

    renderHook(() => useTaskEvents(taskId, store), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <DaemonClientProvider client={client}>{children}</DaemonClientProvider>
      ),
    })
    await waitFor(() => expect(handler).toBeTypeOf('function'))

    act(() => {
      handler?.(gapBatch(40, 50))
      renderFrame?.(0)
    })
    await waitFor(() => expect(client.loadTask).toHaveBeenCalledTimes(2))

    act(() => protocolError?.(new Error('Invalid daemon server frame')))
    await act(async () => {
      resnapshot.resolve(snapshot(50, 'must not apply'))
      await resnapshot.promise
      await Promise.resolve()
    })
    animationFrame.mockRestore()

    expect(store.getState()).toMatchObject({
      connectionError: 'Invalid daemon server frame',
      connectionState: 'protocol_error',
      lastAppliedOffset: 40,
      snapshot: { projection: { title: 'initial' } },
    })
    expect(client.subscribe).toHaveBeenCalledOnce()
  })

  it('batches event rendering on one animation frame without changing offset order', async () => {
    const store = createTaskStore(taskId)
    let handler: DaemonSubscriptionHandler | undefined
    let renderFrame: FrameRequestCallback | undefined
    const animationFrame = vi
      .spyOn(window, 'requestAnimationFrame')
      .mockImplementation((callback) => {
        renderFrame = callback
        return 1
      })
    const client = {
      connect: vi.fn(async () => taskListFrame()),
      loadTask: vi.fn(async () => snapshot(40, 'initial')),
      subscribe: vi.fn(async (_afterOffset, onFrame) => {
        handler = onFrame
        return vi.fn(async () => undefined)
      }),
    } as unknown as DaemonClient

    renderHook(() => useTaskEvents(taskId, store), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <DaemonClientProvider client={client}>{children}</DaemonClientProvider>
      ),
    })
    await waitFor(() => expect(handler).toBeTypeOf('function'))

    act(() => {
      handler?.(eventBatch([event(41)], 40, 41))
      handler?.(eventBatch([event(42)], 41, 42))
    })
    expect(store.getState().lastAppliedOffset).toBe(40)

    act(() => renderFrame?.(0))
    expect(store.getState().lastAppliedOffset).toBe(42)
    expect(store.getState().events.map((item) => item.globalOffset)).toEqual([41, 42])
    expect(animationFrame).toHaveBeenCalledOnce()
    animationFrame.mockRestore()
  })

  it('ingests every queued frame after an earlier frame requests a resnapshot', async () => {
    const store = createTaskStore(taskId)
    let handler: DaemonSubscriptionHandler | undefined
    let renderFrame: FrameRequestCallback | undefined
    const resnapshot = deferred<TaskSnapshot>()
    const animationFrame = vi
      .spyOn(window, 'requestAnimationFrame')
      .mockImplementation((callback) => {
        renderFrame = callback
        return 1
      })
    const client = {
      connect: vi.fn(async () => taskListFrame()),
      loadTask: vi
        .fn<DaemonClient['loadTask']>()
        .mockResolvedValueOnce(snapshot(40, 'initial'))
        .mockImplementationOnce(() => resnapshot.promise),
      subscribe: vi.fn(async (_afterOffset, onFrame) => {
        handler = onFrame
        return vi.fn(async () => undefined)
      }),
    } as unknown as DaemonClient

    const { unmount } = renderHook(() => useTaskEvents(taskId, store), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <DaemonClientProvider client={client}>{children}</DaemonClientProvider>
      ),
    })
    await waitFor(() => expect(handler).toBeTypeOf('function'))

    act(() => {
      handler?.(eventBatch([event(42)], 40, 42))
      handler?.(eventBatch([event(41)], 40, 42))
      renderFrame?.(0)
    })

    expect(store.getState().lastAppliedOffset).toBe(41)
    expect(store.getState().pendingEvents.map((item) => item.globalOffset)).toEqual([42])

    unmount()
    resnapshot.resolve(snapshot(42, 'resynced'))
    animationFrame.mockRestore()
  })

  it('replays a gap resnapshot requested before the initial subscription resolves', async () => {
    const store = createTaskStore(taskId)
    let renderFrame: FrameRequestCallback | undefined
    const subscriptions: number[] = []
    const animationFrame = vi
      .spyOn(window, 'requestAnimationFrame')
      .mockImplementation((callback) => {
        renderFrame = callback
        return 1
      })
    const client = {
      connect: vi.fn(async () => taskListFrame()),
      loadTask: vi
        .fn<DaemonClient['loadTask']>()
        .mockResolvedValueOnce(snapshot(40, 'initial'))
        .mockResolvedValueOnce(snapshot(50, 'resynced')),
      subscribe: vi.fn(async (afterOffset, onFrame) => {
        subscriptions.push(afterOffset)
        if (afterOffset === 40) {
          onFrame(gapBatch(40, 50))
          renderFrame?.(0)
        }
        return vi.fn(async () => undefined)
      }),
    } as unknown as DaemonClient

    renderHook(() => useTaskEvents(taskId, store), {
      wrapper: ({ children }: { children: ReactNode }) => (
        <DaemonClientProvider client={client}>{children}</DaemonClientProvider>
      ),
    })

    await waitFor(() => expect(subscriptions).toEqual([40, 50]))
    expect(client.loadTask).toHaveBeenCalledTimes(2)
    expect(store.getState().lastAppliedOffset).toBe(50)
    animationFrame.mockRestore()
  })
})

function eventBatch(events: ReturnType<typeof event>[], afterOffset: number, latestOffset: number) {
  return {
    requestId: null,
    protocolVersion: 3,
    message: { afterOffset, events, gap: false, latestOffset, type: 'event_batch' as const },
  }
}

function gapBatch(afterOffset: number, latestOffset: number) {
  return {
    requestId: null,
    protocolVersion: 3,
    message: { afterOffset, events: [], gap: true, latestOffset, type: 'event_batch' as const },
  }
}

function event(globalOffset: number) {
  return {
    eventId: `000000000000000000000000${String(globalOffset).padStart(2, '0')}`,
    eventType: 'engine.assistant_text',
    globalOffset,
    payload: {},
    recordedAt: '2026-07-11T00:00:00Z',
    schemaVersion: 1,
    source: { kind: 'assistant' as const },
    streamSequence: globalOffset - 40,
    taskId,
  }
}

function snapshot(offset: number, title: string): TaskSnapshot {
  return {
    projection: {
      archived: false,
      lastGlobalOffset: offset,
      queue: [],
      state: 'idle',
      streamVersion: 1,
      taskId,
      title,
    },
    snapshotOffset: offset,
    timeline: [],
  }
}

function taskListFrame() {
  return {
    requestId: null,
    protocolVersion: 3,
    message: { type: 'task_list' as const, tasks: [] },
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((next) => {
    resolve = next
  })
  return { promise, resolve }
}
