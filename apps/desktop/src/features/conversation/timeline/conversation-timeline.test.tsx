import '@testing-library/jest-dom/vitest'

import { render, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { AssistantStreamingBlock } from './conversation-blocks'
import { ConversationTimeline } from './conversation-timeline'

const timestamp = '2026-06-17T00:00:00.000Z'

function streamingBlock(body: string): AssistantStreamingBlock {
  return {
    body,
    conversationId: 'conversation-001',
    conversationSequence: 1,
    createdAt: timestamp,
    id: 'assistant-stream:run-001',
    kind: 'assistantStreaming',
    runId: 'run-001',
    status: 'streaming',
    updatedAt: timestamp,
  }
}

describe('ConversationTimeline', () => {
  it('keeps following the latest streaming block as its body grows', async () => {
    const scrollIntoView = vi.fn()
    const scrollIntoViewSpy = vi
      .spyOn(Element.prototype, 'scrollIntoView')
      .mockImplementation(scrollIntoView)

    try {
      const rendered = render(
        <ConversationTimeline blocks={[streamingBlock('Hel')]} title="Streaming conversation" />,
      )

      await waitFor(() => expect(scrollIntoView).toHaveBeenCalledTimes(1))

      rendered.rerender(
        <ConversationTimeline blocks={[streamingBlock('Hello')]} title="Streaming conversation" />,
      )

      await waitFor(() => expect(scrollIntoView).toHaveBeenCalledTimes(2))
    } finally {
      scrollIntoViewSpy.mockRestore()
    }
  })
})
