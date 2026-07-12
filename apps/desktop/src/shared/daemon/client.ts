import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { listen as tauriListen } from '@tauri-apps/api/event'

import type {
  ClientRequest,
  ServerFrame,
  ServerMessage,
  TypedUlid,
} from '@/generated/daemon-protocol'
import type { AttachmentReference, ListReferenceCandidatesResponse } from '@/shared/tauri/commands'

import { parseClientFrame, parseServerFrame } from './protocol'
import { createTaskCommandMetadata, requireAcceptedCommand } from './task-command'

const PROTOCOL_VERSION = 1
const DAEMON_EVENT_NAME = 'jyowo://daemon-events'

type BridgeOwnedRequest =
  | Extract<ClientRequest, { type: 'handshake' }>
  | Extract<ClientRequest, { type: 'read_blob' }>
  | Extract<ClientRequest, { type: 'subscribe_events' }>

type DaemonRequest = Exclude<ClientRequest, BridgeOwnedRequest>
export type TaskSnapshot = Omit<Extract<ServerMessage, { type: 'task_snapshot' }>, 'type'>
export type DaemonEventBatch = Extract<ServerMessage, { type: 'event_batch' }>
export type DaemonSubscriptionHandler = (frame: ServerFrame) => void

interface DaemonBlob {
  blobId: TypedUlid
  bytes: Uint8Array | null
  contentHash: number[]
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
  renameTask: (
    taskId: TypedUlid,
    expectedStreamVersion: number,
    title: string,
  ) => Promise<Extract<ServerMessage, { type: 'command_accepted' }>>
  setTaskPinned: (
    taskId: TypedUlid,
    expectedStreamVersion: number,
    pinned: boolean,
  ) => Promise<Extract<ServerMessage, { type: 'command_accepted' }>>
  setTaskArchived: (
    taskId: TypedUlid,
    expectedStreamVersion: number,
    archived: boolean,
  ) => Promise<Extract<ServerMessage, { type: 'command_accepted' }>>
  removeTask: (
    taskId: TypedUlid,
    expectedStreamVersion: number,
  ) => Promise<Extract<ServerMessage, { type: 'command_accepted' }>>
  listReferenceCandidates: (taskId: TypedUlid) => Promise<ListReferenceCandidatesResponse>
  readBlob: (blobId: TypedUlid) => Promise<DaemonBlob>
  stageBlobFromPath: (
    taskId: TypedUlid,
    path: string,
  ) => Promise<{ attachment: AttachmentReference }>
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
    async renameTask(taskId, expectedStreamVersion, title) {
      const frame = await request({
        metadata: createTaskCommandMetadata(taskId, expectedStreamVersion, 'rename'),
        taskId,
        title,
        type: 'rename_task',
      })
      return requireAcceptedCommand(frame, taskId)
    },
    async setTaskPinned(taskId, expectedStreamVersion, pinned) {
      const frame = await request({
        metadata: createTaskCommandMetadata(taskId, expectedStreamVersion, 'pin'),
        pinned,
        taskId,
        type: 'set_task_pinned',
      })
      return requireAcceptedCommand(frame, taskId)
    },
    async setTaskArchived(taskId, expectedStreamVersion, archived) {
      const frame = await request({
        archived,
        metadata: createTaskCommandMetadata(taskId, expectedStreamVersion, 'archive'),
        taskId,
        type: 'set_task_archived',
      })
      return requireAcceptedCommand(frame, taskId)
    },
    async removeTask(taskId, expectedStreamVersion) {
      const frame = await request({
        metadata: createTaskCommandMetadata(taskId, expectedStreamVersion, 'remove'),
        taskId,
        type: 'remove_task',
      })
      return requireAcceptedCommand(frame, taskId)
    },
    async listReferenceCandidates(taskId) {
      return parseReferenceCandidates(
        await transport.invoke('daemon_list_reference_candidates', { taskId }),
      )
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
        contentHash: frame.message.contentHash,
        mediaType: frame.message.mediaType,
        missing: frame.message.missing,
        size: frame.message.size,
      }
    },
    async stageBlobFromPath(taskId, path) {
      const frame = parseServerFrame(
        await transport.invoke('daemon_stage_blob_from_path', { path, taskId }),
      )
      if (frame.message.type === 'error') {
        throw new DaemonResponseError(frame.message.code, frame.message.message)
      }
      if (frame.message.type !== 'blob') {
        throw new Error(`Expected blob, received ${frame.message.type}`)
      }
      if (frame.message.missing || frame.message.base64Data != null) {
        throw new Error('Daemon returned an invalid staged blob')
      }
      const name = path.split(/[\\/]/).pop()?.trim() || 'attachment'
      const contentHash = frame.message.contentHash
      return {
        attachment: {
          blobRef: {
            contentHash,
            contentType: frame.message.mediaType,
            id: frame.message.blobId,
            size: frame.message.size,
          },
          id: `attachment-${contentHash.map(hexByte).join('')}`,
          mimeType: frame.message.mediaType,
          name,
          sizeBytes: frame.message.size,
        },
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

class DaemonResponseError extends Error {
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

function hexByte(byte: number) {
  return byte.toString(16).padStart(2, '0')
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

function parseReferenceCandidates(value: unknown): ListReferenceCandidatesResponse {
  if (!isRecord(value)) throw new Error('Invalid task reference candidates')
  const keys = [
    'artifacts',
    'conversations',
    'files',
    'memories',
    'mcpServers',
    'skills',
    'tools',
  ] as const
  if (Object.keys(value).length !== keys.length || keys.some((key) => !Object.hasOwn(value, key))) {
    throw new Error('Invalid task reference candidate categories')
  }
  const parsed = Object.fromEntries(keys.map((key) => [key, parseCandidateList(value[key])]))
  return parsed as unknown as ListReferenceCandidatesResponse
}

function parseCandidateList(value: unknown) {
  if (!Array.isArray(value)) throw new Error('Invalid task reference candidate list')
  return value.map((candidate) => {
    if (!isRecord(candidate)) throw new Error('Invalid task reference candidate')
    const keys = Object.keys(candidate)
    if (
      keys.some((key) => !['id', 'label', 'path'].includes(key)) ||
      typeof candidate.label !== 'string' ||
      candidate.label.length === 0 ||
      (candidate.id !== undefined &&
        (typeof candidate.id !== 'string' || candidate.id.length === 0)) ||
      (candidate.path !== undefined &&
        (typeof candidate.path !== 'string' || !isSafeRelativeReferencePath(candidate.path)))
    ) {
      throw new Error('Invalid task reference candidate')
    }
    return {
      ...(candidate.id === undefined ? {} : { id: candidate.id }),
      label: candidate.label,
      ...(candidate.path === undefined ? {} : { path: candidate.path }),
    }
  })
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isSafeRelativeReferencePath(path: string) {
  if (
    path.length === 0 ||
    path.includes('\0') ||
    path.startsWith('/') ||
    path.startsWith('\\') ||
    /^[a-zA-Z]:[\\/]/.test(path)
  ) {
    return false
  }
  return !path.split(/[\\/]/).some((part) => part === '..')
}
