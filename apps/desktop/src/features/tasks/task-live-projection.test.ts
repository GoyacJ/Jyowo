import { describe, expect, it } from 'vitest'

import type { TaskEventEnvelope } from '@/generated/daemon-protocol'
import { deriveLiveTaskSnapshot } from './task-live-projection'
import type { TaskSnapshot } from './task-store'

describe('deriveLiveTaskSnapshot', () => {
  it('projects real engine stream envelopes and live run completion without internal notices', () => {
    const messageId = id(40)
    const segmentId = id(30)
    const events = [
      taskEvent(3, 'run.started', { segmentId, startedAt: '2026-07-12T01:00:00Z' }),
      engineEvent(4, 'assistant_delta_produced', {
        at: '2026-07-12T01:00:01Z',
        delta: { text: 'First ' },
        message_id: messageId,
        run_id: id(31),
      }),
      engineEvent(5, 'assistant_delta_produced', {
        at: '2026-07-12T01:00:02Z',
        delta: { text: 'answer' },
        message_id: messageId,
        run_id: id(31),
      }),
      engineEvent(6, 'assistant_message_completed', {
        at: '2026-07-12T01:00:03Z',
        content: { text: 'First answer' },
        message_id: messageId,
        pricing_snapshot_id: null,
        run_id: id(31),
        stop_reason: 'end_turn',
        tool_uses: [],
        usage: usage(),
      }),
      engineEvent(7, 'run_ended', {
        ended_at: '2026-07-12T01:00:04Z',
        reason: 'completed',
        run_id: id(31),
        usage: usage(),
      }),
      taskEvent(8, 'run.completed', {
        endedAt: '2026-07-12T01:00:04Z',
        incompleteOutput: false,
        segmentId,
        terminalReason: 'completed',
      }),
    ]

    const result = deriveLiveTaskSnapshot(snapshot, events)

    expect(result.projection.state).toBe('completed')
    expect(result.projection.currentRun).toMatchObject({
      endedAt: '2026-07-12T01:00:04Z',
      segmentId,
      state: 'completed',
      terminalReason: 'completed',
    })
    expect(
      result.timeline.filter((item) => item.kind === 'assistant_text').map((item) => item.summary),
    ).toEqual(['First ', 'answer'])
    expect(
      result.timeline
        .filter((item) => item.kind === 'assistant_text')
        .map((item) => [item.semanticGroupId, item.incomplete]),
    ).toEqual([
      [messageId, true],
      [messageId, false],
    ])
    expect(result.timeline.some((item) => item.summary === 'run ended')).toBe(false)
  })

  it('uses completed assistant content only when no text delta exists', () => {
    const messageId = id(50)
    const result = deriveLiveTaskSnapshot(snapshot, [
      taskEvent(3, 'run.started', {
        segmentId: id(30),
        startedAt: '2026-07-12T01:00:00Z',
      }),
      engineEvent(4, 'assistant_message_completed', {
        at: '2026-07-12T01:00:01Z',
        content: { text: 'Completion fallback' },
        message_id: messageId,
        pricing_snapshot_id: null,
        run_id: id(31),
        stop_reason: 'end_turn',
        tool_uses: [],
        usage: usage(),
      }),
    ])

    expect(result.timeline.at(-1)).toMatchObject({
      incomplete: false,
      kind: 'assistant_text',
      semanticGroupId: messageId,
      summary: 'Completion fallback',
    })
  })

  it('scopes engine output and assistant completion to the originating run segment', () => {
    const messageId = id(50)
    const firstSegmentId = id(30)
    const secondSegmentId = id(33)
    const result = deriveLiveTaskSnapshot(snapshot, [
      taskEvent(3, 'run.started', {
        segmentId: firstSegmentId,
        startedAt: '2026-07-12T01:00:00Z',
      }),
      engineEvent(
        4,
        'assistant_delta_produced',
        {
          at: '2026-07-12T01:00:01Z',
          delta: { text: 'First run' },
          message_id: messageId,
          run_id: id(31),
        },
        firstSegmentId,
      ),
      taskEvent(5, 'run.completed', {
        endedAt: '2026-07-12T01:00:02Z',
        incompleteOutput: false,
        segmentId: firstSegmentId,
        terminalReason: 'completed',
      }),
      taskEvent(6, 'run.started', {
        segmentId: secondSegmentId,
        startedAt: '2026-07-12T01:00:03Z',
      }),
      engineEvent(
        7,
        'assistant_message_completed',
        {
          at: '2026-07-12T01:00:04Z',
          content: { text: 'Second run' },
          message_id: messageId,
          pricing_snapshot_id: null,
          run_id: id(34),
          stop_reason: 'end_turn',
          tool_uses: [],
          usage: usage(),
        },
        secondSegmentId,
      ),
      engineEvent(
        8,
        'assistant_delta_produced',
        {
          at: '2026-07-12T01:00:05Z',
          delta: { text: ' late' },
          message_id: messageId,
          run_id: id(31),
        },
        firstSegmentId,
      ),
    ])

    expect(
      result.timeline
        .filter((item) => item.kind === 'assistant_text')
        .map((item) => [item.summary, item.runSegmentId, item.incomplete]),
    ).toEqual([
      ['First run', firstSegmentId, true],
      ['Second run', secondSegmentId, false],
      [' late', firstSegmentId, true],
    ])
  })

  it('matches canonical transient state transitions for safe points, permissions, and actor failure', () => {
    const segmentId = id(30)
    const queueItemId = id(60)
    const permission = { requestId: id(61), revision: 1, route: 'foreground_task' as const }
    const result = deriveLiveTaskSnapshot(snapshot, [
      taskEvent(3, 'run.started', { segmentId, startedAt: '2026-07-12T01:00:00Z' }),
      taskEvent(4, 'message.queued', {
        attachments: [],
        content: 'queued prompt',
        contextReferences: [],
        createdAt: '2026-07-12T01:00:01Z',
        queueItemId,
      }),
      taskEvent(5, 'run.yield_requested', { force: true, segmentId }),
      taskEvent(6, 'message.promoted', { queueItemId, revision: 1 }),
      taskEvent(7, 'run.safe_point_reached', {
        forced: true,
        incompleteOutput: true,
        segmentId,
      }),
    ])

    expect(result.projection.state).toBe('running')
    expect(result.projection.currentRun?.state).toBe('yielding')

    const failed = deriveLiveTaskSnapshot(snapshot, [
      taskEvent(3, 'run.started', { segmentId, startedAt: '2026-07-12T01:00:00Z' }),
      taskEvent(4, 'message.queued', {
        attachments: [],
        content: 'queued prompt',
        contextReferences: [],
        createdAt: '2026-07-12T01:00:01Z',
        queueItemId,
      }),
      taskEvent(5, 'run.yield_requested', { force: false, segmentId }),
      taskEvent(6, 'message.promoted', { queueItemId, revision: 1 }),
      taskEvent(7, 'task.actor_failed', { failedAt: '2026-07-12T01:00:02Z', segmentId }),
    ])
    expect(failed.projection.queue).toEqual([
      expect.objectContaining({ queueItemId, state: 'queued' }),
    ])

    const permissionOnly = deriveLiveTaskSnapshot(snapshot, [
      taskEvent(3, 'permission.requested', permission),
      taskEvent(4, 'permission.resolved', { requestId: permission.requestId, revision: 1 }),
    ])
    expect(permissionOnly.projection.state).toBe('idle')
  })

  it('does not rewrite a completed run when the idle actor later fails', () => {
    const completedRun = {
      endedAt: '2026-07-12T01:00:01Z',
      incompleteOutput: false,
      segmentId: id(30),
      startedAt: '2026-07-12T01:00:00Z',
      state: 'completed' as const,
      terminalReason: 'completed' as const,
    }
    const result = deriveLiveTaskSnapshot(
      {
        ...snapshot,
        projection: {
          ...snapshot.projection,
          currentRun: completedRun,
          state: 'completed',
        },
      },
      [taskEvent(3, 'task.actor_failed', { failedAt: '2026-07-12T01:00:02Z' })],
    )

    expect(result.projection.state).toBe('failed')
    expect(result.projection.currentRun).toEqual(completedRun)
  })

  it('keeps live metadata timeline projection in parity with reload', () => {
    const result = deriveLiveTaskSnapshot(snapshot, [
      taskEvent(3, 'task.pinned', { pinned: true }),
      taskEvent(4, 'task.pinned', { pinned: false }),
      taskEvent(5, 'task.removed', { removed: true }),
    ])

    expect(result.timeline.map((item) => item.summary)).toEqual([
      'Task pinned',
      'Task unpinned',
      'Task removed',
    ])
  })

  it('updates queue and permission state from committed live events without replaying the snapshot', () => {
    const queueItemId = id(60)
    const permission = { requestId: id(61), revision: 1, route: 'foreground_task' as const }
    const result = deriveLiveTaskSnapshot(snapshot, [
      taskEvent(2, 'task.title_changed', { title: 'replayed title' }),
      taskEvent(3, 'message.queued', {
        attachments: [],
        content: 'queued prompt',
        contextReferences: [
          {
            kind: 'skill',
            label: 'Review',
            parameters: { focus: 'correctness' },
            skillId: 'user:review',
            source: 'user',
            version: 1,
          },
        ],
        createdAt: '2026-07-12T01:00:00Z',
        queueItemId,
      }),
      taskEvent(4, 'permission.requested', permission),
      taskEvent(5, 'permission.resolved', { requestId: permission.requestId, revision: 1 }),
    ])

    expect(result.projection.title).toBe(snapshot.projection.title)
    expect(result.projection.queue).toEqual([
      expect.objectContaining({
        content: 'queued prompt',
        contextReferences: [
          {
            kind: 'skill',
            label: 'Review',
            parameters: { focus: 'correctness' },
            skillId: 'user:review',
            source: 'user',
            version: 1,
          },
        ],
        queueItemId,
        state: 'queued',
      }),
    ])
    expect(result.projection.pendingPermission).toBeNull()
    expect(result.projection.lastGlobalOffset).toBe(5)
    expect(result.projection.streamVersion).toBe(5)
  })

  it('keeps live subagent projections current', () => {
    const child = {
      actorId: id(71),
      childTaskId: id(72),
      contextCursor: 0,
      delegationId: id(73),
      detached: false,
      parentSegmentId: id(74),
      parentTaskId: snapshot.projection.taskId,
      segmentId: id(75),
      startedAt: '2026-07-12T01:00:00Z',
      state: 'starting',
    }
    const result = deriveLiveTaskSnapshot(snapshot, [
      taskEvent(3, 'subagent.spawned', {
        actorId: child.actorId,
        child,
        startedAt: child.startedAt,
      }),
      taskEvent(4, 'subagent.state_changed', {
        ...child,
        contextCursor: 3,
        state: 'running',
        summary: 'Checking projection parity',
      }),
    ])

    expect(result.projection.subagents).toEqual([
      expect.objectContaining({
        childTaskId: child.childTaskId,
        contextCursor: 3,
        state: 'running',
        summary: 'Checking projection parity',
      }),
    ])
  })
})

