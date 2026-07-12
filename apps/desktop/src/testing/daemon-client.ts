import type {
  ClientFrame,
  ServerFrame,
  TaskEventEnvelope,
  TaskProjection,
  TimelineItemProjection,
} from '@/generated/daemon-protocol'
import {
  createDaemonClient,
  type DaemonClient,
  type DaemonTransport,
  type TaskSnapshot,
} from '@/shared/daemon/client'

const taskId = '01J00000000000000000000051'
const blobId = '01J00000000000000000000052'
const eventId = '01J00000000000000000000053'
const queueItemId = '01J00000000000000000000054'
const segmentId = '01J00000000000000000000055'
const runId = '01J00000000000000000000056'
const messageId = '01J00000000000000000000057'
const sessionId = '01J00000000000000000000058'
const correlationId = '01J00000000000000000000059'
const stateKey = 'jyowo-e2e-daemon-recovery-state'
const telemetryKey = 'jyowo-e2e-daemon-recovery-telemetry'

type FixtureState = {
  generation: number
  messagePublished: boolean
  published: boolean
  recovered: boolean
  replyPrompt: string | null
  running: boolean
}

type FixtureTelemetry = {
  blobReads: Array<{ blobId: string; generation: number }>
  deliveries: Array<{ afterOffset: number; generation: number; offsets: number[] }>
  subscriptions: Array<{ afterOffset: number; generation: number }>
}

const initialState: FixtureState = {
  generation: 0,
  messagePublished: false,
  published: false,
  recovered: false,
  replyPrompt: null,
  running: true,
}
const initialTelemetry: FixtureTelemetry = { blobReads: [], deliveries: [], subscriptions: [] }

export function createE2eDaemonClient(): DaemonClient {
  exposeControls()
  return createDaemonClient(createE2eDaemonTransport())
}

function createE2eDaemonTransport(): DaemonTransport {
  const listeners = new Map<string, (payload: unknown) => void>()
  const subscriptions = new Map<string, number>()

  return {
    async invoke(command, args) {
      const state = requireRunning()
      if (command === 'daemon_connect') {
        return frame({
          daemonVersion: 'e2e-production-protocol',
          latestGlobalOffset: latestOffset(state),
          type: 'handshake',
          userInstanceId: 'e2e-production-protocol',
        })
      }
      if (command === 'daemon_request') {
        return handleRequest((args?.frame as ClientFrame).request, publishMessage)
      }
      if (command === 'daemon_subscribe') {
        const subscriptionId = String(args?.subscriptionId)
        const afterOffset = Number(args?.afterOffset)
        subscriptions.set(subscriptionId, afterOffset)
        appendTelemetry('subscriptions', { afterOffset, generation: state.generation })
        const replay = replayEvents(state, afterOffset)
        if (replay.length > 0) {
          queueMicrotask(() => deliver(subscriptionId, afterOffset, replay))
        }
        return subscriptionId
      }
      if (command === 'daemon_unsubscribe') {
        subscriptions.delete(String(args?.subscriptionId))
        return null
      }
      if (command === 'daemon_read_blob') {
        const requestedBlobId = String(args?.blobId)
        appendTelemetry('blobReads', { blobId: requestedBlobId, generation: state.generation })
        if (requestedBlobId !== blobId) return errorFrame('not_found', 'blob not found')
        const text = 'renderer bridge output loaded by blob id'
        return frame({
          base64Data: btoa(text),
          blobId,
          contentHash: [
            7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
            7, 7, 7,
          ],
          mediaType: 'text/plain',
          missing: false,
          size: new TextEncoder().encode(text).byteLength,
          type: 'blob',
        })
      }
      if (command === 'daemon_list_reference_candidates') {
        return {
          artifacts: [],
          conversations: [],
          files: [],
          memories: [],
          mcpServers: [],
          skills: [],
          tools: [],
        }
      }
      return errorFrame('invalid_frame', `unsupported fixture command: ${command}`)
    },
    async listen(event, handler) {
      listeners.set(event, handler)
      return () => {
        listeners.delete(event)
      }
    },
  }

  function publishMessage(prompt: string) {
    const state = requireRunning()
    const events = messageEvents(prompt)
    writeState({ ...state, messagePublished: true, replyPrompt: prompt })
    window.setTimeout(() => {
      for (const [subscriptionId, afterOffset] of subscriptions) {
        deliver(
          subscriptionId,
          afterOffset,
          events.filter((event) => event.globalOffset > afterOffset),
        )
      }
    }, 0)
  }

  function deliver(subscriptionId: string, afterOffset: number, events: TaskEventEnvelope[]) {
    if (events.length === 0 || !readState().running) return
    const handler = listeners.get(`jyowo://daemon-events/${subscriptionId}`)
    if (!handler) return
    appendTelemetry('deliveries', {
      afterOffset,
      generation: readState().generation,
      offsets: events.map((event) => event.globalOffset),
    })
    handler(
      frame({
        afterOffset,
        events,
        gap: false,
        latestOffset: events.at(-1)?.globalOffset ?? afterOffset,
        type: 'event_batch',
      }),
    )
  }
}

