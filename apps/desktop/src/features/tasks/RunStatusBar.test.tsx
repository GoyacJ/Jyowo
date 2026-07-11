import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import type { TaskProjection, TimelineItemProjection } from '@/generated/daemon-protocol'

import { RunStatusBar } from './RunStatusBar'

describe('RunStatusBar', () => {
  it('shows the projected run step, elapsed time, queue count, and change summary', () => {
    render(
      <RunStatusBar
        items={items}
        now={Date.parse('2026-07-11T06:01:05Z')}
        projection={projection}
      />,
    )

    const status = screen.getByRole('status', { name: 'Current run status' })
    expect(status).toHaveTextContent('Running tests')
    expect(status).toHaveTextContent('1m 5s')
    expect(status).toHaveTextContent('2 queued')
    expect(status).toHaveTextContent('Changed recovery.rs')
  })

  it('is absent when no segment is active', () => {
    const { container } = render(
      <RunStatusBar
        items={[]}
        projection={{ ...projection, currentRun: null, state: 'completed' }}
      />,
    )

    expect(container).toBeEmptyDOMElement()
  })
})

const projection: TaskProjection = {
  archived: false,
  currentRun: {
    incompleteOutput: false,
    segmentId: '01J00000000000000000000002',
    startedAt: '2026-07-11T06:00:00Z',
    state: 'running',
  },
  lastGlobalOffset: 8,
  queue: [queueItem('01J00000000000000000000003'), queueItem('01J00000000000000000000004')],
  state: 'running',
  streamVersion: 8,
  taskId: '01J00000000000000000000001',
  title: 'Repair scheduler',
}

const items: TimelineItemProjection[] = [
  item(6, 'diff', 'Changed scheduler.rs'),
  item(7, 'diff', 'Changed recovery.rs'),
  item(8, 'tool_activity', 'Running tests'),
]

function queueItem(queueItemId: string) {
  return {
    attachments: [],
    content: 'Queued',
    contextReferences: [],
    createdAt: '2026-07-11T06:00:30Z',
    createdGlobalOffset: 7,
    queueItemId,
    revision: 1,
    state: 'queued' as const,
  }
}

function item(
  globalOffset: number,
  kind: TimelineItemProjection['kind'],
  summary: string,
): TimelineItemProjection {
  return {
    globalOffset,
    id: `event-${globalOffset}`,
    incomplete: false,
    kind,
    runSegmentId: projection.currentRun?.segmentId,
    summary,
  }
}
