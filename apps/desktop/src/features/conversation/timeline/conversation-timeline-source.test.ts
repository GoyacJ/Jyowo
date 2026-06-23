import { describe, expect, it, vi } from 'vitest'
import type { RunEvent } from '@/shared/events/run-event-schema'
import type { CommandClient, ConversationEventBatchPayload } from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import type { ConversationTimelineAction } from './conversation-timeline-actions'
import { createConversationTimelineSource } from './conversation-timeline-source'

const timestamp = '2026-06-17T00:00:00.000Z'

function cursor(_label: string, conversationSequence = 1) {
  return { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence }
}

const replayEvent: RunEvent = {
  id: 'evt-replay',
  conversationSequence: 1,
  payload: { sessionId: 'conversation-001' },
  runId: 'run-001',
  sequence: 1,
  source: 'engine',
  timestamp,
  type: 'run.started',
  visibility: 'public',
}

const liveEvent: RunEvent = {
  id: 'evt-live',
  conversationSequence: 2,
  payload: { text: 'Hello' },
  runId: 'run-001',
  sequence: 2,
  source: 'assistant',
  timestamp,
  type: 'assistant.delta',
  visibility: 'public',
}

function createClient(overrides: Partial<CommandClient> = {}) {
  let listener: ((batch: ConversationEventBatchPayload) => void) | undefined
  const unlisten = vi.fn()
  const client = {
    ...createMockCommandClient(),
    listenConversationEventBatches: vi.fn(async (callback) => {
      listener = callback
      return unlisten
    }),
    subscribeConversationEvents: vi.fn(async () => ({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      replayEvents: [replayEvent],
      cursor: cursor(''),
      gap: false,
    })),
    unsubscribeConversationEvents: vi.fn(async (subscriptionId: string) => ({
      subscriptionId,
      status: 'unsubscribed' as const,
    })),
    ...overrides,
  } satisfies CommandClient

  return {
    client,
    emit(batch: ConversationEventBatchPayload) {
      listener?.(batch)
    },
    unlisten,
  }
}

describe('createConversationTimelineSource', () => {
  it('applies replayed events before live batches for the same subscription', async () => {
    const { client, emit } = createClient()
    const actions: ConversationTimelineAction[] = []

    await createConversationTimelineSource(client).subscribe('conversation-001', null, (action) => {
      actions.push(action)
    })
    emit({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      events: [liveEvent],
      cursor: cursor(''),
      gap: false,
      phase: 'live',
    })

    expect(actions).toEqual([
      { type: 'applyEvents', events: [replayEvent], cursor: cursor('') },
      { type: 'applyEvents', events: [liveEvent], cursor: cursor('') },
    ])
  })

  it('ignores stale subscription and stale conversation batches', async () => {
    const { client, emit } = createClient()
    const actions: ConversationTimelineAction[] = []

    await createConversationTimelineSource(client).subscribe('conversation-001', null, (action) => {
      actions.push(action)
    })
    emit({
      subscriptionId: 'subscription-old',
      conversationId: 'conversation-001',
      events: [liveEvent],
      cursor: cursor(''),
      gap: false,
      phase: 'live',
    })
    emit({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-999',
      events: [liveEvent],
      cursor: cursor(''),
      gap: false,
      phase: 'live',
    })

    expect(actions).toEqual([{ type: 'applyEvents', events: [replayEvent], cursor: cursor('') }])
  })

  it('unsubscribes and removes the shared tauri listener on cleanup', async () => {
    const { client, unlisten } = createClient()
    const cleanup = await createConversationTimelineSource(client).subscribe(
      'conversation-001',
      cursor('evt-before'),
      () => undefined,
    )

    await cleanup()

    expect(client.subscribeConversationEvents).toHaveBeenCalledWith({
      conversationId: 'conversation-001',
      afterCursor: cursor(''),
    })
    expect(unlisten).toHaveBeenCalledTimes(1)
    expect(client.unsubscribeConversationEvents).toHaveBeenCalledWith('subscription-001')
  })

  it('marks gap for replay and live overflow batches', async () => {
    const { client, emit } = createClient({
      subscribeConversationEvents: vi.fn(async () => ({
        subscriptionId: 'subscription-001',
        conversationId: 'conversation-001',
        replayEvents: [],
        cursor: cursor(''),
        gap: true,
      })),
    })
    const actions: ConversationTimelineAction[] = []

    await createConversationTimelineSource(client).subscribe('conversation-001', null, (action) => {
      actions.push(action)
    })
    emit({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      events: [liveEvent],
      cursor: cursor(''),
      gap: true,
      phase: 'live',
    })

    expect(actions).toEqual([
      { type: 'applyEvents', events: [], cursor: cursor('') },
      { type: 'markGap', afterCursor: cursor('') },
      { type: 'applyEvents', events: [liveEvent], cursor: cursor('') },
      { type: 'markGap', afterCursor: cursor('') },
    ])
  })

  it('falls back to gap state when subscribe fails', async () => {
    const { client } = createClient({
      subscribeConversationEvents: vi.fn(async () => {
        throw new Error('subscription unavailable')
      }),
    })
    const actions: ConversationTimelineAction[] = []

    await createConversationTimelineSource(client).subscribe(
      'conversation-001',
      cursor('evt-before'),
      (action) => {
        actions.push(action)
      },
    )

    expect(actions).toEqual([{ type: 'markGap', afterCursor: cursor('') }])
  })
})
