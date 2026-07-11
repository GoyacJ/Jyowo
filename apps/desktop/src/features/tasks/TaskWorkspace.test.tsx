import '@testing-library/jest-dom/vitest'

import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { TaskWorkspaceView } from './TaskWorkspace'
import type { TaskSnapshot } from './task-store'

describe('TaskWorkspace', () => {
  it('renders a centered readable timeline and connection state', () => {
    render(<TaskWorkspaceView connectionState="connected" snapshot={snapshot} />)

    expect(screen.getByRole('heading', { name: 'Repair scheduler recovery' })).toBeInTheDocument()
    expect(screen.getByTestId('task-reading-column')).toHaveClass('max-w-[820px]')
    expect(screen.getByText('Connected')).toBeInTheDocument()
  })

  it('renders an unavailable state without partial task content', () => {
    render(
      <TaskWorkspaceView
        connectionError="Malformed daemon frame"
        connectionState="protocol_error"
        snapshot={null}
      />,
    )

    expect(screen.getByRole('alert')).toHaveTextContent('Malformed daemon frame')
  })
})

const snapshot: TaskSnapshot = {
  projection: {
    archived: false,
    lastGlobalOffset: 2,
    queue: [],
    state: 'completed',
    streamVersion: 2,
    taskId: '01J00000000000000000000000',
    title: 'Repair scheduler recovery',
  },
  snapshotOffset: 2,
  timeline: [
    {
      globalOffset: 2,
      id: 'event-2',
      incomplete: false,
      kind: 'assistant_text',
      summary: 'Recovery is verified.',
    },
  ],
}
