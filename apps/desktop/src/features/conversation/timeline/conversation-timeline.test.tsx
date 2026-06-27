import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { appI18n } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient, ConversationTurn } from '@/shared/tauri/commands'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import { CommandClientProvider } from '@/shared/tauri/react'
import { ConversationTimeline } from './conversation-timeline'

const timestamp = '2026-06-17T00:00:00.000Z'

describe('ConversationTimeline', () => {
  afterEach(() => {
    act(() => {
      uiStore.getState().clearTimelineScrollRequest()
    })
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
      cursor: {
        eventId: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
        conversationSequence: 3,
      },
    })
  })

  it('renders a MiniMax-style failed tool flow as one safe assistant work tree', () => {
    render(<ConversationTimeline title="MiniMax flow" turns={[minimaxTurn()]} />)

    expect(screen.getByText('帮我生成一张海报图')).toBeInTheDocument()
    expect(screen.getByText('正在检查可用的图像工具')).toBeInTheDocument()
    expect(screen.getByText('MiniMaxTextToImage')).toBeInTheDocument()
    expect(screen.getByText('Execution: failed')).toBeInTheDocument()
    expect(screen.getByText('Permission: approved')).toBeInTheDocument()
    expect(screen.getByText('工具执行失败。详情可在 Activity 中查看。')).toBeInTheDocument()
    expect(screen.getByText('海报生成提示词')).toBeInTheDocument()
    expect(screen.getByText('可复用的图像生成提示词已准备好。')).toBeInTheDocument()
    expect(
      screen.getByText('图像工具失败后，我保留了可复用的提示词和下一步建议。'),
    ).toBeInTheDocument()

    const renderedText = document.body.textContent ?? ''
    for (const hiddenText of [
      'raw provider failure',
      '/Users/alice/private',
      'secret-token',
      'blob-secret',
      'hash-secret',
    ]) {
      expect(renderedText).not.toContain(hiddenText)
    }
  })

  it('renders safe reasoning process steps when thinking is expanded', () => {
    render(<ConversationTimeline title="Reasoning flow" turns={[reasoningTurn()]} />)

    expect(screen.getByText('已完成推理过程')).toBeInTheDocument()
    expect(screen.getByText('Checked project context.')).not.toBeVisible()

    fireEvent.click(screen.getByText('Reasoning process'))

    expect(screen.getByText('Reasoning process')).toBeInTheDocument()
    expect(screen.getByText('Checked project context.')).toBeInTheDocument()
    expect(screen.getByText('准备使用 read_file')).toBeInTheDocument()
    expect(screen.getByText('read_file 已完成')).toBeInTheDocument()
    expect(document.body.textContent ?? '').not.toContain('raw private chain')
  })

  it('renders process steps and image artifact previews from the safe projection', async () => {
    const previewRequests: Array<Parameters<CommandClient['getArtifactMediaPreview']>[0]> = []
    const dataUrl =
      'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII='
    const commandClient = {
      ...createMockCommandClient(),
      getArtifactMediaPreview: async (
        request: Parameters<CommandClient['getArtifactMediaPreview']>[0],
      ) => {
        previewRequests.push(request)
        return {
          dataUrl,
          mimeType: 'image/png',
          sizeBytes: 68,
        }
      },
    } satisfies CommandClient

    renderTimelineWithClient(
      <ConversationTimeline title="Image flow" turns={[imageProcessTurn()]} />,
      commandClient,
    )

    expect(screen.getByText('确认需要生成图片并展示结果。')).toBeInTheDocument()
    expect(screen.getByText('已搜索图片工具')).toBeInTheDocument()
    expect(screen.getByText('pnpm check:desktop')).toBeInTheDocument()
    expect(screen.getByText(/render process preview/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Open in editor' })).toBeInTheDocument()
    expect(screen.getByText('Generated image')).toBeInTheDocument()
    expect(screen.queryByText('Image artifact ready')).not.toBeInTheDocument()
    expect(screen.getByText('图片已生成。')).toBeInTheDocument()

    const image = await screen.findByRole('img', { name: 'Generated image' })
    expect(image).toHaveAttribute('src', dataUrl)
    expect(previewRequests).toEqual([
      {
        conversationId: 'conversation-image',
        artifactId: 'artifact-image-001',
      },
    ])
    expect(document.body.textContent ?? '').not.toContain('[REDACTED]')
  })

  it('shows a safe placeholder when a process image artifact preview is loading or unavailable', async () => {
    const commandClient = {
      ...createMockCommandClient(),
      getArtifactMediaPreview: () => Promise.reject(new Error('preview unavailable')),
    } satisfies CommandClient

    renderTimelineWithClient(
      <ConversationTimeline title="Image flow" turns={[imageProcessTurn()]} />,
      commandClient,
    )

    expect(screen.getByText('Generated artifact')).toBeInTheDocument()
    expect(await screen.findByText('Image preview unavailable')).toBeInTheDocument()
    expect(document.body.textContent ?? '').not.toContain('/Users/')
    expect(document.body.textContent ?? '').not.toContain('.jyowo')
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

function renderTimelineWithClient(children: ReactNode, commandClient: CommandClient) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  })

  return render(
    <CommandClientProvider client={commandClient}>
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    </CommandClientProvider>,
  )
}

function turn(
  finalBody: string,
  suffix = '001',
  permissionRequestId = 'request-001',
): ConversationTurn {
  return {
    id: `turn:user-message-${suffix}`,
    conversationId: 'conversation-001',
    position: 0,
    user: {
      id: `user:user-message-${suffix}`,
      messageId: `user-message-${suffix}`,
      body: 'Prompt',
      timestamp,
    },
    assistant: {
      id: `assistant:run-${suffix}`,
      runId: `run-${suffix}`,
      status: 'running',
      segments: [
        {
          kind: 'thinking',
          id: `segment:thinking:run-${suffix}`,
          order: 0,
          status: 'withheld',
          summary: { text: '思考内容已折叠' },
        },
        {
          kind: 'toolGroup',
          id: `segment:tools:tool-use-${suffix}`,
          order: 1,
          attempts: [
            {
              id: `tool:tool-use-${suffix}`,
              order: 0,
              toolUseId: `tool-use-${suffix}`,
              toolName: 'read_file',
              status: 'failed',
              permission: {
                id: `permission:${permissionRequestId}`,
                requestId: permissionRequestId,
                toolUseId: `tool-use-${suffix}`,
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
          id: `segment:text:assistant-message-${suffix}`,
          order: 2,
          messageId: `assistant-message-${suffix}`,
          body: finalBody,
        },
      ],
    },
  }
}

function reasoningTurn(): ConversationTurn {
  return {
    ...turn('Final answer', 'reasoning'),
    assistant: {
      id: 'assistant:run-reasoning',
      runId: 'run-reasoning',
      status: 'complete',
      segments: [
        {
          kind: 'thinking',
          id: 'segment:thinking:run-reasoning',
          order: 0,
          status: 'complete',
          summary: { text: '已完成推理过程' },
          steps: [
            {
              id: 'thinking-step:run-reasoning:summary',
              order: 0,
              kind: 'reasoningSummary',
              status: 'complete',
              title: '推理过程',
              body: 'Checked project context.',
            },
            {
              id: 'thinking-step:run-reasoning:tool-plan:tool-1',
              order: 1,
              kind: 'toolPlanning',
              status: 'complete',
              title: '准备使用 read_file',
            },
            {
              id: 'thinking-step:run-reasoning:tool-result:tool-1',
              order: 2,
              kind: 'toolResult',
              status: 'complete',
              title: 'read_file 已完成',
            },
          ],
        },
        {
          kind: 'text',
          id: 'segment:text:assistant-message-reasoning',
          order: 1,
          messageId: 'assistant-message-reasoning',
          body: 'Final answer',
        },
      ],
    },
  }
}

function minimaxTurn(): ConversationTurn {
  return {
    id: 'turn:user-minimax',
    conversationId: 'conversation-minimax',
    position: 0,
    user: {
      id: 'user:user-minimax',
      messageId: 'user-minimax',
      body: '帮我生成一张海报图',
      timestamp,
    },
    assistant: {
      id: 'assistant:run-minimax',
      runId: 'run-minimax',
      status: 'complete',
      segments: [
        {
          kind: 'thinking',
          id: 'segment:thinking:run-minimax',
          order: 0,
          status: 'running',
          summary: { text: '正在检查可用的图像工具' },
        },
        {
          kind: 'toolGroup',
          id: 'segment:tools:tool-minimax',
          order: 1,
          attempts: [
            {
              id: 'tool:tool-minimax',
              order: 0,
              toolUseId: 'tool-minimax',
              toolName: 'MiniMaxTextToImage',
              status: 'failed',
              permission: {
                id: 'permission:permission-minimax',
                requestId: 'permission-minimax',
                toolUseId: 'tool-minimax',
                status: 'approved',
              },
              failureSummary: '工具执行失败。详情可在 Activity 中查看。',
            },
          ],
        },
        {
          kind: 'artifact',
          id: 'segment:artifact:artifact-minimax',
          order: 2,
          artifactId: 'artifact-minimax',
          title: '海报生成提示词',
          summary: '可复用的图像生成提示词已准备好。',
        },
        {
          kind: 'text',
          id: 'segment:text:assistant-final',
          order: 3,
          messageId: 'assistant-final',
          body: '图像工具失败后，我保留了可复用的提示词和下一步建议。',
        },
      ],
    },
  }
}

function imageProcessTurn(): ConversationTurn {
  return {
    id: 'turn:user-image',
    conversationId: 'conversation-image',
    position: 0,
    user: {
      id: 'user:user-image',
      messageId: 'user-image',
      body: '生成一张草鱼图片',
      timestamp,
    },
    assistant: {
      id: 'assistant:run-image',
      runId: 'run-image',
      status: 'complete',
      segments: [
        {
          kind: 'process',
          id: 'segment:process:run-image',
          order: 0,
          status: 'complete',
          summary: '已完成工作过程',
          steps: [
            {
              id: 'process-step:reasoning',
              order: 0,
              kind: 'reasoning',
              status: 'complete',
              title: '分析请求',
              body: '确认需要生成图片并展示结果。',
            },
            {
              id: 'process-step:activity',
              order: 1,
              kind: 'fileSearch',
              status: 'complete',
              title: '已搜索图片工具',
              detail: {
                type: 'activity',
                summary: '已搜索图片工具',
                itemCount: 1,
              },
            },
            {
              id: 'process-step:command',
              order: 2,
              kind: 'command',
              status: 'complete',
              title: '运行检查',
              detail: {
                type: 'command',
                command: 'pnpm check:desktop',
                output: 'passed',
                exitCode: 0,
                durationMs: 1200,
              },
            },
            {
              id: 'process-step:diff',
              order: 3,
              kind: 'diff',
              status: 'complete',
              title: '更新图片展示',
              detail: {
                type: 'diff',
                files: [
                  {
                    path: 'apps/desktop/src/features/conversation/timeline/artifact-segment-view.tsx',
                    addedLines: 1,
                    removedLines: 0,
                    preview: '+ render process preview',
                  },
                ],
              },
            },
            {
              id: 'process-step:artifact',
              order: 4,
              kind: 'artifact',
              status: 'complete',
              title: 'Generated image',
              detail: {
                type: 'artifact',
                artifactId: 'artifact-image-001',
                media: {
                  kind: 'image',
                  mimeType: 'image/png',
                  sizeBytes: 68,
                },
              },
            },
          ],
        },
        {
          kind: 'text',
          id: 'segment:text:assistant-final-image',
          order: 2,
          messageId: 'assistant-final-image',
          body: '图片已生成。',
        },
        {
          kind: 'artifact',
          id: 'segment:artifact:artifact-image-001',
          order: 1,
          artifactId: 'artifact-image-001',
          artifactKind: 'image',
          status: 'ready',
          source: 'tool',
          title: 'Generated image',
          summary: 'Image artifact ready',
          media: {
            kind: 'image',
            mimeType: 'image/png',
            sizeBytes: 68,
          },
        },
      ],
    },
  }
}
