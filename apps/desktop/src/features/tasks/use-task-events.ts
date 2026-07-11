import { useEffect } from 'react'

import type { ServerFrame, TypedUlid } from '@/generated/daemon-protocol'
import { useDaemonClient } from '@/shared/tauri/react'

import type { TaskStore } from './task-store'

export function useTaskEvents(taskId: TypedUlid, store: TaskStore) {
  const client = useDaemonClient()

  useEffect(() => {
    let cancelled = false
    let frameHandle: number | null = null
    let queuedFrames: ServerFrame[] = []
    let resyncing = false
    let resnapshotRequested = false
    let protocolFailed = false
    let unsubscribe: (() => Promise<void>) | undefined

    const protocolError = (error: Error) => {
      if (cancelled || protocolFailed) return
      protocolFailed = true
      queuedFrames = []
      if (frameHandle !== null) {
        cancelAnimationFrameSafe(frameHandle)
        frameHandle = null
      }
      store.getState().setConnectionState('protocol_error', error)
    }

    const subscribeFrom = async (offset: number) => {
      const nextUnsubscribe = await client.subscribe(offset, handleFrame, protocolError)
      if (cancelled || protocolFailed) {
        await nextUnsubscribe()
        return
      }
      unsubscribe = nextUnsubscribe
    }

    const loadSnapshot = async (connect: boolean) => {
      if (cancelled || protocolFailed) return
      if (resyncing) {
        if (!connect) resnapshotRequested = true
        return
      }
      resyncing = true
      try {
        if (connect) {
          store.getState().setConnectionState('connecting')
          await client.connect()
        } else {
          store.getState().setConnectionState('resyncing')
        }
        const snapshot = await client.loadTask(taskId)
        if (cancelled || protocolFailed) return
        store.getState().replaceSnapshot(snapshot)
        if (unsubscribe) {
          const previous = unsubscribe
          unsubscribe = undefined
          await previous()
        }
        await subscribeFrom(snapshot.snapshotOffset)
      } catch (error) {
        if (!cancelled && !protocolFailed) {
          store.getState().setConnectionState('disconnected', asError(error))
        }
      } finally {
        resyncing = false
        if (resnapshotRequested && !cancelled && !protocolFailed) {
          resnapshotRequested = false
          void loadSnapshot(false)
        }
      }
    }

    function handleFrame(frame: ServerFrame) {
      if (protocolFailed) return
      if (frame.message.type !== 'event_batch') {
        protocolError(new Error(`Expected event_batch, received ${frame.message.type}`))
        return
      }
      queuedFrames.push(frame)
      if (frameHandle === null) frameHandle = requestAnimationFrameSafe(flushFrames)
    }

    function flushFrames() {
      frameHandle = null
      if (protocolFailed) {
        queuedFrames = []
        return
      }
      const frames = queuedFrames
      queuedFrames = []
      let resnapshotRequired = false
      for (const frame of frames) {
        if (frame.message.type !== 'event_batch') continue
        const result = store.getState().ingestBatch(frame.message)
        resnapshotRequired ||= result.resnapshotRequired
      }
      if (resnapshotRequired) void loadSnapshot(false)
    }

    void loadSnapshot(true)

    return () => {
      cancelled = true
      if (frameHandle !== null && !protocolFailed) {
        cancelAnimationFrameSafe(frameHandle)
        frameHandle = null
        flushFrames()
      }
      void unsubscribe?.()
    }
  }, [client, store, taskId])
}

function requestAnimationFrameSafe(callback: FrameRequestCallback) {
  if (typeof window.requestAnimationFrame === 'function') {
    return window.requestAnimationFrame(callback)
  }
  return window.setTimeout(() => callback(performance.now()), 16)
}

function cancelAnimationFrameSafe(handle: number) {
  if (typeof window.cancelAnimationFrame === 'function') {
    window.cancelAnimationFrame(handle)
  } else {
    window.clearTimeout(handle)
  }
}

function asError(error: unknown) {
  return error instanceof Error ? error : new Error(String(error))
}
