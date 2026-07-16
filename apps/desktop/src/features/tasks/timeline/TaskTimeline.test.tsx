import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it, vi } from 'vitest'

import type { TimelineItemProjection } from '@/generated/daemon-protocol'
import { createAppI18n } from '@/shared/i18n/i18n'
import { uiStore } from '@/shared/state/ui-store'
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
    ).toEqual(
      items.filter((entry) => entry.summary !== 'Run started').map((entry) => entry.globalOffset),
    )
    expect(screen.getAllByTestId('user-message')[0]).toHaveClass('ml-auto')
    expect(
      screen.getByText('I am reading the scheduler.').closest('[data-narrative]'),
    ).not.toHaveClass('border')
    expect(
      screen.getByText('cargo test -p scheduler').closest('[data-artifact]'),
    ).toBeInTheDocument()
    expect(screen.getByText('Read scheduler.rs').closest('[data-artifact]')).toBeNull()
    expect(screen.getByText(/left this sentence/).closest('[data-incomplete]')).toHaveAttribute(
      'data-incomplete',
      'true',
    )
  })

  it('opens file and artifact outputs in the workbench', () => {
    const onSelectItem = vi.fn()
    const file = { ...item(30, 'file', 'report.md'), blobId: 'report-blob' }
    const artifact = { ...item(31, 'artifact', 'demo.mp4'), blobId: 'artifact-blob' }

    render(<TaskTimeline items={[file, artifact]} onSelectItem={onSelectItem} />)

    fireEvent.click(screen.getByRole('button', { name: 'Open File' }))
    fireEvent.click(screen.getByRole('button', { name: 'Open Artifact' }))
    expect(onSelectItem).toHaveBeenNthCalledWith(1, file, expect.any(HTMLElement))
    expect(onSelectItem).toHaveBeenNthCalledWith(2, artifact, expect.any(HTMLElement))
  })

  it('renders mixed assistant content blocks in protocol order', () => {
    const mixed = {
      ...item(31, 'assistant_text', 'legacy summary', 'segment-mixed'),
      contentBlocks: [
        { format: 'markdown' as const, text: 'Before media', type: 'text' as const },
        {
          artifact: {
            artifactKind: 'file',
            mediaType: 'text/plain',
            preview: 'Video transcript',
            presentation: { preferredSurface: 'inline' as const },
            title: 'demo.mp4',
          },
          type: 'artifact' as const,
        },
        { format: 'plain' as const, text: 'After media', type: 'text' as const },
      ],
    }

    const { container } = render(<TaskTimeline items={[mixed]} />)
    const before = screen.getByText('Before media')
    const preview = screen.getByText('Video transcript')
    const after = screen.getByText('After media')

    expect(before.compareDocumentPosition(preview) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(preview.compareDocumentPosition(after) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(container.querySelectorAll('[data-artifact]')).toHaveLength(1)
    expect(screen.queryByText('legacy summary')).not.toBeInTheDocument()
  })

  it('replaces streamed deltas with the completed assistant blocks', () => {
    const delta = {
      ...item(35, 'assistant_text', 'Draft response', 'segment-authoritative', true),
      contentBlocks: [
        { format: 'markdown' as const, text: 'Draft response', type: 'text' as const },
      ],
      semanticGroupId: 'message-authoritative',
    }
    const completed = {
      ...item(36, 'assistant_text', 'Canonical before  after', 'segment-authoritative'),
      contentBlocks: [
        { format: 'markdown' as const, text: 'Canonical before ', type: 'text' as const },
        {
          artifact: {
            artifactKind: 'file',
            mediaType: 'text/plain',
            preview: 'media preview',
            presentation: { preferredSurface: 'inline' as const },
            title: 'result.txt',
          },
          type: 'artifact' as const,
        },
        { format: 'plain' as const, text: ' after', type: 'text' as const },
      ],
      semanticGroupId: 'message-authoritative',
    }

    render(<TaskTimeline items={[delta, completed]} />)
    const before = screen.getByText('Canonical before')
    const media = screen.getByText('media preview')
    const after = screen.getByText('after')

    expect(screen.queryByText('Draft response')).not.toBeInTheDocument()
    expect(before.compareDocumentPosition(media) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(media.compareDocumentPosition(after) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it.each(['command', 'terminal'])('does not offer to open %s artifacts', (artifactKind) => {
    const entry = {
      ...item(37, artifactKind === 'command' ? 'command' : 'artifact', 'Task output'),
      contentBlocks: [
        {
          artifact: {
            artifactKind,
            blobId: `${artifactKind}-blob`,
            mediaType: 'application/octet-stream',
            title: `${artifactKind} output`,
          },
          type: 'artifact' as const,
        },
      ],
    }

    render(<TaskTimeline items={[entry]} onSelectItem={() => undefined} />)

    expect(screen.queryByRole('button', { name: /^Open / })).not.toBeInTheDocument()
  })

  it('uses notice levels and tool activities from canonical content blocks', () => {
    const warning = {
      ...item(32, 'notice', 'legacy warning'),
      contentBlocks: [
        { level: 'warning' as const, text: 'Review required', type: 'notice' as const },
      ],
    }
    const tool = {
      ...item(33, 'tool_activity', 'legacy tool', 'segment-block-tool'),
      contentBlocks: [
        {
          activity: {
            operation: 'read' as const,
            status: 'completed' as const,
            subject: 'src/main.ts',
            toolName: 'read_file',
            toolUseId: 'content-block-tool',
          },
          type: 'tool_activity' as const,
        },
      ],
    }

    const { container } = render(<TaskTimeline items={[warning, tool]} />)

    expect(screen.getByText('Review required').closest('[data-notice-level]')).toHaveAttribute(
      'data-notice-level',
      'warning',
    )
    expect(screen.getAllByText('Read src/main.ts').length).toBeGreaterThan(0)
    expect(container.querySelector('[data-event-id="event-33"]')).toBeInTheDocument()
  })

  it('opens the selected artifact when one assistant message contains several', () => {
    const onSelectItem = vi.fn()
    const mixed = {
      ...item(34, 'assistant_text', 'Two files', 'segment-two-files'),
      contentBlocks: [
        {
          artifact: {
            artifactId: 'first-artifact',
            artifactKind: 'file',
            mediaType: 'text/plain',
            preview: 'first preview',
            title: 'first.txt',
          },
          type: 'artifact' as const,
        },
        {
          artifact: {
            artifactId: 'second-artifact',
            artifactKind: 'file',
            mediaType: 'text/plain',
            preview: 'second preview',
            title: 'second.txt',
          },
          type: 'artifact' as const,
        },
      ],
    }

    render(<TaskTimeline items={[mixed]} onSelectItem={onSelectItem} />)
    const openButtons = screen.getAllByRole('button', { name: 'Open File' })
    fireEvent.click(openButtons[0] as HTMLButtonElement)
    fireEvent.click(openButtons[1] as HTMLButtonElement)

    expect(onSelectItem).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        contentBlocks: [
          expect.objectContaining({
            artifact: expect.objectContaining({ artifactId: 'first-artifact' }),
          }),
        ],
      }),
      expect.any(HTMLElement),
    )
    expect(onSelectItem).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        contentBlocks: [
          expect.objectContaining({
            artifact: expect.objectContaining({ artifactId: 'second-artifact' }),
          }),
        ],
      }),
      expect.any(HTMLElement),
    )
  })

  it('opens user message attachments without making plain messages interactive', () => {
    const onSelectItem = vi.fn()
    const attachment = { ...item(32, 'user_message', 'design.png'), blobId: 'attachment-blob' }
    const plainMessage = item(33, 'user_message', 'No attachment')

    render(<TaskTimeline items={[attachment, plainMessage]} onSelectItem={onSelectItem} />)

    fireEvent.click(screen.getByRole('button', { name: 'Open File' }))
    expect(onSelectItem).toHaveBeenCalledWith(attachment, expect.any(HTMLElement))
    expect(screen.getByText('No attachment').closest('button')).toBeNull()
  })

  it('restores paused position per task without cross-task prepend compensation', () => {
    const first = item(42, 'assistant_text', 'Task one', 'segment-task-one')
    const second = item(43, 'user_message', 'Task one update')
    uiStore.getState().setTimelineScrollSession('timeline-task-2', {
      hasNewContent: false,
      mode: 'paused',
      newItemCount: 0,
      scrollTop: 300,
      showJumpToLatest: true,
      visibleAnchor: null,
    })
    const { rerender } = render(<TaskTimeline items={[first]} taskId="timeline-task-1" />)
    const viewport = screen.getByTestId('task-timeline-viewport')
    let scrollHeight = 1_200
    Object.defineProperties(viewport, {
      clientHeight: { configurable: true, value: 300 },
      scrollHeight: { configurable: true, get: () => scrollHeight },
      scrollTop: { configurable: true, value: 200, writable: true },
    })
    fireEvent.wheel(viewport, { deltaY: -24 })
    rerender(<TaskTimeline items={[first, second]} taskId="timeline-task-1" />)

    scrollHeight = 1_600
    rerender(
      <TaskTimeline items={[item(90, 'user_message', 'Task two')]} taskId="timeline-task-2" />,
    )
    expect(viewport.scrollTop).toBe(300)

    rerender(<TaskTimeline items={[first, second]} taskId="timeline-task-1" />)
    expect(viewport.scrollTop).toBe(200)
  })

  it('restores a saved task session after the timeline remounts', () => {
    uiStore.getState().setTimelineScrollSession('timeline-remount', {
      hasNewContent: true,
      mode: 'paused',
      newItemCount: 2,
      scrollTop: 375,
      showJumpToLatest: true,
      visibleAnchor: null,
    })

    render(
      <TaskTimeline
        items={[item(46, 'assistant_text', 'Restored timeline', 'segment-remount')]}
        taskId="timeline-remount"
      />,
    )

    expect(screen.getByTestId('task-timeline-viewport').scrollTop).toBe(375)
  })

  it('accepts scrollbar pointer and touch upward gestures', () => {
    const { unmount } = render(
      <TaskTimeline
        items={[item(44, 'assistant_text', 'Pointer interruption', 'segment-pointer')]}
        taskId="timeline-pointer"
      />,
    )
    const pointerViewport = screen.getByTestId('task-timeline-viewport')
    Object.defineProperties(pointerViewport, {
      clientHeight: { configurable: true, value: 300 },
      scrollHeight: { configurable: true, value: 1_200 },
      scrollTop: { configurable: true, value: 900, writable: true },
    })
    fireEvent.pointerDown(pointerViewport, { clientY: 10 })
    pointerViewport.scrollTop = 800
    fireEvent.scroll(pointerViewport)
    unmount()
    expect(uiStore.getState().timelineScrollByTaskId['timeline-pointer']?.mode).toBe('paused')

    const { unmount: unmountTouch } = render(
      <TaskTimeline
        items={[item(45, 'assistant_text', 'Touch interruption', 'segment-touch')]}
        taskId="timeline-touch"
      />,
    )
    const touchViewport = screen.getByTestId('task-timeline-viewport')
    Object.defineProperties(touchViewport, {
      clientHeight: { configurable: true, value: 300 },
      scrollHeight: { configurable: true, value: 1_200 },
      scrollTop: { configurable: true, value: 800, writable: true },
    })
    fireEvent.touchStart(touchViewport, { touches: [{ clientY: 260 }] })
    fireEvent.touchMove(touchViewport, { touches: [{ clientY: 300 }] })
    unmountTouch()
    expect(uiStore.getState().timelineScrollByTaskId['timeline-touch']?.mode).toBe('paused')
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

    expect(screen.getByText('Run cancelled')).toBeInTheDocument()
    expect(screen.getByText('Run superseded')).toBeInTheDocument()
  })

  it('keeps ordinary terminal lifecycle events out of the conversation', () => {
    render(<TaskTimeline items={[item(60, 'notice', 'Run completed', 'segment-current')]} />)

    expect(screen.queryByTestId('timeline-item')).not.toBeInTheDocument()
    expect(screen.queryByText('Run completed')).not.toBeInTheDocument()
  })

  it('keeps adjacent assistant messages in separate narrative groups', () => {
    const { container } = render(
      <TaskTimeline
        items={[
          {
            ...item(70, 'assistant_text', 'First ', 'segment-grouped', true),
            semanticGroupId: 'message-a',
          },
          {
            ...item(71, 'assistant_text', 'First message', 'segment-grouped'),
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

  it('does not split one assistant narrative at a virtualization chunk boundary', () => {
    const { container } = render(
      <TaskTimeline
        items={Array.from({ length: 17 }, (_, index) => ({
          ...item(80 + index, 'assistant_text', String(index), 'segment-streamed', true),
          semanticGroupId: 'one-message',
        }))}
      />,
    )

    expect(container.querySelectorAll('[data-narrative]')).toHaveLength(1)
  })

  it('coalesces a long streamed assistant message into one rendered timeline node', () => {
    const streamed = Array.from({ length: 500 }, (_, index) => ({
      ...item(1_000 + index, 'assistant_text', String(index % 10), 'segment-streamed', true),
      semanticGroupId: 'one-long-message',
    }))

    render(<TaskTimeline items={streamed} />)

    expect(screen.getAllByTestId('timeline-item')).toHaveLength(1)
    expect(screen.getByTestId('timeline-item')).toHaveTextContent(
      streamed.map((entry) => entry.summary).join(''),
    )
  })

  it('renders assistant Markdown with GFM structure and long-word wrapping', () => {
    const { container } = render(
      <TaskTimeline
        items={[
          item(
            1_600,
            'assistant_text',
            '## Result\n\nThis is **done** with `code`.\n\n| File | State |\n| --- | --- |\n| scheduler.rs | fixed |\n\nverylongtokenwithoutbreakpoints0123456789',
            'segment-markdown',
          ),
        ]}
      />,
    )

    expect(screen.getByRole('heading', { name: 'Result', level: 2 })).toHaveClass('text-xl')
    expect(screen.getByText('done').tagName).toBe('STRONG')
    expect(screen.getByText('code').tagName).toBe('CODE')
    expect(screen.getByRole('table')).toBeInTheDocument()
    expect(container.querySelector('.overflow-wrap-anywhere')).toBeInTheDocument()
  })

  it('does not insert a duplicate run-status heading into the narrative', () => {
    render(
      <TaskTimeline
        items={[
          item(1_700, 'assistant_text', 'First response', 'segment-reused'),
          item(1_701, 'user_message', 'Continue'),
          item(1_702, 'assistant_text', 'Second response', 'segment-reused'),
        ]}
      />,
    )

    expect(screen.queryByText('running')).not.toBeInTheDocument()
    expect(screen.getByText('First response')).toBeInTheDocument()
    expect(screen.getByText('Second response')).toBeInTheDocument()
  })

  it('removes adjacent low-value lifecycle events from the conversation', () => {
    render(
      <TaskTimeline
        items={[
          item(1_800, 'notice', 'Run started', 'segment-lifecycle'),
          item(1_801, 'notice', 'Workspace acquired', 'segment-lifecycle'),
          item(1_802, 'notice', 'Workspace released', 'segment-lifecycle'),
        ]}
      />,
    )

    expect(screen.queryByTestId('timeline-item')).not.toBeInTheDocument()
    expect(screen.queryByText(/run record/)).not.toBeInTheDocument()
  })

  it('groups projected tool calls under a semantic activity summary', () => {
    const read = {
      ...item(1_900, 'tool_activity', 'Read scheduler.rs', 'segment-tools'),
      tool: {
        durationMs: 42,
        operation: 'read' as const,
        resultSummary: '18 lines returned',
        status: 'completed' as const,
        subject: 'src/scheduler.rs',
        toolName: 'read_file',
        toolUseId: 'tool-read',
      },
    }
    const command = {
      ...item(1_901, 'tool_activity', 'Ran command', 'segment-tools'),
      tool: {
        command: 'pnpm test',
        durationMs: 1_200,
        operation: 'command' as const,
        output: '69 test files passed',
        status: 'completed' as const,
        toolName: 'exec_command',
        toolUseId: 'tool-command',
      },
    }

    render(<TaskTimeline items={[read, command]} onSelectItem={() => undefined} />)

    expect(screen.getByText('Read 1 file · Ran 1 command')).toBeInTheDocument()
    expect(screen.getByText('Read 1 file · Ran 1 command').closest('details')).toHaveAttribute(
      'open',
    )
    expect(screen.getByRole('region', { name: 'Command output' })).toHaveTextContent('$ pnpm test')
    expect(screen.getByRole('region', { name: 'Command output' })).toHaveTextContent(
      '69 test files passed',
    )
    expect(screen.queryByRole('button', { name: /Ran command/ })).not.toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Task update: Ran command')
  })

  it.each([
    'BrowserUse',
    'BrowserDevTools',
  ])('opens %s activity in the browser panel', (toolName) => {
    const onSelectItem = vi.fn()
    const browser = {
      ...item(1_902, 'tool_activity', toolName, 'segment-browser'),
      tool: {
        operation: 'browse' as const,
        status: 'completed' as const,
        toolName,
        toolUseId: `tool-${toolName}`,
      },
    }

    render(<TaskTimeline items={[browser]} onSelectItem={onSelectItem} />)
    fireEvent.click(screen.getByRole('button', { name: 'Open Browser' }))

    expect(onSelectItem).toHaveBeenCalledWith(browser, expect.any(HTMLButtonElement))
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
            item(704, 'notice', 'Task pinned'),
            item(705, 'notice', 'Task removed'),
          ]}
          onSelectItem={() => undefined}
        />
      </I18nextProvider>,
    )

    expect(screen.queryByText('运行已开始')).not.toBeInTheDocument()
    expect(screen.queryByText('运行已完成')).not.toBeInTheDocument()
    expect(screen.getByText('Run completed')).toBeInTheDocument()
    expect(screen.getByText('未完成')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '打开更改' })).not.toBeInTheDocument()
    expect(screen.getByText('详情')).toBeInTheDocument()
    expect(screen.getByText('任务已置顶')).toBeInTheDocument()
    expect(screen.getByText('任务已移除')).toBeInTheDocument()
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
