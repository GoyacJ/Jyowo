import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { appI18n } from '@/shared/i18n/i18n'
import { createTestCommandClient } from '@/testing/command-client'
import {
  codexLargeDiffTurns,
  codexStyleEvidenceTurns,
} from '@/testing/conversation-evidence-fixtures'
import { commandDetail } from '@/testing/conversation-worktree-builders'
import { ConversationTimeline } from './conversation-timeline'
import {
  processHistoryTurn,
  renderTimelineWithClient,
  resetTimelineTestState,
  toolEvidenceTurn,
  turn,
} from './conversation-timeline-test-utils'
import { parseDiffEvidenceLines } from './diff-evidence-block'
import { TimelineBlockRenderer } from './timeline-block-renderer'
import type { TimelineRenderBlock } from './timeline-render-blocks'

describe('ConversationTimeline', () => {
  const originalClipboard = navigator.clipboard

  afterEach(() => {
    resetTimelineTestState()
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: originalClipboard,
    })
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

      fireEvent.click(screen.getByRole('button', { name: /已编辑 1 个文件/ }))

      expect(screen.getAllByText('SkillsPage.test.tsx').length).toBeGreaterThan(0)
      expect(screen.getAllByText('+61').length).toBeGreaterThan(0)
      expect(screen.getAllByText('-2').length).toBeGreaterThan(0)
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

    fireEvent.click(screen.getByRole('button', { name: /Edited 1 file/ }))

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
    expect(screen.getByRole('button', { name: /Ran 2 commands/ })).toHaveAttribute(
      'aria-expanded',
      'true',
    )
    expect(screen.getAllByTestId('command-output-scroll-region')[0]).toHaveClass('overflow-auto')
    expect(screen.getByText('exit 1')).toBeInTheDocument()
    expect(screen.getByText('$ pnpm -C apps/desktop test -- SkillsPage')).toBeVisible()
    expect(screen.getByText('$ rg "SkillsPage" apps/desktop/src')).toBeInTheDocument()

    const compaction = screen.getByText('上下文已自动压缩').closest('div')
    expect(compaction).toHaveTextContent('上下文已自动压缩')

    const userBubble = screen
      .getByText('请按 Codex 风格把这次红测、文件修改和失败命令展示在同一条对话里。')
      .closest('div')
    expect(userBubble).toHaveClass('bg-muted')
    expect(userBubble).not.toHaveClass('bg-primary')
  })

  it('uses explicit command and output copy actions instead of combined command evidence copy', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    renderTimelineWithClient(
      <ConversationTimeline title="Evidence conversation" turns={codexStyleEvidenceTurns} />,
      createTestCommandClient(),
    )

    expect(screen.queryByRole('button', { name: 'Copy' })).not.toBeInTheDocument()

    fireEvent.click(screen.getAllByRole('button', { name: 'Copy command' })[0])
    expect(writeText).toHaveBeenCalledWith('pnpm -C apps/desktop test -- SkillsPage')

    fireEvent.click(screen.getAllByRole('button', { name: 'Copy output' })[0])
    expect(writeText).toHaveBeenCalledWith(expect.not.stringContaining('$ pnpm'))
    expect(writeText).toHaveBeenCalledWith(expect.not.stringContaining('exit 1'))
  })

  it('keeps large diff content inside the evidence scroll region', () => {
    renderTimelineWithClient(
      <ConversationTimeline title="Large diff" turns={codexLargeDiffTurns} />,
      createTestCommandClient(),
    )

    fireEvent.click(screen.getByRole('button', { name: /Edited 1 file/ }))

    expect(screen.getAllByText('ConversationTimeline.test.tsx').length).toBeGreaterThan(0)
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

      expect(screen.queryByText('已结束但存在失败步骤')).not.toBeInTheDocument()

      const collapsedGroup = screen.getByRole('button', { name: /已读取\/搜索 3 项/ })
      expect(collapsedGroup).toHaveAttribute('aria-expanded', 'false')
      expect(screen.queryByText('已读取 package.json')).not.toBeInTheDocument()
      expect(screen.queryByText('已搜索 timeline')).not.toBeInTheDocument()

      const commandGroup = screen.getByRole('button', { name: /已运行 3 条命令/ })
      expect(commandGroup).toHaveAttribute('aria-expanded', 'true')
      expect(screen.getByText('$ rg "timeline" apps/desktop/src')).toBeVisible()
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
})

