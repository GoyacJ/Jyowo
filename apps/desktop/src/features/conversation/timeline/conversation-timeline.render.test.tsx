import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { uiStore } from '@/shared/state/ui-store'
import type { ConversationTurn } from '@/shared/tauri/commands'
import { createTestCommandClient } from '@/testing/command-client'
import { assistantWork } from '@/testing/conversation-worktree-builders'
import { ConversationTimeline } from './conversation-timeline'
import {
  imageProcessTurn,
  renderTimelineWithClient,
  resetTimelineTestState,
  timestamp,
  toolEvidenceTurn,
  turn,
} from './conversation-timeline-test-utils'

describe('ConversationTimeline', () => {
  afterEach(() => {
    resetTimelineTestState()
  })

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

      const nextTurn = turn('Hello')
      if (!nextTurn.assistant) {
        throw new Error('test fixture must include assistant work')
      }
      rendered.rerender(
        <ConversationTimeline
          title="Streaming conversation"
          turns={[
            {
              ...nextTurn,
              assistant: {
                ...nextTurn.assistant,
                streamVersion: 1,
              },
            },
          ]}
        />,
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
    expect(screen.getByText('GPT-4.1')).toBeInTheDocument()
    expect(screen.queryByText('Jyowo Complete')).not.toBeInTheDocument()
    expect(screen.getByText('Tools')).toBeInTheDocument()
    expect(screen.getByText('Execution: failed')).toBeInTheDocument()
    expect(screen.getByText('Permission: approved')).toBeInTheDocument()
    expect(screen.getByText('工具执行失败。可在详情中查看。')).toBeInTheDocument()
    expect(screen.getByText('Final answer')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Details' }))
    expect(onOpenDetails).toHaveBeenCalledWith({
      eventId: 'event-tool',
      cursor: {
        eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
        conversationSequence: 3,
      },
    })
  })

  it('renders user timestamp and copies only the user message body', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    const originalClipboard = navigator.clipboard
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    const messageTurn = {
      ...turn('Final answer', 'user-copy'),
      user: {
        ...turn('Final answer', 'user-copy').user,
        body: 'Prompt with attachment',
        attachments: [
          {
            id: 'attachment-copy',
            name: 'reference.png',
            mimeType: 'image/png',
            sizeBytes: 2048,
            blobRef: {
              id: 'blob-copy',
              size: 2048,
              contentHash: new Array(32).fill(1),
              contentType: 'image/png',
            },
          },
        ],
      },
    } satisfies ConversationTurn

    try {
      renderTimelineWithClient(
        <ConversationTimeline title="User message actions" turns={[messageTurn]} />,
        createTestCommandClient(),
      )

      const time = screen.getByTitle('Message timestamp')
      expect(time.tagName).toBe('TIME')
      expect(time).toHaveAttribute('dateTime', timestamp)

      fireEvent.click(screen.getByRole('button', { name: 'Copy message' }))

      expect(writeText).toHaveBeenCalledWith('Prompt with attachment')
      expect(writeText).not.toHaveBeenCalledWith(expect.stringContaining('reference.png'))
    } finally {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: originalClipboard,
      })
    }
  })

  it('keeps timeline bottom padding large enough for the composer reserve', () => {
    render(<ConversationTimeline title="Composer padding" turns={[turn('Final answer')]} />)

    expect(screen.getByTestId('conversation-timeline-scroll-content')).toHaveClass('pb-28')
  })

  it('renders paging controls and calls the supplied loaders', () => {
    const loadEarlier = vi.fn()
    const loadLater = vi.fn()
    render(
      <ConversationTimeline
        hasMoreAfter
        hasMoreBefore
        loadEarlier={loadEarlier}
        loadLater={loadLater}
        title="Paged conversation"
        turns={[turn('Final answer')]}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Load earlier' }))
    fireEvent.click(screen.getByRole('button', { name: 'Load newer' }))

    expect(loadEarlier).toHaveBeenCalledTimes(1)
    expect(loadLater).toHaveBeenCalledTimes(1)
  })

  it('keeps the viewport anchored when earlier turns are loaded', async () => {
    const originalRequestAnimationFrame = window.requestAnimationFrame
    window.requestAnimationFrame = (callback) => {
      callback(0)
      return 0
    }
    let scrollHeight = 300
    const loadEarlier = vi.fn().mockImplementation(async () => {
      scrollHeight = 520
    })

    try {
      render(
        <ConversationTimeline
          hasMoreBefore
          loadEarlier={loadEarlier}
          title="Paged conversation"
          turns={[turn('Final answer')]}
        />,
      )

      const viewport = screen
        .getByRole('button', { name: 'Load earlier' })
        .closest('div.min-h-0') as HTMLDivElement | null
      if (!viewport) {
        throw new Error('timeline viewport not found')
      }
      Object.defineProperty(viewport, 'scrollHeight', {
        configurable: true,
        get: () => scrollHeight,
      })
      viewport.scrollTop = 120

      fireEvent.click(screen.getByRole('button', { name: 'Load earlier' }))

      await waitFor(() => expect(loadEarlier).toHaveBeenCalledTimes(1))
      await waitFor(() => expect(viewport.scrollTop).toBe(340))
    } finally {
      window.requestAnimationFrame = originalRequestAnimationFrame
    }
  })

  it('renders timeline gap markers and retries through the supplied callback', () => {
    const retryGap = vi.fn()
    render(
      <ConversationTimeline
        gapMarkers={[{ id: 'gap-001' }]}
        retryGap={retryGap}
        title="Gapped conversation"
        turns={[turn('Final answer')]}
      />,
    )

    expect(screen.getByText('Timeline gap')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(retryGap).toHaveBeenCalledTimes(1)
  })

  it('renders review and clarification requests inside assistant work', () => {
    render(
      <ConversationTimeline
        title="Requests"
        turns={[
          {
            ...turn(''),
            assistant: assistantWork({
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
            }),
          },
        ]}
      />,
    )

    expect(screen.getByText('Review changes')).toBeInTheDocument()
    expect(screen.getByText('Which style should I use?')).toBeInTheDocument()
  })

  it('opens command and diff process cards in the inspector without clearing selection', () => {
    renderTimelineWithClient(
      <ConversationTimeline title="Process cards" turns={[imageProcessTurn()]} />,
      createTestCommandClient(),
    )

    fireEvent.click(screen.getByRole('button', { name: 'Open command in inspector' }))
    expect(uiStore.getState().inspectorOpen).toBe(true)
    expect(uiStore.getState().workbenchSelection).toMatchObject({
      kind: 'command',
      conversationId: 'conversation-image',
    })

    fireEvent.click(screen.getByRole('button', { name: 'Open diff in inspector' }))
    expect(uiStore.getState().inspectorOpen).toBe(true)
    expect(uiStore.getState().workbenchSelection).toEqual({
      kind: 'diff',
      conversationId: 'conversation-image',
      changeSetId: 'change-set-image',
    })
  })

  it('opens tool and artifact cards in the inspector', () => {
    renderTimelineWithClient(
      <>
        <ConversationTimeline title="Tool cards" turns={[toolEvidenceTurn()]} />
        <ConversationTimeline title="Artifact cards" turns={[imageProcessTurn()]} />
      </>,
      createTestCommandClient(),
    )

    expect(
      screen.queryByRole('button', { name: 'Open read_file in inspector' }),
    ).not.toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: 'Open list_files in inspector' }),
    ).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /Ran 2 tools/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Open read_file in inspector' }))
    expect(uiStore.getState().workbenchSelection).toEqual({
      kind: 'tool',
      conversationId: 'conversation-tool-evidence',
      toolUseId: 'tool-read-file',
    })

    expect(
      screen.queryByRole('button', { name: 'Open permission permission-write-file in inspector' }),
    ).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Open artifact in inspector' }))
    expect(uiStore.getState().workbenchSelection).toEqual({
      kind: 'artifact',
      conversationId: 'conversation-image',
      artifactId: 'artifact-image-001',
      revisionId: 'revision-image-001',
    })
  })

  it('adds wrapping constraints for long user and assistant content', () => {
    const longPath = `/Users/goya/${'very-long-path-segment-'.repeat(12)}file.ts`
    render(<ConversationTimeline title="Long content" turns={[turn(longPath)]} />)

    const userMessage = screen.getByText('Prompt')
    const assistantMessage = screen.getByText(longPath)

    expect(userMessage).toHaveClass('break-words')
    expect(assistantMessage).toHaveClass('break-words')
    expect(assistantMessage.closest('section')).toHaveClass('min-w-0')
  })
})
