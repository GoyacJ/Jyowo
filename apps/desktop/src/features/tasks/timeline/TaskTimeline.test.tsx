import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it } from 'vitest'

import type { TimelineItemProjection } from '@/generated/daemon-protocol'
import { createAppI18n } from '@/shared/i18n/i18n'
import { TaskTimeline } from './TaskTimeline'

const items: TimelineItemProjection[] = [
  item(12, 'user_message', 'Inspect the scheduler'),
  item(13, 'notice', 'Run started', 'segment-a'),
  item(14, 'assistant_text', 'I am reading the scheduler.', 'segment-a'),
  item(15, 'tool_activity', 'Read scheduler.rs', 'segment-a'),
  item(16, 'assistant_text', 'The state is restored from the journal.', 'segment-a'),
  item(17, 'command', 'cargo test -p scheduler', 'segment-a'),
  item(18, 'diff', '2 files changed', 'segment-a'),
  item(19, 'image', 'Timeline preview', 'segment-a'),
  item(20, 'permission', 'Permission requested', 'segment-a'),
  item(21, 'compaction', 'Context compacted', 'segment-a'),
  item(22, 'subagent', 'Reviewer completed', 'segment-a'),
  item(23, 'assistant_text', 'The interruption left this sentence', 'segment-a', true),
  item(24, 'notice', 'Run interrupted by restart', 'segment-a', true),
  item(25, 'user_message', 'Continue safely'),
  item(26, 'notice', 'Run started', 'segment-b'),
  item(27, 'error', 'Command failed; choose how to continue', 'segment-b'),
]

describe('TaskTimeline', () => {
  it('preserves global offset order and uses the Codex visual hierarchy', () => {
    render(<TaskTimeline items={[...items].reverse()} />)

    expect(
      screen.getAllByTestId('timeline-item').map((node) => Number(node.dataset.offset)),
    ).toEqual(items.map((entry) => entry.globalOffset))
    expect(screen.getAllByTestId('user-message')[0]).toHaveClass('ml-auto')
    expect(
      screen.getByText('I am reading the scheduler.').closest('[data-narrative]'),
    ).not.toHaveClass('border')
    expect(
      screen.getByText('cargo test -p scheduler').closest('[data-artifact]'),
    ).toBeInTheDocument()
    expect(screen.getByText('Read scheduler.rs').closest('[data-artifact]')).toBeNull()
    expect(screen.getByText(/left this sentence/)).toHaveAttribute('data-incomplete', 'true')
  })

  it('offers a jump to latest when partial output grows away from the bottom', () => {
    const partial = item(40, 'assistant_text', 'partial', 'segment-streaming', true)
    const { rerender } = render(<TaskTimeline items={[partial]} />)
    const viewport = screen.getByTestId('task-timeline-viewport')
    Object.defineProperties(viewport, {
      clientHeight: { configurable: true, value: 300 },
      scrollHeight: { configurable: true, value: 1_200 },
      scrollTop: { configurable: true, value: 200, writable: true },
    })
    fireEvent.scroll(viewport)

    rerender(<TaskTimeline items={[{ ...partial, summary: 'partial output grew' }]} />)

    expect(screen.getByRole('button', { name: 'Jump to latest' })).toBeInTheDocument()
  })

  it('renders cancelled and superseded historical run terminal states', () => {
    render(
      <TaskTimeline
        items={[
          item(50, 'notice', 'Run started', 'segment-cancelled'),
          item(51, 'notice', 'Run cancelled', 'segment-cancelled'),
          item(52, 'user_message', 'Replace it'),
          item(53, 'notice', 'Run started', 'segment-superseded'),
          item(54, 'notice', 'Run superseded', 'segment-superseded'),
        ]}
      />,
    )

    expect(screen.getByRole('region', { name: 'Run cancelled' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Run superseded' })).toBeInTheDocument()
  })

  it('prefers the generated terminal reason for the projected current run', () => {
    render(
      <TaskTimeline
        currentRun={{
          endedAt: '2026-07-11T06:01:00Z',
          incompleteOutput: false,
          segmentId: 'segment-current',
          startedAt: '2026-07-11T06:00:00Z',
          state: 'completed',
          terminalReason: 'cancelled',
        }}
        items={[item(60, 'notice', 'Run completed', 'segment-current')]}
      />,
    )

    expect(screen.getByRole('region', { name: 'Run cancelled' })).toBeInTheDocument()
  })

  it('keeps adjacent assistant messages in separate narrative groups', () => {
    const { container } = render(
      <TaskTimeline
        items={[
          {
            ...item(70, 'assistant_text', 'First ', 'segment-grouped'),
            semanticGroupId: 'message-a',
          },
          {
            ...item(71, 'assistant_text', 'message', 'segment-grouped'),
            semanticGroupId: 'message-a',
          },
          {
            ...item(72, 'assistant_text', 'Second message', 'segment-grouped'),
            semanticGroupId: 'message-b',
          },
        ]}
      />,
    )

    const narratives = container.querySelectorAll('[data-narrative]')
    expect(narratives).toHaveLength(2)
    expect(narratives[0]).toHaveTextContent('First message')
    expect(narratives[1]).toHaveTextContent('Second message')
  })

  it('virtualizes a long single-run history instead of mounting every event', () => {
    const longRun = Array.from({ length: 500 }, (_, index) =>
      item(100 + index, 'tool_activity', `Read file ${index}`, 'segment-long'),
    )

    render(<TaskTimeline items={longRun} />)

    const renderedItems = screen.getAllByTestId('timeline-item')
    expect(renderedItems.length).toBeGreaterThan(0)
    expect(renderedItems.length).toBeLessThan(longRun.length)
    expect(screen.getByTestId('task-timeline-scroll-content')).toHaveClass('relative')
  })

  it('localizes canonical task lifecycle summaries without translating assistant content', () => {
    render(
      <I18nextProvider i18n={createAppI18n('zh-CN')}>
        <TaskTimeline
          items={[
            item(700, 'notice', 'Run started', 'segment-localized'),
            item(701, 'assistant_text', 'Run completed', 'segment-localized'),
            item(702, 'notice', 'Run completed', 'segment-localized'),
            item(703, 'diff', 'Artifact updated', 'segment-localized', true),
          ]}
          onSelectItem={() => undefined}
        />
      </I18nextProvider>,
    )

    expect(screen.getByText('运行已开始')).toBeInTheDocument()
    expect(screen.getByText('运行已完成')).toBeInTheDocument()
    expect(screen.getByText('Run completed')).toBeInTheDocument()
    expect(screen.getByText('未完成')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '打开更改' })).toHaveTextContent('打开')
    expect(screen.getByText('详情')).toBeInTheDocument()
  })
})

function item(
  globalOffset: number,
  kind: TimelineItemProjection['kind'],
  summary: string,
  runSegmentId?: string,
  incomplete = false,
): TimelineItemProjection {
  return { globalOffset, id: `event-${globalOffset}`, incomplete, kind, runSegmentId, summary }
}