describe('TimelineBlockRenderer', () => {
  afterEach(async () => {
    resetTimelineTestState()
    await appI18n.changeLanguage('en-US')
  })

  it('renders collapsed and expanded file edit evidence blocks', async () => {
    await appI18n.changeLanguage('zh-CN')
    const block: TimelineRenderBlock = {
      kind: 'fileEdit',
      id: 'process:process-1:file-edit:edit-1',
      order: 0,
      processSegmentId: 'process-1',
      defaultOpen: false,
      forcedOpen: false,
      steps: [],
      files: [
        {
          changeSetId: 'changeset-1',
          path: 'src/worker_service_test.go',
          status: 'modified',
          addedLines: 67,
          removedLines: 0,
          preview: '@@ -1,1 +1,2 @@\n package worker\n+func TestWorker() {}',
          fullPatchRef: 'patch-ref-1',
          riskFlags: [],
        },
      ],
    }

    renderTimelineWithClient(
      <TimelineBlockRenderer
        block={block}
        conversationId="conversation-1"
        runId="run-1"
        turnId="turn-1"
      />,
      createTestCommandClient(),
    )

    const summary = screen.getByRole('button', { name: /已编辑 1 个文件/ })
    expect(summary).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getByText('worker_service_test.go +67 -0')).toBeInTheDocument()
    expect(screen.queryByText('已编辑的文件')).not.toBeInTheDocument()

    fireEvent.click(summary)

    expect(summary).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('已编辑的文件')).toBeInTheDocument()
    expect(screen.getByTestId('diff-scroll-region')).toBeInTheDocument()
  })

  it('renders read/search activity collapsed counts and expanded item labels', async () => {
    await appI18n.changeLanguage('zh-CN')
    const block: TimelineRenderBlock = {
      kind: 'activity',
      id: 'process:process-1:activity:read-1',
      order: 0,
      processSegmentId: 'process-1',
      defaultOpen: false,
      forcedOpen: false,
      steps: [],
      title: 'Read files',
      itemCount: 3,
      items: [
        { id: 'read-1:file:src%2Fmain.ts:', kind: 'file', label: 'src/main.ts' },
        {
          id: 'search-1:search:TimelineBlockRenderer:src',
          kind: 'search',
          label: 'TimelineBlockRenderer',
          detail: 'src',
        },
      ],
    }

    renderTimelineWithClient(
      <TimelineBlockRenderer
        block={block}
        conversationId="conversation-1"
        runId="run-1"
        turnId="turn-1"
      />,
      createTestCommandClient(),
    )

    const summary = screen.getByRole('button', { name: /已读取\/搜索 3 项/ })
    expect(summary).toHaveAttribute('aria-expanded', 'false')
    expect(screen.queryByText('src/main.ts')).not.toBeInTheDocument()

    fireEvent.click(summary)

    expect(screen.getByText('src/main.ts')).toBeInTheDocument()
    expect(screen.getByText('TimelineBlockRenderer')).toBeInTheDocument()
    expect(screen.getByText('src')).toBeInTheDocument()
  })

  it('renders command groups without fetching full output from the main timeline', async () => {
    await appI18n.changeLanguage('zh-CN')
    const getConversationCommandOutput = vi.fn()
    const block: TimelineRenderBlock = {
      kind: 'commandGroup',
      id: 'process:process-1:commands:command-1',
      order: 0,
      processSegmentId: 'process-1',
      defaultOpen: false,
      forcedOpen: false,
      steps: [],
      commands: [
        {
          id: 'command-1',
          stepId: 'command-1',
          status: 'complete',
          command: commandDetail({
            command: 'git status --short',
            exitCode: 0,
            stdoutPreview: 'M file.ts',
            fullOutputRef: 'full-output-ref-1',
          }),
        },
        {
          id: 'command-2',
          stepId: 'command-2',
          status: 'complete',
          command: commandDetail({
            command: 'pnpm -C apps/desktop test',
            exitCode: 0,
            stdoutPreview: 'passed',
          }),
        },
      ],
    }

    renderTimelineWithClient(
      <TimelineBlockRenderer
        block={block}
        conversationId="conversation-1"
        runId="run-1"
        turnId="turn-1"
      />,
      {
        ...createTestCommandClient(),
        getConversationCommandOutput,
      },
    )

    const summary = screen.getByRole('button', { name: /已运行 2 条命令/ })
    expect(summary).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getByText('git status --short')).toBeInTheDocument()
    expect(screen.getByText('pnpm -C apps/desktop test')).toBeInTheDocument()
    expect(screen.queryByText('$ git status --short')).not.toBeInTheDocument()

    fireEvent.click(summary)

    expect(screen.getByText('$ git status --short')).toBeInTheDocument()
    expect(screen.getByText('$ pnpm -C apps/desktop test')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '加载输出分页' })).not.toBeInTheDocument()
    expect(getConversationCommandOutput).not.toHaveBeenCalled()
  })

  it('defaults failed and non-zero command groups open', async () => {
    await appI18n.changeLanguage('zh-CN')
    const block: TimelineRenderBlock = {
      kind: 'commandGroup',
      id: 'process:process-1:commands:command-1',
      order: 0,
      processSegmentId: 'process-1',
      defaultOpen: true,
      forcedOpen: true,
      steps: [],
      commands: [
        {
          id: 'command-1',
          stepId: 'command-1',
          status: 'complete',
          command: commandDetail({
            command: 'cargo test',
            exitCode: 101,
            stderrPreview: 'failed',
          }),
        },
      ],
    }

    renderTimelineWithClient(
      <TimelineBlockRenderer
        block={block}
        conversationId="conversation-1"
        runId="run-1"
        turnId="turn-1"
      />,
      createTestCommandClient(),
    )

    const summary = screen.getByRole('button', { name: /已运行 1 条命令/ })
    expect(summary).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('$ cargo test')).toBeInTheDocument()
    expect(screen.getByText('退出码 101')).toBeVisible()
  })
})
