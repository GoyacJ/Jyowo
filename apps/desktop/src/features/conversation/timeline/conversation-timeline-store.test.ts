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
    draft: { prompt },
    at: timestamp,
  }
}