const snapshot: TaskSnapshot = {
  projection: {
    archived: false,
    lastGlobalOffset: 2,
    queue: [],
    state: 'idle',
    streamVersion: 2,
    taskId: id(1),
    title: 'Original title',
  },
  snapshotOffset: 2,
  timeline: [],
}

function taskEvent(globalOffset: number, eventType: string, payload: unknown): TaskEventEnvelope {
  return {
    eventId: id(100 + globalOffset),
    eventType,
    globalOffset,
    payload,
    recordedAt: '2026-07-12T01:00:00Z',
    schemaVersion: 1,
    source: { kind: 'supervisor' },
    streamSequence: globalOffset,
    taskId: snapshot.projection.taskId,
  }
}

function engineEvent(
  globalOffset: number,
  type: string,
  event: Record<string, unknown>,
  runSegmentId = id(30),
): TaskEventEnvelope {
  return {
    ...taskEvent(globalOffset, `engine.${type}`, {
      causationId: null,
      correlationId: id(70),
      event: { type, ...event },
      journalOffset: globalOffset - 4,
      runId: id(31),
      runSegmentId,
      sessionId: id(32),
      tenantId: id(0),
    }),
    source: { kind: 'engine' },
  }
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

function id(value: number) {
  return `000000000000000000000000${String(value).padStart(2, '0')}`
}
