import { describe, expect, it } from 'vitest'

import type { TimelineRunEvent } from './conversation-blocks'
import {
  conversationTimelineReducer,
  createConversationTimelineState,
} from './conversation-timeline-reducer'

const timestamp = '2026-06-17T00:00:00.000Z'

function event(
  type: TimelineRunEvent['type'],
  payload: TimelineRunEvent['payload'],
  options: Partial<TimelineRunEvent> = {},
): TimelineRunEvent {
  return {
    id: options.id ?? `evt-${type}-${options.sequence ?? 1}`,
    conversationSequence: options.conversationSequence ?? options.sequence ?? 1,
    runId: options.runId ?? 'run-001',
    sequence: options.sequence ?? 1,
    source: options.source ?? 'engine',
    timestamp: options.timestamp ?? timestamp,
    type,
    visibility: options.visibility ?? 'public',
    payload,
  } as TimelineRunEvent
}

function reduce(
  actions: Parameters<typeof conversationTimelineReducer>[1][],
  conversationId = 'conversation-001',
) {
  return actions.reduce(
    conversationTimelineReducer,
    createConversationTimelineState(conversationId),
  )
}

describe('conversationTimelineReducer', () => {
  it('confirms optimistic user messages only by clientMessageId', () => {
    const state = reduce([
      {
        type: 'localSubmit',
        clientMessageId: 'client-001',
        draft: { prompt: 'Repeat' },
        at: timestamp,
      },
      {
        type: 'localSubmit',
        clientMessageId: 'client-002',
        draft: { prompt: 'Repeat' },
        at: timestamp,
      },
      {
        type: 'applyEvents',
        events: [
          event(
            'user.message.appended',
            { messageId: 'message-001', clientMessageId: 'client-001', body: 'Repeat' },
            { id: 'evt-user-001', source: 'user', sequence: 1 },
          ),
        ],
        cursor: 'evt-user-001',
      },
    ])

    const userBlocks = state.blocks.filter((block) => block.kind === 'userMessage')

    expect(userBlocks).toHaveLength(2)
    expect(userBlocks[0]).toMatchObject({
      id: 'message:message-001',
      messageId: 'message-001',
      clientMessageId: 'client-001',
      status: 'sent',
    })
    expect(userBlocks[1]).toMatchObject({
      id: 'local:client-002',
      clientMessageId: 'client-002',
      status: 'sending',
    })
  })

  it('binds a later commandAccepted run id after user confirmation arrives first', () => {
    const state = reduce([
      {
        type: 'localSubmit',
        clientMessageId: 'client-001',
        draft: { prompt: 'Continue' },
        at: timestamp,
      },
      {
        type: 'applyEvents',
        events: [
          event(
            'user.message.appended',
            { messageId: 'message-001', clientMessageId: 'client-001', body: 'Continue' },
            { id: 'evt-user-001', runId: 'run-001', source: 'user' },
          ),
        ],
      },
      { type: 'commandAccepted', clientMessageId: 'client-001', runId: 'run-001' },
    ])

    expect(state.blocks[0]).toMatchObject({ runId: 'run-001', status: 'sent' })
    expect(state.clientMessageByRunId).toMatchObject({ 'run-001': 'client-001' })
  })

  it('keeps a stream gap after hydrating a snapshot because snapshots cannot recover all event blocks', () => {
    const state = reduce([
      {
        type: 'markGap',
        afterCursor: 'evt-before-gap',
      },
      {
        type: 'hydrateSnapshot',
        snapshot: {
          id: 'conversation-001',
          title: 'Conversation',
          modelConfigId: null,
          updatedAt: '2026-06-17T00:00:01.000Z',
          messages: [
            {
              author: 'user',
              body: 'Recovered prompt',
              id: 'message-001',
              timestamp: '2026-06-17T00:00:01.000Z',
            },
          ],
        },
      },
    ])

    expect(state.hasGap).toBe(true)
    expect(state.blocks).toContainEqual(
      expect.objectContaining({
        kind: 'userMessage',
        body: 'Recovered prompt',
      }),
    )
  })

  it('keeps a stream gap after an empty replay batch', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event('run.started', { sessionId: 'conversation-001' }, { id: 'evt-run-1', sequence: 1 }),
        ],
        cursor: 'evt-run-1',
      },
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.delta',
            { text: 'missed event' },
            {
              conversationSequence: 3,
              id: 'evt-delta-3',
              sequence: 3,
              source: 'assistant',
            },
          ),
        ],
        cursor: 'evt-delta-3',
      },
      {
        type: 'applyEvents',
        events: [],
        cursor: null,
      },
    ])

    expect(state.hasGap).toBe(true)
    expect(state.cursor).toBe('evt-run-1')
  })

  it('clears a transient gap when replay only contains already applied events', () => {
    const applied = event(
      'run.started',
      { sessionId: 'conversation-001' },
      {
        id: 'evt-run-1',
        sequence: 1,
      },
    )
    const state = reduce([
      {
        type: 'applyEvents',
        events: [applied],
        cursor: 'evt-run-1',
      },
      {
        type: 'markGap',
        afterCursor: 'evt-run-1',
      },
      {
        type: 'applyEvents',
        events: [applied],
        cursor: 'evt-run-1',
      },
    ])

    expect(state.hasGap).toBe(false)
    expect(state.cursor).toBe('evt-run-1')
  })

  it('clears a stream gap only after replaying the missing contiguous event', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event('run.started', { sessionId: 'conversation-001' }, { id: 'evt-run-1', sequence: 1 }),
        ],
        cursor: 'evt-run-1',
      },
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.delta',
            { text: 'future' },
            {
              conversationSequence: 3,
              id: 'evt-delta-3',
              sequence: 3,
              source: 'assistant',
            },
          ),
        ],
        cursor: 'evt-delta-3',
      },
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.delta',
            { text: 'missing ' },
            {
              conversationSequence: 2,
              id: 'evt-delta-2',
              sequence: 2,
              source: 'assistant',
            },
          ),
          event(
            'assistant.delta',
            { text: 'future' },
            {
              conversationSequence: 3,
              id: 'evt-delta-3',
              sequence: 3,
              source: 'assistant',
            },
          ),
        ],
        cursor: 'evt-delta-3',
      },
    ])

    expect(state.hasGap).toBe(false)
    expect(state.cursor).toBe('evt-delta-3')
    expect(state.blocks).toContainEqual(
      expect.objectContaining({
        kind: 'assistantStreaming',
        body: 'missing future',
      }),
    )
  })

  it('keeps a stream gap when only part of a rejected forward jump has arrived', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event('run.started', { sessionId: 'conversation-001' }, { id: 'evt-run-1', sequence: 1 }),
        ],
        cursor: 'evt-run-1',
      },
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.delta',
            { text: 'future' },
            {
              conversationSequence: 3,
              id: 'evt-delta-3',
              sequence: 3,
              source: 'assistant',
            },
          ),
        ],
        cursor: 'evt-delta-3',
      },
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.delta',
            { text: 'missing ' },
            {
              conversationSequence: 2,
              id: 'evt-delta-2',
              sequence: 2,
              source: 'assistant',
            },
          ),
        ],
        cursor: 'evt-delta-2',
      },
    ])

    expect(state.hasGap).toBe(true)
    expect(state.cursor).toBe('evt-run-1')
    expect(state.blocks).toContainEqual(
      expect.objectContaining({
        kind: 'assistantStreaming',
        body: 'missing ',
      }),
    )
  })

  it('marks forward conversation sequence jumps as stream gaps without applying the event', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event('run.started', { sessionId: 'conversation-001' }, { id: 'evt-run-1', sequence: 1 }),
        ],
        cursor: 'evt-run-1',
      },
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.delta',
            { text: 'missed event' },
            {
              conversationSequence: 3,
              id: 'evt-delta-3',
              sequence: 3,
              source: 'assistant',
            },
          ),
        ],
        cursor: 'evt-delta-3',
      },
    ])

    expect(state.hasGap).toBe(true)
    expect(state.cursor).toBe('evt-run-1')
    expect(state.blocks).not.toContainEqual(
      expect.objectContaining({
        kind: 'assistantStreaming',
        body: 'missed event',
      }),
    )
  })

  it('creates streaming assistant text and finalizes from redacted final body', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event('run.started', { sessionId: 'conversation-001' }, { id: 'evt-run', sequence: 1 }),
          event(
            'assistant.delta',
            { text: 'Hel' },
            { id: 'evt-delta-1', source: 'assistant', sequence: 2 },
          ),
          event(
            'assistant.delta',
            { text: 'lo' },
            { id: 'evt-delta-2', source: 'assistant', sequence: 3 },
          ),
          event(
            'assistant.completed',
            { messageId: 'message-002', body: 'Hello final' },
            { id: 'evt-complete', source: 'assistant', sequence: 4 },
          ),
        ],
      },
    ])

    expect(state.blocks).toContainEqual(
      expect.objectContaining({
        kind: 'assistantMessage',
        id: 'message:message-002',
        messageId: 'message-002',
        body: 'Hello final',
        status: 'complete',
      }),
    )
    expect(state.streamingBlockByRunId['run-001']).toBeUndefined()
  })

  it('keeps streamed text pending reconciliation when assistant final body is missing', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.delta',
            { text: 'partial' },
            { id: 'evt-delta', source: 'assistant', sequence: 1 },
          ),
          event(
            'assistant.completed',
            { messageId: 'message-002' },
            { id: 'evt-complete', source: 'assistant', sequence: 2 },
          ),
        ],
      },
      {
        type: 'snapshotReconciled',
        snapshot: {
          id: 'conversation-001',
          title: 'Conversation',
          modelConfigId: null,
          updatedAt: '2026-06-17T00:00:01.000Z',
          messages: [
            {
              author: 'assistant',
              body: 'final from snapshot',
              id: 'message-002',
              timestamp: '2026-06-17T00:00:01.000Z',
            },
          ],
        },
      },
    ])

    expect(state.pendingAssistantReconcileByMessageId['message-002']).toBeUndefined()
    expect(state.blocks).toContainEqual(
      expect.objectContaining({
        kind: 'assistantMessage',
        messageId: 'message-002',
        body: 'final from snapshot',
        status: 'complete',
      }),
    )
  })

  it('aggregates tool events into one block and expands failures', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event(
            'tool.requested',
            {
              toolUseId: 'tool-001',
              toolName: 'read_file',
              argumentsSummary: 'Input withheld from conversation timeline.',
            },
            { id: 'evt-tool-1', source: 'tool', sequence: 1, visibility: 'redacted' },
          ),
          event(
            'tool.approved',
            { toolUseId: 'tool-001' },
            { id: 'evt-tool-2', source: 'tool', sequence: 2 },
          ),
          event(
            'tool.failed',
            {
              toolUseId: 'tool-001',
              code: 'tool_error',
              message: 'Tool error withheld from conversation timeline.',
            },
            { id: 'evt-tool-3', source: 'tool', sequence: 3 },
          ),
        ],
      },
    ])

    expect(state.blocks).toContainEqual(
      expect.objectContaining({
        kind: 'toolGroup',
        expanded: true,
        items: [
          expect.objectContaining({
            id: 'tool-001',
            name: 'read_file',
            status: 'failed',
            errorMessage: 'Tool error withheld from conversation timeline.',
          }),
        ],
      }),
    )
  })

  it('keeps permission state frontend-submitted until policy resolution arrives', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event(
            'permission.requested',
            {
              requestId: '01HZ0000000000000000000001',
              operation: 'Install dependencies',
              reason: 'The run requested package installation.',
              target: 'workspace package manager',
              severity: 'high',
              decisionScope: 'current run',
              exposure: 'Can modify package metadata and lockfile.',
              workspaceBoundary: 'workspace://local',
            },
            { id: 'evt-permission-1', source: 'policy', sequence: 1 },
          ),
        ],
      },
      {
        type: 'permissionSubmitting',
        requestId: '01HZ0000000000000000000001',
        decision: 'approve',
      },
      {
        type: 'applyEvents',
        events: [
          event(
            'permission.resolved',
            { requestId: '01HZ0000000000000000000001', decision: 'approve' },
            { id: 'evt-permission-2', source: 'policy', sequence: 2 },
          ),
        ],
      },
    ])

    expect(state.blocks).toContainEqual(
      expect.objectContaining({
        kind: 'permissionRequest',
        requestId: '01HZ0000000000000000000001',
        status: 'resolved',
        decision: 'approve',
        submitDecision: undefined,
      }),
    )
  })

  it('patches one artifact block from lifecycle events and artifact snapshots', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event(
            'artifact.created',
            { artifactId: 'artifact-001', status: 'pending' },
            { id: 'evt-artifact-1', sequence: 1 },
          ),
          event(
            'artifact.updated',
            { artifactId: 'artifact-001', status: 'ready' },
            { id: 'evt-artifact-2', sequence: 2 },
          ),
        ],
      },
      {
        type: 'applyArtifacts',
        artifacts: [
          {
            id: 'artifact-001',
            kind: 'markdown',
            title: 'Generated notes',
            description: 'Ready for review',
            actionLabel: 'Open',
            status: 'ready',
          },
        ],
      },
    ])

    const artifactBlocks = state.blocks.filter((block) => block.kind === 'artifact')

    expect(artifactBlocks).toHaveLength(1)
    expect(artifactBlocks[0]).toMatchObject({
      artifactId: 'artifact-001',
      title: 'Generated notes',
      status: 'ready',
    })
  })

  it('ignores duplicate events and renders withheld events as safe notices', () => {
    const withheld = event('tool.completed', undefined, {
      id: 'evt-withheld',
      source: 'tool',
      visibility: 'withheld',
      sequence: 1,
    })
    const state = reduce([{ type: 'applyEvents', events: [withheld, withheld] }])

    expect(state.blocks).toEqual([
      expect.objectContaining({
        kind: 'systemNotice',
        message: 'Event details are withheld.',
      }),
    ])
  })

  it('hydrates snapshots without erasing optimistic or live blocks', () => {
    const state = reduce([
      {
        type: 'localSubmit',
        clientMessageId: 'client-001',
        draft: { prompt: 'Local draft' },
        at: timestamp,
      },
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.delta',
            { text: 'streaming' },
            { id: 'evt-delta', source: 'assistant', sequence: 1 },
          ),
        ],
      },
      {
        type: 'hydrateSnapshot',
        snapshot: {
          id: 'conversation-001',
          title: 'Conversation',
          modelConfigId: null,
          updatedAt: timestamp,
          messages: [
            {
              author: 'user',
              body: 'Persisted',
              id: 'message-001',
              timestamp,
            },
          ],
        },
      },
    ])

    expect(state.blocks).toEqual([
      expect.objectContaining({ kind: 'userMessage', body: 'Persisted' }),
      expect.objectContaining({ kind: 'userMessage', body: 'Local draft' }),
      expect.objectContaining({ kind: 'assistantStreaming', body: 'streaming' }),
    ])
  })

  it('hydrates the same snapshot without shifting live blocks repeatedly', () => {
    const snapshot = {
      id: 'conversation-001',
      title: 'Conversation',
      modelConfigId: null,
      updatedAt: timestamp,
      messages: [
        {
          author: 'user' as const,
          body: 'Persisted',
          id: 'message-001',
          timestamp,
        },
      ],
    }
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.delta',
            { text: 'streaming' },
            { id: 'evt-delta', source: 'assistant', sequence: 1 },
          ),
        ],
      },
      { type: 'hydrateSnapshot', snapshot },
      { type: 'hydrateSnapshot', snapshot },
    ])

    expect(state.blocks.find((block) => block.kind === 'assistantStreaming')).toMatchObject({
      conversationSequence: 1,
    })
  })

  it('keeps existing live message blocks before later live blocks after snapshot hydrate', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event(
            'user.message.appended',
            { messageId: 'message-001', body: 'Persisted' },
            { id: 'evt-user', source: 'user', sequence: 1 },
          ),
          event(
            'assistant.delta',
            { text: 'streaming' },
            { id: 'evt-delta', source: 'assistant', sequence: 2 },
          ),
        ],
      },
      {
        type: 'hydrateSnapshot',
        snapshot: {
          id: 'conversation-001',
          title: 'Conversation',
          modelConfigId: null,
          updatedAt: timestamp,
          messages: [
            {
              author: 'user',
              body: 'Persisted',
              id: 'message-001',
              timestamp,
            },
          ],
        },
      },
    ])

    expect(state.blocks.map((block) => block.kind)).toEqual(['userMessage', 'assistantStreaming'])
  })

  it('reconciles snapshot user messages with matching optimistic client ids', () => {
    const state = reduce([
      {
        type: 'localSubmit',
        clientMessageId: 'client-001',
        draft: { prompt: 'Repeat' },
        at: timestamp,
      },
      {
        type: 'hydrateSnapshot',
        snapshot: {
          id: 'conversation-001',
          title: 'Conversation',
          modelConfigId: null,
          updatedAt: timestamp,
          messages: [
            {
              author: 'user',
              body: 'Repeat',
              clientMessageId: 'client-001',
              id: 'message-001',
              timestamp,
            },
          ],
        },
      },
      {
        type: 'applyEvents',
        events: [
          event(
            'user.message.appended',
            { messageId: 'message-001', clientMessageId: 'client-001', body: 'Repeat' },
            { id: 'evt-user-001', source: 'user', sequence: 1 },
          ),
        ],
      },
    ])

    const userBlocks = state.blocks.filter((block) => block.kind === 'userMessage')

    expect(userBlocks).toHaveLength(1)
    expect(userBlocks[0]).toMatchObject({
      id: 'message:message-001',
      clientMessageId: 'client-001',
      messageId: 'message-001',
      body: 'Repeat',
      status: 'sent',
    })
  })

  it('does not reconcile snapshot user messages by body when client ids differ', () => {
    const state = reduce([
      {
        type: 'localSubmit',
        clientMessageId: 'client-001',
        draft: { prompt: 'Repeat' },
        at: timestamp,
      },
      {
        type: 'localSubmit',
        clientMessageId: 'client-002',
        draft: { prompt: 'Repeat' },
        at: timestamp,
      },
      {
        type: 'hydrateSnapshot',
        snapshot: {
          id: 'conversation-001',
          title: 'Conversation',
          modelConfigId: null,
          updatedAt: timestamp,
          messages: [
            {
              author: 'user',
              body: 'Repeat',
              clientMessageId: 'client-002',
              id: 'message-002',
              timestamp,
            },
          ],
        },
      },
    ])

    expect(state.blocks).toContainEqual(
      expect.objectContaining({
        id: 'message:message-002',
        clientMessageId: 'client-002',
        messageId: 'message-002',
        status: 'sent',
      }),
    )
    expect(state.blocks).toContainEqual(
      expect.objectContaining({
        id: 'local:client-001',
        clientMessageId: 'client-001',
        status: 'sending',
      }),
    )
  })

  it('marks explicit gaps without guessing order', () => {
    const state = reduce([{ type: 'markGap', afterCursor: 'evt-001' }])

    expect(state.hasGap).toBe(true)
    expect(state.cursor).toBe('evt-001')
  })

  it('does not duplicate assistant messages when snapshot hydrate precedes event replay', () => {
    const state = reduce([
      {
        type: 'hydrateSnapshot',
        snapshot: {
          id: 'conversation-001',
          title: 'Conversation',
          modelConfigId: null,
          updatedAt: timestamp,
          messages: [
            {
              author: 'user',
              body: 'Hello',
              id: 'message-001',
              timestamp,
            },
            {
              author: 'assistant',
              body: 'Hello final',
              id: 'message-002',
              timestamp,
            },
          ],
        },
      },
      {
        type: 'applyEvents',
        events: [
          event('run.started', { sessionId: 'conversation-001' }, { id: 'evt-run', sequence: 1 }),
          event(
            'assistant.delta',
            { text: 'Hello' },
            { id: 'evt-delta-1', source: 'assistant', sequence: 2 },
          ),
          event(
            'assistant.completed',
            { messageId: 'message-002', body: 'Hello final' },
            { id: 'evt-complete', source: 'assistant', sequence: 3 },
          ),
        ],
        cursor: 'evt-complete',
      },
    ])

    const assistantBlocks = state.blocks.filter(
      (block) => block.kind === 'assistantMessage' || block.kind === 'assistantStreaming',
    )

    expect(assistantBlocks).toHaveLength(1)
    expect(assistantBlocks[0]).toMatchObject({
      id: 'message:message-002',
      kind: 'assistantMessage',
      body: 'Hello final',
    })
  })

  it('reconciles snapshot after replay without duplicating assistant messages', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.delta',
            { text: 'Hello' },
            { id: 'evt-delta', source: 'assistant', sequence: 1 },
          ),
          event(
            'assistant.completed',
            { messageId: 'message-002', body: 'Hello final' },
            { id: 'evt-complete', source: 'assistant', sequence: 2 },
          ),
        ],
      },
      {
        type: 'snapshotReconciled',
        snapshot: {
          id: 'conversation-001',
          title: 'Conversation',
          modelConfigId: null,
          updatedAt: timestamp,
          messages: [
            {
              author: 'assistant',
              body: 'Hello final',
              id: 'message-002',
              timestamp,
            },
          ],
        },
      },
    ])

    const assistantBlocks = state.blocks.filter((block) => block.kind === 'assistantMessage')

    expect(assistantBlocks).toHaveLength(1)
  })

  it('creates live thinking blocks and removes them when the run ends', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.thinking.delta',
            { text: 'plan step' },
            { id: 'evt-thinking', source: 'assistant', sequence: 1 },
          ),
          event('run.ended', { reason: 'completed' }, { id: 'evt-ended', sequence: 2 }),
        ],
      },
    ])

    expect(state.blocks.some((block) => block.kind === 'thinking')).toBe(false)
  })

  it('keeps answer text separate from thinking deltas', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.thinking.delta',
            { text: 'hidden plan' },
            { id: 'evt-thinking', source: 'assistant', sequence: 1 },
          ),
          event(
            'assistant.delta',
            { text: 'Visible answer' },
            { id: 'evt-answer', source: 'assistant', sequence: 2 },
          ),
        ],
      },
    ])

    expect(state.blocks).toContainEqual(
      expect.objectContaining({ kind: 'thinking', body: 'hidden plan' }),
    )
    expect(state.blocks).toContainEqual(
      expect.objectContaining({ kind: 'assistantStreaming', body: 'Visible answer' }),
    )
  })

  it('removes thinking blocks when the engine fails', () => {
    const state = reduce([
      {
        type: 'applyEvents',
        events: [
          event(
            'assistant.thinking.delta',
            { text: 'plan step' },
            { id: 'evt-thinking', runId: 'run-001', source: 'assistant', sequence: 1 },
          ),
          event(
            'engine.failed',
            { message: 'model unavailable' },
            { id: 'evt-failed', runId: 'run-001', sequence: 2 },
          ),
        ],
      },
    ])

    expect(state.blocks.some((block) => block.kind === 'thinking')).toBe(false)
    expect(state.blocks.some((block) => block.kind === 'error')).toBe(true)
  })
})
