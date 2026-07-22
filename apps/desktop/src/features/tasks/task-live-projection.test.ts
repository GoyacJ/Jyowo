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
        content: { text: 'Canonical answer' },
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
    ).toEqual(['First ', 'Canonical answer'])
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

  it('projects pure media and appends media to streamed assistant content', () => {
    const mediaMessageId = id(51)
    const streamedMessageId = id(52)
    const imageBlobId = id(53)
    const videoBlobId = id(54)
    const fileBlobId = id(55)
    const streamedImageBlobId = id(56)
    const result = deriveLiveTaskSnapshot(snapshot, [
      engineEvent(3, 'assistant_message_completed', {
        content: {
          multimodal: [
            mediaPart('image', imageBlobId, 'image/png', 10),
            mediaPart('video', videoBlobId, 'video/mp4', 20),
            mediaPart('file', fileBlobId, 'application/pdf', 30),
          ],
        },
        message_id: mediaMessageId,
      }),
      engineEvent(4, 'assistant_delta_produced', {
        delta: { text: 'Rendered image' },
        message_id: streamedMessageId,
      }),
      engineEvent(5, 'assistant_message_completed', {
        content: {
          multimodal: [
            { text: 'Canonical before ' },
            mediaPart('image', streamedImageBlobId, 'image/webp', 40),
            { text: ' after image' },
          ],
        },
        message_id: streamedMessageId,
      }),
    ])

    expect(result.timeline).toHaveLength(2)
    expect(result.timeline[0]).toMatchObject({
      blobId: imageBlobId,
      contentBlocks: [
        {
          artifact: {
            artifactKind: 'image',
            blobId: imageBlobId,
            mediaType: 'image/png',
            presentation: { preferredSurface: 'inline' },
          },
          type: 'artifact',
        },
        {
          artifact: {
            artifactKind: 'video',
            blobId: videoBlobId,
            mediaType: 'video/mp4',
            presentation: { preferredSurface: 'inline' },
          },
          type: 'artifact',
        },
        {
          artifact: {
            artifactKind: 'file',
            blobId: fileBlobId,
            mediaType: 'application/pdf',
            presentation: { preferredSurface: 'card' },
          },
          type: 'artifact',
        },
      ],
      incomplete: false,
      kind: 'assistant_text',
      semanticGroupId: mediaMessageId,
      summary: 'Image',
    })
    expect(result.timeline[1]).toMatchObject({
      blobId: streamedImageBlobId,
      contentBlocks: [
        { format: 'markdown', text: 'Canonical before ', type: 'text' },
        {
          artifact: {
            artifactKind: 'image',
            blobId: streamedImageBlobId,
            mediaType: 'image/webp',
          },
          type: 'artifact',
        },
        { format: 'markdown', text: ' after image', type: 'text' },
      ],
      incomplete: false,
      summary: 'Canonical before  after image',
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

  it('preserves queued attachments in consumed user message content blocks', () => {
    const queueItemId = id(57)
    const attachmentId = id(58)
    const segmentId = id(59)
    const result = deriveLiveTaskSnapshot(snapshot, [
      taskEvent(3, 'message.queued', {
        attachments: [attachmentId],
        content: 'Inspect this file',
        contextReferences: [],
        createdAt: '2026-07-12T01:00:00Z',
        queueItemId,
      }),
      taskEvent(4, 'run.started', { segmentId, startedAt: '2026-07-12T01:00:01Z' }),
      taskEvent(5, 'message.consumed', { queueItemId, revision: 1, segmentId }),
    ])

    expect(result.timeline.at(-1)).toMatchObject({
      blobId: attachmentId,
      contentBlocks: [
        { format: 'plain', text: 'Inspect this file', type: 'text' },
        {
          artifact: {
            artifactKind: 'file',
            blobId: attachmentId,
            presentation: { preferredSurface: 'card' },
          },
          type: 'artifact',
        },
      ],
      kind: 'user_message',
      summary: 'Inspect this file',
    })
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

  it('projects a pending question from the committed flat event payload', () => {
    const segmentId = id(62)
    const requestId = id(63)
    const toolUseId = id(64)
    const result = deriveLiveTaskSnapshot(snapshot, [
      taskEvent(3, 'run.started', {
        segmentId,
        startedAt: '2026-07-12T01:00:00Z',
      }),
      taskEvent(4, 'question.requested', {
        expiresAt: '2026-07-12T01:05:00Z',
        questions: [
          {
            allowCustom: false,
            header: '控制方式',
            id: 'control',
            multiSelect: false,
            options: [
              {
                description: '同时支持电脑和移动设备',
                id: 'both',
                label: '键盘 + 触屏',
              },
            ],
            question: '游戏运行在哪种设备上？',
          },
        ],
        requestId,
        revision: 1,
        segmentId,
        toolUseId,
      }),
    ])

    expect(result.projection).toMatchObject({
      currentRun: { segmentId, state: 'waiting_input' },
      pendingQuestion: {
        questions: [
          expect.objectContaining({
            id: 'control',
            question: '游戏运行在哪种设备上？',
          }),
        ],
        requestId,
        revision: 1,
        segmentId,
        toolUseId,
      },
      state: 'waiting_input',
    })
    expect(result.timeline.at(-1)).toMatchObject({
      kind: 'notice',
      summary: 'User input requested',
    })
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

  it('preserves file and generated media artifact kinds in the live timeline', () => {
    const blobId = id(82)
    const sourceToolUseId = id(84)
    const result = deriveLiveTaskSnapshot(snapshot, [
      engineEvent(3, 'artifact_created', {
        blob_ref: {
          content_hash: Array(32).fill(1),
          content_type: 'text/markdown',
          id: blobId,
          size: 12,
        },
        kind: 'file',
        source_tool_use_id: sourceToolUseId,
        title: 'report.md',
      }),
      engineEvent(4, 'artifact_created', {
        blob_ref: {
          content_hash: Array(32).fill(2),
          content_type: 'video/mp4',
          id: id(83),
          size: 24,
        },
        kind: 'video',
        title: 'demo.mp4',
      }),
    ])

    expect(result.timeline).toEqual([
      expect.objectContaining({
        blobId,
        contentBlocks: [
          expect.objectContaining({
            artifact: expect.objectContaining({
              artifactKind: 'file',
              mediaType: 'text/markdown',
              size: 12,
              sourceToolUseId,
            }),
            type: 'artifact',
          }),
        ],
        kind: 'file',
        summary: 'report.md',
      }),
      expect.objectContaining({
        contentBlocks: [
          expect.objectContaining({
            artifact: expect.objectContaining({
              artifactKind: 'video',
              mediaType: 'video/mp4',
              presentation: { preferredSurface: 'inline' },
            }),
            type: 'artifact',
          }),
        ],
        kind: 'artifact',
        summary: 'demo.mp4',
      }),
    ])
  })

  it('updates requested, started, and completed tool events as one timeline item', () => {
    const toolUseId = id(90)
    const result = deriveLiveTaskSnapshot(snapshot, [
      engineEvent(3, 'tool_use_requested', {
        input: { path: '/workspace/src/scheduler.rs' },
        tool_name: 'read_file',
        tool_use_id: toolUseId,
      }),
      engineEvent(4, 'tool_use_started', { tool_use_id: toolUseId }),
      engineEvent(5, 'tool_use_completed', {
        duration_ms: 42,
        result: { text: 'first\nsecond' },
        tool_use_id: toolUseId,
      }),
    ])

    expect(result.timeline).toEqual([
      expect.objectContaining({
        globalOffset: 3,
        incomplete: false,
        kind: 'tool_activity',
        summary: 'Read src/scheduler.rs',
        tool: {
          durationMs: 42,
          operation: 'read',
          resultSummary: '2 lines returned',
          status: 'completed',
          subject: 'src/scheduler.rs',
          toolName: 'read_file',
          toolUseId,
        },
      }),
    ])
  })

  it('projects a nonzero shell exit as a failed command', () => {
    const toolUseId = id(91)
    const result = deriveLiveTaskSnapshot(snapshot, [
      engineEvent(3, 'tool_use_requested', {
        input: { command: 'pwd && ls -la' },
        tool_name: 'Bash',
        tool_use_id: toolUseId,
      }),
      engineEvent(4, 'tool_use_completed', {
        duration_ms: 9,
        result: {
          mixed: [
            { kind: 'text', text: 'command failed' },
            {
              kind: 'structured',
              schema_ref: null,
              value: { exit_status: { code: 127 }, success: false },
            },
          ],
        },
        tool_use_id: toolUseId,
      }),
    ])

    expect(result.timeline[0]).toEqual(
      expect.objectContaining({
        incomplete: false,
        summary: 'Tool failed',
        tool: expect.objectContaining({
          operation: 'command',
          output: 'command failed',
          status: 'failed',
        }),
      }),
    )
  })

  it('preserves the tool denial reason in the live timeline', () => {
    const toolUseId = id(92)
    const result = deriveLiveTaskSnapshot(snapshot, [
      engineEvent(3, 'tool_use_requested', {
        input: { command: 'pwd' },
        tool_name: 'Bash',
        tool_use_id: toolUseId,
      }),
      engineEvent(4, 'tool_use_denied', {
        reason: { other: 'workspace lease is read-only' },
        tool_use_id: toolUseId,
      }),
    ])

    expect(result.timeline[0]?.tool).toEqual(
      expect.objectContaining({
        output: 'Tool use denied: workspace lease is read-only',
        resultSummary: 'Tool use denied: workspace lease is read-only',
        status: 'denied',
      }),
    )
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

function mediaPart(
  kind: 'file' | 'image' | 'video',
  blobId: string,
  mimeType: string,
  size: number,
) {
  return {
    [kind]: {
      blob_ref: {
        content_hash: Array(32).fill(1),
        content_type: 'application/octet-stream',
        id: blobId,
        size,
      },
      mime_type: mimeType,
    },
  }
}

function id(value: number) {
  return `000000000000000000000000${String(value).padStart(2, '0')}`
}
