import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import type { RunEvent } from '@/shared/events/run-event-schema'
import { ReplayTimeline } from './ReplayTimeline'

const replayEvents = [
  {
    id: 'evt-redacted',
    payload: { outputSummary: '[REDACTED]', toolUseId: 'tool-001' },
    runId: 'run-001',
    sequence: 2,
    source: 'tool',
    timestamp: '2026-06-17T00:00:02.000Z',
    type: 'tool.completed',
    visibility: 'redacted',
  },
  {
    id: 'evt-withheld',
    runId: 'run-001',
    sequence: 3,
    source: 'tool',
    timestamp: '2026-06-17T00:00:03.000Z',
    type: 'tool.completed',
    visibility: 'withheld',
  },
] satisfies RunEvent[]

describe('ReplayTimeline', () => {
  it('renders replayed events in supplied order without withheld payload details', () => {
    render(<ReplayTimeline events={replayEvents} replayed />)

    expect(screen.getByText('Replay')).toBeInTheDocument()
    expect(screen.getByText('Read-only')).toBeInTheDocument()
    expect(screen.getByText('tool-001')).toBeInTheDocument()
    expect(screen.getByText('Withheld event')).toBeInTheDocument()
    expect(screen.queryByText(/secret/i)).not.toBeInTheDocument()
  })

  it('renders loading, empty, and error states', () => {
    const { rerender } = render(<ReplayTimeline events={[]} loading replayed />)

    expect(screen.getByText('Loading replay')).toBeInTheDocument()

    rerender(<ReplayTimeline events={[]} replayed />)

    expect(screen.getByText('No replay events available.')).toBeInTheDocument()

    rerender(<ReplayTimeline errorMessage="Replay unavailable" events={[]} replayed />)

    expect(screen.getByText('Replay unavailable')).toBeInTheDocument()
  })

  it('virtualizes long replay timelines instead of rendering every event row', () => {
    const manyEvents = Array.from({ length: 500 }, (_, index) => ({
      id: `evt-${index}`,
      payload: { text: `delta-${index}` },
      runId: 'run-001',
      sequence: index,
      source: 'assistant',
      timestamp: '2026-06-17T00:00:02.000Z',
      type: 'assistant.delta',
      visibility: 'public',
    })) satisfies RunEvent[]

    render(<ReplayTimeline events={manyEvents} replayed />)

    expect(screen.getByText('500 events')).toBeInTheDocument()
    const renderedItems = screen.getAllByRole('listitem')
    expect(renderedItems.length).toBeLessThan(500)
    expect(renderedItems[0]).toHaveAttribute('aria-setsize', '500')
    expect(renderedItems[0]).toHaveAttribute('aria-posinset', '1')
  })
})
