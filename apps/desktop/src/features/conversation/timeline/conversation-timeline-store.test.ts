import { describe, expect, it } from 'vitest'

import type { ConversationTimelineAction } from './conversation-timeline-actions'
import { selectBlocks } from './conversation-timeline-selectors'
import {
  conversationTimelineRootReducerFromAction,
  createConversationTimelineRoot,
  getConversationTimelineState,
} from './conversation-timeline-store'

const timestamp = '2026-06-17T00:00:00.000Z'

function cursor(_label: string, conversationSequence = 1) {
  return { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence }
}

function blocks(state: ReturnType<typeof getConversationTimelineState>) {
  return selectBlocks(state)
}

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

    expect(blocks(getConversationTimelineState(root, 'conversation-a'))).toHaveLength(1)
    expect(blocks(getConversationTimelineState(root, 'conversation-b'))).toHaveLength(1)
    expect(blocks(getConversationTimelineState(root, 'conversation-a'))[0]).toMatchObject({
      body: 'A',
    })
    expect(blocks(getConversationTimelineState(root, 'conversation-b'))[0]).toMatchObject({
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
      cursor: cursor(''),
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

    expect(conversationA.cursor).toEqual(cursor('evt-a-1'))
    expect(conversationA.eventIds['evt-a-1']).toBe(true)
  })
})
