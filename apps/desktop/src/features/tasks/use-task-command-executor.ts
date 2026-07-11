import { useCallback, useEffect, useRef } from 'react'

import type {
  ClientRequest,
  CommandMetadata,
  ServerFrame,
  TypedUlid,
} from '@/generated/daemon-protocol'
import type { DaemonClient } from '@/shared/daemon/client'

import { createTaskCommandMetadata } from './task-command'

type TaskMutationRequest = Extract<ClientRequest, { metadata: CommandMetadata }>

export type TaskCommandExecutor = (
  operation: string,
  buildRequest: (metadata: CommandMetadata) => TaskMutationRequest,
) => Promise<ServerFrame>

export function useTaskCommandExecutor({
  client,
  expectedStreamVersion,
  onCommandAccepted,
  taskId,
}: {
  client: Pick<DaemonClient, 'request'> | null
  expectedStreamVersion: number
  onCommandAccepted?: (streamVersion: number) => void
  taskId: TypedUlid | null
}): TaskCommandExecutor | undefined {
  const clientRef = useRef(client)
  const currentTaskIdRef = useRef(taskId)
  const acceptedRef = useRef(onCommandAccepted)
  const versionsRef = useRef(new Map<TypedUlid, number>())
  const metadataRef = useRef(new Map<string, CommandMetadata>())
  const tailRef = useRef<Promise<void>>(Promise.resolve())

  clientRef.current = client
  currentTaskIdRef.current = taskId
  acceptedRef.current = onCommandAccepted

  useEffect(() => {
    if (!taskId) return
    const current = versionsRef.current.get(taskId) ?? 0
    versionsRef.current.set(taskId, Math.max(current, expectedStreamVersion))
  }, [expectedStreamVersion, taskId])

  const execute = useCallback<TaskCommandExecutor>((operation, buildRequest) => {
    const callTaskId = currentTaskIdRef.current
    const callClient = clientRef.current
    if (!callTaskId || !callClient) {
      return Promise.reject(new Error('task command client is unavailable'))
    }
    const key = `${callTaskId}\0${operation}`
    const invoke = async () => {
      let metadata = metadataRef.current.get(key)
      if (!metadata) {
        metadata = createTaskCommandMetadata(
          callTaskId,
          versionsRef.current.get(callTaskId) ?? 0,
          operation,
        )
        metadataRef.current.set(key, metadata)
      }

      const frame = await callClient.request(buildRequest(metadata))
      metadataRef.current.delete(key)
      const message = frame.message
      const nextVersion =
        message.type === 'command_accepted' && message.taskId === callTaskId
          ? message.streamVersion
          : message.type === 'command_rejected' && message.taskId === callTaskId
            ? message.currentStreamVersion
            : undefined
      if (typeof nextVersion === 'number') {
        const current = versionsRef.current.get(callTaskId) ?? 0
        versionsRef.current.set(callTaskId, Math.max(current, nextVersion))
        if (currentTaskIdRef.current === callTaskId) acceptedRef.current?.(nextVersion)
      }
      return frame
    }

    const result = tailRef.current.then(invoke, invoke)
    tailRef.current = result.then(
      () => undefined,
      () => undefined,
    )
    return result
  }, [])

  return client && taskId ? execute : undefined
}
