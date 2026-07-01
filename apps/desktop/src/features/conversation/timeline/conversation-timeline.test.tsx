import '@testing-library/jest-dom/vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { appI18n } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
import type { CommandClient, ConversationTurn, RunModelSnapshot } from '@/shared/tauri/commands'
import { CommandClientProvider } from '@/shared/tauri/react'
import { createTestCommandClient } from '@/testing/command-client'
import {
  codexAttachmentStressTurns,
  codexLargeDiffTurns,
  codexStyleEvidenceTurns,
} from '@/testing/conversation-evidence-fixtures'
import { ConversationTimeline } from './conversation-timeline'
import { parseDiffEvidenceLines } from './diff-evidence-block'

const timestamp = '2026-06-17T00:00:00.000Z'
const openAiRunModelSnapshot: RunModelSnapshot = {
  modelConfigId: 'provider-config-001',
  providerId: 'openai',
  modelId: 'gpt-4.1',
  displayName: 'GPT-4.1',
  protocol: 'responses',
}
const minimaxRunModelSnapshot: RunModelSnapshot = {
  ...openAiRunModelSnapshot,
  modelConfigId: 'minimax-config',
  providerId: 'minimax',
  modelId: 'MiniMax-M3',
  displayName: 'MiniMax M3',
  protocol: 'chat_completions',
}

