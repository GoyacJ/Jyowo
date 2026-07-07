import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type {
  CommandClient,
  ConversationEventBatchPayload,
  ConversationTurn,
  PageConversationWorktreeResponse,
} from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import { useConversationTimeline } from './use-conversation-timeline'

const timestamp = '2026-06-17T00:00:00.000Z'

function cursor(conversationSequence = 1) {
  return { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence }
}

function liveBatch(
  sequence: number,
  type: 'assistant.delta' | 'run.ended' = 'assistant.delta',
  gap = false,
): ConversationEventBatchPayload {
  if (type === 'run.ended') {
    return {
      subscriptionId: 'subscription-001',
      conversationId: 'conversation-001',
      events: [
        {
          id: `evt-${sequence}`,
          conversationSequence: sequence,
          payload: { reason: 'completed' },
          runId: 'run-001',
          sequence,
          source: 'engine',
          timestamp,
          type,
          visibility: 'public',
        },
      ],
      cursor: cursor(sequence),
      gap,
      phase: 'live',
    }
  }

  return {
    subscriptionId: 'subscription-001',
    conversationId: 'conversation-001',
    events: [
      {
        id: `evt-${sequence}`,
        conversationSequence: sequence,
        payload: { messageId: `message-${sequence}`, text: `delta-${sequence}` },
        runId: 'run-001',
        sequence,
        source: 'assistant',
        timestamp,
        type,
        visibility: 'public',
      },
    ],
    cursor: cursor(sequence),
    gap,
    phase: 'live',
  }
}

function worktreeTurn(position: number): ConversationTurn {
  return {
    id: `turn-${position}`,
    conversationId: 'conversation-001',
    position,
    user: {
      id: `user-${position}`,
      messageId: `message-${position}`,
      body: `turn ${position}`,
      timestamp,
    },
  }
}

function worktreePage(
  turns: ConversationTurn[],
  {
    hasMoreAfter = false,
    hasMoreBefore = false,
  }: {
    hasMoreAfter?: boolean
    hasMoreBefore?: boolean
  } = {},
): PageConversationWorktreeResponse {
  const cursorTurn = turns[0]
  return {
    turns,
    pageCursor: cursorTurn
      ? {
          turnId: cursorTurn.id,
          position: cursorTurn.position,
        }
      : undefined,
    eventCursor: cursor(turns.at(-1)?.position ?? 1),
    hasMoreBefore,
    hasMoreAfter,
    gap: false,
  }
}

function renderTimelineHook(commandClient: CommandClient) {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { gcTime: 0, retry: false },
    },
  })

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <CommandClientProvider client={commandClient}>
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      </CommandClientProvider>
    )
  }

  return renderHook(() => useConversationTimeline({ conversationId: 'conversation-001' }), {
    wrapper: Wrapper,
  })
}

async function flushAsync(ms = 0) {
  await act(async () => {
    if (ms > 0) {
      await vi.advanceTimersByTimeAsync(ms)
    }
    await Promise.resolve()
  })
}

async function flushUntil(assertion: () => void) {
  let lastError: unknown
  for (let attempt = 0; attempt < 20; attempt += 1) {
    try {
      assertion()
      return
    } catch (error) {
      lastError = error
      await flushAsync()
    }
  }
  assertion()
  throw lastError
}

