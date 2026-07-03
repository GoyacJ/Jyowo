import '@testing-library/jest-dom/vitest'

import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { appI18n } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import { ConversationTimeline } from './conversation-timeline'
import { resetTimelineTestState, toolEvidenceTurn, turn } from './conversation-timeline-test-utils'
import { PermissionInlinePanel } from './permission-inline-panel'

describe('ConversationTimeline', () => {
  afterEach(() => {
    resetTimelineTestState()
  })

  it('does not show generic pending permission text after approval', () => {
    const approvedTurn = turn('Final answer', 'approved-generic-summary')
    const toolGroup = approvedTurn.assistant?.segments.find(
      (segment) => segment.kind === 'toolGroup',
    )
    if (toolGroup?.kind === 'toolGroup' && toolGroup.attempts[0].permission) {
      toolGroup.attempts[0].permission = {
        ...toolGroup.attempts[0].permission,
        summary: '需要批准后才能继续。',
      }
    }

    render(<ConversationTimeline title="Approved permission" turns={[approvedTurn]} />)

    expect(screen.getByText('Permission: approved')).toBeInTheDocument()
    expect(screen.queryByText('需要批准后才能继续。')).not.toBeInTheDocument()
  })

  it('keeps permission requests nested under the owning tool row', () => {
    render(<ConversationTimeline title="Tool evidence" turns={[toolEvidenceTurn()]} />)

    const toolRow = screen.getByText('write_file').closest('[data-tool-attempt-id]')

    expect(toolRow).not.toBeNull()
    expect(within(toolRow as HTMLElement).getByText('Permission: pending')).toBeInTheDocument()
    expect(screen.queryByText('Permission request')).not.toBeInTheDocument()
  })

  it('submits confirmation text when approving type-to-confirm permissions', () => {
    const onResolve = vi.fn()

    render(
      <PermissionInlinePanel
        conversationId="conversation-001"
        onResolve={onResolve}
        permission={{
          confirmationExpected: 'DELETE',
          id: 'permission:request-confirm',
          requestId: 'request-confirm',
          status: 'pending',
          toolUseId: 'tool-confirm',
        }}
        turnId="turn-001"
      />,
    )

    fireEvent.change(screen.getByLabelText('Confirmation text'), {
      target: { value: 'DELETE' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Approve' }))

    expect(onResolve).toHaveBeenCalledWith({
      conversationId: 'conversation-001',
      requestId: 'request-confirm',
      decision: 'approve',
      confirmationText: 'DELETE',
    })
  })

  it('scopes tool disclosure state by conversation and run identity', () => {
    const rendered = render(
      <ConversationTimeline title="Tool evidence" turns={[toolEvidenceTurn()]} />,
    )

    fireEvent.click(screen.getByRole('button', { name: /Ran 2 tools/ }))
    expect(screen.getByText('Permission: approved')).toBeInTheDocument()

    rendered.rerender(
      <ConversationTimeline
        title="Tool evidence"
        turns={[
          toolEvidenceTurn({ conversationId: 'conversation-tool-evidence-2', runId: 'run-2' }),
        ]}
      />,
    )

    expect(screen.queryByText('Permission: approved')).not.toBeInTheDocument()
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
      const renderedText = document.body.textContent ?? ''
      for (const leakedLabel of ['Tools', 'Approved', 'Complete', 'failed', 'View raw events']) {
        expect(renderedText).not.toContain(leakedLabel)
      }
      expect(renderedText).not.toContain('Activity')
      expect(renderedText).not.toContain('The runtime requires approval before continuing.')
    } finally {
      await appI18n.changeLanguage('en-US')
    }
  })

  it('scrolls permission anchors to the rendered permission panel', async () => {
    const scrollIntoView = vi.fn()
    const scrollIntoViewSpy = vi
      .spyOn(Element.prototype, 'scrollIntoView')
      .mockImplementation(scrollIntoView)

    try {
      render(
        <ConversationTimeline title="Permission conversation" turns={[turn('Final answer')]} />,
      )

      act(() => {
        uiStore.getState().requestTimelineScroll('permission:request-001')
      })

      await waitFor(() => {
        expect(
          scrollIntoViewSpy.mock.instances.some(
            (element) =>
              (element as Element).getAttribute('data-permission-request-id') === 'request-001',
          ),
        ).toBe(true)
      })
    } finally {
      scrollIntoViewSpy.mockRestore()
    }
  })

  it('does not resolve permission anchors outside the timeline viewport', async () => {
    const outsideTarget = document.createElement('div')
    outsideTarget.dataset.permissionRequestId = 'outside-request'
    document.body.appendChild(outsideTarget)

    const scrollIntoView = vi.fn()
    const scrollIntoViewSpy = vi
      .spyOn(Element.prototype, 'scrollIntoView')
      .mockImplementation(scrollIntoView)

    try {
      render(
        <ConversationTimeline title="Permission conversation" turns={[turn('Final answer')]} />,
      )
      await waitFor(() => expect(scrollIntoView).toHaveBeenCalled())
      scrollIntoView.mockClear()

      act(() => {
        uiStore.getState().requestTimelineScroll('permission:outside-request')
      })

      await new Promise((resolve) => window.requestAnimationFrame(resolve))

      expect(scrollIntoView).not.toHaveBeenCalled()
    } finally {
      scrollIntoViewSpy.mockRestore()
      outsideTarget.remove()
    }
  })

  it('scrolls virtualized permission anchors toward the target turn before the panel is mounted', async () => {
    const originalScrollTo = HTMLElement.prototype.scrollTo
    const scrollTo = vi.fn()
    Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
      configurable: true,
      value: scrollTo,
    })

    try {
      render(
        <ConversationTimeline
          title="Virtual permission conversation"
          turns={Array.from({ length: 30 }, (_, index) =>
            turn(`Final ${index}`, String(index), `request-${index}`),
          )}
        />,
      )

      act(() => {
        uiStore.getState().requestTimelineScroll('permission:request-29')
      })

      await waitFor(() => expect(scrollTo).toHaveBeenCalled())
    } finally {
      Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
        configurable: true,
        value: originalScrollTo,
      })
    }
  })
})
