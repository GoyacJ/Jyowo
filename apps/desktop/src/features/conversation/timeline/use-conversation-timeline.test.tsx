import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { CommandClient, ConversationEventBatchPayload } from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'
import { useConversationTimeline } from './use-conversation-timeline'

const timestamp = '2026-06-17T00:00:00.000Z'

function cursor(conversationSequence = 1) {
  return { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence }
}

function liveBatch(
  sequence: number,
  type: 'assistant.delta' | 'run.ended' = 'assistant.delta',
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
      gap: false,
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
    gap: false,
    phase: 'live',
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
    const baseClient = createMockCommandClient()
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
})
