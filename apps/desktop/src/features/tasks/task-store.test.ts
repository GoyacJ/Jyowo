import { describe, expect, it } from 'vitest'

import type { TaskEventEnvelope } from '@/generated/daemon-protocol'

import { createTaskStore, type TaskSnapshot } from './task-store'

const taskId = '00000000000000000000000001'
const otherTaskId = '00000000000000000000000002'

describe('task store global offset ordering', () => {
  it('applies contiguous offsets once, holds a gap, and replaces state from a snapshot', () => {
    const store = createTaskStore(taskId)
    store.getState().replaceSnapshot(snapshot(40, 'before gap'))

    const result = store.getState().ingestBatch({
      afterOffset: 40,
      latestOffset: 44,
      gap: false,
      events: [event(41), event(42), event(42), event(44)],
    })

    expect(result).toEqual({ resnapshotRequired: true })
    expect(store.getState().lastAppliedOffset).toBe(42)
    expect(store.getState().events.map((item) => item.globalOffset)).toEqual([41, 42])
    expect(store.getState().pendingEvents.map((item) => item.globalOffset)).toEqual([44])

    store.getState().replaceSnapshot(snapshot(50, 'authoritative'))

    expect(store.getState()).toMatchObject({
      connectionState: 'connected',
      events: [],
      lastAppliedOffset: 50,
      pendingEvents: [],
      snapshot: { projection: { title: 'authoritative' }, snapshotOffset: 50 },
    })
  })

  it('advances the global cursor across events owned by another task', () => {
    const store = createTaskStore(taskId)
    store.getState().replaceSnapshot(snapshot(40, 'same'))

    expect(store.getState().ingestBatch(batch(40, [event(41, otherTaskId), event(42)]))).toEqual({
      resnapshotRequired: false,
    })
    expect(store.getState().lastAppliedOffset).toBe(42)
    expect(store.getState().events.map((item) => item.globalOffset)).toEqual([42])
  })

  it('stays in resyncing while a held gap event awaits an authoritative snapshot', () => {
    const store = createTaskStore(taskId)
    store.getState().replaceSnapshot(snapshot(40, 'same'))
    store.getState().ingestBatch(batch(40, [event(42)]))

    expect(store.getState().ingestBatch(batch(40, [event(41)]))).toEqual({
      resnapshotRequired: true,
    })
    expect(store.getState().connectionState).toBe('resyncing')
    expect(store.getState().pendingEvents.map((item) => item.globalOffset)).toEqual([42])
  })

  it('applies held events that are contiguous after an authoritative snapshot', () => {
    const store = createTaskStore(taskId)
    store.getState().replaceSnapshot(snapshot(40, 'initial'))
    store.getState().ingestBatch(batch(40, [event(42)]))

    store.getState().replaceSnapshot(snapshot(41, 'authoritative'))

    expect(store.getState()).toMatchObject({
      connectionState: 'connected',
      lastAppliedOffset: 42,
      pendingEvents: [],
    })
    expect(store.getState().events.map((item) => item.globalOffset)).toEqual([42])
  })

  it('converges across browser windows with different batch boundaries', () => {
    const first = createTaskStore(taskId)
    const second = createTaskStore(taskId)
    first.getState().replaceSnapshot(snapshot(40, 'same'))
    second.getState().replaceSnapshot(snapshot(40, 'same'))

    first.getState().ingestBatch(batch(40, [event(41), event(42), event(43)]))
    second.getState().ingestBatch(batch(40, [event(41)]))
    second.getState().ingestBatch(batch(41, [event(42), event(43)]))

    expect(serializableState(first.getState())).toEqual(serializableState(second.getState()))
  })
})

function serializableState(state: ReturnType<ReturnType<typeof createTaskStore>['getState']>) {
  return {
    connectionState: state.connectionState,
    events: state.events,
    lastAppliedOffset: state.lastAppliedOffset,
    pendingEvents: state.pendingEvents,
    snapshot: state.snapshot,
  }
}

function batch(afterOffset: number, events: TaskEventEnvelope[]) {
  return {
    afterOffset,
    events,
    gap: false,
    latestOffset: events.at(-1)?.globalOffset ?? afterOffset,
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

function event(globalOffset: number, eventTaskId = taskId): TaskEventEnvelope {
  return {
    eventId: `000000000000000000000000${String(globalOffset).padStart(2, '0')}`,
    eventType: 'engine.assistant_text',
    globalOffset,
    payload: {},
    recordedAt: '2026-07-11T00:00:00Z',
    schemaVersion: 1,
    source: { kind: 'assistant' },
    streamSequence: globalOffset - 40,
    taskId: eventTaskId,
  }
}
