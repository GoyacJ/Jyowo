import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { appI18n } from '@/shared/i18n/i18n'
import type { ConversationTurn } from '@/shared/tauri/commands'
import { ConversationTimeline } from './conversation-timeline'

const timestamp = '2026-06-17T00:00:00.000Z'

describe('ConversationTimeline', () => {
  it('keeps following the latest running turn as assistant work grows', async () => {
    const scrollIntoView = vi.fn()
    const scrollIntoViewSpy = vi
      .spyOn(Element.prototype, 'scrollIntoView')
      .mockImplementation(scrollIntoView)

    try {
      const rendered = render(
        <ConversationTimeline title="Streaming conversation" turns={[turn('Hel')]} />,
      )

      await waitFor(() => expect(scrollIntoView).toHaveBeenCalledTimes(1))

      rendered.rerender(
        <ConversationTimeline title="Streaming conversation" turns={[turn('Hello')]} />,
      )

      await waitFor(() => expect(scrollIntoView).toHaveBeenCalledTimes(2))
    } finally {
      scrollIntoViewSpy.mockRestore()
    }
  })

  it('renders one assistant work tree with nested tools, permissions, and final text', () => {
    const onOpenDetails = vi.fn()
    render(
      <ConversationTimeline
        onOpenDetails={onOpenDetails}
        title="Worktree conversation"
        turns={[turn('Final answer')]}
      />,
    )

    expect(screen.getByText('Prompt')).toBeInTheDocument()
    expect(screen.getAllByText('Jyowo')).toHaveLength(1)
    expect(screen.queryByText('Jyowo Complete')).not.toBeInTheDocument()
    expect(screen.getByText('Tools')).toBeInTheDocument()
    expect(screen.getByText('Execution: failed')).toBeInTheDocument()
    expect(screen.getByText('Permission: approved')).toBeInTheDocument()
    expect(screen.getByText('工具执行失败。详情可在 Activity 中查看。')).toBeInTheDocument()
    expect(screen.getByText('Final answer')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Details' }))
    expect(onOpenDetails).toHaveBeenCalledWith({
      eventId: 'event-tool',
      cursor: { eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV', conversationSequence: 3 },
    })
  })

  it('localizes execution and permission statuses in Chinese', async () => {
    await appI18n.changeLanguage('zh-CN')
    try {
      render(<ConversationTimeline title="Worktree conversation" turns={[turn('Final answer')]} />)

      expect(screen.getByText('执行：失败')).toBeInTheDocument()
      expect(screen.getByText('权限：已批准')).toBeInTheDocument()
      expect(screen.queryByText('Execution: failed')).not.toBeInTheDocument()
      expect(screen.queryByText('Permission: approved')).not.toBeInTheDocument()
      expect(screen.queryByText('completed')).not.toBeInTheDocument()
      expect(screen.queryByText('approved')).not.toBeInTheDocument()
    } finally {
      await appI18n.changeLanguage('en-US')
    }
  })

  it('renders review and clarification requests inside assistant work', () => {
    render(
      <ConversationTimeline
        title="Requests"
        turns={[
          {
            ...turn(''),
            assistant: {
              id: 'assistant:run-001',
              runId: 'run-001',
              status: 'complete',
              segments: [
                {
                  kind: 'reviewRequest',
                  id: 'segment:review:request-001',
                  order: 0,
                  requestId: 'request-001',
                  title: 'Review changes',
                  body: 'Confirm before applying.',
                },
                {
                  kind: 'clarificationRequest',
                  id: 'segment:clarification:request-002',
                  order: 1,
                  requestId: 'request-002',
                  prompt: 'Which style should I use?',
                },
              ],
            },
          },
        ]}
      />,
    )

    expect(screen.getByText('Review changes')).toBeInTheDocument()
    expect(screen.getByText('Which style should I use?')).toBeInTheDocument()
  })
})

function turn(finalBody: string): ConversationTurn {
  return {
    id: 'turn:user-message-001',
    conversationId: 'conversation-001',
    position: 0,
    user: {
      id: 'user:user-message-001',
      messageId: 'user-message-001',
      body: 'Prompt',
      timestamp,
    },
    assistant: {
      id: 'assistant:run-001',
      runId: 'run-001',
      status: 'running',
      segments: [
        {
          kind: 'thinking',
          id: 'segment:thinking:run-001',
          order: 0,
          status: 'withheld',
          summary: { text: '思考内容已折叠' },
        },
        {
          kind: 'toolGroup',
          id: 'segment:tools:tool-use-001',
          order: 1,
          attempts: [
            {
              id: 'tool:tool-use-001',
              order: 0,
              toolUseId: 'tool-use-001',
              toolName: 'read_file',
              status: 'failed',
              permission: {
                id: 'permission:request-001',
                requestId: 'request-001',
                toolUseId: 'tool-use-001',
                status: 'approved',
              },
              failureSummary: '工具执行失败。详情可在 Activity 中查看。',
              eventRefs: [
                {
                  eventId: 'event-tool',
                  cursor: {
                    eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
                    conversationSequence: 3,
                  },
                },
              ],
            },
          ],
        },
        {
          kind: 'text',
          id: 'segment:text:assistant-message-001',
          order: 2,
          messageId: 'assistant-message-001',
          body: finalBody,
        },
      ],
    },
  }
}
