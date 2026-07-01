import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { appI18n } from '@/shared/i18n/i18n'
import { createTestCommandClient } from '@/testing/command-client'
import {
  codexLargeDiffTurns,
  codexStyleEvidenceTurns,
} from '@/testing/conversation-evidence-fixtures'
import { ConversationTimeline } from './conversation-timeline'
import {
  processHistoryTurn,
  renderTimelineWithClient,
  resetTimelineTestState,
  toolEvidenceTurn,
  turn,
} from './conversation-timeline-test-utils'
import { parseDiffEvidenceLines } from './diff-evidence-block'

describe('ConversationTimeline', () => {
  afterEach(() => {
    resetTimelineTestState()
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
