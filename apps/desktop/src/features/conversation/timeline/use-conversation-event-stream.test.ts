import { describe, expect, it } from 'vitest'

import type { RunEvent } from '@/shared/events/run-event-schema'
import { coalesceTimelineActions } from './use-conversation-event-stream'

const timestamp = '2026-06-17T00:00:00.000Z'

function deltaEvent(id: string, conversationSequence: number, text: string): RunEvent {
  return {
    conversationSequence,
    id,
    payload: { text },
    runId: 'run-001',
    sequence: conversationSequence,
    source: 'assistant',
    timestamp,
    type: 'assistant.delta',
    visibility: 'public',
  }
}

describe('coalesceTimelineActions', () => {
  it('merges consecutive event actions into one reducer update', () => {
    const firstEvent = deltaEvent('evt-001', 1, 'Hel')
    const secondEvent = deltaEvent('evt-002', 2, 'lo')

    expect(
      coalesceTimelineActions([
        { type: 'applyEvents', events: [firstEvent], cursor: 'evt-001' },
        { type: 'applyEvents', events: [secondEvent], cursor: 'evt-002' },
      ]),
    ).toEqual([{ type: 'applyEvents', events: [firstEvent, secondEvent], cursor: 'evt-002' }])
  })

  it('keeps gap ordering between event updates', () => {
    const firstEvent = deltaEvent('evt-001', 1, 'Hel')
    const secondEvent = deltaEvent('evt-002', 2, 'lo')

    expect(
      coalesceTimelineActions([
        { type: 'applyEvents', events: [firstEvent], cursor: 'evt-001' },
        { type: 'markGap', afterCursor: 'evt-001' },
        { type: 'applyEvents', events: [secondEvent], cursor: 'evt-002' },
      ]),
    ).toEqual([
      { type: 'applyEvents', events: [firstEvent], cursor: 'evt-001' },
      { type: 'markGap', afterCursor: 'evt-001' },
      { type: 'applyEvents', events: [secondEvent], cursor: 'evt-002' },
    ])
  })
})