describe('ConversationTimeline', () => {
  afterEach(() => {
    act(() => {
      uiStore.getState().clearTimelineScrollRequest()
      uiStore.getState().resetEvidenceDisclosure()
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

  it('renders a MiniMax-style failed tool flow as one safe assistant work tree', () => {
    render(<ConversationTimeline title="MiniMax flow" turns={[minimaxTurn()]} />)

    expect(screen.getByText('帮我生成一张海报图')).toBeInTheDocument()
    expect(screen.getByText('MiniMax M3')).toBeInTheDocument()
    expect(screen.getByText('正在检查可用的图像工具')).toBeInTheDocument()
    expect(screen.getByText('MiniMaxTextToImage')).toBeInTheDocument()
    expect(screen.getByText('Execution: failed')).toBeInTheDocument()
    expect(screen.getByText('Permission: approved')).toBeInTheDocument()
    expect(screen.getByText('工具执行失败。可在详情中查看。')).toBeInTheDocument()
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
      ...createTestCommandClient(),
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
    expect(screen.queryByText('已搜索图片工具')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Collapsed 1 history steps/ }))
    expect(screen.getByText('已搜索图片工具')).toBeInTheDocument()
    expect(screen.getByText('$ pnpm check:desktop')).toBeInTheDocument()
    expect(screen.getByText(/render process preview/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Copy diff' })).toBeInTheDocument()
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

  it('renders a Codex-style evidence conversation from the worktree projection', async () => {
    await appI18n.changeLanguage('zh-CN')
    try {
      renderTimelineWithClient(
        <ConversationTimeline title="Evidence conversation" turns={codexStyleEvidenceTurns} />,
        createTestCommandClient(),
      )

      expect(screen.getByText('已编辑 1 个文件')).toBeInTheDocument()
      expect(screen.getByText('reference.png')).toBeInTheDocument()
      expect(screen.getByText('notes.txt')).toBeInTheDocument()
      expect(screen.getByText('2 KB')).toBeInTheDocument()
      expect(screen.getByText('128 B')).toBeInTheDocument()
      expect(screen.getByText('SkillsPage.test.tsx')).toBeInTheDocument()
      expect(screen.getByText('+61')).toBeInTheDocument()
      expect(screen.getByText('-2')).toBeInTheDocument()
      expect(screen.getByText('$ pnpm -C apps/desktop test -- SkillsPage')).toBeInTheDocument()
      expect(screen.getByText('退出码 1')).toBeInTheDocument()
      expect(screen.getByText('上下文已自动压缩')).toBeInTheDocument()
      expect(screen.getByText('红测和失败证据已经保留，下一步修复实现。')).toBeInTheDocument()
    } finally {
      await appI18n.changeLanguage('en-US')
    }
  })

  it('renders Codex evidence blocks with stable DOM shape and disclosure rules', () => {
    renderTimelineWithClient(
      <ConversationTimeline title="Evidence conversation" turns={codexStyleEvidenceTurns} />,
      createTestCommandClient(),
    )

    const diffScrollRegion = screen.getByTestId('diff-scroll-region')
    expect(diffScrollRegion).toHaveClass('overflow-auto')
    expect(diffScrollRegion).toHaveClass('bg-code-background')
    expect(screen.getByText('12/12')).toBeInTheDocument()

    const metadataLine = screen.getByText('+++ b/SkillsPage.test.tsx').closest('div')
    expect(metadataLine).not.toHaveClass('bg-success/10')

    const commandBlock = screen
      .getByText('$ pnpm -C apps/desktop test -- SkillsPage')
      .closest('section')
    expect(commandBlock).toHaveClass('bg-terminal-background')
    expect(screen.getByTestId('command-output-scroll-region')).toHaveClass('overflow-auto')
    expect(screen.getByText('exit 1')).toBeInTheDocument()
    expect(screen.getByText('$ pnpm -C apps/desktop test -- SkillsPage')).toBeVisible()

    expect(screen.queryByText('$ rg "SkillsPage" apps/desktop/src')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Ran 1 historical commands/ }))
    expect(screen.getByText('$ rg "SkillsPage" apps/desktop/src')).toBeInTheDocument()

    const compaction = screen.getByText('上下文已自动压缩').closest('div')
    expect(compaction).toHaveTextContent('上下文已自动压缩')

    const userBubble = screen
      .getByText('请按 Codex 风格把这次红测、文件修改和失败命令展示在同一条对话里。')
      .closest('div')
    expect(userBubble).toHaveClass('bg-muted')
    expect(userBubble).not.toHaveClass('bg-primary')
  })

  it('keeps large diff content inside the evidence scroll region', () => {
    renderTimelineWithClient(
      <ConversationTimeline title="Large diff" turns={codexLargeDiffTurns} />,
      createTestCommandClient(),
    )

    expect(screen.getByText('ConversationTimeline.test.tsx')).toBeInTheDocument()
    const diffScrollRegion = screen.getByTestId('diff-scroll-region')

    expect(diffScrollRegion).toHaveClass('max-h-[360px]')
    expect(diffScrollRegion).toHaveClass('overflow-auto')
    expect(diffScrollRegion).toHaveTextContent('row 0')
  })

  it('preserves indentation when parsing added and removed diff lines', () => {
    const lines = parseDiffEvidenceLines(
      ['@@ -1,2 +1,2 @@', '-  oldValue()', '+  newValue()'].join('\n'),
    )

    expect(lines[1]).toMatchObject({
      content: '  oldValue()',
      oldLineNumber: 1,
      prefix: '-',
      type: 'removed',
    })
    expect(lines[2]).toMatchObject({
      content: '  newValue()',
      newLineNumber: 1,
      prefix: '+',
      type: 'added',
    })
  })

  it('renders historical attachments before the user message without fetching blob content', () => {
    const getArtifactMediaPreview = vi.fn()
    renderTimelineWithClient(
      <ConversationTimeline title="Evidence conversation" turns={codexStyleEvidenceTurns} />,
      {
        ...createTestCommandClient(),
        getArtifactMediaPreview,
      },
    )

    const article = screen.getByLabelText('Conversation turn')
    const reference = screen.getByText('reference.png')
    const prompt = screen.getByText(
      '请按 Codex 风格把这次红测、文件修改和失败命令展示在同一条对话里。',
    )

    expect(article.compareDocumentPosition(reference) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    )
    expect(reference.compareDocumentPosition(prompt) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    )
    expect(screen.getByText('image/png')).toBeInTheDocument()
    expect(getArtifactMediaPreview).not.toHaveBeenCalled()
  })

  it('keeps image attachments as metadata chips when safe preview fails', async () => {
    const getAttachmentMediaPreview = vi.fn().mockRejectedValue(new Error('preview unavailable'))
    const getArtifactMediaPreview = vi.fn()
    const imageAttachment = codexStyleEvidenceTurns[0].user.attachments?.[0]
    renderTimelineWithClient(
      <ConversationTimeline title="Evidence conversation" turns={codexStyleEvidenceTurns} />,
      {
        ...createTestCommandClient(),
        getArtifactMediaPreview,
        getAttachmentMediaPreview,
      },
    )

    await waitFor(() => {
      expect(getAttachmentMediaPreview).toHaveBeenCalledWith({
        conversationId: codexStyleEvidenceTurns[0].conversationId,
        attachmentId: imageAttachment?.id,
      })
    })
    expect(screen.getByText('reference.png')).toBeInTheDocument()
    expect(screen.getByText('image/png')).toBeInTheDocument()
    expect(screen.queryByRole('img', { name: 'reference.png' })).not.toBeInTheDocument()
    expect(getArtifactMediaPreview).not.toHaveBeenCalled()
  })

  it('renders image attachment thumbnails through safe preview without fetch or artifact preview', async () => {
    const getAttachmentMediaPreview = vi.fn().mockResolvedValue({
      dataUrl: 'data:image/png;base64,iVBORw0KGgo=',
      mimeType: 'image/png',
      sizeBytes: 67,
    })
    const getArtifactMediaPreview = vi.fn()
    const fetchSpy = vi.fn()
    const imageAttachment = codexStyleEvidenceTurns[0].user.attachments?.[0]
    vi.stubGlobal('fetch', fetchSpy)

    try {
      renderTimelineWithClient(
        <ConversationTimeline title="Evidence conversation" turns={codexStyleEvidenceTurns} />,
        {
          ...createTestCommandClient(),
          getArtifactMediaPreview,
          getAttachmentMediaPreview,
        },
      )

      expect(await screen.findByRole('img', { name: 'reference.png' })).toHaveAttribute(
        'src',
        'data:image/png;base64,iVBORw0KGgo=',
      )
      expect(getAttachmentMediaPreview).toHaveBeenCalledWith({
        conversationId: codexStyleEvidenceTurns[0].conversationId,
        attachmentId: imageAttachment?.id,
      })
      expect(getArtifactMediaPreview).not.toHaveBeenCalled()
      expect(fetchSpy).not.toHaveBeenCalled()
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('renders attachment chips as a right-aligned metadata strip with internal overflow', () => {
    const getArtifactMediaPreview = vi.fn()
    const fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)

    try {
      renderTimelineWithClient(
        <ConversationTimeline title="Attachment evidence" turns={codexAttachmentStressTurns} />,
        {
          ...createTestCommandClient(),
          getArtifactMediaPreview,
        },
      )

      const attachmentStrip = screen.getByLabelText('User attachments')
      expect(attachmentStrip).toHaveClass('ml-auto')
      expect(attachmentStrip.parentElement).toHaveClass('overflow-x-auto')
      expect(within(attachmentStrip).getAllByRole('listitem')).toHaveLength(5)

      const reference = screen.getByText('reference.png')
      const prompt = screen.getByText(
        '请按 Codex 风格把这次红测、文件修改和失败命令展示在同一条对话里。',
      )
      expect(reference.compareDocumentPosition(prompt) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
        Node.DOCUMENT_POSITION_FOLLOWING,
      )

      expect(screen.getAllByText('image/png')).not.toHaveLength(0)
      expect(screen.getByText('report.pdf')).toBeInTheDocument()
      expect(screen.getByText('32 KB')).toBeInTheDocument()
      expect(screen.queryByRole('img', { name: /reference\.png/ })).not.toBeInTheDocument()
      expect(getArtifactMediaPreview).not.toHaveBeenCalled()
      expect(fetchSpy).not.toHaveBeenCalled()
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('summarizes tool attempts and collapses low-signal completed rows', async () => {
    await appI18n.changeLanguage('zh-CN')
    try {
      render(<ConversationTimeline title="Tool evidence" turns={[toolEvidenceTurn()]} />)

      expect(screen.getByText('已运行 2 条工具')).toBeInTheDocument()
      expect(screen.getByText('失败 1 条')).toBeInTheDocument()
      expect(screen.getByText('运行中 1 条')).toBeInTheDocument()
      expect(screen.getByText('等待权限 1 条')).toBeInTheDocument()

      const summary = screen.getByRole('button', { name: /已运行 2 条工具/ })
      expect(summary).toHaveAttribute('aria-expanded', 'false')
      expect(screen.queryByText('read_file')).not.toBeInTheDocument()
      expect(screen.queryByText('list_files')).not.toBeInTheDocument()
      expect(screen.queryByText('权限：已批准')).not.toBeInTheDocument()
      expect(screen.getByText('exec_command')).toBeInTheDocument()
      expect(screen.getByText('search_code')).toBeInTheDocument()
      expect(screen.getByText('write_file')).toBeInTheDocument()
      expect(screen.getAllByText('工具执行失败。可在详情中查看。')).toHaveLength(1)

      fireEvent.click(summary)

      expect(summary).toHaveAttribute('aria-expanded', 'true')
      expect(screen.getByText('read_file')).toBeInTheDocument()
      expect(screen.getByText('list_files')).toBeInTheDocument()
      expect(screen.getByText('权限：已批准')).toBeInTheDocument()
    } finally {
      await appI18n.changeLanguage('en-US')
    }
  })

  it('collapses completed process history while keeping failures and non-zero commands visible', async () => {
    await appI18n.changeLanguage('zh-CN')
    try {
      render(<ConversationTimeline title="Process history" turns={[processHistoryTurn()]} />)

      const collapsedGroup = screen.getByRole('button', { name: /已折叠 3 条历史步骤/ })
      expect(collapsedGroup).toHaveAttribute('aria-expanded', 'false')
      expect(screen.queryByText('已读取 package.json')).not.toBeInTheDocument()
      expect(screen.queryByText('已搜索 timeline')).not.toBeInTheDocument()
      expect(screen.queryByText('$ rg "timeline" apps/desktop/src')).not.toBeInTheDocument()

      expect(screen.getByText('$ pnpm -C apps/desktop test')).toBeVisible()
      expect(screen.getByText('退出码 1')).toBeVisible()
      expect(screen.getByText('$ pnpm -C apps/desktop lint')).toBeVisible()
      expect(screen.getByText('退出码 2')).toBeVisible()

      fireEvent.click(collapsedGroup)

      expect(collapsedGroup).toHaveAttribute('aria-expanded', 'true')
      expect(screen.getByText('已读取 package.json')).toBeInTheDocument()
      expect(screen.getByText('已搜索 timeline')).toBeInTheDocument()
      expect(screen.getByText('$ rg "timeline" apps/desktop/src')).toBeInTheDocument()
    } finally {
      await appI18n.changeLanguage('en-US')
    }
  })

  it('keeps permission requests nested under the owning tool row', () => {
    render(<ConversationTimeline title="Tool evidence" turns={[toolEvidenceTurn()]} />)

    const toolRow = screen.getByText('write_file').closest('[data-tool-attempt-id]')

    expect(toolRow).not.toBeNull()
    expect(within(toolRow as HTMLElement).getByText('Permission: pending')).toBeInTheDocument()
    expect(screen.queryByText('Permission request')).not.toBeInTheDocument()
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

  it('shows a safe placeholder when a process image artifact preview is loading or unavailable', async () => {
    const commandClient = {
      ...createTestCommandClient(),
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
      expect(renderedText).not.toContain('Activity')
      expect(renderedText).not.toContain('The runtime requires approval before continuing.')
    } finally {
      await appI18n.changeLanguage('en-US')
    }
  })

  it('keeps timeline bottom padding large enough for the composer reserve', () => {
    render(<ConversationTimeline title="Composer padding" turns={[turn('Final answer')]} />)

    expect(screen.getByTestId('conversation-timeline-scroll-content')).toHaveClass('pb-28')
  })

  it('adds composer reserve to virtual timeline height', () => {
    render(
      <ConversationTimeline
        title="Virtual composer padding"
        turns={Array.from({ length: 24 }, (_, index) =>
          turn(`Virtual answer ${index}`, `virtual-${index}`),
        )}
      />,
    )

    const scrollContent = screen.getByTestId('conversation-timeline-scroll-content')
    expect(scrollContent).toHaveStyle({ height: '4432px' })
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
      model: openAiRunModelSnapshot,
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
              failureSummary: '工具执行失败。可在详情中查看。',
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
      model: minimaxRunModelSnapshot,
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
              failureSummary: '工具执行失败。可在详情中查看。',
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

function toolEvidenceTurn({
  conversationId = 'conversation-tool-evidence',
  runId = 'run-tool-evidence',
}: {
  conversationId?: string
  runId?: string
} = {}): ConversationTurn {
  return {
    id: 'turn:user-tool-evidence',
    conversationId,
    position: 0,
    user: {
      id: 'user:user-tool-evidence',
      messageId: 'user-tool-evidence',
      body: '检查工具执行过程',
      timestamp,
    },
    assistant: {
      id: 'assistant:run-tool-evidence',
      runId,
      status: 'running',
      segments: [
        {
          kind: 'toolGroup',
          id: 'segment:tools:tool-evidence',
          order: 0,
          attempts: [
            {
              id: 'tool:read-file',
              order: 0,
              toolUseId: 'tool-read-file',
              toolName: 'read_file',
              status: 'completed',
              permission: {
                id: 'permission:read-file',
                requestId: 'permission-read-file',
                toolUseId: 'tool-read-file',
                status: 'approved',
              },
            },
            {
              id: 'tool:list-files',
              order: 1,
              toolUseId: 'tool-list-files',
              toolName: 'list_files',
              status: 'completed',
            },
            {
              id: 'tool:exec-command',
              order: 2,
              toolUseId: 'tool-exec-command',
              toolName: 'exec_command',
              status: 'failed',
              failureSummary: '工具执行失败。可在详情中查看。',
            },
            {
              id: 'tool:search-code',
              order: 3,
              toolUseId: 'tool-search-code',
              toolName: 'search_code',
              status: 'running',
            },
            {
              id: 'tool:write-file',
              order: 4,
              toolUseId: 'tool-write-file',
              toolName: 'write_file',
              status: 'waitingPermission',
              permission: {
                id: 'permission:write-file',
                requestId: 'permission-write-file',
                toolUseId: 'tool-write-file',
                status: 'pending',
              },
            },
          ],
        },
      ],
    },
  }
}

function processHistoryTurn(): ConversationTurn {
  return {
    id: 'turn:user-process-history',
    conversationId: 'conversation-process-history',
    position: 0,
    user: {
      id: 'user:user-process-history',
      messageId: 'user-process-history',
      body: '整理执行历史',
      timestamp,
    },
    assistant: {
      id: 'assistant:run-process-history',
      runId: 'run-process-history',
      status: 'complete',
      segments: [
        {
          kind: 'process',
          id: 'segment:process:history',
          order: 0,
          status: 'failed',
          summary: '已结束但存在失败步骤',
          steps: [
            {
              id: 'process-step:read-package',
              order: 0,
              kind: 'fileRead',
              status: 'complete',
              title: '已读取 package.json',
              detail: {
                type: 'activity',
                summary: '读取 package.json',
                itemCount: 1,
              },
            },
            {
              id: 'process-step:search-timeline',
              order: 1,
              kind: 'fileSearch',
              status: 'complete',
              title: '已搜索 timeline',
              detail: {
                type: 'activity',
                summary: '搜索 timeline',
                itemCount: 2,
              },
            },
            {
              id: 'process-step:rg-complete',
              order: 2,
              kind: 'command',
              status: 'complete',
              title: '已运行历史命令',
              detail: {
                type: 'command',
                command: 'rg "timeline" apps/desktop/src',
                output: 'apps/desktop/src/features/conversation/timeline/conversation-timeline.tsx',
                exitCode: 0,
                durationMs: 180,
              },
            },
            {
              id: 'process-step:test-failed',
              order: 3,
              kind: 'command',
              status: 'failed',
              title: '测试失败',
              detail: {
                type: 'command',
                command: 'pnpm -C apps/desktop test',
                output: '1 failed',
                exitCode: 1,
                durationMs: 2100,
              },
            },
            {
              id: 'process-step:lint-non-zero',
              order: 4,
              kind: 'command',
              status: 'complete',
              title: 'lint 退出码非零',
              detail: {
                type: 'command',
                command: 'pnpm -C apps/desktop lint',
                output: 'lint errors',
                exitCode: 2,
                durationMs: 900,
              },
            },
          ],
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