function handleRequest(
  request: ClientFrame['request'],
  publishMessage: (prompt: string) => void,
): ServerFrame {
  const state = requireRunning()
  if (request.type === 'list_tasks') {
    return frame({ tasks: [projection(state)], type: 'task_list' })
  }
  if (request.type === 'load_task') {
    if (request.taskId !== taskId) return errorFrame('not_found', 'task not found')
    return frame({ ...snapshot(state), type: 'task_snapshot' })
  }
  if (request.type === 'submit_message') {
    if (request.taskId !== taskId) return errorFrame('not_found', 'task not found')
    publishMessage(request.content)
    return frame({
      commandId: request.metadata.commandId,
      committedOffset: 3,
      streamVersion: 3,
      taskId,
      type: 'command_accepted',
    })
  }
  return errorFrame('invalid_frame', `unsupported fixture request: ${request.type}`)
}

function exposeControls() {
  Reflect.set(window, '__JYOWO_E2E_DAEMON__', {
    restart() {
      const current = readState()
      writeState({
        ...current,
        generation: current.generation + 1,
        published: true,
        recovered: true,
        running: true,
      })
    },
    stop() {
      writeState({ ...readState(), running: false })
    },
    telemetry: readTelemetry,
  })
}

function snapshot(state: FixtureState): TaskSnapshot {
  const recovered = state.recovered || state.published
  return {
    projection: projection(state),
    snapshotOffset: recovered ? 3 : 2,
    timeline: recovered ? [...baseTimeline, committedTimelineItem] : baseTimeline,
  }
}

function projection(state: FixtureState): TaskProjection {
  return {
    archived: false,
    lastGlobalOffset: state.messagePublished ? 2 : latestOffset(state),
    queue: [],
    state: 'completed',
    streamVersion: state.messagePublished ? 2 : latestOffset(state),
    taskId,
    title: state.recovered ? 'Daemon recovery after restart' : 'Daemon recovery before restart',
  }
}

const baseTimeline: TimelineItemProjection[] = [
  {
    blobId,
    globalOffset: 2,
    id: 'fixture-diff',
    incomplete: false,
    kind: 'diff',
    summary: 'Renderer bridge recovery evidence',
  },
]

const committedTimelineItem: TimelineItemProjection = {
  globalOffset: 3,
  id: eventId,
  incomplete: false,
  kind: 'notice',
  summary: 'Committed event delivered once',
}

const committedEvent: TaskEventEnvelope = engineEvent(3, 'assistant_notice', {
  body: 'Committed event delivered once',
})

