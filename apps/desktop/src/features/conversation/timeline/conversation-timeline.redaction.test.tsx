import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { ConversationTimeline } from './conversation-timeline'
import {
  minimaxTurn,
  reasoningTurn,
  resetTimelineTestState,
} from './conversation-timeline-test-utils'

describe('ConversationTimeline', () => {
  afterEach(() => {
    resetTimelineTestState()
  })

  it('renders a MiniMax-style failed tool flow as one safe assistant work tree', () => {
    render(<ConversationTimeline title="MiniMax flow" turns={[minimaxTurn()]} />)

    expect(screen.getByText('帮我生成一张海报图')).toBeInTheDocument()
    expect(screen.getByText('MiniMax M3')).toBeInTheDocument()
    expect(screen.queryByText('正在检查可用的图像工具')).not.toBeInTheDocument()
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

  it('does not render raw reasoning process rows after projection', () => {
    render(<ConversationTimeline title="Reasoning flow" turns={[reasoningTurn()]} />)

    expect(screen.queryByText('已完成推理过程')).not.toBeInTheDocument()
    expect(screen.queryByText('推理过程')).not.toBeInTheDocument()
    expect(screen.queryByText('Checked project context.')).not.toBeInTheDocument()
    expect(screen.queryByText('准备使用 read_file')).not.toBeInTheDocument()
    expect(screen.queryByText('read_file 已完成')).not.toBeInTheDocument()
    expect(screen.getByText('Final answer')).toBeInTheDocument()
    expect(document.body.textContent ?? '').not.toContain('raw private chain')
  })
})
