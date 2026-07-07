import { describe, expect, it, vi } from 'vitest'
import type { CommandClient, ConversationEventBatchPayload } from '@/shared/tauri/commands'
import { createTestCommandClient } from '@/testing/command-client'
import type { ConversationTimelineAction } from './conversation-timeline-actions'
import { createConversationTimelineSource } from './conversation-timeline-source'

const timestamp = '2026-06-17T00:00:00.000Z'
const runModelSnapshot = {
  modelConfigId: 'provider-config-001',
  providerId: 'openai',
  modelId: 'gpt-4.1',
  displayName: 'GPT-4.1',
  protocol: 'responses' as const,
  contextWindow: 128000,
  maxOutputTokens: 16384,
  conversationCapability: {
    inputModalities: ['text', 'image'] as Array<'text' | 'image'>,
    outputModalities: ['text'] as Array<'text'>,
    contextWindow: 128000,
    maxOutputTokens: 16384,
    streaming: true,
    toolCalling: true,
    reasoning: false,
    promptCache: true,
    structuredOutput: true,
  },
}

function cursor(conversationSequence = 1) {
  return { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}

function nextTask() {
  return new Promise<void>((resolve) => setTimeout(resolve, 0))
}

const replayEvent: ConversationEventBatchPayload['events'][number] = {
  id: 'evt-replay',
  conversationSequence: 1,
  payload: { sessionId: 'conversation-001', model: runModelSnapshot },
  runId: 'run-001',
  sequence: 1,
  source: 'engine',
  timestamp,
  type: 'run.started',
  visibility: 'public',
}

const liveEvent: ConversationEventBatchPayload['events'][number] = {
  id: 'evt-live',
  conversationSequence: 2,
  payload: { messageId: 'message-live', text: 'Hello' },
  runId: 'run-001',
  sequence: 2,
  source: 'assistant',
  timestamp,
  type: 'assistant.delta',
  visibility: 'public',
}

const terminalEvent: ConversationEventBatchPayload['events'][number] = {
  id: 'evt-terminal',
  conversationSequence: 3,
  payload: { reason: 'completed' },
  runId: 'run-001',
  sequence: 3,
  source: 'engine',
  timestamp,
  type: 'run.ended',
  visibility: 'public',
}

function createClient(overrides: Partial<CommandClient> = {}) {
  let listener: ((batch: ConversationEventBatchPayload) => void) | undefined
  const unlisten = vi.fn()
  const client = {
    ...createTestCommandClient(),
    listenConversationEventBatches: vi.fn(async (callback) => {
      listener = callback
      return unlisten
    }),
    subscribeConversationEvents: vi.fn(async () => ({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      replayEvents: [replayEvent],
      cursor: cursor(),
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
  it('uses replay and live batches as worktree refetch signals', async () => {
    const { client, emit } = createClient()
    const actions: ConversationTimelineAction[] = []

    await createConversationTimelineSource(client).subscribe('conversation-001', null, (action) => {
      actions.push(action)
    })
    emit({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      events: [liveEvent],
      cursor: cursor(),
      gap: false,
      phase: 'live',
    })

    expect(actions).toEqual([
      { type: 'worktreeRefreshRequested', immediate: true },
      { type: 'worktreeRefreshRequested', immediate: false },
    ])
  })

  it('marks terminal raw events as immediate projection refetches', async () => {
    const { client, emit } = createClient({
      subscribeConversationEvents: vi.fn(async () => ({
        subscriptionId: 'subscription-001',
        conversationId: 'conversation-001',
        replayEvents: [],
        cursor: cursor(),
        gap: false,
      })),
    })
    const actions: ConversationTimelineAction[] = []

    await createConversationTimelineSource(client).subscribe('conversation-001', null, (action) => {
      actions.push(action)
    })
    emit({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      events: [terminalEvent],
      cursor: cursor(3),
      gap: false,
      phase: 'live',
    })

    expect(actions).toEqual([
      { type: 'worktreeRefreshRequested', immediate: false },
      { type: 'worktreeRefreshRequested', immediate: true },
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
      cursor: cursor(),
      gap: false,
      phase: 'live',
    })
    emit({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-999',
      events: [liveEvent],
      cursor: cursor(),
      gap: false,
      phase: 'live',
    })

    expect(actions).toEqual([{ type: 'worktreeRefreshRequested', immediate: true }])
  })

  it('unsubscribes and removes the shared tauri listener on cleanup', async () => {
    const { client, unlisten } = createClient()
    const cleanup = await createConversationTimelineSource(client).subscribe(
      'conversation-001',
      cursor(),
      () => undefined,
    )

    await cleanup()

    expect(client.subscribeConversationEvents).toHaveBeenCalledWith({
      conversationId: 'conversation-001',
      afterCursor: cursor(),
    })
    expect(unlisten).toHaveBeenCalledTimes(1)
    expect(client.unsubscribeConversationEvents).toHaveBeenCalledWith('subscription-001')
  })

  it('uses replay and live gap as internal resync signals', async () => {
    const subscribeConversationEvents = vi
      .fn<CommandClient['subscribeConversationEvents']>()
      .mockResolvedValueOnce({
        subscriptionId: 'subscription-001',
        conversationId: 'conversation-001',
        replayEvents: [],
        cursor: cursor(),
        gap: true,
      })
      .mockResolvedValueOnce({
        subscriptionId: 'subscription-002',
        conversationId: 'conversation-001',
        replayEvents: [],
        cursor: cursor(2),
        gap: false,
      })
    const { client, emit } = createClient({
      subscribeConversationEvents,
    })
    const actions: ConversationTimelineAction[] = []

    await createConversationTimelineSource(client).subscribe('conversation-001', null, (action) => {
      actions.push(action)
    })
    emit({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      events: [liveEvent],
      cursor: cursor(),
      gap: true,
      phase: 'live',
    })
    await Promise.resolve()
    await Promise.resolve()

    expect(actions).toEqual([
      { type: 'worktreeRefreshRequested', immediate: true },
      { type: 'worktreeRefreshRequested', immediate: true },
    ])
    expect(subscribeConversationEvents).toHaveBeenCalledTimes(2)
    expect(subscribeConversationEvents).toHaveBeenNthCalledWith(2, {
      conversationId: 'conversation-001',
      afterCursor: cursor(),
    })
  })

  it('serializes live gap resubscriptions', async () => {
    const pendingSubscription =
      deferred<Awaited<ReturnType<CommandClient['subscribeConversationEvents']>>>()
    const subscribeConversationEvents = vi
      .fn<CommandClient['subscribeConversationEvents']>()
      .mockResolvedValueOnce({
        subscriptionId: 'subscription-001',
        conversationId: 'conversation-001',
        replayEvents: [],
        cursor: cursor(),
        gap: false,
      })
      .mockReturnValueOnce(pendingSubscription.promise)
      .mockResolvedValueOnce({
        subscriptionId: 'subscription-003',
        conversationId: 'conversation-001',
        replayEvents: [],
        cursor: cursor(3),
        gap: false,
      })
    const { client, emit } = createClient({
      subscribeConversationEvents,
    })
    const cleanup = await createConversationTimelineSource(client).subscribe(
      'conversation-001',
      null,
      () => undefined,
    )

    emit({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      events: [],
      cursor: cursor(),
      gap: true,
      phase: 'live',
    })
    emit({
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      events: [],
      cursor: cursor(2),
      gap: true,
      phase: 'live',
    })
    await Promise.resolve()
    await Promise.resolve()

    expect(subscribeConversationEvents).toHaveBeenCalledTimes(2)

    pendingSubscription.resolve({
      subscriptionId: 'subscription-002',
      conversationId: 'conversation-001',
      replayEvents: [],
      cursor: cursor(2),
      gap: false,
    })
    await Promise.resolve()
    await nextTask()

    expect(subscribeConversationEvents).toHaveBeenCalledTimes(3)
    expect(client.unsubscribeConversationEvents).toHaveBeenCalledWith('subscription-001')
    expect(client.unsubscribeConversationEvents).toHaveBeenCalledWith('subscription-002')

    await cleanup()
  })

  it('retries stale subscribe failures as resync without visible gap actions', async () => {
    const subscribeConversationEvents = vi
      .fn<CommandClient['subscribeConversationEvents']>()
      .mockRejectedValueOnce(new Error('conversation cursor is unknown'))
      .mockResolvedValueOnce({
        subscriptionId: 'subscription-002',
        conversationId: 'conversation-001',
        replayEvents: [],
        cursor: cursor(2),
        gap: true,
      })
    const { client } = createClient({
      subscribeConversationEvents,
    })
    const actions: ConversationTimelineAction[] = []

    await createConversationTimelineSource(client).subscribe(
      'conversation-001',
      cursor(),
      (action) => {
        actions.push(action)
      },
    )

    expect(subscribeConversationEvents).toHaveBeenNthCalledWith(1, {
      conversationId: 'conversation-001',
      afterCursor: cursor(),
    })
    expect(subscribeConversationEvents).toHaveBeenNthCalledWith(2, {
      conversationId: 'conversation-001',
    })
    expect(actions).toEqual([
      { type: 'worktreeRefreshRequested', immediate: true },
      { type: 'worktreeRefreshRequested', immediate: true },
    ])
  })
})
