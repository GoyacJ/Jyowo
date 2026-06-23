import { describe, expect, it } from 'vitest'

import type { ConversationTimelineAction } from './conversation-timeline-actions'
import {
  conversationTimelineRootReducerFromAction,
  createConversationTimelineRoot,
  getConversationTimelineState,
} from './conversation-timeline-store'

const timestamp = '2026-06-17T00:00:00.000Z'

describe('conversationTimelineStore', () => {
  it('keeps independent buckets per conversation', () => {
    let root = createConversationTimelineRoot()

    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: {
        type: 'localSubmit',
        clientMessageId: 'client-a',
        draft: { prompt: 'A' },
        at: timestamp,
      },
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-b',
      action: {
        type: 'localSubmit',
        clientMessageId: 'client-b',
        draft: { prompt: 'B' },
        at: timestamp,
      },
    })

    expect(getConversationTimelineState(root, 'conversation-a').blocks).toHaveLength(1)
    expect(getConversationTimelineState(root, 'conversation-b').blocks).toHaveLength(1)
    expect(getConversationTimelineState(root, 'conversation-a').blocks[0]).toMatchObject({
      body: 'A',
    })
    expect(getConversationTimelineState(root, 'conversation-b').blocks[0]).toMatchObject({
      body: 'B',
    })
  })

  it('preserves cursor and events when switching conversations', () => {
    let root = createConversationTimelineRoot()
    const applyA: ConversationTimelineAction = {
      type: 'applyEvents',
      events: [
        {
          id: 'evt-a-1',
          conversationSequence: 1,
          payload: { sessionId: 'conversation-a' },
          runId: 'run-a',
          sequence: 1,
          source: 'engine',
          timestamp,
          type: 'run.started',
          visibility: 'public',
        },
      ],
      cursor: 'evt-a-1',
    }

    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-a',
      action: applyA,
    })
    root = conversationTimelineRootReducerFromAction(root, {
      conversationId: 'conversation-b',
      action: {
        type: 'localSubmit',
        clientMessageId: 'client-b',
        draft: { prompt: 'B' },
        at: timestamp,
      },
    })

    const conversationA = getConversationTimelineState(root, 'conversation-a')

    expect(conversationA.cursor).toBe('evt-a-1')
    expect(conversationA.eventsById['evt-a-1']).toBe(true)
  })
})