function messageEvents(prompt: string): TaskEventEnvelope[] {
  return [
    taskEvent(3, 'message.queued', {
      attachments: [],
      content: prompt,
      contextReferences: [],
      createdAt: '2026-07-12T02:00:00Z',
      queueItemId,
    }),
    taskEvent(4, 'message.consumed', { queueItemId, revision: 1, runSegmentId: segmentId }),
    taskEvent(5, 'run.started', { segmentId, startedAt: '2026-07-12T02:00:01Z' }),
    engineEvent(6, 'run_started', { at: '2026-07-12T02:00:01Z', run_id: runId }),
    engineEvent(7, 'assistant_delta_produced', {
      at: '2026-07-12T02:00:02Z',
      delta: { text: 'Protocol reply ' },
      message_id: messageId,
      run_id: runId,
    }),
    engineEvent(8, 'assistant_delta_produced', {
      at: '2026-07-12T02:00:03Z',
      delta: { text: 'is visible once.' },
      message_id: messageId,
      run_id: runId,
    }),
    engineEvent(9, 'assistant_message_completed', {
      at: '2026-07-12T02:00:04Z',
      content: { text: 'Protocol reply is visible once.' },
      message_id: messageId,
      pricing_snapshot_id: null,
      run_id: runId,
      stop_reason: 'end_turn',
      tool_uses: [],
      usage: usage(),
    }),
    engineEvent(10, 'run_ended', {
      ended_at: '2026-07-12T02:00:05Z',
      reason: 'completed',
      run_id: runId,
      usage: usage(),
    }),
    taskEvent(11, 'run.completed', {
      endedAt: '2026-07-12T02:00:05Z',
      incompleteOutput: false,
      segmentId,
      terminalReason: 'completed',
    }),
  ]
}

function taskEvent(globalOffset: number, eventType: string, payload: unknown): TaskEventEnvelope {
  return {
    eventId: `01J00000000000000000000${String(globalOffset).padStart(3, '0')}`,
    eventType,
    globalOffset,
    payload,
    recordedAt: '2026-07-12T02:00:00Z',
    schemaVersion: 1,
    source: { kind: 'supervisor' },
    streamSequence: globalOffset,
    taskId,
  }
}

function engineEvent(
  globalOffset: number,
  type: string,
  event: Record<string, unknown>,
): TaskEventEnvelope {
  return {
    ...taskEvent(globalOffset, `engine.${type}`, {
      causationId: null,
      correlationId,
      event: { type, ...event },
      journalOffset: globalOffset - 6,
      runId,
      sessionId,
      tenantId: '00000000000000000000000000',
    }),
    source: { kind: 'engine' },
  }
}

function replayEvents(state: FixtureState, afterOffset: number) {
  if (state.messagePublished && state.replyPrompt) {
    return messageEvents(state.replyPrompt).filter((event) => event.globalOffset > afterOffset)
  }
  if (state.recovered || state.published || afterOffset >= 3) return []
  return [committedEvent]
}

function usage() {
  return {
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    cost_micros: 0,
    input_tokens: 1,
    output_tokens: 1,
    tool_calls: 0,
  }
}

function frame(message: ServerFrame['message']): ServerFrame {
  return { message, protocolVersion: 1 }
}

function errorFrame(code: 'invalid_frame' | 'not_found', message: string) {
  return frame({ code, message, type: 'error' })
}

function requireRunning() {
  const state = readState()
  if (!state.running) throw new Error('E2E daemon fixture is stopped')
  return state
}

function latestOffset(state: FixtureState) {
  if (state.messagePublished) return 11
  return state.recovered || state.published ? 3 : 2
}

function readState(): FixtureState {
  return readJson(stateKey, initialState)
}

function writeState(state: FixtureState) {
  localStorage.setItem(stateKey, JSON.stringify(state))
}

function readTelemetry(): FixtureTelemetry {
  return readJson(telemetryKey, initialTelemetry)
}

function appendTelemetry<Key extends keyof FixtureTelemetry>(
  key: Key,
  value: FixtureTelemetry[Key][number],
) {
  const telemetry = readTelemetry()
  telemetry[key].push(value as never)
  localStorage.setItem(telemetryKey, JSON.stringify(telemetry))
}

function readJson<Value>(key: string, fallback: Value): Value {
  const value = localStorage.getItem(key)
  if (!value) return structuredClone(fallback)
  try {
    return JSON.parse(value) as Value
  } catch {
    return structuredClone(fallback)
  }
}