describe('useConversationTimeline', () => {
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('coalesces streaming batches into one delayed worktree refetch and refetches terminal batches immediately', async () => {
    vi.useFakeTimers()

    let listener: ((batch: ConversationEventBatchPayload) => void) | undefined
    const baseClient = createTestCommandClient()
    const pageConversationWorktree = vi.fn(baseClient.pageConversationWorktree)
    const commandClient = {
      ...baseClient,
      listenConversationEventBatches: vi.fn(async (callback) => {
        listener = callback
        return () => undefined
      }),
      pageConversationWorktree,
      subscribeConversationEvents: vi.fn(async () => ({
        subscriptionId: 'subscription-001',
        conversationId: 'conversation-001',
        replayEvents: [],
        cursor: cursor(),
        gap: false,
      })),
    } satisfies CommandClient

    renderTimelineHook(commandClient)

    await flushUntil(() => {
      expect(listener).toBeDefined()
      expect(pageConversationWorktree).toHaveBeenCalled()
    })
    expect(pageConversationWorktree).toHaveBeenCalledWith({
      conversationId: 'conversation-001',
      direction: 'before',
      limit: 100,
    })

    await flushAsync(520)
    pageConversationWorktree.mockClear()

    await flushAsync()
    act(() => {
      listener?.(liveBatch(2))
    })
    await flushAsync(250)
    act(() => {
      listener?.(liveBatch(3))
    })

    expect(pageConversationWorktree).not.toHaveBeenCalled()

    await flushAsync(249)
    expect(pageConversationWorktree).not.toHaveBeenCalled()

    await flushAsync(1)
    await flushUntil(() => expect(pageConversationWorktree).toHaveBeenCalledTimes(1))

    pageConversationWorktree.mockClear()

    act(() => {
      listener?.(liveBatch(200, 'run.ended'))
    })
    await flushAsync(16)

    await flushUntil(() => expect(pageConversationWorktree).toHaveBeenCalledTimes(1))
  })

  it('loads earlier and newer worktree pages from the current page cursor', async () => {
    const initialPage = worktreePage([worktreeTurn(10), worktreeTurn(11)], {
      hasMoreAfter: true,
      hasMoreBefore: true,
    })
    const earlierPage = worktreePage([worktreeTurn(8), worktreeTurn(9)], {
      hasMoreBefore: false,
    })
    const laterPage = worktreePage([worktreeTurn(12), worktreeTurn(13)], {
      hasMoreAfter: false,
    })
    const baseClient = createTestCommandClient()
    const pageConversationWorktree = vi.fn(
      async (request: Parameters<CommandClient['pageConversationWorktree']>[0]) => {
        if (!request.pageCursor) {
          return initialPage
        }
        return request.direction === 'before' ? earlierPage : laterPage
      },
    )
    const commandClient = {
      ...baseClient,
      pageConversationWorktree,
    } satisfies CommandClient

    const { result } = renderTimelineHook(commandClient)

    await flushUntil(() => {
      expect(result.current.hasMoreBefore).toBe(true)
      expect(result.current.hasMoreAfter).toBe(true)
    })
    pageConversationWorktree.mockClear()

    await act(async () => {
      await result.current.loadEarlier()
    })

    expect(pageConversationWorktree).toHaveBeenCalledWith({
      conversationId: 'conversation-001',
      direction: 'before',
      pageCursor: { turnId: 'turn-10', position: 10 },
      limit: 50,
    })

    await act(async () => {
      await result.current.loadLater()
    })

    expect(pageConversationWorktree).toHaveBeenCalledWith({
      conversationId: 'conversation-001',
      direction: 'after',
      pageCursor: { turnId: 'turn-11', position: 11 },
      limit: 50,
    })
  })

  it('refetches the canonical worktree and resubscribes after a live gap signal', async () => {
    vi.useFakeTimers()

    let listener: ((batch: ConversationEventBatchPayload) => void) | undefined
    const baseClient = createTestCommandClient()
    const pageConversationWorktree = vi.fn(baseClient.pageConversationWorktree)
    const subscribeConversationEvents = vi
      .fn<CommandClient['subscribeConversationEvents']>()
      .mockResolvedValueOnce({
        subscriptionId: 'subscription-001',
        conversationId: 'conversation-001',
        replayEvents: [],
        cursor: cursor(1),
        gap: false,
      })
      .mockResolvedValueOnce({
        subscriptionId: 'subscription-002',
        conversationId: 'conversation-001',
        replayEvents: [],
        cursor: cursor(2),
        gap: false,
      })
    const commandClient = {
      ...baseClient,
      listenConversationEventBatches: vi.fn(async (callback) => {
        listener = callback
        return () => undefined
      }),
      pageConversationWorktree,
      subscribeConversationEvents,
    } satisfies CommandClient

    renderTimelineHook(commandClient)

    await flushUntil(() => {
      expect(listener).toBeDefined()
      expect(pageConversationWorktree).toHaveBeenCalled()
      expect(subscribeConversationEvents).toHaveBeenCalledTimes(1)
    })
    pageConversationWorktree.mockClear()

    act(() => {
      listener?.({
        ...liveBatch(2, 'assistant.delta', true),
        cursor: cursor(2),
      })
    })
    await flushAsync(16)

    await flushUntil(() => expect(pageConversationWorktree).toHaveBeenCalled())
    await flushUntil(() => expect(subscribeConversationEvents).toHaveBeenCalledTimes(2))
    expect(subscribeConversationEvents).toHaveBeenNthCalledWith(2, {
      conversationId: 'conversation-001',
      afterCursor: cursor(2),
    })
  })

  it('exposes canonical worktree query errors', async () => {
    const baseClient = createTestCommandClient()
    const commandClient = {
      ...baseClient,
      pageConversationWorktree: vi.fn(async () => {
        throw new Error('worktree unavailable')
      }),
    } satisfies CommandClient

    const { result } = renderTimelineHook(commandClient)

    await flushUntil(() => expect(result.current.error).toBeInstanceOf(Error))
    expect(result.current.error).toMatchObject({ message: 'worktree unavailable' })
  })
})
