import { describe, expect, it } from 'vitest'

import type { ConversationBlock } from './conversation-blocks'
import { selectTurnGroups } from './conversation-timeline-selectors'

describe('selectTurnGroups', () => {
  it('groups consecutive blocks by turnId', () => {
    const blocks = [
      block({ id: 'user-1', kind: 'userMessage', turnId: 'turn-1' }),
      block({ id: 'thinking-1', kind: 'thinking', turnId: 'turn-1' }),
      block({ id: 'answer-1', kind: 'assistantMessage', turnId: 'turn-1' }),
      block({ id: 'user-2', kind: 'userMessage', turnId: 'turn-2' }),
    ] satisfies ConversationBlock[]

    expect(selectTurnGroups(blocks)).toEqual([
      {
        turnId: 'turn-1',
        blocks: [blocks[0], blocks[1], blocks[2]],
      },
      {
        turnId: 'turn-2',
        blocks: [blocks[3]],
      },
    ])
  })
})

function block(input: {
  id: string
  kind: ConversationBlock['kind']
  turnId?: string
}): ConversationBlock {
  return {
    id: input.id,
    kind: input.kind,
    conversationId: 'conversation-001',
    conversationSequence: 1,
    createdAt: '2026-06-23T00:00:00.000Z',
    turnId: input.turnId,
    body: 'body',
    status: input.kind === 'thinking' ? 'streaming' : 'complete',
    collapsed: true,
  } as ConversationBlock
}
