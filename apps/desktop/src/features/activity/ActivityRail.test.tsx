import '@testing-library/jest-dom/vitest'

import { render, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { ActivityRail } from './ActivityRail'

describe('ActivityRail', () => {
  it('renders a compact status bar instead of an activity drilldown', () => {
    render(<ActivityRail onOpenSettings={() => undefined} />)

    const statusBar = screen.getByRole('region', { name: 'Status' })

    expect(within(statusBar).getByText('Ready')).toBeInTheDocument()
    expect(within(statusBar).getByText('Local')).toBeInTheDocument()
    expect(within(statusBar).getByRole('button', { name: 'Settings' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'View all activity' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Expand activity' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Collapse activity' })).not.toBeInTheDocument()
  })

  it('shows active work without exposing the raw run id', () => {
    render(<ActivityRail activeRunId="run-001" onOpenSettings={() => undefined} />)

    const statusBar = screen.getByRole('region', { name: 'Status' })

    expect(within(statusBar).getByText('Running')).toBeInTheDocument()
    expect(within(statusBar).getByText('In progress')).toBeInTheDocument()
    expect(within(statusBar).queryByText('run-001')).not.toBeInTheDocument()
  })
})
