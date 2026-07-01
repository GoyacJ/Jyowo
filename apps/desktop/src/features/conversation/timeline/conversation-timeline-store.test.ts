import { describe, expect, it } from 'vitest'

import type { ConversationTimelineAction } from './conversation-timeline-actions'
import { selectTurns } from './conversation-timeline-selectors'
import {
  conversationTimelineRootReducerFromAction,
  createConversationTimelineRoot,
  getConversationTimelineState,
} from './conversation-timeline-store'

const timestamp = '2026-06-17T00:00:00.000Z'

function cursor(conversationSequence = 1) {
  return { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence }
}

describe('conversationTimelineStore', () => {
  it('keeps independent projected turn buckets per conversation', () => {
    let root = createConversationTimelineRoot()

    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: localSubmit('client-a', 'A'),
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-b',
      action: localSubmit('client-b', 'B'),
    })

    expect(selectTurns(getConversationTimelineState(root, 'conversation-a'))[0].user.body).toBe('A')
    expect(selectTurns(getConversationTimelineState(root, 'conversation-b'))[0].user.body).toBe('B')
  })

  it('hydrates worktree pages and replaces matching optimistic turns by clientMessageId', () => {
    let root = createConversationTimelineRoot()

    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: localSubmit('client-a', 'A'),
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: {
        type: 'hydrateWorktree',
        page: {
          turns: [
            {
              id: 'turn:user-message-001',
              conversationId: 'conversation-a',
              position: 0,
              user: {
                id: 'user:user-message-001',
                messageId: 'user-message-001',
                clientMessageId: 'client-a',
                body: 'A',
                timestamp,
              },
              assistant: {
                id: 'assistant:run-001',
                runId: 'run-001',
                status: 'complete',
                segments: [],
              },
            },
          ],
          pageCursor: { turnId: 'turn:user-message-001', position: 0 },
          eventCursor: cursor(),
          hasMoreBefore: false,
          hasMoreAfter: false,
          gap: false,
        },
      },
    })

    const state = getConversationTimelineState(root, 'conversation-a')

    expect(selectTurns(state)).toHaveLength(1)
    expect(selectTurns(state)[0].id).toBe('turn:user-message-001')
    expect(state.eventCursor).toEqual(cursor())
    expect(state.activeRunIds).toEqual([])
  })

  it('hydrates worktree pages and replaces accepted optimistic turns by runId when clientMessageId is missing', () => {
    let root = createConversationTimelineRoot()

    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: localSubmit('550e8400-e29b-41d4-a716-446655440000', 'A'),
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: {
        type: 'commandAccepted',
        clientMessageId: '550e8400-e29b-41d4-a716-446655440000',
        runId: 'run-001',
      },
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: {
        type: 'hydrateWorktree',
        page: {
          turns: [
            {
              id: 'turn:user-message-001',
              conversationId: 'conversation-a',
              position: 0,
              user: {
                id: 'user:user-message-001',
                messageId: 'user-message-001',
                body: 'A',
                timestamp,
              },
              assistant: {
                id: 'assistant:run-001',
                runId: 'run-001',
                status: 'complete',
                segments: [],
              },
            },
          ],
          pageCursor: { turnId: 'turn:user-message-001', position: 0 },
          eventCursor: cursor(),
          hasMoreBefore: false,
          hasMoreAfter: false,
          gap: false,
        },
      },
    })

    const state = getConversationTimelineState(root, 'conversation-a')

    expect(selectTurns(state)).toHaveLength(1)
    expect(selectTurns(state)[0].id).toBe('turn:user-message-001')
    expect(state.activeRunIds).toEqual([])
  })

  it('redacts obvious secrets and private paths in optimistic user turns', () => {
    let root = createConversationTimelineRoot()

    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: localSubmit('client-a', 'Use token: abcdefghijklmnop from /Users/goya/.ssh/config'),
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-b',
      action: localSubmit('client-b', 'Inspect C:/Users/goya/.ssh/config'),
    })

    expect(selectTurns(getConversationTimelineState(root, 'conversation-a'))[0].user.body).toBe(
      '[REDACTED]',
    )
    expect(selectTurns(getConversationTimelineState(root, 'conversation-b'))[0].user.body).toBe(
      '[REDACTED]',
    )
  })

  it('redacts bare auth secrets in optimistic user turns', () => {
    let root = createConversationTimelineRoot()

    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: localSubmit('client-a', 'Bearer abcdefghijklmnop'),
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-b',
      action: localSubmit('client-b', 'client_secret: abcdefghijklmnop'),
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-c',
      action: localSubmit('client-c', 'OPENAI_API_KEY sk-abcdefghijklmnop'),
    })

    expect(selectTurns(getConversationTimelineState(root, 'conversation-a'))[0].user.body).toBe(
      '[REDACTED]',
    )
    expect(selectTurns(getConversationTimelineState(root, 'conversation-b'))[0].user.body).toBe(
      '[REDACTED]',
    )
    expect(selectTurns(getConversationTimelineState(root, 'conversation-c'))[0].user.body).toBe(
      '[REDACTED]',
    )
  })

  it('redacts failed command messages before storing error segments', () => {
    let root = createConversationTimelineRoot()

    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: localSubmit('client-a', 'safe prompt'),
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: {
        type: 'commandFailed',
        clientMessageId: 'client-a',
        errorMessage: 'runtime failed with Bearer abcdefghijklmnop',
      },
    })

    const segment = selectTurns(getConversationTimelineState(root, 'conversation-a'))[0].assistant
      ?.segments[0]

    if (segment?.kind !== 'error') {
      throw new Error('Expected error segment')
    }

    expect(segment.body).toBe('[REDACTED]')
  })

  it('redacts failed permission summaries before storing tool attempts', () => {
    let root = createConversationTimelineRoot()

    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: {
        type: 'hydrateWorktree',
        page: {
          turns: [
            {
              id: 'turn:user-message-001',
              conversationId: 'conversation-a',
              position: 0,
              user: {
                id: 'user:user-message-001',
                messageId: 'user-message-001',
                clientMessageId: 'client-a',
                body: 'A',
                timestamp,
              },
              assistant: {
                id: 'assistant:run-001',
                runId: 'run-001',
                status: 'running',
                segments: [
                  {
                    kind: 'toolGroup',
                    id: 'segment:tool-group:001',
                    order: 0,
                    attempts: [
                      {
                        id: 'tool-attempt-001',
                        order: 0,
                        toolUseId: 'tool-use-001',
                        toolName: 'bash',
                        status: 'waitingPermission',
                        permission: {
                          id: 'permission-001',
                          requestId: 'permission-request-001',
                          toolUseId: 'tool-use-001',
                          status: 'pending',
                        },
                      },
                    ],
                  },
                ],
              },
            },
          ],
          pageCursor: { turnId: 'turn:user-message-001', position: 0 },
          eventCursor: cursor(),
          hasMoreBefore: false,
          hasMoreAfter: false,
          gap: false,
        },
      },
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: {
        type: 'permissionSubmitFailed',
        requestId: 'permission-request-001',
        errorMessage: 'failed with client_secret abcdefghijklmnop',
      },
    })

    const segment = selectTurns(getConversationTimelineState(root, 'conversation-a'))[0].assistant
      ?.segments[0]

    if (segment?.kind !== 'toolGroup') {
      throw new Error('Expected tool group segment')
    }

    expect(segment.attempts[0].permission?.status).toBe('failed')
    expect(segment.attempts[0].permission?.summary).toBe('[REDACTED]')
  })

  it('tracks raw event stream refresh requests without storing raw events', () => {
    let root = createConversationTimelineRoot()

    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: { type: 'worktreeRefreshRequested', immediate: false },
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: { type: 'worktreeRefreshRequested', immediate: true },
    })

    const state = getConversationTimelineState(root, 'conversation-a')

    expect(state.refreshRequests).toBe(2)
    expect(state.immediateRefreshRequests).toBe(1)
  })
})

function localSubmit(clientMessageId: string, prompt: string): ConversationTimelineAction {
  return {
    type: 'localSubmit',
    clientMessageId,
    draft: { modelConfigId: 'provider-config-001', prompt },
    at: timestamp,
  }
}
