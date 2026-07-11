import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { listen as tauriListen } from '@tauri-apps/api/event'

import type {
  ClientRequest,
  ServerFrame,
  ServerMessage,
  TypedUlid,
} from '@/generated/daemon-protocol'

import { parseClientFrame, parseServerFrame } from './protocol'

const PROTOCOL_VERSION = 1
const DAEMON_EVENT_NAME = 'jyowo://daemon-events'

type BridgeOwnedRequest =
  | Extract<ClientRequest, { type: 'handshake' }>
  | Extract<ClientRequest, { type: 'read_blob' }>
  | Extract<ClientRequest, { type: 'subscribe_events' }>

export type DaemonRequest = Exclude<ClientRequest, BridgeOwnedRequest>
export type TaskSnapshot = Omit<Extract<ServerMessage, { type: 'task_snapshot' }>, 'type'>
export type DaemonEventBatch = Extract<ServerMessage, { type: 'event_batch' }>
export type DaemonSubscriptionHandler = (frame: ServerFrame) => void

export interface DaemonBlob {
  blobId: TypedUlid
  bytes: Uint8Array | null
  mediaType: string
  missing: boolean
  size: number
}

export interface DaemonTransport {
  invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
  listen: (event: string, handler: (payload: unknown) => void) => Promise<() => void>
}

export interface DaemonClient {
  connect: () => Promise<ServerFrame>
  request: (request: DaemonRequest) => Promise<ServerFrame>
  loadTask: (taskId: TypedUlid) => Promise<TaskSnapshot>
  listTasks: () => Promise<Extract<ServerMessage, { type: 'task_list' }>>
  readBlob: (blobId: TypedUlid) => Promise<DaemonBlob>
  subscribe: (
    afterOffset: number,
    onFrame: DaemonSubscriptionHandler,
    onProtocolError?: (error: Error) => void,
  ) => Promise<() => Promise<void>>
}

export function createDaemonClient(
  transport: DaemonTransport,
  options: { requestId?: () => string } = {},
): DaemonClient {
  const nextRequestId = options.requestId ?? defaultRequestId

  async function request(request: DaemonRequest) {
    const frame = parseClientFrame({
      protocolVersion: PROTOCOL_VERSION,
      request,
      requestId: nextRequestId(),
    })
    return parseServerFrame(await transport.invoke('daemon_request', { frame }))
  }

  return {
    async connect() {
      return parseServerFrame(await transport.invoke('daemon_connect'))
    },
    request,
    async loadTask(taskId) {
      const frame = await request({ taskId, type: 'load_task' })
      if (frame.message.type === 'error') {
        throw new DaemonResponseError(frame.message.code, frame.message.message)
      }
      if (frame.message.type !== 'task_snapshot') {
        throw new Error(`Expected task_snapshot, received ${frame.message.type}`)
      }
      if (frame.message.projection.taskId !== taskId) {
        throw new Error('Daemon returned a snapshot for another task')
      }
      const { projection, snapshotOffset, timeline } = frame.message
      return { projection, snapshotOffset, timeline }
    },
    async listTasks() {
      const frame = await request({ type: 'list_tasks' })
      if (frame.message.type === 'error') {
        throw new DaemonResponseError(frame.message.code, frame.message.message)
      }
      if (frame.message.type !== 'task_list') {
        throw new Error(`Expected task_list, received ${frame.message.type}`)
      }
      return frame.message
    },
    async readBlob(blobId) {
      parseClientFrame({
        protocolVersion: PROTOCOL_VERSION,
        request: { blobId, type: 'read_blob' },
        requestId: nextRequestId(),
      })
      const frame = parseServerFrame(await transport.invoke('daemon_read_blob', { blobId }))
      if (frame.message.type === 'error') {
        throw new DaemonResponseError(frame.message.code, frame.message.message)
      }
      if (frame.message.type !== 'blob') {
        throw new Error(`Expected blob, received ${frame.message.type}`)
      }
      if (frame.message.blobId !== blobId) {
        throw new Error('Daemon returned another blob')
      }
      return {
        blobId: frame.message.blobId,
        bytes: decodeBlobBytes(frame.message),
        mediaType: frame.message.mediaType,
        missing: frame.message.missing,
        size: frame.message.size,
      }
    },
    async subscribe(afterOffset, onFrame, onProtocolError = () => undefined) {
      const subscriptionId = nextRequestId()
      parseClientFrame({
        protocolVersion: PROTOCOL_VERSION,
        request: { afterOffset, type: 'subscribe_events' },
        requestId: subscriptionId,
      })
      let active = true
      let listenerClosed = false
      let registered = false
      let unsubscribed = false
      const eventName = `${DAEMON_EVENT_NAME}/${subscriptionId}`
      const unlisten = await transport.listen(eventName, (payload) => {
        if (!active) return
        try {
          const frame = parseServerFrame(payload)
          if (frame.message.type !== 'event_batch') {
            throw new Error(`Expected event_batch, received ${frame.message.type}`)
          }
          onFrame(frame)
        } catch (error) {
          active = false
          onProtocolError(asError(error))
          void close()
        }
      })

      async function close() {
        active = false
        if (!listenerClosed) {
          listenerClosed = true
          unlisten()
        }
        if (registered && !unsubscribed) {
          unsubscribed = true
          await transport.invoke('daemon_unsubscribe', { subscriptionId })
        }
      }

      try {
        const value = await transport.invoke('daemon_subscribe', { afterOffset, subscriptionId })
        if (value !== subscriptionId) {
          throw new Error('Invalid daemon subscription id')
        }
        registered = true
        if (!active) await close()
      } catch (error) {
        await close()
        throw error
      }

      return close
    },
  }
}

export class DaemonResponseError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message)
    this.name = 'DaemonResponseError'
  }
}

const tauriTransport: DaemonTransport = {
  invoke: tauriInvoke,
  async listen(event, handler) {
    return tauriListen<unknown>(event, (message) => handler(message.payload))
  },
}

export const tauriDaemonClient = createDaemonClient(tauriTransport)

let fallbackRequestSequence = 0

function defaultRequestId() {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  fallbackRequestSequence += 1
  return `desktop-${Date.now()}-${fallbackRequestSequence}`
}

function asError(error: unknown) {
  return error instanceof Error ? error : new Error(String(error))
}

function decodeBlobBytes(message: Extract<ServerMessage, { type: 'blob' }>) {
  if (message.missing) {
    if (message.base64Data != null) throw new Error('Missing daemon blob included data')
    return null
  }
  if (message.base64Data == null) throw new Error('Daemon blob data is missing')
  const binary = atob(message.base64Data)
  const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0))
  if (bytes.byteLength !== message.size) throw new Error('Daemon blob size mismatch')
  return bytes
}
