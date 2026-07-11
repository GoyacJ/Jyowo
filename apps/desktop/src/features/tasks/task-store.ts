import { createStore, type StoreApi } from 'zustand/vanilla'

import type { TaskEventEnvelope, TypedUlid } from '@/generated/daemon-protocol'
import type { DaemonEventBatch, TaskSnapshot as DaemonTaskSnapshot } from '@/shared/daemon/client'

const MAX_PENDING_EVENTS = 1_000

export type TaskConnectionState =
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'resyncing'
  | 'protocol_error'

export type TaskSnapshot = DaemonTaskSnapshot

interface TaskStoreState {
  taskId: TypedUlid
  snapshot: TaskSnapshot | null
  events: TaskEventEnvelope[]
  pendingEvents: TaskEventEnvelope[]
  lastAppliedOffset: number
  connectionState: TaskConnectionState
  connectionError: string | null
  ingestBatch: (batch: Omit<DaemonEventBatch, 'type'> | DaemonEventBatch) => {
    resnapshotRequired: boolean
  }
  replaceSnapshot: (snapshot: TaskSnapshot) => void
  setConnectionState: (state: TaskConnectionState, error?: Error) => void
}

export type TaskStore = StoreApi<TaskStoreState>

export function createTaskStore(taskId: TypedUlid): TaskStore {
  return createStore<TaskStoreState>()((set, get) => ({
    connectionError: null,
    connectionState: 'disconnected',
    events: [],
    lastAppliedOffset: 0,
    pendingEvents: [],
    snapshot: null,
    taskId,
    ingestBatch: (batch) => {
      const current = get()
      let lastAppliedOffset = current.lastAppliedOffset
      let events = current.events
      let pendingEvents = current.pendingEvents
      let resnapshotRequired =
        batch.gap || current.connectionState === 'resyncing' || current.pendingEvents.length > 0

      for (const event of batch.events) {
        if (event.globalOffset <= lastAppliedOffset) continue
        if (event.globalOffset !== lastAppliedOffset + 1) {
          pendingEvents = addPendingEvent(pendingEvents, event)
          resnapshotRequired = true
          continue
        }
        lastAppliedOffset = event.globalOffset
        if (event.taskId === taskId) events = [...events, event]
      }

      set({
        connectionError: null,
        connectionState: resnapshotRequired ? 'resyncing' : 'connected',
        events,
        lastAppliedOffset,
        pendingEvents,
      })
      return { resnapshotRequired }
    },
    replaceSnapshot: (snapshot) => {
      const retainedEvents = get().pendingEvents.filter(
        (event) => event.globalOffset > snapshot.snapshotOffset,
      )
      const events: TaskEventEnvelope[] = []
      const pendingEvents: TaskEventEnvelope[] = []
      let lastAppliedOffset = snapshot.snapshotOffset
      let gapFound = false
      for (const event of retainedEvents) {
        if (!gapFound && event.globalOffset === lastAppliedOffset + 1) {
          lastAppliedOffset = event.globalOffset
          if (event.taskId === taskId) events.push(event)
        } else {
          gapFound = true
          pendingEvents.push(event)
        }
      }
      set({
        connectionError: null,
        connectionState: pendingEvents.length === 0 ? 'connected' : 'resyncing',
        events,
        lastAppliedOffset,
        pendingEvents,
        snapshot,
      })
    },
    setConnectionState: (connectionState, error) =>
      set({ connectionError: error?.message ?? null, connectionState }),
  }))
}

function addPendingEvent(events: TaskEventEnvelope[], event: TaskEventEnvelope) {
  if (events.some((candidate) => candidate.globalOffset === event.globalOffset)) return events
  return [...events, event]
    .sort((left, right) => left.globalOffset - right.globalOffset)
    .slice(0, MAX_PENDING_EVENTS)
}
