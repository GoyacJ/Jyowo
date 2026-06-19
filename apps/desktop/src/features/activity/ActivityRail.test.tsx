import '@testing-library/jest-dom/vitest'

import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ActivityRail, type ActivityRailItem } from './ActivityRail'

const activityItems = [
  { id: 'evt-1', label: 'write_file', status: 'success', time: '10:22 AM' },
  { id: 'evt-2', label: 'install_deps', status: 'failed', time: '10:23 AM' },
  { id: 'evt-3', label: 'start_dev', status: 'running', time: '10:24 AM' },
] satisfies ActivityRailItem[]

describe('ActivityRail', () => {
  it('renders compact recent activity without exposing raw payloads', () => {
    render(
      <ActivityRail
        currentRun={{ label: 'Current run', status: 'running' }}
        items={activityItems}
      />,
    )

    const rail = screen.getByRole('region', { name: 'Activity' })

    expect(within(rail).getByText('Activity')).toBeInTheDocument()
    expect(within(rail).getByText('write_file')).toBeInTheDocument()
    expect(within(rail).getByText('Success')).toBeInTheDocument()
    expect(within(rail).getByText('install_deps')).toBeInTheDocument()
    expect(within(rail).getByText('Failed')).toBeInTheDocument()
    expect(within(rail).getByText('start_dev')).toBeInTheDocument()
    expect(within(rail).getAllByText('Running')).toHaveLength(2)
    expect(within(rail).getByText('Current run')).toBeInTheDocument()
    expect(within(rail).queryByText(/Raw JSON/i)).not.toBeInTheDocument()
  })

  it('emits view-all intent when the action is provided', () => {
    const onViewAll = vi.fn()

    render(<ActivityRail items={activityItems} onViewAll={onViewAll} />)

    fireEvent.click(screen.getByRole('button', { name: 'View all activity' }))

    expect(onViewAll).toHaveBeenCalledTimes(1)
  })

  it('does not render a dead view-all button without a callback', () => {
    render(<ActivityRail items={activityItems} />)

    expect(screen.queryByRole('button', { name: 'View all activity' })).not.toBeInTheDocument()
  })

  it('renders collapsed and expanded states', () => {
    const onCollapse = vi.fn()
    const onExpand = vi.fn()

    const { rerender } = render(
      <ActivityRail collapsed items={activityItems} onExpand={onExpand} />,
    )

    expect(screen.getByRole('region', { name: 'Activity' })).toHaveAttribute(
      'data-collapsed',
      'true',
    )
    fireEvent.click(screen.getByRole('button', { name: 'Expand activity' }))
    expect(onExpand).toHaveBeenCalledTimes(1)

    rerender(<ActivityRail expanded items={activityItems} onCollapse={onCollapse} />)

    expect(screen.getByRole('region', { name: 'Activity' })).toHaveAttribute(
      'data-expanded',
      'true',
    )
    fireEvent.click(screen.getByRole('button', { name: 'Collapse activity' }))
    expect(onCollapse).toHaveBeenCalledTimes(1)
  })

  it('shows overflow activity only in the expanded state', () => {
    const overflowItems = [
      ...activityItems,
      { id: 'evt-4', label: 'request_permission', status: 'blocked', time: '10:25 AM' },
      { id: 'evt-5', label: 'read_secret', status: 'redacted', time: '10:26 AM' },
    ] satisfies ActivityRailItem[]

    const { rerender } = render(<ActivityRail items={overflowItems} />)

    expect(screen.queryByText('request_permission')).not.toBeInTheDocument()
    expect(screen.queryByText('read_secret')).not.toBeInTheDocument()

    rerender(<ActivityRail expanded items={overflowItems} />)

    expect(screen.getByText('request_permission')).toBeInTheDocument()
    expect(screen.getByText('Blocked')).toBeInTheDocument()
    expect(screen.getByText('read_secret')).toBeInTheDocument()
    expect(screen.getByText('Redacted')).toBeInTheDocument()
  })

  it('renders queued activity and status-aware current run state', () => {
    render(
      <ActivityRail
        currentRun={{ label: 'Current run', status: 'failed' }}
        items={[{ id: 'evt-queued', label: 'queue_tool', status: 'queued', time: '10:21 AM' }]}
      />,
    )

    expect(screen.getByText('queue_tool')).toBeInTheDocument()
    expect(screen.getByText('Queued')).toBeInTheDocument()
    expect(screen.getByTestId('current-run-status')).toHaveClass('text-destructive')
  })

  it('renders loading and error states without showing stale activity items', () => {
    const { rerender } = render(<ActivityRail items={activityItems} loading />)

    expect(screen.getByText('Loading activity')).toBeInTheDocument()
    expect(screen.queryByText('write_file')).not.toBeInTheDocument()

    rerender(<ActivityRail errorMessage="Activity unavailable" items={activityItems} />)

    expect(screen.getByText('Activity unavailable')).toBeInTheDocument()
    expect(screen.queryByText('write_file')).not.toBeInTheDocument()
  })

  it('renders duplicate labels and times using stable item ids', () => {
    render(
      <ActivityRail
        items={[
          { id: 'evt-a', label: 'write_file', status: 'success', time: '10:22 AM' },
          { id: 'evt-b', label: 'write_file', status: 'failed', time: '10:22 AM' },
        ]}
      />,
    )

    expect(screen.getAllByText('write_file')).toHaveLength(2)
    expect(screen.getByText('Success')).toBeInTheDocument()
    expect(screen.getByText('Failed')).toBeInTheDocument()
  })
})
