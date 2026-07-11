import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import type { TimelineItemProjection } from '@/generated/daemon-protocol'
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
