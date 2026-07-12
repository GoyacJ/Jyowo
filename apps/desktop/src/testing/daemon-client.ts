import type {
  ServerFrame,
  TaskEventEnvelope,
  TaskProjection,
  TimelineItemProjection,
} from '@/generated/daemon-protocol'
import type { DaemonClient, TaskSnapshot } from '@/shared/daemon/client'

const taskId = '01J00000000000000000000051'
const blobId = '01J00000000000000000000052'
const eventId = '01J00000000000000000000053'
const stateKey = 'jyowo-e2e-daemon-recovery-state'
const telemetryKey = 'jyowo-e2e-daemon-recovery-telemetry'

type FixtureState = {
  generation: number
  published: boolean
  recovered: boolean
  running: boolean
}

type FixtureTelemetry = {
  blobReads: Array<{ blobId: string; generation: number }>
  deliveries: Array<{ afterOffset: number; generation: number; offsets: number[] }>
  subscriptions: Array<{ afterOffset: number; generation: number }>
}

const initialState: FixtureState = {
  generation: 0,
  published: false,
  recovered: false,
  running: true,
}
const initialTelemetry: FixtureTelemetry = { blobReads: [], deliveries: [], subscriptions: [] }

export function createE2eDaemonClient(): DaemonClient {
  exposeControls()

  const requireRunning = () => {
    const state = readState()
    if (!state.running) throw new Error('E2E daemon fixture is stopped')
    return state
  }

  return {
    async connect() {
      const state = requireRunning()
      return frame({
        agentCapabilities: {
          agentTeams: true,
          backgroundAgents: true,
          subagents: true,
        },
        daemonVersion: 'e2e-renderer-bridge',
        latestGlobalOffset: latestOffset(state),
        type: 'handshake',
        userInstanceId: 'e2e-renderer-bridge',
      })
    },
    async listTasks() {
      const state = requireRunning()
      return { tasks: [projection(state)], type: 'task_list' }
    },
    async loadTask(requestedTaskId) {
      const state = requireRunning()
      if (requestedTaskId !== taskId) throw new Error('task not found')
      return snapshot(state)
    },
    async listReferenceCandidates(requestedTaskId) {
      requireRunning()
      if (requestedTaskId !== taskId) throw new Error('task not found')
      return {
        artifacts: [],
        conversations: [],
        files: [],
        memories: [],
        mcpServers: [],
        skills: [],
        tools: [],
      }
    },
    async readBlob(requestedBlobId) {
      const state = requireRunning()
      appendTelemetry('blobReads', { blobId: requestedBlobId, generation: state.generation })
      if (requestedBlobId !== blobId) throw new Error('blob not found')
      const bytes = new TextEncoder().encode('renderer bridge output loaded by blob id')
      return {
        blobId,
        bytes,
        contentHash: Array.from({ length: 32 }, () => 7),
        mediaType: 'text/plain',
        missing: false,
        size: bytes.byteLength,
      }
    },
    async request() {
      requireRunning()
      return frame({ code: 'invalid_frame', message: 'not used by recovery E2E', type: 'error' })
    },
    async renameTask() {
      throw new Error('task metadata is outside the renderer recovery fixture')
    },
    async setTaskPinned() {
      throw new Error('task metadata is outside the renderer recovery fixture')
    },
    async setTaskArchived() {
      throw new Error('task metadata is outside the renderer recovery fixture')
    },
    async removeTask() {
      throw new Error('task metadata is outside the renderer recovery fixture')
    },
    async stageBlobFromPath() {
      requireRunning()
      throw new Error('path staging is outside the renderer recovery fixture')
    },
    async subscribe(afterOffset, onFrame) {
      const state = requireRunning()
      appendTelemetry('subscriptions', { afterOffset, generation: state.generation })
      let active = true
      const events = state.recovered || state.published || afterOffset >= 3 ? [] : [committedEvent]
      if (events.length > 0) {
        queueMicrotask(() => {
          if (!active || !readState().running) return
          writeState({ ...readState(), published: true })
          appendTelemetry('deliveries', {
            afterOffset,
            generation: state.generation,
            offsets: events.map((event) => event.globalOffset),
          })
          onFrame(
            frame({
              afterOffset,
              events,
              gap: false,
              latestOffset: 3,
              type: 'event_batch',
            }),
          )
        })
      }
      return async () => {
        active = false
      }
    },
  }
}

function exposeControls() {
  Reflect.set(window, '__JYOWO_E2E_DAEMON__', {
    restart() {
      const current = readState()
      writeState({
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
    lastGlobalOffset: latestOffset(state),
    queue: [],
    state: 'completed',
    streamVersion: latestOffset(state),
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

const committedEvent: TaskEventEnvelope = {
  eventId,
  eventType: 'engine.committed_event_delivered_once',
  globalOffset: 3,
  payload: {},
  recordedAt: '2026-07-11T10:00:00Z',
  schemaVersion: 1,
  source: { kind: 'recovery' },
  streamSequence: 3,
  taskId,
}

function frame(message: ServerFrame['message']): ServerFrame {
  return { message, protocolVersion: 1 }
}

function latestOffset(state: FixtureState) {
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
